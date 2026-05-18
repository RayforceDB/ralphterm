#!/usr/bin/env sh
# fake-agent.sh — dual-mode fixture.
#
# When run through ralphterm's v0.3 agent_driver (RALPHTERM_OUTPUT_FILE
# is set), follows the file-handoff contract: reads the prompt from
# $RALPHTERM_PROMPT_FILE and writes its account between
# <<<BEGIN>>>/<<<END>>> markers into $RALPHTERM_OUTPUT_FILE.
#
# When run through the legacy `ralphterm smoke` path (no env vars set),
# reads the prompt from stdin and prints ALL_TASKS_DONE to stdout.
set -eu

if [ -n "${RALPHTERM_OUTPUT_FILE:-}" ]; then
  prompt=$(cat "$RALPHTERM_PROMPT_FILE")
  driver_mode=1
else
  prompt=$(cat)
  driver_mode=0
fi

plan_file=$(printf '%s' "$prompt" | grep -oE 'Read the plan file at [^[:space:]]+' | head -1 | sed 's/.*at //' | sed 's/[.,;:]*$//')
if [ -z "${plan_file:-}" ]; then
  if [ -n "${1:-}" ]; then
    prompt="$1"
    plan_file=$(printf '%s' "$prompt" | grep -oE 'Read the plan file at [^[:space:]]+' | head -1 | sed 's/.*at //' | sed 's/[.,;:]*$//')
  fi
fi

emit_result() {
  body=$1
  done_signal=$2
  if [ "$driver_mode" = "1" ]; then
    {
      echo "<<<BEGIN>>>"
      printf '%s\n' "$body"
      if [ "$done_signal" = "1" ]; then
        echo "All checkboxes in the plan are now complete."
        echo "ALL_TASKS_DONE"
      fi
      echo "<<<END>>>"
    } > "$RALPHTERM_OUTPUT_FILE"
  else
    printf '%s\n' "$body"
    if [ "$done_signal" = "1" ]; then
      printf 'All checkboxes in the plan are now complete.\n'
      printf 'ALL_TASKS_DONE\n'
    fi
  fi
}

if [ -z "${plan_file:-}" ]; then
  emit_result "FAILED: could not find plan path in prompt" 0
  exit 1
fi
if [ ! -f "$plan_file" ]; then
  emit_result "FAILED: plan file does not exist: $plan_file" 0
  exit 1
fi

task_line=$(grep -nE '^- \[ \]' "$plan_file" | head -1 || true)
if [ -z "$task_line" ]; then
  emit_result "" 1
  exit 0
fi

line_num=$(printf '%s' "$task_line" | cut -d: -f1)
task_text=$(printf '%s' "$task_line" | cut -d: -f2-)

if printf '%s' "$task_text" | grep -q 'Write first.txt'; then
  printf 'created by fake agent\n' > first.txt
elif printf '%s' "$task_text" | grep -q 'Write second.txt'; then
  printf 'created by fake agent\n' > second.txt
elif printf '%s' "$task_text" | grep -q 'Write nested/generated.txt'; then
  mkdir -p nested
  printf 'nested content from fake agent\n' > nested/generated.txt
elif printf '%s' "$task_text" | grep -q 'Change tracked.txt'; then
  printf 'run-change\n' > tracked.txt
elif printf '%s' "$task_text" | grep -q 'Recreate tracked.txt with base content'; then
  printf 'base\n' > tracked.txt
elif printf '%s' "$task_text" | grep -q 'Create a file named hello.txt'; then
  printf 'hi' > hello.txt
fi

tmp=$(mktemp)
awk -v ln="$line_num" 'NR==ln { sub(/- \[ \]/, "- [x]"); print; next } { print }' "$plan_file" > "$tmp"
mv "$tmp" "$plan_file"

remaining=$(grep -cE '^- \[ \]' "$plan_file" || true)
body=$(printf 'Marked task at line %s as complete.' "$line_num")
if [ "${remaining:-0}" -eq 0 ]; then
  emit_result "$body" 1
else
  emit_result "$body" 0
fi
