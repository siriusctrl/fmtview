#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "$REPO_ROOT"

usage() {
  cat >&2 <<'EOF'
Usage:
  scripts/record-emulator-demo.sh [output-dir] [-- command...]

Runs fmtview inside a real Kitty terminal on an Xvfb display, records the
visible terminal window, extracts frames, and writes a small inspection bundle.

The default command is:
  target/debug/fmtview examples/chat.jsonl

Examples:
  scripts/record-emulator-demo.sh
  scripts/record-emulator-demo.sh target/fmtview-emulator-recordings/html -- target/debug/fmtview examples/messy.html
  scripts/record-emulator-demo.sh target/fmtview-emulator-recordings/diff -- target/debug/fmtview diff examples/chat.jsonl examples/chat.jsonl

Set FMTVIEW_EMULATOR_USE_EXISTING_DISPLAY=1 to reuse the current DISPLAY
instead of starting Xvfb.

Set FMTVIEW_EMULATOR_FOLLOW_FILE to the JSONL file named by a --follow demo
command to record append, detach, reattach, and pause/resume actions.
EOF
}

for tool in Xvfb kitty xdotool ffmpeg xwininfo xdpyinfo python3; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "recording requires $tool" >&2
    exit 1
  fi
done

output_dir="${1:-}"
if [[ -n "$output_dir" && "$output_dir" != "--" ]]; then
  shift
else
  output_dir="target/fmtview-emulator-recordings/$(date +%Y%m%d-%H%M%S)"
fi

if [[ "${1:-}" == "--" ]]; then
  shift
  if [[ "$#" -eq 0 ]]; then
    usage
    exit 1
  fi
  demo_command="$*"
else
  demo_command="target/debug/fmtview examples/chat.jsonl"
fi

screen_w="${FMTVIEW_EMULATOR_SCREEN_W:-1440}"
screen_h="${FMTVIEW_EMULATOR_SCREEN_H:-960}"
window_w="${FMTVIEW_EMULATOR_WINDOW_W:-1180}"
window_h="${FMTVIEW_EMULATOR_WINDOW_H:-820}"
fps="${FMTVIEW_EMULATOR_FPS:-24}"
use_existing_display="${FMTVIEW_EMULATOR_USE_EXISTING_DISPLAY:-0}"
follow_file="${FMTVIEW_EMULATOR_FOLLOW_FILE:-}"
class_name="fmtview-emulator-$$"
display=""
xvfb_pid=""
kitty_pid=""
ffmpeg_pid=""

mkdir -p "$output_dir/frames" "$output_dir/keyframes"
rm -f "$output_dir/frames"/frame-*.png "$output_dir/keyframes"/frame-*.png

cleanup() {
  if [[ -n "${ffmpeg_pid:-}" ]]; then
    kill -INT "$ffmpeg_pid" 2>/dev/null || true
    wait "$ffmpeg_pid" 2>/dev/null || true
  fi
  if [[ -n "${kitty_pid:-}" ]]; then
    kill "$kitty_pid" 2>/dev/null || true
    wait "$kitty_pid" 2>/dev/null || true
  fi
  if [[ -n "${xvfb_pid:-}" ]]; then
    kill "$xvfb_pid" 2>/dev/null || true
    wait "$xvfb_pid" 2>/dev/null || true
  fi
}
trap cleanup EXIT

pick_display() {
  for candidate in $(seq 90 119); do
    if [[ ! -e "/tmp/.X11-unix/X${candidate}" && ! -e "/tmp/.X${candidate}-lock" ]]; then
      echo ":${candidate}"
      return 0
    fi
  done
  return 1
}

printf 'Building fmtview...\n'
cargo build --quiet

if [[ "$use_existing_display" == "1" ]]; then
  display="${DISPLAY:-}"
  if [[ -z "$display" ]]; then
    echo "FMTVIEW_EMULATOR_USE_EXISTING_DISPLAY=1 requires DISPLAY to be set" >&2
    exit 1
  fi
  if DISPLAY="$display" xdpyinfo >/dev/null 2>&1; then
    printf 'Using existing DISPLAY %s...\n' "$display"
  else
    echo "existing DISPLAY is not reachable: $display" >&2
    exit 1
  fi
