#!/bin/sh
# Installs the nomux binaries from a GitHub release: nomux, nomuxd and
# nomux-holder. The script supports Linux and macOS on x86_64 and aarch64.
# It does not use sudo.
#
# Usage: install.sh [--version <version>] [--dir <directory>]
#
#   --version <version>  The version to install, for example 0.1.0.
#                        Default: the newest version that has no suffix.
#   --dir <directory>    The directory for the binaries.
#                        Default: ~/.local/bin
#
# From the network:
#   curl -fsSL https://raw.githubusercontent.com/nomux-sh/nomux/main/install.sh | sh
#   curl -fsSL https://raw.githubusercontent.com/nomux-sh/nomux/main/install.sh | sh -s -- --version 0.1.0
#
# NOMUX_RELEASE_API_URL and NOMUX_RELEASE_DOWNLOAD_URL replace the two GitHub
# addresses. The tests of this script use them to read local files.
set -eu

BINARIES="nomux nomuxd nomux-holder"
RELEASE_API_URL="${NOMUX_RELEASE_API_URL:-https://api.github.com/repos/nomux-sh/nomux/releases?per_page=100}"
RELEASE_DOWNLOAD_URL="${NOMUX_RELEASE_DOWNLOAD_URL:-https://github.com/nomux-sh/nomux/releases/download}"

fail() {
  echo "install.sh: error: $1" >&2
  exit 1
}

usage() {
  echo "Usage: install.sh [--version <version>] [--dir <directory>]"
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || fail "The command '$1' is necessary. Install it, then run this script again."
}

detect_target() {
  system="$(uname -s)"
  machine="$(uname -m)"
  case "$machine" in
    x86_64 | amd64) processor=x86_64 ;;
    aarch64 | arm64) processor=aarch64 ;;
    *) fail "The processor type $machine is not supported. Supported: x86_64 and aarch64." ;;
  esac
  case "$system" in
    Linux) echo "$processor-unknown-linux-musl" ;;
    Darwin) echo "$processor-apple-darwin" ;;
    *) fail "The operating system $system is not supported. Supported: Linux and macOS." ;;
  esac
}

# A version with a suffix (0.2.0-beta.1) is not a candidate. The sort is
# numeric, thus 0.10.0 is newer than 0.9.0.
newest_version() {
  curl -fsSL "$RELEASE_API_URL" |
    tr ',' '\n' |
    sed -n 's/.*"tag_name": *"v\([0-9][0-9]*\.[0-9][0-9]*\.[0-9][0-9]*\)".*/\1/p' |
    sort -t . -k 1,1n -k 2,2n -k 3,3n |
    tail -n 1
}

download() {
  curl -fsSL -o "$2" "$1" || fail "Cannot download $1"
}

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{ print $1 }'
  else
    shasum -a 256 "$1" | awk '{ print $1 }'
  fi
}

verify_checksum() {
  archive="$1"
  sums="$2"
  expected="$(awk -v name="$(basename "$archive")" '$2 == name { print $1 }' "$sums")"
  [ -n "$expected" ] || fail "SHA256SUMS has no checksum for $(basename "$archive")."
  actual="$(sha256_of "$archive")"
  [ "$actual" = "$expected" ] ||
    fail "The checksum of $(basename "$archive") is wrong. Expected $expected, got $actual. Nothing was installed."
}

install_binaries() {
  source_directory="$1"
  directory="$2"
  mkdir -p "$directory"
  for binary in $BINARIES; do
    # A rename replaces a binary that runs. A copy to the same path fails then.
    cp "$source_directory/$binary" "$directory/$binary.new"
    chmod 755 "$directory/$binary.new"
    mv -f "$directory/$binary.new" "$directory/$binary"
  done
}

main() {
  version=""
  directory="${HOME:?}/.local/bin"
  while [ $# -gt 0 ]; do
    case "$1" in
      --version)
        [ $# -ge 2 ] || fail "The option --version needs a value."
        version="${2#v}"
        shift 2
        ;;
      --dir)
        [ $# -ge 2 ] || fail "The option --dir needs a value."
        directory="$2"
        shift 2
        ;;
      -h | --help)
        usage
        exit 0
        ;;
      *)
        usage >&2
        fail "Unknown argument: $1"
        ;;
    esac
  done

  require_command curl
  require_command tar
  command -v sha256sum >/dev/null 2>&1 || require_command shasum

  target="$(detect_target)"
  if [ -z "$version" ]; then
    version="$(newest_version)"
    [ -n "$version" ] || fail "Found no release at $RELEASE_API_URL"
  fi
  name="nomux-$version-$target"

  work="$(mktemp -d)"
  trap 'rm -rf "$work"' EXIT

  echo "Downloading $name.tar.gz"
  download "$RELEASE_DOWNLOAD_URL/v$version/$name.tar.gz" "$work/$name.tar.gz"
  download "$RELEASE_DOWNLOAD_URL/v$version/SHA256SUMS" "$work/SHA256SUMS"
  verify_checksum "$work/$name.tar.gz" "$work/SHA256SUMS"
  echo "The SHA-256 checksum is correct."

  tar -xzf "$work/$name.tar.gz" -C "$work"
  install_binaries "$work/$name" "$directory"
  echo "Installed nomux $version in $directory: $BINARIES"

  case ":$PATH:" in
    *":$directory:"*) ;;
    *) echo "$directory is not in PATH. Add it to PATH to run nomux by name." ;;
  esac
}

# The call is on the last line, thus a download that stops early runs nothing.
main "$@"
