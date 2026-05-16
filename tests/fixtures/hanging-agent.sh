#!/usr/bin/env sh
set -eu
prompt=$(cat)
printf '%s\n' "$prompt" > hanging-agent-last-prompt.txt
printf 'still waiting for external input\n'
sleep 60
