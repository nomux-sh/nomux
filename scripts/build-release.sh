#!/bin/sh
# Builds the three release binaries for one target, packages the archive and
# checks it. On Linux, the musl targets need a C compiler for musl, because
# a dependency of the SSH client compiles C code.
# Usage: scripts/build-release.sh <target>
set -eu

target=$1
version="$(scripts/workspace-version.sh)"

case "$target" in
  *-linux-musl)
    command -v musl-gcc >/dev/null 2>&1 || {
      echo "build-release.sh: musl-gcc is necessary for $target. Install the package musl-tools." >&2
      exit 1
    }
    variable="CC_$(echo "$target" | tr '-' '_')"
    export "$variable=musl-gcc"
    ;;
esac

rustup target add "$target"
cargo build --release --locked --target "$target" -p nomux-cli -p nomuxd -p nomux-holder
archive="$(scripts/package-archive.sh "$version" "$target")"
scripts/check-archive.sh "$archive" "$version"
