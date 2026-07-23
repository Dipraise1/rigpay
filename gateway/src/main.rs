mod adapter;
mod config;
mod solana;

use axum::body::Bytes;
use axum::extract::{Path as UrlPath, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{Local, Utc};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone)]
struct Quote {
    service_id: String,
    reference: String,
    amount_micros: u64,
    pay_url: String,
    expires_at: u64,
}

struct App {
    cfg: config::Config,
    rpc: solana::Rpc,
    quotes: Mutex<HashMap<String, Quote>>,
    data_dir: PathBuf,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cfg_path = std::env::args().nth(1).unwrap_or_else(|| "services.toml".into());
    let cfg = config::load(&cfg_path)?;
    let data_dir = PathBuf::from(&cfg.gateway.data_dir);
    std::fs::create_dir_all(&data_dir)?;

    let bind = cfg.gateway.bind.clone();
    let app = Arc::new(App {
        rpc: solana::Rpc::new(&cfg.operator.rpc_url),
        quotes: Mutex::new(HashMap::new()),
        data_dir,
        cfg,
    });

    let router = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/services", get(services))
        .route("/jobs/{service_id}", post(jobs))
        .route("/report/today", get(report_today))
        .with_state(app);

    println!("rigpay-gateway listening on {bind}");
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    axum::serve(listener, router).await?;
    Ok(())
}

/// Public catalog — shaped small on purpose (see bounty trap #3: never dump
/// raw data into a model's context).
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
/// Solana Pay URL. The client pays, then retries with X-Job-Id and the payload;
/// the gateway verifies the payment on-chain and runs the adapter.
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
        // Phase 1: issue a quote.
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
        let resp = json!({
            "status": "payment_required",
            "job_id": job_id,
            "service": service.id,
            "amount_usdc": service.price,
            "pay_url": quote.pay_url,
            "reference": reference,
            "expires_at": quote.expires_at,
            "next": "pay the URL, then POST again with header X-Job-Id"
        });
        app.quotes.lock().unwrap().insert(job_id, quote);
        return (StatusCode::PAYMENT_REQUIRED, Json(resp)).into_response();
    };

    // Phase 2: verify payment, then dispatch.
    let quote = match app.quotes.lock().unwrap().get(&job_id) {
        Some(q) if q.service_id == service.id => q.clone(),
        Some(_) => return err(StatusCode::BAD_REQUEST, "job_id belongs to a different service"),
        None => return err(StatusCode::NOT_FOUND, "unknown or already-completed job_id"),
    };
    if now() > quote.expires_at {
        app.quotes.lock().unwrap().remove(&job_id);
        return err(StatusCode::GONE, "quote expired — request a new one");
    }

    let paid = app
        .rpc
        .find_payment(
            &quote.reference,
            &app.cfg.operator.receive_address,
            &app.cfg.operator.usdc_mint,
            quote.amount_micros,
        )
        .await;
    let sig = match paid {
        Ok(Some(sig)) => sig,
        Ok(None) => {
            return (
                StatusCode::PAYMENT_REQUIRED,
                Json(json!({
                    "status": "unpaid",
                    "job_id": job_id,
                    "pay_url": quote.pay_url,
                    "expires_at": quote.expires_at
                })),
            )
                .into_response()
        }
        Err(e) => return err(StatusCode::BAD_GATEWAY, &format!("rpc error: {e}")),
    };

    match adapter::run(&service, &job_id, &app.data_dir, &body).await {
        Ok(result) => {
            app.quotes.lock().unwrap().remove(&job_id);
            ledger(&app, &job_id, &service.id, service.price, &sig, "completed");
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
            // Paid but failed: keep the quote so a retry doesn't double-charge,
            // and flag it for the operator's refund review.
            ledger(&app, &job_id, &service.id, service.price, &sig, "failed_refund_review");
            err(StatusCode::INTERNAL_SERVER_ERROR, &format!("job failed (payment recorded, flagged for refund review): {e}"))
        }
    }
}

/// Compact daily summary for the ZeroClaw reconciliation SOP — ~100 tokens,
/// never a raw dump.
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
    Json(json!({
        "date": today,
        "jobs_completed": completed,
        "usdc_earned": (earned * 1e6).round() / 1e6,
        "by_service": by_service,
        "refund_review": flagged
    }))
}

fn ledger(app: &App, job_id: &str, service: &str, amount: f64, sig: &str, status: &str) {
    use std::io::Write;
    let rec = json!({
        "ts": Utc::now().to_rfc3339(),
        "job_id": job_id,
        "service": service,
        "amount_usdc": amount,
        "signature": sig,
        "status": status
    });
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(app.data_dir.join("ledger.jsonl"))
    {
        let _ = writeln!(f, "{rec}");
    }
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
}

fn err(code: StatusCode, msg: &str) -> Response {
    (code, Json(json!({"status": "error", "error": msg}))).into_response()
}
