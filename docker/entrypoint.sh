#!/usr/bin/env bash
# Entrypoint for the ralphterm docker isolation image.
#
# Receives the agent command + args from `docker run` and execs them with the
# inherited environment. Provides a single hook point for future setup work
# (signal forwarding, log redirection, etc.) without changing every callsite.

set -euo pipefail

if [ "$#" -eq 0 ]; then
    echo "ralphterm-entrypoint: no command supplied" >&2
    exit 2
fi

exec "$@"