else
  display="$(pick_display)"
  if [[ -z "$display" ]]; then
    echo "could not find a free X display" >&2
    exit 1
  fi

  printf 'Starting Xvfb on %s...\n' "$display"
  Xvfb "$display" -screen 0 "${screen_w}x${screen_h}x24" +extension GLX +render -nolisten tcp \
    >"$output_dir/xvfb.log" 2>&1 &
  xvfb_pid=$!
  for _ in $(seq 1 50); do
    if ! kill -0 "$xvfb_pid" 2>/dev/null; then
      echo "Xvfb exited before the display became ready" >&2
      sed -n '1,160p' "$output_dir/xvfb.log" >&2 || true
      exit 1
    fi
    if DISPLAY="$display" xdpyinfo >/dev/null 2>&1; then
      break
    fi
    sleep 0.1
  done
  if ! DISPLAY="$display" xdpyinfo >/dev/null 2>&1; then
    echo "Xvfb display did not become reachable: $display" >&2
    sed -n '1,160p' "$output_dir/xvfb.log" >&2 || true
    exit 1
  fi
fi

export DISPLAY="$display"
export LIBGL_ALWAYS_SOFTWARE="${LIBGL_ALWAYS_SOFTWARE:-1}"
export MESA_GL_VERSION_OVERRIDE="${MESA_GL_VERSION_OVERRIDE:-3.3}"
export KITTY_CONFIG_DIRECTORY="$output_dir/kitty-config"
mkdir -p "$KITTY_CONFIG_DIRECTORY"
cat >"$KITTY_CONFIG_DIRECTORY/kitty.conf" <<EOF
font_family DejaVu Sans Mono
font_size 15
remember_window_size no
initial_window_width ${window_w}
initial_window_height ${window_h}
background #0b1117
foreground #cbd5e1
cursor #cbd5e1
enable_audio_bell no
confirm_os_window_close 0
EOF

printf 'Starting Kitty terminal...\n'
kitty --class "$class_name" --title "$class_name" sh -lc "cd '$REPO_ROOT' && exec bash --noprofile --norc" \
  >"$output_dir/kitty.stdout" 2>"$output_dir/kitty.stderr" &
kitty_pid=$!

window_id=""
for _ in $(seq 1 100); do
  window_id="$(xdotool search --onlyvisible --class "$class_name" 2>/dev/null | head -n 1 || true)"
  if [[ -n "$window_id" ]]; then
    break
  fi
  sleep 0.1
done

if [[ -z "$window_id" ]]; then
  echo "could not find Kitty window" >&2
  sed -n '1,160p' "$output_dir/kitty.stderr" >&2 || true
  exit 1
fi

xwininfo -id "$window_id" >"$output_dir/window.txt"
xdotool windowfocus "$window_id" 2>/dev/null || true
xdotool mousemove "$((screen_w - 8))" "$((screen_h - 8))" 2>/dev/null || true
sleep 0.5

printf 'Launching: %s\n' "$demo_command"
xdotool type --clearmodifiers --delay 0 -- "$demo_command"
xdotool key --clearmodifiers Return
sleep 1.2

video="$output_dir/session.mp4"
printf 'Recording %s...\n' "$video"
start_ms="$(date +%s%3N)"
ffmpeg -hide_banner -loglevel warning -y \
  -f x11grab -framerate "$fps" -video_size "${screen_w}x${screen_h}" -i "$display" \
  -c:v libx264 -preset veryfast -crf 18 -pix_fmt yuv420p "$video" \
  >"$output_dir/ffmpeg.stdout" 2>"$output_dir/ffmpeg.stderr" &
ffmpeg_pid=$!

actions_file="$output_dir/actions.jsonl"
: >"$actions_file"

if [[ -n "$follow_file" && ! -f "$follow_file" ]]; then
  echo "FMTVIEW_EMULATOR_FOLLOW_FILE is not a file: $follow_file" >&2
  exit 1
