#!/usr/bin/env bash
# Tests install.sh with local files and no network. The archives come from
# scripts/package-archive.sh, thus the test fails when the archive layout and
# the install script do not agree. The binaries in the archives are small
# shell scripts that print a version line. A `uname` replacement sets the
# platform that install.sh detects.
#
# Usage: scripts/test-install.sh
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

export CARGO_TARGET_DIR="$work/target"
export NOMUX_RELEASE_API_URL="file://$work/releases.json"
export NOMUX_RELEASE_DOWNLOAD_URL="file://$work/download"

mkdir "$work/bin"
cat > "$work/bin/uname" << 'EOF'
#!/bin/sh
case "$1" in
  -s) echo "$TEST_SYSTEM" ;;
  -m) echo "$TEST_MACHINE" ;;
esac
EOF
chmod +x "$work/bin/uname"
export PATH="$work/bin:$PATH"

checksum_tool=(sha256sum)
command -v sha256sum > /dev/null || checksum_tool=(shasum -a 256)

publish_release() {
  local version="$1" target="$2" binary archive
  mkdir -p "$CARGO_TARGET_DIR/$target/release" "$work/download/v$version"
  for binary in nomux nomuxd nomux-holder; do
    printf '#!/bin/sh\necho "%s %s %s"\n' "$binary" "$version" "$target" \
      > "$CARGO_TARGET_DIR/$target/release/$binary"
    chmod +x "$CARGO_TARGET_DIR/$target/release/$binary"
  done
  archive="$(scripts/package-archive.sh "$version" "$target")"
  mv "$archive" "$work/download/v$version/"
  (cd "$work/download/v$version" && "${checksum_tool[@]}" nomux-*.tar.gz > SHA256SUMS)
}

install_as() {
  TEST_SYSTEM="$1" TEST_MACHINE="$2" sh install.sh "${@:3}"
}

expect_installed() {
  local directory="$1" version="$2" target="$3" binary
  for binary in nomux nomuxd nomux-holder; do
    if [ "$("$directory/$binary")" != "$binary $version $target" ]; then
      echo "FAIL: $directory/$binary is not $binary $version for $target." >&2
      exit 1
    fi
  done
}

expect_failure() {
  local message="$1" directory="$2" output
  if output="$("${@:3}" 2>&1)"; then
    echo "FAIL: the command passed, but an error with '$message' was expected." >&2
    exit 1
  fi
  if ! grep -q "$message" <<< "$output"; then
    echo "FAIL: the error has no '$message': $output" >&2
    exit 1
  fi
  if [ -e "$directory" ]; then
    echo "FAIL: $directory exists after an install that failed." >&2
    exit 1
  fi
}

# The newest release in the list has a suffix, and 0.10.0 sorts before 0.9.0
# as text. The format is that of the GitHub REST API.
cat > "$work/releases.json" << 'EOF'
[
  {
    "html_url": "https://github.com/nomux-sh/nomux/releases/tag/v0.11.0-beta.1",
    "tag_name": "v0.11.0-beta.1",
    "prerelease": true
  },
  {
    "html_url": "https://github.com/nomux-sh/nomux/releases/tag/v0.9.0",
    "tag_name": "v0.9.0",
    "prerelease": true
  },
  {
    "html_url": "https://github.com/nomux-sh/nomux/releases/tag/v0.10.0",
    "tag_name": "v0.10.0",
    "prerelease": true
  }
]
EOF
publish_release 0.9.0 aarch64-apple-darwin
publish_release 0.10.0 x86_64-unknown-linux-musl
publish_release 0.10.0 aarch64-apple-darwin

echo "test: no --version installs the newest version without a suffix"
install_as Linux x86_64 --dir "$work/newest"
expect_installed "$work/newest" 0.10.0 x86_64-unknown-linux-musl

echo "test: --version installs that version for the detected target"
install_as Darwin arm64 --version v0.9.0 --dir "$work/pinned"
expect_installed "$work/pinned" 0.9.0 aarch64-apple-darwin

echo "test: a second install replaces the binaries"
install_as Darwin arm64 --dir "$work/pinned"
expect_installed "$work/pinned" 0.10.0 aarch64-apple-darwin

echo "test: the default directory is ~/.local/bin"
HOME="$work/home" install_as Linux amd64
expect_installed "$work/home/.local/bin" 0.10.0 x86_64-unknown-linux-musl

echo "test: an unsupported operating system fails"
expect_failure "FreeBSD is not supported" "$work/freebsd" \
  install_as FreeBSD x86_64 --dir "$work/freebsd"

echo "test: an unsupported processor type fails"
expect_failure "riscv64 is not supported" "$work/riscv64" \
  install_as Linux riscv64 --dir "$work/riscv64"

echo "test: a version that is not published fails"
expect_failure "Cannot download" "$work/missing" \
  install_as Linux x86_64 --version 0.8.0 --dir "$work/missing"

echo "test: a wrong checksum installs nothing"
echo "changed" >> "$work/download/v0.10.0/nomux-0.10.0-x86_64-unknown-linux-musl.tar.gz"
expect_failure "checksum of nomux-0.10.0-x86_64-unknown-linux-musl.tar.gz is wrong" "$work/corrupt" \
  install_as Linux x86_64 --dir "$work/corrupt"

echo "All install.sh tests passed."
