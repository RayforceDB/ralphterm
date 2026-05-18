#!/usr/bin/env sh
# RalphTerm wrapper for the GitHub Copilot CLI.
#
# Target CLI: gh copilot suggest (https://cli.github.com/manual/gh_copilot)
# Auth env:
#   GH_TOKEN     - required for the gh CLI to authenticate against GitHub.
#                  Alternatively, an interactive `gh auth login` session works.
# Optional env:
#   CLAUDE_MODEL - forwarded as `--model <value>` so plans authored for
#                  ralphex's model selection knob keep working.
#   PROVIDER_OVERRIDE - override the binary name (mostly for tests).
#
# Behaviour: reads a single prompt from stdin, hands it to gh copilot in
# plain interactive mode (no --print / -p / --non-interactive), then emits
# the COMPLETED marker on success or FAILED on a non-zero exit.
set -eu

PROVIDER_CMD="${PROVIDER_OVERRIDE:-gh}"

if [ -n "${RALPHTERM_PROMPT_FILE:-}" ]; then
  prompt=$(cat "$RALPHTERM_PROMPT_FILE")
else
  prompt=$(cat)
fi

if [ -z "${prompt}" ]; then
  printf 'FAILED: no prompt provided\n' >&2
  exit 1
fi

tmpfile=$(mktemp)
provider_out=$(mktemp)
trap 'rm -f "$tmpfile" "$provider_out"' EXIT
trap 'kill "${child:-0}" 2>/dev/null || true; exit 130' INT TERM

printf '%s\n' "$prompt" > "$tmpfile"

model_arg=""
if [ -n "${CLAUDE_MODEL:-}" ]; then
  model_arg="--model ${CLAUDE_MODEL}"
fi

if [ "$PROVIDER_CMD" = "gh" ]; then
  # shellcheck disable=SC2086
  "$PROVIDER_CMD" copilot suggest $model_arg < "$tmpfile" > "$provider_out" 2>&1 &
else
  # shellcheck disable=SC2086
  "$PROVIDER_CMD" $model_arg < "$tmpfile" > "$provider_out" 2>&1 &
fi
child=$!
set +e
wait "$child"
rc=$?
set -e

cat "$provider_out"

if [ -n "${RALPHTERM_OUTPUT_FILE:-}" ]; then
  {
    printf '<<<BEGIN>>>\n'
    cat "$provider_out"
    printf '\n'
    if [ "$rc" -eq 0 ]; then
      printf 'REVIEW_PASS\n'
    else
      printf 'REVIEW_FAIL rc=%s\n' "$rc"
    fi
    printf '<<<END>>>\n'
  } > "$RALPHTERM_OUTPUT_FILE"
fi

if [ "$rc" -eq 0 ]; then
  printf '\nCOMPLETED\n'
else
  printf '\nFAILED rc=%s\n' "$rc"
  exit "$rc"
fi
