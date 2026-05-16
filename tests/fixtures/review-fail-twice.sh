#!/usr/bin/env sh
set -eu
prompt=$(cat)
count_file=review-count.txt
count=0
if [ -f "$count_file" ]; then
  count=$(cat "$count_file")
fi
count=$((count + 1))
printf '%s\n' "$count" > "$count_file"
printf '%s\n' "$prompt" > "review-prompt-$count.txt"
if [ "$count" -le 2 ]; then
  printf 'Review: attempt %s needs another fix\n' "$count"
  printf 'REVIEW_FAIL\n'
else
  printf 'Review: third attempt accepted\n'
  printf 'REVIEW_PASS\n'
fi
