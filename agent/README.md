# rende ZeroClaw agent

The agent half of rende: Telegram front-of-house + two SOPs over the
gateway's API. Authored against ZeroClaw master, schema_version 3.

## Install (stock release binary, no plugins)

```sh
# 1. Merge the config (or copy wholesale if you have no ~/.zeroclaw yet)
cp agent/config.example.toml ~/.zeroclaw/config.toml

# 2. Skills and SOPs into the shared tree
mkdir -p ~/.zeroclaw/shared
cp -r agent/shared/skills ~/.zeroclaw/shared/
cp -r agent/shared/sops   ~/.zeroclaw/shared/

# 3. Agent identity file
mkdir -p ~/.zeroclaw/agents/rende/workspace
cp agent/workspace/AGENTS.md ~/.zeroclaw/agents/rende/workspace/

# 4. Secrets via env, never in the file
export ZEROCLAW_providers__models__anthropic__default__api_key="sk-ant-..."
export ZEROCLAW_channels__telegram__default__bot_token="123:ABC..."

# 5. Validate the SOPs, then run
zeroclaw sop validate
zeroclaw daemon
```

The gateway must be running on `127.0.0.1:4020` (see `docs/setup.md`).

## What's wired

| Piece | File | Purpose |
|---|---|---|
| Config | `config.example.toml` | agent + Telegram + least-privilege tools |
| Skill | `shared/skills/rende/rende-operator/SKILL.md` | reports, quotes, refund proposals |
| SOP | `shared/sops/daily-reconciliation/` | cron 21:00 → post day's earnings |
| SOP | `shared/sops/refund-review/` | cron 4h → **human checkpoint** → proposals |
| Identity | `workspace/AGENTS.md` | standing prompt-injection posture |

## Least privilege, spelled out

- `http_request` is allowlisted to `127.0.0.1` only (the gateway); `web_fetch`
  is disabled; `shell` is excluded by the risk profile.
- The agent has no RPC access and no keys — chain truth lives in the gateway.
- Refunds exist only as human-approved proposals; the checkpoint in
  `refund-review/SOP.md` (`kind: checkpoint`, `requires_confirmation: true`)
  is the enforcement point, and the prompt-injection transcript in
  `docs/threat-model.md` exercises exactly this boundary.
