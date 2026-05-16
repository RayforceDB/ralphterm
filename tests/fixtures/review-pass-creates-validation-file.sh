#!/usr/bin/env sh
set -eu
prompt=$(cat)
printf '%s\n' "$prompt" > review-prompt.txt
printf 'review approved before validation\n' > review-before-validation.txt
printf 'Review: pass\n'
printf 'REVIEW_PASS\n'
