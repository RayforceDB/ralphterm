#!/usr/bin/env sh
set -eu
# Phase 1 / Phase 3 reviewers expect structured findings ("CRITICAL:" or
# "[CRITICAL]" or "Severity: critical"). Phase 2 (external/codex review)
# still uses the REVIEW_PASS / REVIEW_FAIL protocol. Emit both so this
# fixture works as a "fail" stand-in for any phase.
if [ -n "${RALPHTERM_OUTPUT_FILE:-}" ]; then
  prompt=$(cat "$RALPHTERM_PROMPT_FILE")
  printf '%s\n' "$prompt" > review-prompt.txt
  {
    echo "<<<BEGIN>>>"
    echo "Severity: critical"
    echo "CRITICAL: missing logging in implementation"
    echo "REVIEW_FAIL"
    echo "<<<END>>>"
  } > "$RALPHTERM_OUTPUT_FILE"
else
  prompt=$(cat)
  printf '%s\n' "$prompt" > review-prompt.txt
  printf 'Severity: critical\n'
  printf 'CRITICAL: missing logging in implementation\n'
  printf 'REVIEW_FAIL\n'
fi
