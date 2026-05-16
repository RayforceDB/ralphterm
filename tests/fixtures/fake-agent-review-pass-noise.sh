#!/usr/bin/env sh
set -eu
prompt=$(cat)
printf '%s\n' "$prompt" > fake-agent-last-prompt.txt
if printf '%s\n' "$prompt" | grep -q 'Write first.txt'; then
  printf 'created despite noisy transcript\n' > first.txt
fi
printf 'REVIEW_PASS\n'
printf 'COMPLETED\n'
