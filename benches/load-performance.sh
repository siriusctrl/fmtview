#!/usr/bin/env bash
set -euo pipefail

samples=7

usage() {
  cat <<'USAGE'
Usage: benches/load-performance.sh [--samples N]

Runs fmtview load-layer performance smoke tests in release mode and prints
per-sample timings plus median/min/max summaries.

Use this before and after raw line indexing, window reads, load planning,
lazy record spooling, or preload behavior changes.

This script separates lazy record first-window work from lazy record preload
work. Use benches/format-performance.sh for the separate single huge record
transform baseline before experimenting with inline parser/formatter work.
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
  local test_name="$1"
  local pattern="$2"
  local output line

  output="$(
    cargo test --release "$test_name" -- \
      --ignored --nocapture --test-threads=1 2>&1
  )"
  line="$(printf '%s\n' "$output" | grep -E "$pattern" | tail -n 1 || true)"
  if [[ -z "$line" ]]; then
    printf '%s\n' "$output" >&2
    echo "failed to parse benchmark output for $test_name" >&2
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
  grep -oE "(^|[ ,])${field}=[0-9]+" | tail -n 1 | sed -E 's/.*=//' || true
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

bench_one() {
  local label="$1"
  local test_name="$2"
  local pattern="$3"
  local result_file="$tmpdir/${test_name}.tsv"
  local line ms records lines indexed_lines input_bytes window_lines sample

  echo
  echo "== $label =="
  for sample in $(seq 1 "$samples"); do
    line="$(run_test "$test_name" "$pattern")"
    ms="$(printf '%s\n' "$line" | duration_ms)"
    records="$(printf '%s\n' "$line" | extract_field records)"
    lines="$(printf '%s\n' "$line" | extract_field lines)"
    indexed_lines="$(printf '%s\n' "$line" | extract_field indexed_lines)"
    input_bytes="$(printf '%s\n' "$line" | extract_field input_bytes)"
    window_lines="$(printf '%s\n' "$line" | extract_field window_lines)"
    records="${records:-0}"
    lines="${lines:-0}"
    indexed_lines="${indexed_lines:-0}"
    input_bytes="${input_bytes:-0}"
    window_lines="${window_lines:-0}"
    printf '%s\n' "$ms" >> "$result_file"
    printf 'sample %02d: %8.3fms  records=%s  lines=%s  indexed_lines=%s  window_lines=%s  input_bytes=%s\n' \
      "$sample" "$ms" "$records" "$lines" "$indexed_lines" "$window_lines" "$input_bytes"
  done

  printf 'time: '; summarize "$result_file"; echo
}

echo "fmtview load performance smoke"
echo "samples: $samples"

bench_one \
  "raw indexed load" \
  "perf_raw_indexed_load" \
  "raw indexed load"

bench_one \
  "lazy record first-window load+transform" \
  "perf_lazy_first_window_format" \
  "lazy first window format"

bench_one \
  "lazy record preload load+transform" \
  "perf_lazy_preload_records_format" \
  "lazy preload records format"

bench_one \
  "lazy huge string first-window load+transform" \
  "perf_lazy_huge_string_first_window_format" \
  "lazy huge string first window format"
