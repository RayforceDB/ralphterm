#!/usr/bin/env sh
set -eu
prompt=$(cat)
printf '%s\n' "$prompt" > slow-two-task-agent-last-prompt.txt
if printf '%s\n' "$prompt" | grep -q 'Write first.txt'; then
  printf 'created first by slow fake agent\n' > first.txt
  sleep 1
fi
if printf '%s\n' "$prompt" | grep -q 'Write second.txt'; then
  printf 'created second by slow fake agent\n' > second.txt
fi
printf 'COMPLETED\n'
