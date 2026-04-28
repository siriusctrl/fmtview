#!/usr/bin/env bash
set -euo pipefail

samples=7
indent=2
build_release=1
candidates=()

usage() {
  cat <<'USAGE'
Usage: scripts/bench-format-algorithm.sh [--samples N] [--candidate NAME=COMMAND] [--no-build]

Runs complete-output formatter candidates against generated fixtures. The script
uses the current release fmtview binary as the reference output, then records
candidate wall time and verifies byte-for-byte output alignment.

COMMAND placeholders:
  {input}   input fixture path
  {output}  output path; if omitted, stdout is redirected to the output path
  {type}    fmtview type for the fixture, such as json or jsonl
  {indent}  indentation width

Examples:
  scripts/bench-format-algorithm.sh --samples 3
  scripts/bench-format-algorithm.sh --candidate 'experiment=target/release/fmtview --type {type} --indent {indent} {input}'
  scripts/bench-format-algorithm.sh --candidate 'tool=./target/release/my-parser {type} {input} {output}'
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --samples)
      samples="${2:-}"
      shift 2
      ;;
    --candidate)
      candidates+=("${2:-}")
      shift 2
      ;;
    --no-build)
      build_release=0
      shift
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

if [[ "$build_release" -eq 1 ]]; then
  cargo build --release --locked >/dev/null
fi

if [[ "${#candidates[@]}" -eq 0 ]]; then
  candidates+=("current=target/release/fmtview --type {type} --indent {indent} {input}")
fi

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

fixture_names=()
fixture_paths=()
fixture_types=()

repeat_char() {
  local char="$1"
  local count="$2"
  printf '%*s' "$count" '' | tr ' ' "$char"
}

generate_jsonl_many_records() {
  local path="$1"
  local count=16384
  local message
  message="$(repeat_char x 512)"
  : > "$path"
  for ((index = 0; index < count; index++)); do
    printf '{"index":%d,"level":"info","message":"%s","payload":{"xml":"<root><item id=\\"%d\\"><name>visible</name></item></root>","items":[{"a":1},{"b":2},{"c":{"d":true}}]}}\n' \
      "$index" "$message" "$index" >> "$path"
  done
}

generate_jsonl_single_huge_record() {
  local path="$1"
  local items=32768
  local message
  message="$(repeat_char y 128)"
  printf '{"kind":"huge","items":[' > "$path"
  for ((index = 0; index < items; index++)); do
    if [[ "$index" -gt 0 ]]; then
      printf ',' >> "$path"
    fi
    printf '{"index":%d,"message":"%s","nested":{"ok":true,"value":%d}}' \
      "$index" "$message" "$index" >> "$path"
  done
  printf ']}\n' >> "$path"
}

generate_json_whole_document() {
  local path="$1"
  local items=8192
  local message
  message="$(repeat_char w 128)"
  printf '{"kind":"document","items":[' > "$path"
  for ((index = 0; index < items; index++)); do
    if [[ "$index" -gt 0 ]]; then
      printf ',' >> "$path"
    fi
    printf '{"index":%d,"message":"%s","enabled":true,"nested":{"left":%d,"right":%d}}' \
      "$index" "$message" "$index" "$((items - index))" >> "$path"
  done
  printf ']}\n' >> "$path"
}

add_fixture() {
  local name="$1"
  local type="$2"
  local path="$tmpdir/${name}.${type}"
  "$3" "$path"
  fixture_names+=("$name")
  fixture_paths+=("$path")
  fixture_types+=("$type")
}

add_fixture "jsonl-many-records" "jsonl" generate_jsonl_many_records
add_fixture "jsonl-single-huge-record" "jsonl" generate_jsonl_single_huge_record
add_fixture "json-whole-document" "json" generate_json_whole_document

duration_ms() {
  local started_ns="$1"
  local ended_ns="$2"
  awk -v started="$started_ns" -v ended="$ended_ns" 'BEGIN { printf "%.6f", (ended - started) / 1000000 }'
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

expand_command() {
  local template="$1"
  local input="$2"
  local output="$3"
  local type="$4"
  local expanded="$template"
  expanded="${expanded//\{input\}/$input}"
  expanded="${expanded//\{output\}/$output}"
  expanded="${expanded//\{type\}/$type}"
  expanded="${expanded//\{indent\}/$indent}"
  printf '%s' "$expanded"
}

run_candidate() {
  local template="$1"
  local input="$2"
  local output="$3"
  local type="$4"
  local command
  command="$(expand_command "$template" "$input" "$output" "$type")"
  if [[ "$template" == *"{output}"* ]]; then
    bash -o pipefail -c "$command"
  else
    bash -o pipefail -c "$command" > "$output"
  fi
}

echo "fmtview formatter algorithm comparison"
echo "samples: $samples"
echo "reference: target/release/fmtview"

for fixture_index in "${!fixture_names[@]}"; do
  name="${fixture_names[$fixture_index]}"
  input="${fixture_paths[$fixture_index]}"
  type="${fixture_types[$fixture_index]}"
  reference="$tmpdir/reference.$name.out"
  target/release/fmtview --type "$type" --indent "$indent" "$input" > "$reference"
  reference_hash="$(sha256sum "$reference" | awk '{ print $1 }')"
  input_bytes="$(wc -c < "$input" | tr -d ' ')"
  output_bytes="$(wc -c < "$reference" | tr -d ' ')"

  echo
  echo "== $name ($type) =="
  echo "input_bytes=$input_bytes output_bytes=$output_bytes reference_sha256=$reference_hash"

  for spec in "${candidates[@]}"; do
    if [[ "$spec" != *=* ]]; then
      echo "--candidate must use NAME=COMMAND: $spec" >&2
      exit 2
    fi
    candidate_name="${spec%%=*}"
    candidate_command="${spec#*=}"
    result_file="$tmpdir/$name.$candidate_name.tsv"

    echo
    echo "-- candidate: $candidate_name --"
    for sample in $(seq 1 "$samples"); do
      output="$tmpdir/$name.$candidate_name.$sample.out"
      started="$(date +%s%N)"
      run_candidate "$candidate_command" "$input" "$output" "$type"
      ended="$(date +%s%N)"
      ms="$(duration_ms "$started" "$ended")"

      if ! cmp -s "$reference" "$output"; then
        candidate_hash="$(sha256sum "$output" | awk '{ print $1 }')"
        echo "output mismatch for $candidate_name on $name sample $sample" >&2
        echo "reference_sha256=$reference_hash candidate_sha256=$candidate_hash" >&2
        exit 1
      fi

      printf '%s\n' "$ms" >> "$result_file"
      printf 'sample %02d: %8.3fms  aligned=yes\n' "$sample" "$ms"
    done
    printf 'time: '; summarize "$result_file"; echo
  done
done
