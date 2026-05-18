#!/usr/bin/env sh
# RalphTerm wrapper for the OpenAI Codex CLI.
#
# Target CLI: codex (https://github.com/openai/codex)
# Auth env:
#   OPENAI_API_KEY - required for codex to authenticate against OpenAI APIs
# Optional env:
#   CLAUDE_MODEL      - forwarded as `--model <value>`.
#   PROVIDER_OVERRIDE - override the binary name (mostly for tests).
#
# IO contract: this wrapper supports both ralphterm's v0.3+ file-handoff
# (env vars RALPHTERM_PROMPT_FILE + RALPHTERM_OUTPUT_FILE) AND the
# legacy stdin/stdout streaming path. Drive_agent (v0.4+) sets the env
# vars; older callers pipe the prompt on stdin and watch for COMPLETED
# on stdout.
#
# The legacy `prompt=$(cat)` path used to deadlock against drive_agent
# because drive_agent's PTY writer never closes (it keeps the writer
# alive to send /exit on teardown). With RALPHTERM_PROMPT_FILE set we
# read the prompt from disk and never touch the PTY stdin.
set -eu

PROVIDER_CMD="${PROVIDER_OVERRIDE:-codex}"

if [ -n "${RALPHTERM_PROMPT_FILE:-}" ]; then
  prompt=$(cat "$RALPHTERM_PROMPT_FILE")
else
  prompt=$(cat)
fi

if [ -z "${prompt}" ]; then
  printf 'FAILED: no prompt provided\n' >&2
  exit 1
fi

trap 'kill "${child:-0}" 2>/dev/null || true; exit 130' INT TERM

model_arg=""
if [ -n "${CLAUDE_MODEL:-}" ]; then
  model_arg="--model ${CLAUDE_MODEL}"
fi

# Capture codex's output to a tempfile so we can both stream it to the
# PTY (for live transcripts) and write it into the file-handoff target.
codex_out=$(mktemp)
cleanup() { rm -f "$codex_out"; }
trap 'kill "${child:-0}" 2>/dev/null || true; cleanup; exit 130' INT TERM

if [ -n "${PROVIDER_OVERRIDE:-}" ]; then
  # shellcheck disable=SC2086
  printf '%s\n' "$prompt" | "$PROVIDER_CMD" $model_arg > "$codex_out" 2>&1 &
else
  # shellcheck disable=SC2086
  "$PROVIDER_CMD" exec $model_arg "$prompt" </dev/null > "$codex_out" 2>&1 &
fi
child=$!
set +e
wait "$child"
rc=$?
set -e

# Mirror codex's output to the PTY so the live transcript file (and
# any --serve dashboard watching the PTY) sees what codex said.
cat "$codex_out"

if [ -n "${RALPHTERM_OUTPUT_FILE:-}" ]; then
  # Write the file-handoff response. drive_agent's review decision
  # logic recognises REVIEW_PASS / NO ISSUES FOUND on success and
  # REVIEW_FAIL on failure (src/review_phases.rs::external_review_decision).
  {
    printf '<<<BEGIN>>>\n'
    cat "$codex_out"
    printf '\n'
    if [ "$rc" -eq 0 ]; then
      printf 'REVIEW_PASS\n'
    else
      printf 'REVIEW_FAIL rc=%s\n' "$rc"
    fi
    printf '<<<END>>>\n'
  } > "$RALPHTERM_OUTPUT_FILE"
fi

cleanup
if [ "$rc" -eq 0 ]; then
  printf '\nCOMPLETED\n'
else
  printf '\nFAILED rc=%s\n' "$rc"
  exit "$rc"
fi
