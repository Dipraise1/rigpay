# Status

Bounty: ZeroClaw × Solana (Superteam Brasil). Deadline-driven build; this file
is the single source of truth for where the project stands.

## ✅ Built

### Gateway (`gateway/`) — v0.1, working
- [x] Rust binary (axum + tokio), compiles clean, **9/9 unit tests passing**
- [x] `services.toml` catalog: operator address, RPC, per-service price/unit/command
- [x] x402 flow at `POST /jobs/{service}`:
  - no `X-Job-Id` → **HTTP 402** + quote (job_id, USDC price, Solana Pay URL, single-use reference key, TTL)
  - with `X-Job-Id` → on-chain payment verification → adapter dispatch → result
- [x] Payment verification, **read-only, zero keys**: `getSignaturesForAddress`
  on the reference key + token-balance deltas (no ATA derivation, no signing)
- [x] Adapter runner: `sh -c` with hard timeout, kill-on-drop, stderr surfaced
- [x] Failed-but-paid jobs → `refund_review` flag; never auto-refunded
- [x] Ledger (`data/ledger.jsonl`) + `GET /report/today` shaped to ~100 tokens for the agent
- [x] Quote expiry (default 300 s), unknown/cross-service job rejection
- [x] **Smoke-tested live against mainnet RPC**: 402 quote issued, unpaid retry
  correctly detected on-chain, unknown job → 404

### Adapters (`adapters/`)
- [x] `gpu_infer.sh` — Ollama-backed inference (INFER_URL / INFER_MODEL)
- [x] `store_blob.sh` — blob storage with claim tickets, age encryption via STORAGE_AGE_RECIPIENT

### Docs
- [x] README (pitch, architecture, custody tier)
- [x] `docs/setup.md` — evening-reproducibility guide
- [x] `docs/adapters.md` — adapter contract
- [x] `docs/buildlog.md` — build-in-public log (bounty tiebreak)

## 🚧 Left to build

### 1. Real-payment test (next, blocks everything downstream)
- [ ] Point `services.toml` at a real receive address, pay a quote with a real
  wallet, confirm paid path end-to-end (verify → dispatch → ledger → report)

### 2. ZeroClaw agent (`agent/`) — the other half of the product
- [ ] Install ZeroClaw release binary; base agent on **Telegram channel**
- [ ] Skill: rigpay operator — answer "how's business", read `/report/today`,
  explain quotes/services to customers
- [ ] SOP: daily reconciliation cron → post summary to operator chat
- [ ] SOP: refund review — when `refund_review` is non-empty, open an
  **approval checkpoint**; a human approves before any refund is proposed
- [ ] Memory: client history, pricing notes, rig availability

### 3. Wire the real rigs (reference deployment — "are YOU running it")
- [ ] Gaming PC / R720: Ollama (or SD) behind `gpu_infer.sh` via the rig tunnel
- [ ] Storage service live on the R720
- [ ] Public exposure: cloudflared / Tailscale Funnel in front of the gateway
- [ ] Run it daily; keep the ledger growing (judges score real usage)

### 4. Safety deliverables (25% of score)
- [ ] `docs/threat-model.md`: custody tier (T1), trust boundaries, what the
  agent can and cannot do
- [ ] **Prompt-injection transcript**: customer message tries to social-engineer
  a refund to an attacker address → checkpoint fails closed. Verbatim log.

### 5. Showcase (the actual submission)
- [ ] ≤3-min video: phone pays a quote → job runs on the rig → agent posts
  daily report to Telegram
- [ ] Write-up for #solana-bounty Discord: what/who/features/custody/repro links
- [ ] Build-in-public posts on X (tiebreak)

### Stretch (only if time remains)
- [ ] Durable-nonce refund proposals (bounty "worth points")
- [ ] Tiered price menu in the 402 response (negotiation — open territory per brief)
- [ ] Per-client memory-driven pricing in the agent

## Won't do
- Holding any private key in the agent or gateway (T1 is the product)
- Auto-refunds without human approval
- Trading/sniping anything (instant disqualification)
- Registry PRs during the bounty (rules)
