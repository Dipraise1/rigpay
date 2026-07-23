# Build log

- 2026-07-22: repo created, scoping the 402 gateway + ZeroClaw config.
- 2026-07-22: reframed as an operator toolkit — any machine, any service (GPU/storage/bandwidth), one services.toml + adapter scripts. Reference deployment stays GPU + storage on my own rigs.
- 2026-07-23: gateway v0.1 built and smoke-tested — x402 flow (402 quote → Solana Pay URL → live mainnet RPC verification), adapter runner with timeouts, jsonl ledger, shaped /report/today. 9 unit tests, zero keys held.
- 2026-07-23: gateway v0.2 — capacity hardening (bounded quotes + eviction, per-service concurrency, RPC retry/timeouts, tracing, graceful shutdown, config validation) and the mock-RPC paid-path integration test. 16/16 tests, clippy clean. Standards codified in CLAUDE.md.
- 2026-07-23: authored the full ZeroClaw agent composition against real master schema (cloned zeroclaw, extracted v3 config/skill/SOP formats): Telegram config, operator skill, daily-reconciliation + refund-review SOPs with human checkpoint, AGENTS.md injection posture, install guide.
- 2026-07-23: renamed the project rigpay → rende ('o dinheiro rende' — the money yields). GitHub repo, crate, configs, skills, SOPs all swept; 16/16 tests green under the new name.
