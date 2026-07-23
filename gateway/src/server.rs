use crate::{adapter, config, solana};
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path as UrlPath, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{Local, Utc};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::Semaphore;

/// How often expired quotes are swept. Sweeping exists so that quote-spam
/// (which costs an attacker nothing) cannot grow the quote map between TTLs.
const EVICTION_INTERVAL: Duration = Duration::from_secs(60);

#[derive(Clone)]
struct Quote {
    service_id: String,
    reference: String,
    amount_micros: u64,
    pay_url: String,
    expires_at: u64,
}

pub struct App {
    cfg: config::Config,
    rpc: solana::Rpc,
    /// Outstanding quotes. Bounded by `max_outstanding_quotes` at insert and
    /// swept by the eviction task — both bounds are load-bearing.
    quotes: Mutex<HashMap<String, Quote>>,
    /// One semaphore per service: the adapter host's concurrency budget.
    /// Saturation answers 429 — there is deliberately no internal queue.
    slots: HashMap<String, Arc<Semaphore>>,
    /// Serializes ledger appends so concurrent job completions can't
    /// interleave partial lines.
    ledger: Mutex<std::fs::File>,
    data_dir: PathBuf,
}

pub fn build(cfg: config::Config) -> anyhow::Result<(Router, Arc<App>)> {
    let data_dir = PathBuf::from(&cfg.gateway.data_dir);
    std::fs::create_dir_all(&data_dir)?;
    let ledger_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(data_dir.join("ledger.jsonl"))?;

    let slots = cfg
        .services
        .iter()
        .filter(|s| s.enabled)
        .map(|s| (s.id.clone(), Arc::new(Semaphore::new(s.max_concurrent))))
        .collect();

    let max_body = cfg.gateway.max_body_mb * 1024 * 1024;
    let app = Arc::new(App {
        rpc: solana::Rpc::new(&cfg.operator.rpc_url),
        quotes: Mutex::new(HashMap::new()),
        slots,
        ledger: Mutex::new(ledger_file),
        data_dir,
        cfg,
    });

    let router = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/services", get(services))
        .route("/jobs/{service_id}", post(jobs))
        .route("/report/today", get(report_today))
        .layer(DefaultBodyLimit::max(max_body))
        .with_state(app.clone());
    Ok((router, app))
}

/// Periodic sweep of expired quotes. Runs for the life of the process.
pub async fn evict_expired(app: Arc<App>) {
    loop {
        tokio::time::sleep(EVICTION_INTERVAL).await;
        let cutoff = now();
        let removed = {
            let mut quotes = lock(&app.quotes);
            let before = quotes.len();
            quotes.retain(|_, q| q.expires_at >= cutoff);
            before - quotes.len()
        };
        if removed > 0 {
            tracing::info!(removed, "evicted expired quotes");
        }
    }
}

/// Public catalog — shaped small on purpose: this response is read by model
/// contexts as well as humans.
async fn services(State(app): State<Arc<App>>) -> Json<Value> {
    let list: Vec<Value> = app
        .cfg
        .services
        .iter()
        .filter(|s| s.enabled)
        .map(|s| json!({"id": s.id, "summary": s.summary, "price_usdc": s.price, "unit": s.unit}))
        .collect();
    Json(json!({"services": list}))
}

