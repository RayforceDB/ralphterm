#!/usr/bin/env sh
set -eu
if [ -n "${RALPHTERM_OUTPUT_FILE:-}" ]; then
  prompt=$(cat "$RALPHTERM_PROMPT_FILE")
  driver_mode=1
else
  prompt=$(cat)
  driver_mode=0
fi
printf '%s\n' "$prompt" > slow-fake-agent-last-prompt.txt
sleep 2
if printf '%s\n' "$prompt" | grep -q 'Write first.txt'; then
  printf 'created by slow fake agent\n' > first.txt
fi
if [ "$driver_mode" = "1" ]; then
  {
    echo "<<<BEGIN>>>"
    echo "slow-fake-agent finished"
    echo "ALL_TASKS_DONE"
    echo "<<<END>>>"
  } > "$RALPHTERM_OUTPUT_FILE"
else
  printf 'COMPLETED\n'
fi
