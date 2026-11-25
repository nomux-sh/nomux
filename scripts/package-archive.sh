#!/usr/bin/env bash
# Makes the release archive of one target and prints its path. The archive has
# one directory with the three binaries, README.md and the two license files.
#
# Build the binaries first:
#   cargo build --release --target <target> -p nomux-cli -p nomuxd -p nomux-holder
#
# Usage: scripts/package-archive.sh <version> <target>
# Example: scripts/package-archive.sh 0.1.0 x86_64-unknown-linux-musl
set -euo pipefail

version="$1"
target="$2"

cd "$(git rev-parse --show-toplevel)"

target_directory="${CARGO_TARGET_DIR:-target}"
binaries="$target_directory/$target/release"
output="$target_directory/dist"
name="nomux-$version-$target"

rm -rf "${output:?}/$name"
mkdir -p "$output/$name"
cp "$binaries/nomux" "$binaries/nomuxd" "$binaries/nomux-holder" "$output/$name/"
cp README.md LICENSE-APACHE LICENSE-MIT "$output/$name/"
tar -czf "$output/$name.tar.gz" -C "$output" "$name"
rm -rf "${output:?}/$name"

echo "$output/$name.tar.gz"
