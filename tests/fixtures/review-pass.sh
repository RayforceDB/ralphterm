#!/usr/bin/env sh
set -eu
prompt=$(cat)
printf '%s\n' "$prompt" > review-prompt.txt
printf 'Review: pass\n'
printf 'REVIEW_PASS\n'