fi

send_key() {
  local name="$1"
  local key="$2"
  local before
  before="$(date +%s%3N)"
  xdotool windowfocus "$window_id" 2>/dev/null || true
  xdotool key --clearmodifiers --window "$window_id" "$key"
  printf '{"name":"%s","key":"%s","sent_ms":%s}\n' "$name" "$key" "$before" >>"$actions_file"
}

send_type() {
  local name="$1"
  local text="$2"
  local before
  before="$(date +%s%3N)"
  xdotool windowfocus "$window_id" 2>/dev/null || true
  xdotool type --clearmodifiers --delay 0 -- "$text"
  printf '{"name":"%s","text":"%s","sent_ms":%s}\n' "$name" "$text" "$before" >>"$actions_file"
}

append_follow_record() {
  local name="$1"
  local index="$2"
  local before
  before="$(date +%s%3N)"
  printf '{"index":%s,"role":"assistant","message":"recorded follow append %s"}\n' \
    "$index" "$index" >>"$follow_file"
  printf '{"name":"%s","record":%s,"sent_ms":%s}\n' "$name" "$index" "$before" >>"$actions_file"
}

sleep 1.0
if [[ -n "$follow_file" ]]; then
  append_follow_record "append_while_attached" 900001
  sleep 0.8
  send_key "detach_page_up" "Page_Up"
  sleep 0.7
  append_follow_record "append_while_detached" 900002
  sleep 0.8
  send_key "reattach_page_down_1" "Page_Down"
  sleep 0.4
  send_key "reattach_page_down_2" "Page_Down"
  sleep 0.8
  append_follow_record "append_after_reattach" 900003
  sleep 0.8
  send_key "pause_follow" "f"
  sleep 0.7
  append_follow_record "append_while_paused" 900004
  sleep 0.8
  send_key "resume_follow" "f"
  sleep 0.8
else
  send_key "scroll_down" "j"
  sleep 0.5
  send_key "page_down" "Page_Down"
  sleep 0.7
  send_key "search_open" "slash"
  sleep 0.2
  send_type "search_query" "assistant"
  sleep 0.2
  send_key "search_enter" "Return"
  sleep 0.7
  send_key "search_next" "n"
  sleep 0.7
  send_key "next_structure" "bracketright"
  sleep 0.7
  send_key "previous_structure" "bracketleft"
  sleep 0.7
  if [[ "$demo_command" == "target/debug/fmtview examples/chat.jsonl" \
    || "$demo_command" == "target/release/fmtview examples/chat.jsonl" ]]; then
    send_key "tool_search_open" "slash"
    sleep 0.2
    send_type "tool_search_query" "tool_call_id"
    sleep 0.2
    send_key "tool_search_enter" "Return"
    sleep 0.7
    send_key "tool_jump_to_call" "t"
    sleep 0.7
    send_key "tool_jump_to_result" "t"
    sleep 0.7
  fi
fi
send_key "quit" "q"
sleep 0.5

kill -INT "$ffmpeg_pid" 2>/dev/null || true
wait "$ffmpeg_pid" 2>/dev/null || true
ffmpeg_pid=""

if [[ ! -s "$video" ]]; then
  echo "recording did not produce a video" >&2
  sed -n '1,160p' "$output_dir/ffmpeg.stderr" >&2 || true
  exit 1
fi

printf 'Extracting frames...\n'
ffmpeg -hide_banner -loglevel error -y -i "$video" "$output_dir/frames/frame-%04d.png"

printf 'Building timeline sheet...\n'
ffmpeg -hide_banner -loglevel error -y -i "$video" \
  -vf "fps=1,scale=360:-1,tile=4x3:padding=12:margin=12:color=0x0b1117" \
  -frames:v 1 "$output_dir/timeline-sheet.png"

python3 - "$output_dir" "$demo_command" "$display" "$window_id" "$start_ms" "$fps" \
  "$screen_w" "$screen_h" <<'PY'
from __future__ import annotations