/// The x402 endpoint. First call (no X-Job-Id) answers 402 with a quote and a
/// Solana Pay URL. The client pays, then retries with X-Job-Id and the
/// payload; the gateway verifies the payment on-chain and runs the adapter.
async fn jobs(
    State(app): State<Arc<App>>,
    UrlPath(service_id): UrlPath<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Some(service) = app.cfg.service(&service_id).cloned() else {
        return err(StatusCode::NOT_FOUND, "unknown service");
    };

    let job_id = headers
        .get("x-job-id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);

    let Some(job_id) = job_id else {
        return issue_quote(&app, &service);
    };
    execute_paid(&app, &service, &job_id, &body).await
}

/// Phase 1: mint a quote with a fresh single-use reference key.
fn issue_quote(app: &App, service: &config::Service) -> Response {
    let reference = solana::new_reference();
    let job_id = format!("job_{}", &reference[..12]);
    let quote = Quote {
        service_id: service.id.clone(),
        reference: reference.clone(),
        amount_micros: service.price_micros(),
        pay_url: solana::pay_url(
            &app.cfg.operator.receive_address,
            service.price,
            &app.cfg.operator.usdc_mint,
            &reference,
            &format!("rigpay: {}", service.id),
        ),
        expires_at: now() + app.cfg.operator.quote_ttl_secs,
    };

    {
        let mut quotes = lock(&app.quotes);
        // Cap check under the same lock as the insert, or two racing requests
        // could both pass the check at the ceiling.
        if quotes.len() >= app.cfg.gateway.max_outstanding_quotes {
            tracing::warn!(cap = app.cfg.gateway.max_outstanding_quotes, "quote cap hit");
            return err_with(
                StatusCode::TOO_MANY_REQUESTS,
                "quote capacity reached — retry shortly",
                json!({"retry_after_secs": 30}),
            );
        }
        quotes.insert(job_id.clone(), quote.clone());
    }

    tracing::info!(job_id, service = service.id, amount_usdc = service.price, "quote issued");
    (
        StatusCode::PAYMENT_REQUIRED,
        Json(json!({
            "status": "payment_required",
            "job_id": job_id,
            "service": service.id,
            "amount_usdc": service.price,
            "pay_url": quote.pay_url,
            "reference": reference,
            "expires_at": quote.expires_at,
            "next": "pay the URL, then POST again with header X-Job-Id"
        })),
    )
        .into_response()
}

/// Phase 2: verify the payment on-chain, then dispatch under the service's
/// concurrency budget.
async fn execute_paid(
    app: &Arc<App>,
    service: &config::Service,
    job_id: &str,
    body: &Bytes,
) -> Response {
    let quote = match lock(&app.quotes).get(job_id) {
        Some(q) if q.service_id == service.id => q.clone(),
        Some(_) => return err(StatusCode::BAD_REQUEST, "job_id belongs to a different service"),
        None => return err(StatusCode::NOT_FOUND, "unknown or already-completed job_id"),
    };
    if now() > quote.expires_at {
        lock(&app.quotes).remove(job_id);
        return err(StatusCode::GONE, "quote expired — request a new one");
    }

    // Verify BEFORE taking an execution slot: an unpaid probe must never
    // consume adapter capacity.
    let sig = match app
        .rpc
        .find_payment(
            &quote.reference,
            &app.cfg.operator.receive_address,
            &app.cfg.operator.usdc_mint,
            quote.amount_micros,
        )
        .await
    {
        Ok(Some(sig)) => sig,
        Ok(None) => {
            return err_with(
                StatusCode::PAYMENT_REQUIRED,
                "unpaid",
                json!({"job_id": job_id, "pay_url": quote.pay_url, "expires_at": quote.expires_at}),
            )
        }
        Err(e) => {
            tracing::error!(job_id, error = %e, "rpc verification failed");
            return err(StatusCode::BAD_GATEWAY, "payment verification unavailable — retry");
        }
    };

    // A paid job whose service is saturated keeps its quote: the client
    // retries with the same X-Job-Id and pays nothing extra.
    let Some(slots) = app.slots.get(&service.id) else {
        return err(StatusCode::NOT_FOUND, "unknown service");
    };
    let Ok(_permit) = slots.clone().try_acquire_owned() else {
        tracing::warn!(service = service.id, "service saturated");
        return err_with(
            StatusCode::TOO_MANY_REQUESTS,
            "service at capacity — payment recorded, retry with the same X-Job-Id",
            json!({"job_id": job_id, "retry_after_secs": 15}),
        );
    };

    tracing::info!(job_id, service = service.id, sig, "payment verified, dispatching");
    match adapter::run(service, job_id, &app.data_dir, body).await {
        Ok(result) => {
            lock(&app.quotes).remove(job_id);
            ledger(app, job_id, &service.id, service.price, &sig, "completed");
            (
                StatusCode::OK,
                Json(json!({
                    "status": "completed",
                    "job_id": job_id,
                    "paid_signature": sig,
                    "result": String::from_utf8_lossy(&result)
                })),
            )
                .into_response()
        }
        Err(e) => {
            // Paid but failed: the quote stays so a retry can't double-charge,
            // and the ledger flags it for the human refund-review checkpoint.
            // No code path here or anywhere may move money.
            tracing::error!(job_id, service = service.id, error = %e, "paid job failed");
            ledger(app, job_id, &service.id, service.price, &sig, "failed_refund_review");
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "job failed — payment recorded and flagged for refund review",
            )
        }
    }
}

