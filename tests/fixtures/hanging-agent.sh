#!/usr/bin/env sh
set -eu
if [ -n "${RALPHTERM_OUTPUT_FILE:-}" ]; then
  prompt=$(cat "$RALPHTERM_PROMPT_FILE")
else
  prompt=$(cat)
fi
printf '%s\n' "$prompt" > hanging-agent-last-prompt.txt
printf 'still waiting for external input\n'
sleep 60
