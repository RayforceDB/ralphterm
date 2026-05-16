#!/usr/bin/env sh
set -eu
prompt=$(cat)
printf '%s\n' "$prompt" > review-prompt.txt
count=0
if [ -f review-count.txt ]; then
  count=$(cat review-count.txt)
fi
count=$((count + 1))
printf '%s\n' "$count" > review-count.txt
if [ "$count" -eq 1 ]; then
  printf 'REVIEW_FAIL needs a better file\n'
else
  printf 'REVIEW_PASS\n'
fi
