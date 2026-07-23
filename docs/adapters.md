# Writing a service adapter

An adapter is how rende hands a *paid* job to your machine. The gateway only
runs an adapter after payment is verified on-chain.

## The contract

An adapter is an executable (script or binary) that:

1. Receives the job input path as `{input}` and (optionally) writes results to `{output}`
2. Exits `0` on success — the gateway returns `{output}` (or stdout) to the client
3. Exits non-zero on failure — the client gets a job-failed response, and the
   failure lands in the operator's daily report as a refund candidate

Placeholders in the `command` string of your `[[service]]` block:

| Placeholder | Meaning                                    |
|-------------|--------------------------------------------|
| `{input}`   | path to the client's uploaded payload      |
| `{output}`  | path the gateway will read the result from |
| `{job_id}`  | unique job id (also the payment reference) |

## Rules

- **Adapters never see keys or payment data.** They receive a job, nothing else.
- **Bound your resources.** Set `timeout_secs`; the gateway kills overruns.
- **Shape your output.** If the result feeds back through the agent's chat
  channel, keep it small — don't return megabytes into a model context.
- **Fail loudly.** A non-zero exit with a one-line stderr message beats a
  silent hang.

## Example: minimal storage adapter

```bash
#!/usr/bin/env bash
# adapters/store_blob.sh — encrypt and stash a client blob, print a claim ticket
set -euo pipefail
input="$1"
ticket=$(uuidgen)
age -r "$STORAGE_RECIPIENT" -o "/srv/rende-store/$ticket.age" "$input"
echo "{\"ticket\": \"$ticket\", \"retention_days\": 30}"
```

That's the whole integration surface. If your service can be started from a
shell command, rende can sell it.
