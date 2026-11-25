#!/usr/bin/env bash
# Prints the version that all crates and binaries of the workspace share.
#
# Usage: scripts/workspace-version.sh
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

cargo metadata --no-deps --format-version 1 |
  jq -r '.packages[] | select(.name == "nomux-core") | .version'
