# Minimal isolation image for ralphterm `--docker` mode.
#
# This image only needs the basics required to run an agent CLI (bash, git,
# network access). Heavier tooling (claude, codex, language toolchains) is
# expected to either be baked into a downstream image or mounted in via
# RALPHEX_EXTRA_VOLUMES.
FROM debian:stable-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        bash \
        ca-certificates \
        curl \
        git \
    && rm -rf /var/lib/apt/lists/*

COPY entrypoint.sh /usr/local/bin/ralphterm-entrypoint
RUN chmod +x /usr/local/bin/ralphterm-entrypoint

ENTRYPOINT ["/usr/local/bin/ralphterm-entrypoint"]
