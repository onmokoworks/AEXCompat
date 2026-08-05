#!/bin/sh
set -eu

if [ "$(uname -s)" != "Darwin" ]; then
  echo "package-macos-aex-carriers.sh requires macOS" >&2
  exit 2
fi

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
arm64_worker=${1:-"$root/guest/target/release/aex-guest-worker"}
native_worker=${2:-"$root/guest/target/x86_64-apple-darwin/release/aex-guest-worker"}
output=${3:-"$root/guest/target/aexcompat-macos-carriers.dmg"}
allow_adhoc=${AEXCOMPAT_ALLOW_ADHOC_PACKAGE:-0}
scratch=$(mktemp -d "${TMPDIR:-/tmp}/aexcompat-package.XXXXXX")
trap 'rm -rf "$scratch"' EXIT HUP INT TERM

if [ -e "$output" ]; then
  echo "refusing to replace existing package: $output" >&2
  exit 2
fi

verify_worker() {
  worker=$1
  architecture=$2

  if [ ! -f "$worker" ]; then
    echo "missing worker: $worker" >&2
    exit 2
  fi
  file "$worker" | grep -q "$architecture"
  codesign --verify --strict --verbose=2 "$worker"
  signature=$(codesign -dvv "$worker" 2>&1)
  echo "$signature" | grep -q 'flags=.*runtime'
  if echo "$signature" | grep -q '^Signature=adhoc$' && [ "$allow_adhoc" != "1" ]; then
    echo "refusing to package an ad-hoc signed worker: $worker" >&2
    echo "set AEXCOMPAT_ALLOW_ADHOC_PACKAGE=1 only for a local package smoke test" >&2
    exit 2
  fi
  if [ "$allow_adhoc" != "1" ]; then
    echo "$signature" | grep -q '^Authority=Developer ID Application:' || {
      echo "worker is not signed with Developer ID Application: $worker" >&2
      exit 2
    }
  fi
}

verify_worker "$arm64_worker" arm64
verify_worker "$native_worker" x86_64

payload="$scratch/AEXCompat Carriers"
mkdir -p "$payload/arm64" "$payload/x86_64"
ditto "$arm64_worker" "$payload/arm64/aex-guest-worker"
ditto "$native_worker" "$payload/x86_64/aex-guest-worker"

arm64_sha=$(shasum -a 256 "$payload/arm64/aex-guest-worker" | awk '{print $1}')
native_sha=$(shasum -a 256 "$payload/x86_64/aex-guest-worker" | awk '{print $1}')
arm64_size=$(stat -f %z "$payload/arm64/aex-guest-worker")
native_size=$(stat -f %z "$payload/x86_64/aex-guest-worker")

cat >"$payload/manifest.json" <<EOF
{
  "schema": "aexcompat-macos-carriers-v1",
  "workers": [
    {"architecture": "arm64", "backend": "unicorn", "path": "arm64/aex-guest-worker", "sha256": "$arm64_sha", "size": $arm64_size},
    {"architecture": "x86_64", "backend": "native-carrier-trusted-only", "path": "x86_64/aex-guest-worker", "sha256": "$native_sha", "size": $native_size}
  ]
}
EOF

mkdir -p "$(dirname -- "$output")"
hdiutil create -quiet -fs HFS+ -format UDZO -volname "AEXCompat Carriers" \
  -srcfolder "$payload" "$output"

echo "Created macOS carrier distribution: $output"
if [ "$allow_adhoc" = "1" ]; then
  echo "Local smoke package only: ad-hoc signatures are not eligible for notarization"
else
  echo "Submit, staple, and assess: $root/tools/notarize-macos-aex-carriers.sh '$output'"
fi
