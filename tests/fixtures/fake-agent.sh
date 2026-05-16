#!/usr/bin/env sh
set -eu
prompt=$(cat)
printf '%s\n' "$prompt" > fake-agent-last-prompt.txt
if printf '%s\n' "$prompt" | grep -q 'Write first.txt'; then
  printf 'created by fake agent\n' > first.txt
fi
if printf '%s\n' "$prompt" | grep -q 'Write second.txt'; then
  printf 'created by fake agent\n' > second.txt
fi
printf 'COMPLETED\n'
