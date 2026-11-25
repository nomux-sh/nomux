#!/usr/bin/env bash
# Checks one release archive on a host that can run its binaries: each binary
# prints its name and the version, and the binaries of a musl target are
# static. The script also prints the size and the type of each binary.
#
# Usage: scripts/check-archive.sh <archive> <version>
set -euo pipefail

archive="$1"
version="$2"

name="$(basename "$archive" .tar.gz)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

tar -xzf "$archive" -C "$work"
cd "$work/$name"
ls -l

for binary in nomux nomuxd nomux-holder; do
  description="$(file -b "$binary")"
  echo "$binary: $description"
  if [ "$("./$binary" --version)" != "$binary $version" ]; then
    echo "error: $binary --version does not print '$binary $version'." >&2
    exit 1
  fi
  if [[ "$name" == *-linux-musl && ! "$description" =~ static(-pie|ally)\ linked ]]; then
    echo "error: $binary of $name is not a static binary." >&2
    exit 1
  fi
done
