# rigpay

**A self-hosted ZeroClaw agent that sells your idle GPU compute behind an x402 paywall on Solana — and reports the money to your phone.**

Built for the [ZeroClaw × Solana bounty](https://github.com/zeroclaw-labs/zeroclaw) (Superteam Brasil).

## The job

You own GPUs. They sit idle. `rigpay` puts a ZeroClaw agent in front of them:

1. A client (human or another agent) hits your inference/render endpoint.
2. They get an HTTP **402 Payment Required** with a USDC price on Solana.
3. They pay. The agent verifies settlement on-chain, dispatches the job to the rig, returns the result.
4. Every day, a cron SOP posts a revenue reconciliation to the operator's Telegram: jobs run, USDC earned, anomalies flagged.

The agent **earns** — it never spends. That's the custody story.

## Custody tier: T1 (receive-only)

- **No private keys held by the agent.** Payments arrive at a plain receiving address; verification is read-only RPC (`getSignaturesForAddress` on per-invoice reference keys).
- Refunds (if ever) are *proposed* by the agent and require a human approval checkpoint via ZeroClaw's SOP engine. The agent cannot sign.
- Threat model + prompt-injection transcript: see [`docs/threat-model.md`](docs/threat-model.md).

## ZeroClaw features used

- **Webhook channel** — inbound job requests / 402 handshake surface
- **Telegram channel** — operator reports & approvals
- **Skills** — Solana Pay reference-key construction, 402 response shaping, price menu
- **SOP engine** — cron reconciliation, payment-poll watch loop, approval checkpoint on refunds
- **Memory** — client history, pricing tiers, rig availability

## Architecture

```
client/agent ──HTTP──▶ [402 gateway] ──▶ ZeroClaw agent
                                             │
                       Solana RPC ◀──reads───┤  (T1: read-only)
                       GPU rig    ◀──job─────┤
                       Telegram   ◀──reports─┘
```

## Status

🚧 Bounty build in progress — follow along in `docs/buildlog.md`.

## Reproduce it

Goal: another operator sets this up in an evening. Full config (secrets redacted), SOPs, and skills will live in [`agent/`](agent/) with a step-by-step in [`docs/setup.md`](docs/setup.md).

## License

MIT
