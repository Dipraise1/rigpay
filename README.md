# rigpay

**Turn any machine you own into a paid service.**

rigpay is a self-hosted toolkit that puts a [ZeroClaw](https://github.com/zeroclaw-labs/zeroclaw) agent in front of hardware you already run — and lets it sell that hardware's services for USDC on Solana, behind an x402 paywall.

- A **GPU rig** sells inference and render jobs
- A **NAS or homelab box** sells private encrypted storage
- A **fat internet connection** sells bandwidth / proxy access
- A **Raspberry Pi** sells sensor data feeds
- Anything with a CPU sells transcoding, backups, CI minutes…

You describe your services in one config file. The agent does the rest: quotes prices, answers requests with **HTTP 402 Payment Required**, verifies payment on-chain, dispatches the job to your machine, and reports revenue to your phone every day.

**The agent earns — it never spends.** That's the custody story, and it's non-negotiable.

Built for the ZeroClaw × Solana bounty (Superteam Brasil).

## How it works

```
client / another agent
        │  HTTP request
        ▼
┌───────────────────┐   402 + price + payment reference
│   rigpay gateway   │◀──────────────────────────────────┐
│  (one small binary)│                                    │
└─────────┬─────────┘   client pays USDC (Solana Pay)     │
          │             gateway sees it on-chain ─────────┘
          │  verified → dispatch
          ▼
   your service adapter (GPU / storage / bandwidth / …)
          │
          ▼
   ZeroClaw agent: daily revenue SOP → Telegram/WhatsApp
```

1. Client hits your endpoint → gets `402` with a USDC price and a fresh Solana Pay reference key.
2. Client pays. Verification is **read-only RPC** (`getSignaturesForAddress` on the reference key).
3. Gateway runs the service adapter, returns the result.
4. A ZeroClaw cron SOP reconciles daily: jobs run, USDC earned, anomalies flagged, delivered to the operator's chat.

## Define your services

One file: [`services.example.toml`](services.example.toml)

```toml
[operator]
receive_address = "YOUR_SOLANA_ADDRESS"   # where USDC lands. rigpay never holds this key.
usdc_mint      = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"

[[service]]
id      = "gpu-inference"
summary = "Stable Diffusion / LLM inference on RTX rig"
price   = 0.25          # USDC
unit    = "per_request"
adapter = "command"
command = "./adapters/gpu_infer.sh {input} {output}"

[[service]]
id      = "cold-storage"
summary = "Encrypted private storage, 30-day retention"
price   = 1.50
unit    = "per_gb_month"
adapter = "command"
command = "./adapters/store_blob.sh {input}"
```

Add a service = add a `[[service]]` block + an adapter script. See [`docs/adapters.md`](docs/adapters.md).

## Custody tier: T1 (receive-only)

- The agent and gateway hold **no private keys**. Payments arrive at your address; verification is read-only.
- Refunds are *proposed* by the agent and blocked on a human approval checkpoint (ZeroClaw SOP). The agent cannot sign — ever.
- Threat model + prompt-injection transcript: [`docs/threat-model.md`](docs/threat-model.md).

## ZeroClaw features used

- **Webhook channel** — job requests / 402 handshake surface
- **Telegram channel** — operator reports & refund approvals
- **Skills** — Solana Pay reference construction, price-menu shaping, client Q&A
- **SOP engine** — cron reconciliation, payment watch-loop, refund approval checkpoint
- **Memory** — client history, per-client pricing, rig availability

## Repo layout

```
gateway/     the 402 gateway (Rust)
adapters/    service adapter scripts (GPU, storage examples included)
agent/       ZeroClaw config, skills, SOPs — secrets redacted
docs/        setup guide, adapter guide, threat model, build log
```

## Status

🚧 Bounty build in progress — reference deployment: GPU inference + cold storage on the operator's own rigs. Follow `docs/buildlog.md`.

## Reproduce it

Target: **you set this up in an evening.** Stock ZeroClaw release binary, one gateway binary, your `services.toml`, your adapters. Step-by-step in [`docs/setup.md`](docs/setup.md).

## License

MIT