/// Compact daily summary for the ZeroClaw reconciliation SOP. This feeds a
/// model context: keep it ~100 tokens no matter how big the ledger gets.
async fn report_today(State(app): State<Arc<App>>) -> Json<Value> {
    let today = Local::now().format("%Y-%m-%d").to_string();
    let mut earned = 0.0_f64;
    let mut completed = 0u32;
    let mut flagged: Vec<String> = vec![];
    let mut by_service: HashMap<String, u32> = HashMap::new();

    if let Ok(raw) = std::fs::read_to_string(app.data_dir.join("ledger.jsonl")) {
        for line in raw.lines() {
            let Ok(rec) = serde_json::from_str::<Value>(line) else { continue };
            if rec["ts"].as_str().map(|t| t.starts_with(&today)) != Some(true) {
                continue;
            }
            match rec["status"].as_str() {
                Some("completed") => {
                    completed += 1;
                    earned += rec["amount_usdc"].as_f64().unwrap_or(0.0);
                    if let Some(s) = rec["service"].as_str() {
                        *by_service.entry(s.to_string()).or_default() += 1;
                    }
                }
                Some("failed_refund_review") => {
                    if let Some(j) = rec["job_id"].as_str() {
                        flagged.push(j.to_string());
                    }
                }
                _ => {}
            }
        }
    }
    // Cap the flagged list too — 500 failures must not become 500 lines in a
    // model context.
    let flagged_total = flagged.len();
    flagged.truncate(10);
    Json(json!({
        "date": today,
        "jobs_completed": completed,
        "usdc_earned": (earned * 1e6).round() / 1e6,
        "by_service": by_service,
        "refund_review": flagged,
        "refund_review_total": flagged_total
    }))
}

fn ledger(app: &App, job_id: &str, service: &str, amount: f64, sig: &str, status: &str) {
    let rec = json!({
        "ts": Utc::now().to_rfc3339(),
        "job_id": job_id,
        "service": service,
        "amount_usdc": amount,
        "signature": sig,
        "status": status
    });
    let mut file = lock(&app.ledger);
    if let Err(e) = writeln!(file, "{rec}") {
        // The ledger is the money trail — losing a line is an operator-visible
        // incident, not a silent drop.
        tracing::error!(job_id, error = %e, "LEDGER WRITE FAILED — reconcile manually");
    }
}

/// Mutex poisoning means a panic elsewhere; the maps hold no invariants that
/// a panic could half-apply, so continuing with the inner value is safe and
/// beats taking the whole gateway down.
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn err(code: StatusCode, msg: &str) -> Response {
    (code, Json(json!({"status": "error", "error": msg}))).into_response()
}

fn err_with(code: StatusCode, msg: &str, extra: Value) -> Response {
    let mut body = json!({"status": "error", "error": msg});
    if let (Some(obj), Some(ex)) = (body.as_object_mut(), extra.as_object()) {
        for (k, v) in ex {
            obj.insert(k.clone(), v.clone());
        }
    }
    (code, Json(body)).into_response()
}
