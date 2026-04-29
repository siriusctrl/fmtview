#!/usr/bin/env bash
set -euo pipefail

samples=7

usage() {
  cat <<'USAGE'
Usage: benches/syntax-performance.sh [--samples N]

Runs fmtview syntax-layer performance smoke tests in release mode and prints
per-sample timings plus median/min/max summaries.

Use this before and after highlighter, syntax checkpoint, or visible-window
span generation changes.
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --samples)
      samples="${2:-}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if ! [[ "$samples" =~ ^[0-9]+$ ]] || [[ "$samples" -lt 1 ]]; then
  echo "--samples must be a positive integer" >&2
  exit 2
fi

cd "$(dirname "$0")/.."

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

run_test() {
  local output line

  output="$(
    cargo test --release perf_syntax_highlight_window -- \
      --ignored --nocapture --test-threads=1 2>&1
  )"
  line="$(printf '%s\n' "$output" | grep -E "syntax highlight window" | tail -n 1 || true)"
  if [[ -z "$line" ]]; then
    printf '%s\n' "$output" >&2
    echo "failed to parse benchmark output for perf_syntax_highlight_window" >&2
    exit 1
  fi
  printf '%s\n' "$line"
}

duration_ms() {
  sed -E 's/.*: ([0-9.]+)(ns|µs|us|ms|s),.*/\1 \2/' | awk '
    $2 == "ns" { printf "%.6f", $1 / 1000000; next }
    $2 == "µs" || $2 == "us" { printf "%.6f", $1 / 1000; next }
    $2 == "ms" { printf "%.6f", $1; next }
    $2 == "s" { printf "%.6f", $1 * 1000; next }
  '
}

extract_field() {
  local field="$1"
  sed -nE "s/.*${field}=([0-9]+).*/\\1/p"
}

summarize() {
  local file="$1"
  local sorted="$tmpdir/summary.$RANDOM"
  sort -n "$file" > "$sorted"
  awk '
    {
      values[NR] = $1
      sum += $1
    }
    END {
      if (NR == 0) {
        exit 1
      }
      mid = int((NR + 1) / 2)
      if (NR % 2 == 1) {
        median = values[mid]
      } else {
        median = (values[mid] + values[mid + 1]) / 2
      }
      printf "median=%.3fms min=%.3fms max=%.3fms avg=%.3fms", median, values[1], values[NR], sum / NR
    }
  ' "$sorted"
}

result_file="$tmpdir/syntax.tsv"

echo "fmtview syntax performance smoke"
echo "samples: $samples"
echo
echo "== syntax highlight window =="
for sample in $(seq 1 "$samples"); do
  line="$(run_test)"
  ms="$(printf '%s\n' "$line" | duration_ms)"
  windows="$(printf '%s\n' "$line" | extract_field windows)"
  input_bytes="$(printf '%s\n' "$line" | extract_field input_bytes)"
  spans="$(printf '%s\n' "$line" | extract_field spans)"
  printf '%s\n' "$ms" >> "$result_file"
  printf 'sample %02d: %8.3fms  windows=%s  input_bytes=%s  spans=%s\n' \
    "$sample" "$ms" "${windows:-0}" "${input_bytes:-0}" "${spans:-0}"
done

printf 'time: '; summarize "$result_file"; echo
