#!/usr/bin/env sh
set -eu
prompt=$(cat)
printf '%s\n' "$prompt" > failing-agent-last-prompt.txt
printf 'agent saw prompt for task\n'
printf 'agent failure output before exit\n'
exit 42
