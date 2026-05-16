#!/usr/bin/env sh
set -eu
prompt=$(cat)
printf '%s\n' "$prompt" > slow-fake-agent-last-prompt.txt
sleep 2
if printf '%s\n' "$prompt" | grep -q 'Write first.txt'; then
  printf 'created by slow fake agent\n' > first.txt
fi
printf 'COMPLETED\n'
