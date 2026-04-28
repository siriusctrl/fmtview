#!/usr/bin/env bash
set -euo pipefail

samples=7

usage() {
  cat <<'USAGE'
Usage: benches/diff-performance.sh [--samples N]

Runs fmtview diff model and interactive diff-view rendering performance smoke
tests in release mode, then prints per-sample timings plus median/min/max
summaries.
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
  awk -v field="$field" '
    {
      for (i = 1; i <= NF; i++) {
        if ($i ~ "^" field "=") {
          sub("^" field "=", "", $i)
          sub("[^0-9].*", "", $i)
          print $i
          exit
        }
      }
    }
  '
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
  local line ms rows changes rendered_rows patch_bytes sample

  echo
  echo "== $label =="
  for sample in $(seq 1 "$samples"); do
    line="$(run_test "$test_name" "$pattern")"
    ms="$(printf '%s\n' "$line" | duration_ms)"
    rows="$(printf '%s\n' "$line" | extract_field rows)"
    changes="$(printf '%s\n' "$line" | extract_field changes)"
    rendered_rows="$(printf '%s\n' "$line" | extract_field rendered_rows)"
    patch_bytes="$(printf '%s\n' "$line" | extract_field patch_bytes)"
    rows="${rows:-0}"
    changes="${changes:-0}"
    rendered_rows="${rendered_rows:-0}"
    patch_bytes="${patch_bytes:-0}"
    printf '%s\n' "$ms" >> "$result_file"
    printf 'sample %02d: %8.3fms  rows=%s  changes=%s  rendered_rows=%s  patch_bytes=%s\n' \
      "$sample" "$ms" "$rows" "$changes" "$rendered_rows" "$patch_bytes"
  done

  printf 'time: '; summarize "$result_file"; echo
}

echo "fmtview diff performance smoke"
echo "samples: $samples"

bench_one \
  "diff model build" \
  "perf_diff_model_build" \
  "diff model build"

bench_one \
  "lazy record diff view open" \
  "perf_lazy_record_diff_view_open" \
  "lazy record diff view open"

bench_one \
  "interactive diff view render" \
  "perf_diff_view_render" \
  "diff view render"
