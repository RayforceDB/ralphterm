#!/bin/sh
# RalphTerm one-line installer.
# Downloads and runs the cargo-dist-generated installer from the latest GitHub release.
set -eu
exec sh -c "$(curl -fsSL https://github.com/RayforceDB/ralphterm/releases/latest/download/ralphterm-installer.sh)" -- "$@"
