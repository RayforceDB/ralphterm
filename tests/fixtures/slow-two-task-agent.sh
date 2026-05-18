#!/usr/bin/env sh
set -eu
if [ -n "${RALPHTERM_OUTPUT_FILE:-}" ]; then
  prompt=$(cat "$RALPHTERM_PROMPT_FILE")
  driver_mode=1
else
  prompt=$(cat)
  driver_mode=0
fi
printf '%s\n' "$prompt" > slow-two-task-agent-last-prompt.txt
if printf '%s\n' "$prompt" | grep -q 'Write first.txt'; then
  printf 'created first by slow fake agent\n' > first.txt
  sleep 1
fi
if printf '%s\n' "$prompt" | grep -q 'Write second.txt'; then
  printf 'created second by slow fake agent\n' > second.txt
fi
if [ "$driver_mode" = "1" ]; then
  {
    echo "<<<BEGIN>>>"
    echo "slow-two-task-agent iteration done"
    echo "ALL_TASKS_DONE"
    echo "<<<END>>>"
  } > "$RALPHTERM_OUTPUT_FILE"
else
  printf 'COMPLETED\n'
fi
