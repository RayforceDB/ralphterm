#!/usr/bin/env sh
set -eu
if [ -n "${RALPHTERM_OUTPUT_FILE:-}" ]; then
  prompt=$(cat "$RALPHTERM_PROMPT_FILE")
else
  prompt=$(cat)
fi
printf '%s\n' "$prompt" > failing-agent-last-prompt.txt
printf 'agent saw prompt for task\n'
printf 'agent failure output before exit\n'
exit 42
