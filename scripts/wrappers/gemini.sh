#!/usr/bin/env sh
# RalphTerm wrapper for the Google Gemini CLI.
#
# Target CLI: gemini (https://github.com/google-gemini/gemini-cli)
# Auth env:
#   GEMINI_API_KEY - required for the Gemini CLI to authenticate against
#                    Google's Generative Language APIs.
# Optional env:
#   CLAUDE_MODEL  - forwarded as `--model <value>` so plans authored for
#                   ralphex's model selection knob keep working.
#   PROVIDER_OVERRIDE - override the binary name (mostly for tests).
#
# Behaviour: reads a single prompt from stdin, runs gemini interactively
# (no --print / -p / --non-interactive), then emits the COMPLETED marker
# on success or FAILED on a non-zero exit.
set -eu

PROVIDER_CMD="${PROVIDER_OVERRIDE:-gemini}"

prompt=$(cat)
if [ -z "${prompt}" ]; then
  printf 'FAILED: no prompt on stdin\n' >&2
  exit 1
fi

tmpfile=$(mktemp)
trap 'rm -f "$tmpfile"' EXIT
trap 'kill "${child:-0}" 2>/dev/null || true; exit 130' INT TERM

printf '%s\n' "$prompt" > "$tmpfile"

model_arg=""
if [ -n "${CLAUDE_MODEL:-}" ]; then
  model_arg="--model ${CLAUDE_MODEL}"
fi

# shellcheck disable=SC2086
"$PROVIDER_CMD" $model_arg < "$tmpfile" &
child=$!
set +e
wait "$child"
rc=$?
set -e
if [ "$rc" -eq 0 ]; then
  printf '\nCOMPLETED\n'
else
  printf '\nFAILED rc=%s\n' "$rc"
  exit "$rc"
fi
