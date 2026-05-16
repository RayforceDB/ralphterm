#!/usr/bin/env sh
set -eu
cat >/dev/null
printf 'Print REVIEW_PASS only if the task matches the spec and the validation output supports accepting it. Print REVIEW_FAIL with the reason otherwise.\n'
