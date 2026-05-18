#!/usr/bin/env sh
set -eu
if [ -n "${RALPHTERM_OUTPUT_FILE:-}" ]; then
  prompt=$(cat "$RALPHTERM_PROMPT_FILE")
  driver_mode=1
else
  prompt=$(cat)
  driver_mode=0
fi
printf '%s\n' "$prompt" > staging-agent-last-prompt.txt
if printf '%s\n' "$prompt" | grep -q 'Stage generated.txt'; then
  printf 'staged by fake agent\n' > generated.txt
  git add generated.txt
fi
if [ "$driver_mode" = "1" ]; then
  {
    echo "<<<BEGIN>>>"
    echo "staging-agent done"
    echo "ALL_TASKS_DONE"
    echo "<<<END>>>"
  } > "$RALPHTERM_OUTPUT_FILE"
else
  printf 'COMPLETED\n'
fi
