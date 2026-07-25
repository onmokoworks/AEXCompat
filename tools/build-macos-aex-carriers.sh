#!/bin/sh
set -eu

if [ "$(uname -s)" != "Darwin" ]; then
  echo "build-macos-aex-carriers.sh requires macOS" >&2
  exit 2
fi

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)

cargo build \
  --release \
  --manifest-path "$root/guest/Cargo.toml" \
  -p aex-guest-worker

cargo build \
  --release \
  --target x86_64-apple-darwin \
  --features native-carrier \
  --manifest-path "$root/guest/Cargo.toml" \
  -p aex-guest-worker

echo "Unicorn fallback: $root/guest/target/release/aex-guest-worker"
echo "Native carrier:   $root/guest/target/x86_64-apple-darwin/release/aex-guest-worker"
