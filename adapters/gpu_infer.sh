#!/usr/bin/env bash
# gpu_infer.sh <input> <output>
# Reference GPU adapter: forwards the paid job's prompt to a local inference
# server (Ollama by default) and writes the completion to {output}.
# Point INFER_URL at your own rig's endpoint.
set -euo pipefail

input="$1"
output="$2"
INFER_URL="${INFER_URL:-http://127.0.0.1:11434/api/generate}"
INFER_MODEL="${INFER_MODEL:-llama3.2}"

prompt=$(cat "$input")

curl -sf --max-time 100 "$INFER_URL" \
  -d "$(jq -n --arg m "$INFER_MODEL" --arg p "$prompt" \
        '{model: $m, prompt: $p, stream: false}')" \
  | jq -r '.response' > "$output"

# Never return an empty result for a paid job — fail instead so the gateway
# flags it for refund review.
[ -s "$output" ] || { echo "empty inference result" >&2; exit 1; }
