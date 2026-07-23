# rende engineering standards

This is production infrastructure that sells services for money, not a demo.
Every change is held to these rules.

## Capacity assumptions (design for these numbers)
- Thousands of outstanding quotes at once (quote issuance is free for an
  attacker — it must be cheap for us and bounded).
- Hundreds of concurrent paid jobs across services; each service has its own
  concurrency budget (a GPU can't run 50 jobs at once — saturation returns
  429, never a queue that grows unbounded).
- The gateway must survive an RPC outage degraded (clear errors, no hangs) and
  restart cleanly (anything that must survive restarts lives on disk).

## Code rules
- **Bounded everything.** Payload sizes, outstanding quotes, per-service
  concurrency, adapter runtime, RPC timeouts. If a resource can grow, it has a
  cap and a test for the cap.
- **No `unwrap`/`expect`/`panic!` in the request path.** Startup may fail loudly;
  a request may not take the process down. Every failure returns typed JSON.
- **Custody invariant: this repo never holds, derives, or touches a private
  key.** Read-only RPC only. Any PR that adds signing is wrong by definition.
- **Failed paid jobs are flagged, never auto-refunded.** Refunds go through the
  human approval checkpoint. No code path may move money.
- **Shape all model-facing output.** Anything the ZeroClaw agent reads
  (reports, errors) stays ~100 tokens. Raw dumps into a model context are a bug.
- **Comments state constraints and invariants** — why a bound exists, what a
  caller may assume — never narration of what the next line does.

## Testing rules
- No live network in tests. On-chain behavior is tested against fixture JSON
  and a mock RPC server (`tests/`).
- The paid path must stay covered by the mock-RPC integration test — it cannot
  be manually re-verified without spending real money.
- `cargo test` green before every commit.

## Repo hygiene
- `STATUS.md` is the single source of truth for built/remaining — update it in
  the same commit as the work.
- `docs/buildlog.md` gets a dated line per working session (bounty tiebreak).
- Secrets never in git: real `services.toml`, keys, `.env` are gitignored.
