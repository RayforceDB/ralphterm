#!/usr/bin/env sh
set -eu
prompt=$(cat)
printf '%s\n' "$prompt" > review-prompt.txt
printf 'Review: fail\n'
printf 'REVIEW_FAIL\n'
