#!/usr/bin/env sh
set -eu
prompt=$(cat)
printf '%s\n' "$prompt" > review-prompt.txt
if [ -f external-state.txt ]; then
  printf 'Review: state present\n'
  printf 'REVIEW_PASS\n'
else
  printf 'Review: needs mutation\n'
  printf 'REVIEW_FAIL\n'
fi
