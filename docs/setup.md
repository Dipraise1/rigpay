# Setup

Target: running in an evening. Two pieces — the gateway (sells the services) and
the ZeroClaw agent (talks to you). The gateway works standalone; the agent adds
reporting and refund approvals.

## 1. Gateway

```sh
git clone https://github.com/Dipraise1/rende && cd rende
cp services.example.toml services.toml
# edit services.toml:
#   - operator.receive_address = your Solana address (rende never sees its key)
#   - operator.rpc_url         = your RPC (public mainnet works to start)
#   - one [[service]] block per thing you sell
cargo build --release --manifest-path gateway/Cargo.toml
./gateway/target/release/rende-gateway services.toml
```

Smoke it:

```sh
curl localhost:4020/services
curl -X POST localhost:4020/jobs/gpu-inference
# → HTTP 402 with a job_id and a solana: pay URL
# pay it from any wallet, then:
curl -X POST localhost:4020/jobs/gpu-inference -H "X-Job-Id: <job_id>" -d "your prompt"
# → gateway verifies the payment on-chain and runs your adapter
curl localhost:4020/report/today
# → compact daily summary (this is what the agent reads)
```

The gateway binds `127.0.0.1` by default. To sell publicly, front it with a
reverse proxy or tunnel (Caddy, cloudflared, Tailscale Funnel) — TLS is that
layer's job.

## 2. Adapters

Each `[[service]]` runs one executable per paid job — see
[adapters.md](adapters.md). The repo ships two references:

- `adapters/gpu_infer.sh` — forwards prompts to an Ollama endpoint (`INFER_URL`)
- `adapters/store_blob.sh` — encrypted blob storage with claim tickets
  (`STORAGE_AGE_RECIPIENT` for age encryption)

## 3. ZeroClaw agent

The agent never touches payments — it reads `GET /report/today` on a cron SOP
and delivers the summary to your chat channel, and holds the approval
checkpoint for refund reviews. Config, skills, and SOPs live in
[`agent/`](../agent/) (in progress).

## Custody notes

- The gateway holds **no keys**. `receive_address` is a plain address; payment
  verification is read-only RPC (`getSignaturesForAddress` + token-balance
  deltas on the quote's reference key).
- A paid job that fails is **never silently retried or refunded** — it lands in
  `refund_review` in the daily report, and refunds happen only through the
  agent's human-approval checkpoint.
- Quotes expire (`quote_ttl_secs`, default 300s); each quote's reference key is
  single-use.
