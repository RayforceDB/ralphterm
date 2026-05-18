#!/usr/bin/env sh
set -eu
if [ -n "${RALPHTERM_OUTPUT_FILE:-}" ]; then
  driver_mode=1
else
  cat >/dev/null
  driver_mode=0
fi

printf 'created by large output agent\n' > first.txt

emit_body() {
  i=1
  while [ "$i" -le 200000 ]; do
    printf 'large-output-line-%06d ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcdefghijklmnopqrstuvwxyz\n' "$i"
    i=$((i + 1))
  done
  printf 'LARGE_OUTPUT_SENTINEL_COMPLETED\n'
}

if [ "$driver_mode" = "1" ]; then
  {
    echo "<<<BEGIN>>>"
    emit_body
    echo "ALL_TASKS_DONE"
    echo "<<<END>>>"
  } > "$RALPHTERM_OUTPUT_FILE"
else
  emit_body
  printf 'COMPLETED\n'
fi
