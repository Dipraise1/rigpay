use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::time::Duration;

/// Per-attempt RPC timeout. Verification happens inside a client's HTTP
/// request, so a hung RPC must fail fast rather than pile up connections.
const RPC_TIMEOUT: Duration = Duration::from_secs(10);
/// One retry on transport errors only. JSON-RPC-level errors are not retried:
/// they are deterministic answers, not flakes.
const RPC_ATTEMPTS: usize = 2;
/// Signatures scanned per reference key. A reference is fresh per quote, so
/// legitimate traffic produces one or two entries; the bound exists so a
/// maliciously spammed reference can't make us fetch unbounded transactions.
const SIG_SCAN_LIMIT: usize = 20;

/// Read-only Solana RPC client. This is the entire on-chain surface of rigpay:
/// two methods, zero keys, zero signing.
pub struct Rpc {
    client: reqwest::Client,
    url: String,
}

impl Rpc {
    pub fn new(url: &str) -> Self {
        Self {
            // Building a client with a timeout cannot fail; fall back to the
            // default client rather than panicking at startup.
            client: reqwest::Client::builder()
                .timeout(RPC_TIMEOUT)
                .build()
                .unwrap_or_default(),
            url: url.to_string(),
        }
    }

    async fn call(&self, method: &str, params: Value) -> Result<Value> {
        let body = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
        let mut last_err = None;
        for attempt in 1..=RPC_ATTEMPTS {
            match self.client.post(&self.url).json(&body).send().await {
                Ok(resp) => {
                    let v: Value = resp.json().await.map_err(|e| anyhow!("rpc {method}: bad response body: {e}"))?;
                    if let Some(err) = v.get("error").filter(|e| !e.is_null()) {
                        // Deterministic RPC error — retrying would return the
                        // same answer. Surface it.
                        return Err(anyhow!("rpc {method} failed: {err}"));
                    }
                    return Ok(v["result"].clone());
                }
                Err(e) => {
                    tracing::warn!(method, attempt, error = %e, "rpc transport error");
                    last_err = Some(e);
                }
            }
        }
        Err(anyhow!("rpc {method}: transport failed after {RPC_ATTEMPTS} attempts: {}", last_err.map(|e| e.to_string()).unwrap_or_default()))
    }

    /// Solana Pay verification: the quote's reference pubkey appears in the
    /// paying transaction's account keys, so `getSignaturesForAddress` on the
    /// reference finds it. The amount is checked via token-balance deltas —
    /// no ATA derivation, no owner-account fetch, no trust in memo fields.
    pub async fn find_payment(
        &self,
        reference: &str,
        operator: &str,
        mint: &str,
        min_micros: u64,
    ) -> Result<Option<String>> {
        let sigs = self
            .call(
                "getSignaturesForAddress",
                json!([reference, {"limit": SIG_SCAN_LIMIT}]),
            )
            .await?;
        for entry in sigs.as_array().map(|v| v.as_slice()).unwrap_or(&[]) {
            if !entry["err"].is_null() {
                continue;
            }
            let Some(sig) = entry["signature"].as_str() else {
                continue;
            };
            let tx = self
                .call(
                    "getTransaction",
                    json!([sig, {
                        "encoding": "jsonParsed",
                        "commitment": "confirmed",
                        "maxSupportedTransactionVersion": 0
                    }]),
                )
                .await?;
            if tx.is_null() || !tx["meta"]["err"].is_null() {
                continue;
            }
            if received_micros(&tx["meta"], operator, mint) >= min_micros as i128 {
                return Ok(Some(sig.to_string()));
            }
        }
        Ok(None)
    }
}

/// Net amount of `mint` received by `owner` in this transaction, in raw units.
/// Uses pre/post token balances so it is correct regardless of which token
/// program, instruction shape, or intermediate hops moved the funds.
fn received_micros(meta: &Value, owner: &str, mint: &str) -> i128 {
    let sum = |balances: &Value| -> i128 {
        balances
            .as_array()
            .map(|v| v.as_slice())
            .unwrap_or(&[])
            .iter()
            .filter(|b| b["owner"] == owner && b["mint"] == mint)
            .filter_map(|b| b["uiTokenAmount"]["amount"].as_str())
            .filter_map(|a| a.parse::<i128>().ok())
            .sum()
    };
    sum(&meta["postTokenBalances"]) - sum(&meta["preTokenBalances"])
}

/// A Solana Pay reference is just a pubkey the wallet includes in the paying
/// transaction's account keys. We generate a keypair and keep only the public
/// half — the secret is dropped on the floor, on purpose: a reference that
/// *could* sign would violate the custody invariant.
pub fn new_reference() -> String {
    use rand::rngs::OsRng;
    let key = ed25519_dalek::SigningKey::generate(&mut OsRng);
    bs58::encode(key.verifying_key().to_bytes()).into_string()
}

pub fn pay_url(recipient: &str, amount_ui: f64, mint: &str, reference: &str, label: &str) -> String {
    format!(
        "solana:{recipient}?amount={amount_ui}&spl-token={mint}&reference={reference}&label={}",
        urlencode(label)
    )
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const OWNER: &str = "OpWa11et1111111111111111111111111111111111";
    const MINT: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";

    fn meta(pre: &str, post: &str) -> Value {
        json!({
            "preTokenBalances": [
                {"owner": OWNER, "mint": MINT, "uiTokenAmount": {"amount": pre}}
            ],
            "postTokenBalances": [
                {"owner": OWNER, "mint": MINT, "uiTokenAmount": {"amount": post}}
            ]
        })
    }

    #[test]
    fn payment_delta_counts_only_operator_and_mint() {
        assert_eq!(received_micros(&meta("1000000", "1250000"), OWNER, MINT), 250_000);
        assert_eq!(received_micros(&meta("1000000", "1250000"), "someone_else", MINT), 0);
        assert_eq!(received_micros(&meta("1000000", "1250000"), OWNER, "OtherMint"), 0);
    }

    #[test]
    fn outflow_is_negative_not_payment() {
        assert_eq!(received_micros(&meta("1250000", "1000000"), OWNER, MINT), -250_000);
    }

    #[test]
    fn empty_balances_mean_zero() {
        assert_eq!(received_micros(&json!({}), OWNER, MINT), 0);
    }

    #[test]
    fn reference_is_valid_base58_pubkey_and_unique() {
        let a = new_reference();
        let b = new_reference();
        assert_eq!(bs58::decode(&a).into_vec().unwrap().len(), 32);
        assert_ne!(a, b);
    }

    #[test]
    fn pay_url_shape() {
        let url = pay_url("Recip111", 0.25, MINT, "Ref111", "rigpay: gpu job");
        assert!(url.starts_with("solana:Recip111?amount=0.25&spl-token="));
        assert!(url.contains("reference=Ref111"));
        assert!(url.contains("label=rigpay%3A%20gpu%20job"));
    }
}
