#!/usr/bin/env sh
set -eu
prompt=$(cat)
printf '%s\n' "$prompt" > staging-agent-last-prompt.txt
if printf '%s\n' "$prompt" | grep -q 'Stage generated.txt'; then
  printf 'staged by fake agent\n' > generated.txt
  git add generated.txt
fi
printf 'COMPLETED\n'
