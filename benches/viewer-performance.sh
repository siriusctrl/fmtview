#!/usr/bin/env bash
set -euo pipefail

samples=7

usage() {
  cat <<'USAGE'
Usage: benches/viewer-performance.sh [--samples N]

Runs fmtview viewer performance smoke tests in release mode and prints
per-sample timings plus median/min/max summaries.

The script unsets NO_COLOR for the benchmark process so terminal draw byte
counts include the true-color styling path.
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
    env -u NO_COLOR cargo test --release "$test_name" -- \
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

summarize_last_field() {
  local file="$1"
  local field="$2"
  local values="$tmpdir/field.$RANDOM"
  awk -v field="$field" '{ print $field }' "$file" | sort -n > "$values"
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
      printf "median=%d min=%d max=%d avg=%.1f", median, values[1], values[NR], sum / NR
    }
  ' "$values"
}

bench_one() {
  local label="$1"
  local test_name="$2"
  local pattern="$3"
  local result_file="$tmpdir/${test_name}.tsv"
  local line ms bytes background sample

  echo
  echo "== $label =="
  for sample in $(seq 1 "$samples"); do
    line="$(run_test "$test_name" "$pattern")"
    ms="$(printf '%s\n' "$line" | duration_ms)"
    bytes="$(printf '%s\n' "$line" | extract_field bytes)"
    background="$(printf '%s\n' "$line" | extract_field background_cells)"
    bytes="${bytes:-0}"
    background="${background:-0}"
    printf '%s\t%s\t%s\n' "$ms" "$bytes" "$background" >> "$result_file"
    printf 'sample %02d: %8.3fms  bytes=%s  background_cells=%s\n' \
      "$sample" "$ms" "$bytes" "$background"
  done

  printf 'time: '; cut -f1 "$result_file" | summarize /dev/stdin; echo
  if grep -qv $'\t0\t' "$result_file"; then
    printf 'bytes: '; summarize_last_field "$result_file" 2; echo
  fi
  printf 'background_cells: '; summarize_last_field "$result_file" 3; echo
}

echo "fmtview viewer performance smoke"
echo "samples: $samples"
echo "command env: NO_COLOR is unset for benchmark subprocesses"

bench_one \
  "viewport render CPU" \
  "perf_repeated_viewport_scroll_render" \
  "repeated viewport scroll render"

bench_one \
  "terminal draw bytes" \
  "perf_terminal_scroll_draw_bytes" \
  "terminal scroll draw"

bench_one \
  "terminal visual-row scroll bytes" \
  "perf_terminal_visual_row_scroll_bytes" \
  "terminal visual row scroll"
