#!/usr/bin/env sh
set -eu
prompt=$(cat)
printf '%s\n' "$prompt" > review-prompt.txt
count_file=review-count.txt
count=0
if [ -f "$count_file" ]; then
  count=$(cat "$count_file")
fi
count=$((count + 1))
printf '%s\n' "$count" > "$count_file"
if [ "$count" -eq 1 ]; then
  printf 'Review: first attempt needs fix\n'
  printf 'REVIEW_FAIL\n'
else
  printf 'Review: retry accepted\n'
  printf 'REVIEW_PASS\n'
fi
