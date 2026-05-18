#!/usr/bin/env sh
set -eu
if [ -n "${RALPHTERM_OUTPUT_FILE:-}" ]; then
  prompt=$(cat "$RALPHTERM_PROMPT_FILE")
  driver_mode=1
else
  prompt=$(cat)
  driver_mode=0
fi
state_dir=.ralphterm/retry-after-review-agent
mkdir -p "$state_dir"
count_file="$state_dir/count.txt"
count=0
if [ -f "$count_file" ]; then
  count=$(cat "$count_file")
fi
count=$((count + 1))
printf '%s\n' "$count" > "$count_file"
printf '%s\n' "$count" > agent-count.txt
printf '%s\n' "$prompt" > "$state_dir/prompt-$count.txt"
printf '%s\n' "$prompt" > "agent-prompt-$count.txt"
if [ "$count" -eq 1 ]; then
  printf 'needs review fix\n' > first.txt
  printf 'rejected attempt only\n' > rejected.txt
else
  if ! printf '%s\n' "$prompt" | grep -q 'Previous review failed'; then
    printf 'retry prompt missing review feedback\n' >&2
    exit 42
  fi
  printf 'fixed after review\n' > first.txt
fi
if [ "$driver_mode" = "1" ]; then
  {
    echo "<<<BEGIN>>>"
    printf 'retry-after-review iteration=%s\n' "$count"
    echo "ALL_TASKS_DONE"
    echo "<<<END>>>"
  } > "$RALPHTERM_OUTPUT_FILE"
else
  printf 'COMPLETED\n'
fi
