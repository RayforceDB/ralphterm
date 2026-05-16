#!/usr/bin/env sh
set -eu
prompt=$(cat)
state_dir=.ralphterm/review-fail-twice
mkdir -p "$state_dir"
count_file="$state_dir/count.txt"
count=0
if [ -f "$count_file" ]; then
  count=$(cat "$count_file")
fi
count=$((count + 1))
printf '%s\n' "$count" > "$count_file"
printf '%s\n' "$count" > review-count.txt
printf '%s\n' "$prompt" > "$state_dir/review-prompt-$count.txt"
printf '%s\n' "$prompt" > "review-prompt-$count.txt"
if [ "$count" -le 2 ]; then
  printf 'Review: attempt %s needs another fix\n' "$count"
  printf 'REVIEW_FAIL\n'
else
  printf 'Review: third attempt accepted\n'
  printf 'REVIEW_PASS\n'
fi
