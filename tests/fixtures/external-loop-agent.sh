#!/usr/bin/env sh
set -eu
if [ -n "${RALPHTERM_OUTPUT_FILE:-}" ]; then
  prompt=$(cat "$RALPHTERM_PROMPT_FILE")
  driver_mode=1
else
  prompt=$(cat)
  driver_mode=0
fi
state_dir=.ralphterm/external-loop-agent
mkdir -p "$state_dir"
count_file="$state_dir/count.txt"
count=0
if [ -f "$count_file" ]; then
  count=$(cat "$count_file")
fi
count=$((count + 1))
printf '%s\n' "$count" > "$count_file"
printf '%s\n' "$count" > external-agent-count.txt
printf '%s\n' "$prompt" > "external-agent-prompt-$count.txt"
printf 'iteration %s mutation\n' "$count" > external-state.txt
if [ "$driver_mode" = "1" ]; then
  {
    echo "<<<BEGIN>>>"
    printf 'external-loop iteration=%s done\n' "$count"
    echo "ALL_TASKS_DONE"
    echo "<<<END>>>"
  } > "$RALPHTERM_OUTPUT_FILE"
else
  printf 'COMPLETED\n'
fi
