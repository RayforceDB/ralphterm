#!/usr/bin/env sh
set -eu
prompt=$(cat)
printf '%s\n' "$prompt" >/dev/null
printf 'COMPLETED\n'
