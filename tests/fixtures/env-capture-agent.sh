#!/usr/bin/env sh
# env-capture-agent.sh — like fake-agent.sh but additionally captures the
# CLAUDE_MODEL / CLAUDE_REVIEW_MODEL environment variables to side files.
# Dual-mode: v0.3 driver via env vars, or legacy stdin pipe.
set -eu

if [ -n "${RALPHTERM_OUTPUT_FILE:-}" ]; then
  prompt=$(cat "$RALPHTERM_PROMPT_FILE")
  driver_mode=1
else
  prompt=$(cat)
  driver_mode=0
fi
printf '%s\n' "${CLAUDE_MODEL-}" > claude-model.txt
printf '%s\n' "${CLAUDE_REVIEW_MODEL-}" > claude-review-model.txt

plan_file=$(printf '%s' "$prompt" | grep -oE 'Read the plan file at [^[:space:]]+' | head -1 | sed 's/.*at //' | sed 's/[.,;:]*$//')
if [ -z "${plan_file:-}" ] || [ ! -f "$plan_file" ]; then
  if [ "$driver_mode" = "1" ]; then
    { echo "<<<BEGIN>>>"; echo "FAILED: could not locate plan file"; echo "<<<END>>>"; } > "$RALPHTERM_OUTPUT_FILE"
  else
    printf 'FAILED: could not locate plan file\n'
  fi
  exit 1
fi

task_line=$(grep -nE '^- \[ \]' "$plan_file" | head -1 || true)
if [ -z "$task_line" ]; then
  if [ "$driver_mode" = "1" ]; then
    { echo "<<<BEGIN>>>"; echo "ALL_TASKS_DONE"; echo "<<<END>>>"; } > "$RALPHTERM_OUTPUT_FILE"
  else
    printf 'ALL_TASKS_DONE\n'
  fi
  exit 0
fi

line_num=$(printf '%s' "$task_line" | cut -d: -f1)
task_text=$(printf '%s' "$task_line" | cut -d: -f2-)

if printf '%s' "$task_text" | grep -q 'Write first.txt'; then
  printf 'created by env capture agent\n' > first.txt
fi

tmp=$(mktemp)
awk -v ln="$line_num" 'NR==ln { sub(/- \[ \]/, "- [x]"); print; next } { print }' "$plan_file" > "$tmp"
mv "$tmp" "$plan_file"

remaining=$(grep -cE '^- \[ \]' "$plan_file" || true)
if [ "$driver_mode" = "1" ]; then
  {
    echo "<<<BEGIN>>>"
    printf 'env-capture-agent processed line %s\n' "$line_num"
    if [ "${remaining:-0}" -eq 0 ]; then
      echo "ALL_TASKS_DONE"
    fi
    echo "<<<END>>>"
  } > "$RALPHTERM_OUTPUT_FILE"
else
  if [ "${remaining:-0}" -eq 0 ]; then
    printf 'ALL_TASKS_DONE\n'
  fi
fi
