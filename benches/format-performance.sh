#!/usr/bin/env bash
set -euo pipefail

samples=7

usage() {
  cat <<'USAGE'
Usage: benches/format-performance.sh [--samples N]

Runs fmtview transform-layer performance smoke tests in release mode and
prints per-sample timings plus median/min/max summaries.

Use this before and after parser, formatter, JSONL transform, or future
parallel formatting changes. Use benches/load-performance.sh for lazy loading
and raw line-index checks.
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

bench_one() {
  local label="$1"
  local test_name="$2"
  local pattern="$3"
  local result_file="$tmpdir/${test_name}.tsv"
  local line ms records items string_bytes lines input_bytes output_bytes sample

  echo
  echo "== $label =="
  for sample in $(seq 1 "$samples"); do
    line="$(run_test "$test_name" "$pattern")"
    ms="$(printf '%s\n' "$line" | duration_ms)"
    records="$(printf '%s\n' "$line" | extract_field records)"
    items="$(printf '%s\n' "$line" | extract_field items)"
    string_bytes="$(printf '%s\n' "$line" | extract_field string_bytes)"
    lines="$(printf '%s\n' "$line" | extract_field lines)"
    input_bytes="$(printf '%s\n' "$line" | extract_field input_bytes)"
    output_bytes="$(printf '%s\n' "$line" | extract_field output_bytes)"
    records="${records:-0}"
    items="${items:-0}"
    string_bytes="${string_bytes:-0}"
    lines="${lines:-0}"
    input_bytes="${input_bytes:-0}"
    output_bytes="${output_bytes:-0}"
    printf '%s\n' "$ms" >> "$result_file"
    printf 'sample %02d: %8.3fms  records=%s  items=%s  string_bytes=%s  lines=%s  input_bytes=%s  output_bytes=%s\n' \
      "$sample" "$ms" "$records" "$items" "$string_bytes" "$lines" "$input_bytes" "$output_bytes"
  done

  printf 'time: '; summarize "$result_file"; echo
}

echo "fmtview transform performance smoke"
echo "samples: $samples"

bench_one \
  "jsonl record batch CPU" \
  "perf_jsonl_record_batch_format" \
  "jsonl record batch format"

bench_one \
  "jsonl source full format" \
  "perf_jsonl_source_full_format" \
  "jsonl source full format"

bench_one \
  "single huge object-array record format" \
  "perf_single_huge_object_array_record_format" \
  "single huge object-array record format"

bench_one \
  "single huge string field record format" \
  "perf_single_huge_string_field_record_format" \
  "single huge string field record format"
