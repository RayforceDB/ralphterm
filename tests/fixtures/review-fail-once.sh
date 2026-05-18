#!/usr/bin/env sh
set -eu
if [ -n "${RALPHTERM_OUTPUT_FILE:-}" ]; then
  prompt=$(cat "$RALPHTERM_PROMPT_FILE")
  driver_mode=1
else
  prompt=$(cat)
  driver_mode=0
fi
state_dir=.ralphterm/review-fail-once
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
  verdict="REVIEW_FAIL"
  reason="Review: first attempt needs fix"
else
  verdict="REVIEW_PASS"
  reason="Review: retry accepted"
fi
if [ "$driver_mode" = "1" ]; then
  {
    echo "<<<BEGIN>>>"
    printf '%s\n' "$reason"
    printf '%s\n' "$verdict"
    echo "<<<END>>>"
  } > "$RALPHTERM_OUTPUT_FILE"
else
  printf '%s\n' "$reason"
  printf '%s\n' "$verdict"
fi
