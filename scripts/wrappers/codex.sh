#!/usr/bin/env sh
# RalphTerm wrapper for the OpenAI Codex CLI.
#
# Target CLI: codex (https://github.com/openai/codex)
# Auth env:
#   OPENAI_API_KEY - required for codex to authenticate against OpenAI APIs
# Optional env:
#   CLAUDE_MODEL      - forwarded as `--model <value>` so plans authored for
#                       ralphex's model selection knob keep working.
#   PROVIDER_OVERRIDE - override the binary name (mostly for tests).
#
# Behaviour: reads the prompt from stdin and invokes `codex exec
# "<prompt>"` (codex's non-interactive subcommand). Bare `codex` requires
# a TTY and refuses to read piped stdin, so the wrapper must use `exec`.
# Emits COMPLETED on success or FAILED on a non-zero exit.
set -eu

PROVIDER_CMD="${PROVIDER_OVERRIDE:-codex}"

prompt=$(cat)
if [ -z "${prompt}" ]; then
  printf 'FAILED: no prompt on stdin\n' >&2
  exit 1
fi

trap 'kill "${child:-0}" 2>/dev/null || true; exit 130' INT TERM

model_arg=""
if [ -n "${CLAUDE_MODEL:-}" ]; then
  model_arg="--model ${CLAUDE_MODEL}"
fi

# If PROVIDER_OVERRIDE is set, the test shim expects the prompt on stdin —
# stream it through instead of passing as argv so the shim's
# `prompt=$(cat)` pattern keeps working. Otherwise drive the real codex
# via its non-interactive `exec` subcommand with the prompt as the final
# argv, and close stdin so codex doesn't block waiting for input that
# never comes (the PTY stdin stays open otherwise).
if [ -n "${PROVIDER_OVERRIDE:-}" ]; then
  # shellcheck disable=SC2086
  printf '%s\n' "$prompt" | "$PROVIDER_CMD" $model_arg &
else
  # shellcheck disable=SC2086
  "$PROVIDER_CMD" exec $model_arg "$prompt" </dev/null &
fi
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
