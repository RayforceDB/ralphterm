#!/usr/bin/env sh
set -eu
cat >/dev/null
printf 'created by large output agent\n' > first.txt
i=1
while [ "$i" -le 200000 ]; do
  printf 'large-output-line-%06d ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcdefghijklmnopqrstuvwxyz\n' "$i"
  i=$((i + 1))
done
printf 'LARGE_OUTPUT_SENTINEL_COMPLETED\n'
printf 'COMPLETED\n'
