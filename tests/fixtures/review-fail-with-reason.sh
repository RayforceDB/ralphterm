#!/usr/bin/env sh
set -eu
if [ -n "${RALPHTERM_OUTPUT_FILE:-}" ]; then
  prompt=$(cat "$RALPHTERM_PROMPT_FILE")
  driver_mode=1
else
  prompt=$(cat)
  driver_mode=0
fi
state_dir=.ralphterm/review-fail-with-reason
mkdir -p "$state_dir"
printf '%s\n' "$prompt" > "$state_dir/review-prompt.txt"
printf '%s\n' "$prompt" > review-prompt.txt
count_file="$state_dir/count.txt"
count=0
if [ -f "$count_file" ]; then
  count=$(cat "$count_file")
fi
count=$((count + 1))
printf '%s\n' "$count" > "$count_file"
printf '%s\n' "$count" > review-count.txt
if [ "$count" -eq 1 ]; then
  body="REVIEW_FAIL needs a better file"
else
  body="REVIEW_PASS"
fi
if [ "$driver_mode" = "1" ]; then
  {
    echo "<<<BEGIN>>>"
    printf '%s\n' "$body"
    echo "<<<END>>>"
  } > "$RALPHTERM_OUTPUT_FILE"
else
  printf '%s\n' "$body"
fi
