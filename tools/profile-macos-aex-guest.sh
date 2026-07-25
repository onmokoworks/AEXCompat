#!/bin/sh
set -eu

if [ "$(uname -s)" != "Darwin" ]; then
  echo "profile-macos-aex-guest.sh requires macOS" >&2
  exit 2
fi

if [ "$#" -lt 4 ]; then
  echo "usage: $0 <worker> <x64.aex> <input.png> <output-directory> [name=value ...]" >&2
  exit 2
fi

worker=$1
aex=$2
input=$3
output_directory=$4
shift 4

if [ ! -x "$worker" ]; then
  echo "worker is not executable: $worker" >&2
  exit 2
fi
if [ ! -f "$aex" ]; then
  echo "AEX does not exist: $aex" >&2
  exit 2
fi
if [ ! -f "$input" ]; then
  echo "input PNG does not exist: $input" >&2
  exit 2
fi

mkdir -p "$output_directory"
output_png="$output_directory/output.png"
report_json="$output_directory/report.json"
timing_txt="$output_directory/timing.txt"
sample_txt="$output_directory/sample.txt"
top_stacks_txt="$output_directory/top-stacks.txt"
identity_txt="$output_directory/identity.txt"

{
  sw_vers
  uname -a
  file "$worker"
  shasum -a 256 "$worker" "$aex" "$input"
  printf 'arguments:'
  printf ' %s' "$@"
  printf '\n'
} >"$identity_txt"

started_ns=$(python3 -c 'import time; print(time.monotonic_ns())')
"$worker" render-png "$aex" "$input" "$output_png" "$@" \
  >"$report_json" 2>"$timing_txt" &
render_pid=$!

# Sampling is external to the guest and does not install a Unicorn instruction
# or block hook. A short delay avoids profiling only process initialization.
sleep 0.2
if kill -0 "$render_pid" 2>/dev/null; then
  sample "$render_pid" 8 -file "$sample_txt" >/dev/null 2>&1 || true
fi

if ! wait "$render_pid"; then
  cat "$timing_txt" >&2
  exit 1
fi

finished_ns=$(python3 -c 'import time; print(time.monotonic_ns())')
python3 - "$started_ns" "$finished_ns" >>"$timing_txt" <<'PY'
import sys

started_ns, finished_ns = map(int, sys.argv[1:])
print(f"wall_seconds {(finished_ns - started_ns) / 1_000_000_000:.6f}")
PY

if [ -f "$sample_txt" ]; then
  awk '
    /Sort by top of stack/ { capture = 1; next }
    /Binary Images:/ { capture = 0 }
    capture { print }
  ' "$sample_txt" >"$top_stacks_txt"
fi

shasum -a 256 "$output_png" >>"$identity_txt"
echo "profile written to $output_directory"
