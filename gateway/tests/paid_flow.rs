//! End-to-end coverage of the PAID path against a mock Solana RPC.
//!
//! This test exists because the paid path cannot be manually re-verified
//! without spending real USDC. The mock RPC is a switchable fixture: it starts
//! answering "no payments" and flips to "paid" mid-test, exactly like a real
//! reference key does.

use axum::routing::post;
use axum::{Json, Router};
use rende_gateway::{config, server};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

const OPERATOR: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
const MINT: &str = "So11111111111111111111111111111111111111112";

/// Mock RPC: `getSignaturesForAddress` returns one signature once `paid` is
/// flipped; `getTransaction` returns a tx whose token-balance delta pays the
/// operator exactly 0.01 USDC (10_000 micro-units).
fn mock_rpc(paid: Arc<AtomicBool>) -> Router {
    Router::new().route(
        "/",
        post(move |Json(req): Json<Value>| {
            let paid = paid.clone();
            async move {
                let result = match req["method"].as_str() {
                    Some("getSignaturesForAddress") => {
                        if paid.load(Ordering::SeqCst) {
                            json!([{"signature": "MockSig111", "err": null}])
                        } else {
                            json!([])
                        }
                    }
                    Some("getTransaction") => json!({
                        "meta": {
                            "err": null,
                            "preTokenBalances": [
                                {"owner": OPERATOR, "mint": MINT, "uiTokenAmount": {"amount": "0"}}
                            ],
                            "postTokenBalances": [
                                {"owner": OPERATOR, "mint": MINT, "uiTokenAmount": {"amount": "10000"}}
                            ]
                        }
                    }),
                    _ => Value::Null,
                };
                Json(json!({"jsonrpc": "2.0", "id": 1, "result": result}))
            }
        }),
    )
}

async fn serve(router: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    format!("http://{addr}")
}

fn test_config(rpc_url: &str, data_dir: &str) -> config::Config {
    let cfg: config::Config = toml::from_str(&format!(
        r#"
        [operator]
        receive_address = "{OPERATOR}"
        usdc_mint = "{MINT}"
        rpc_url = "{rpc_url}"
        quote_ttl_secs = 300

        [gateway]
        bind = "127.0.0.1:0"
        data_dir = "{data_dir}"

        [[service]]
        id = "echo"
        summary = "test"
        price = 0.01
        unit = "per_request"
        adapter = "command"
        command = "cat {{input}}"
        timeout_secs = 5
        max_concurrent = 2
        "#
    ))
    .unwrap();
    config::validate(&cfg).unwrap();
    cfg
}

#[tokio::test]
async fn full_x402_flow_unpaid_then_paid() {
    let paid = Arc::new(AtomicBool::new(false));
    let rpc_url = serve(mock_rpc(paid.clone())).await;
    let data_dir = std::env::temp_dir().join(format!("rende-it-{}", std::process::id()));
    let (router, _app) =
        server::build(test_config(&rpc_url, &data_dir.to_string_lossy())).unwrap();
    let base = serve(router).await;
    let http = reqwest::Client::new();

    // Catalog is served and shaped.
    let cat: Value = http.get(format!("{base}/services")).send().await.unwrap().json().await.unwrap();
    assert_eq!(cat["services"][0]["id"], "echo");

    // Phase 1: quote. Must be HTTP 402 with a Solana Pay URL.
    let resp = http.post(format!("{base}/jobs/echo")).send().await.unwrap();
    assert_eq!(resp.status(), 402);
    let quote: Value = resp.json().await.unwrap();
    let job_id = quote["job_id"].as_str().unwrap().to_string();
    assert!(quote["pay_url"].as_str().unwrap().starts_with("solana:"));

    // Phase 2 before paying: on-chain check finds nothing → still 402.
    let resp = http
        .post(format!("{base}/jobs/echo"))
        .header("X-Job-Id", &job_id)
        .body("hello rig")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 402);
    let unpaid: Value = resp.json().await.unwrap();
    assert_eq!(unpaid["error"], "unpaid");

    // The payment lands on-chain.
    paid.store(true, Ordering::SeqCst);

    // Phase 2 after paying: verified → adapter runs → result returned.
    let resp = http
        .post(format!("{base}/jobs/echo"))
        .header("X-Job-Id", &job_id)
        .body("hello rig")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let done: Value = resp.json().await.unwrap();
    assert_eq!(done["status"], "completed");
    assert_eq!(done["result"], "hello rig");
    assert_eq!(done["paid_signature"], "MockSig111");

    // Replay protection: the job id is consumed with the quote.
    let resp = http
        .post(format!("{base}/jobs/echo"))
        .header("X-Job-Id", &job_id)
        .body("again")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);

    // The day's report reflects the completed job, shaped small.
    let report: Value = http.get(format!("{base}/report/today")).send().await.unwrap().json().await.unwrap();
    assert_eq!(report["jobs_completed"], 1);
    assert_eq!(report["usdc_earned"], 0.01);
    assert_eq!(report["by_service"]["echo"], 1);
}

#[tokio::test]
async fn unknown_service_and_unknown_job_rejected() {
    let paid = Arc::new(AtomicBool::new(false));
    let rpc_url = serve(mock_rpc(paid)).await;
    let data_dir = std::env::temp_dir().join(format!("rende-it2-{}", std::process::id()));
    let (router, _app) =
        server::build(test_config(&rpc_url, &data_dir.to_string_lossy())).unwrap();
    let base = serve(router).await;
    let http = reqwest::Client::new();

    let resp = http.post(format!("{base}/jobs/nope")).send().await.unwrap();
    assert_eq!(resp.status(), 404);

    let resp = http
        .post(format!("{base}/jobs/echo"))
        .header("X-Job-Id", "job_forged")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}
