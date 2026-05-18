#!/usr/bin/env sh
set -eu
prompt=$(cat)
printf '%s\n' "$prompt" > review-prompt.txt
# Phase 1 / Phase 3 reviewers expect structured findings ("CRITICAL:" or
# "[CRITICAL]" or "Severity: critical"). Phase 2 (external/codex review)
# still uses the REVIEW_PASS / REVIEW_FAIL protocol. Emit both so this
# fixture works as a "fail" stand-in for any phase.
printf 'Severity: critical\n'
printf 'CRITICAL: missing logging in implementation\n'
printf 'REVIEW_FAIL\n'
