#!/usr/bin/env bash
set -euo pipefail

samples=7
case_filter=""

usage() {
  cat <<'USAGE'
Usage: benches/format-performance.sh [--samples N] [--case FILTER]

Runs fmtview transform-layer performance smoke tests in one release-mode Rust
harness process and prints per-sample timings plus median/min/max summaries.

Use this before and after parser, formatter, JSONL transform, or future
parallel formatting changes. FILTER matches benchmark labels, input shapes, or
layers.
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --samples)
      samples="${2:-}"
      shift 2
      ;;
    --case)
      case_filter="${2:-}"
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

env \
  FMTVIEW_PERF_SAMPLES="$samples" \
  FMTVIEW_PERF_CASE="$case_filter" \
  cargo test --release perf_format_suite -- --ignored --nocapture --test-threads=1