import json
import math
import shutil
import subprocess
import sys
from pathlib import Path

out = Path(sys.argv[1])
command = sys.argv[2]
display = sys.argv[3]
window_id = sys.argv[4]
start_ms = int(sys.argv[5])
fps = float(sys.argv[6])
screen_w = int(sys.argv[7])
screen_h = int(sys.argv[8])

frames = sorted((out / "frames").glob("frame-*.png"))
if not frames:
    raise SystemExit("no extracted frames")

actions = []
for line in (out / "actions.jsonl").read_text().splitlines():
    if line.strip():
        actions.append(json.loads(line))

keyframe_indexes = sorted({0, len(frames) // 2, len(frames) - 1})
for action in actions:
    sent = int(action["sent_ms"])
    index = int(max(0, min(len(frames) - 1, (sent - start_ms) * fps / 1000.0)))
    keyframe_indexes.append(index)
keyframe_indexes = sorted(set(keyframe_indexes))

keyframe_dir = out / "keyframes"
keyframe_paths = []
for index in keyframe_indexes:
    source = frames[index]
    dest = keyframe_dir / f"frame-{index:04d}.png"
    shutil.copyfile(source, dest)
    keyframe_paths.append(str(dest))

cols = min(4, max(1, len(keyframe_indexes)))
rows = max(1, math.ceil(len(keyframe_indexes) / cols))
select_expr = "+".join(f"eq(n\\,{index})" for index in keyframe_indexes)
subprocess.run(
    [
        "ffmpeg",
        "-hide_banner",
        "-loglevel",
        "error",
        "-y",
        "-start_number",
        "1",
        "-i",
        str(out / "frames" / "frame-%04d.png"),
        "-vf",
        f"select='{select_expr}',scale=360:-1,tile={cols}x{rows}:padding=12:margin=12:color=0x0b1117",
        "-frames:v",
        "1",
        str(out / "contact-sheet.png"),
    ],
    check=True,
)

metrics = {
    "command": command,
    "display": display,
    "window_id": window_id,
    "screen": {"width": screen_w, "height": screen_h},
    "fps": fps,
    "frame_count": len(frames),
    "video": str(out / "session.mp4"),
    "contact_sheet": str(out / "contact-sheet.png"),
    "timeline_sheet": str(out / "timeline-sheet.png"),
    "keyframes": keyframe_paths,
    "keyframe_indexes": keyframe_indexes,
    "actions": actions,
    "checks": {
        "video_exists": (out / "session.mp4").stat().st_size > 0,
        "frames_extracted": len(frames) > 0,
        "contact_sheet_exists": (out / "contact-sheet.png").exists(),
        "timeline_sheet_exists": (out / "timeline-sheet.png").exists(),
        "scripted_actions_recorded": len(actions) >= 5,
    },
}

(out / "metrics.json").write_text(json.dumps(metrics, indent=2, sort_keys=True) + "\n")

inspection = [
    f"command: {command}",
    f"display: {display}",
    f"window_id: {window_id}",
    f"video: {out / 'session.mp4'}",
    f"frames: {len(frames)}",
    f"contact_sheet: {out / 'contact-sheet.png'}",
    f"timeline_sheet: {out / 'timeline-sheet.png'}",
    f"keyframes: {len(keyframe_paths)}",
    f"actions: {len(actions)}",
]
for name, passed in metrics["checks"].items():
    inspection.append(f"check_{name}: {str(passed).lower()}")
(out / "inspection.txt").write_text("\n".join(inspection) + "\n")
PY

printf 'recording_dir=%s\n' "$output_dir"
printf 'video=%s/session.mp4\n' "$output_dir"
printf 'frames=%s/frames\n' "$output_dir"
printf 'keyframes=%s/keyframes\n' "$output_dir"
printf 'contact_sheet=%s/contact-sheet.png\n' "$output_dir"
printf 'timeline_sheet=%s/timeline-sheet.png\n' "$output_dir"
printf 'metrics=%s/metrics.json\n' "$output_dir"
printf 'inspection=%s/inspection.txt\n' "$output_dir"
