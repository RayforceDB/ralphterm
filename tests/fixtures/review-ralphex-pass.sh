#!/usr/bin/env sh
set -eu
prompt=$(cat)
printf '%s\n' "$prompt" > review-prompt.txt
printf 'Review: ralphex format\n'
printf '<<<RALPHEX:REVIEW_DONE>>>\n'
