# Visual Verification

Terminal UI changes should leave visual evidence, not only PTY byte output.

## Real Terminal Recording

Use the emulator recording helper for final viewer checks, release candidates,
or changes that affect layout, colors, gutter behavior, search, scrolling, or
structure jumps:

```sh
scripts/record-emulator-demo.sh target/fmtview-emulator-recordings/<name>
```

The helper builds the local binary, starts Xvfb, opens a Kitty terminal, types
the command into the shell, records the real terminal window with `ffmpeg`,
sends a fixed sequence of scroll, search, structure-jump, and quit keys with
`xdotool`, then writes:

- `session.mp4` for replaying the user-facing experience.
- `frames/frame-*.png` for frame-by-frame inspection.
- `keyframes/frame-*.png` for action-adjacent snapshots.
- `contact-sheet.png`, built from the keyframes with `ffmpeg`, for quick visual
  review of the exact frames around scripted actions.
- `timeline-sheet.png`, sampled at 1 fps, for a coarse overview of the whole
  recording.
- `metrics.json` for command, frame, artifact, and scripted-action metadata.
- `inspection.txt` for a short checklist.

The default command is:

```sh
target/debug/fmtview examples/chat.jsonl
```

Record a specific scenario by passing a command after `--`:

```sh
scripts/record-emulator-demo.sh target/fmtview-emulator-recordings/html -- target/debug/fmtview examples/messy.html
scripts/record-emulator-demo.sh target/fmtview-emulator-recordings/diff -- target/debug/fmtview diff examples/chat.jsonl examples/chat.jsonl
```

For follow mode, prepare a disposable JSONL file, name the same file in
`FMTVIEW_EMULATOR_FOLLOW_FILE`, and pass the follow command. The helper then
records attached append, scroll-detach, ordinary PageDown reattach, explicit
pause, and resume states:

```sh
cp examples/chat.jsonl target/follow-demo.jsonl
FMTVIEW_EMULATOR_FOLLOW_FILE=target/follow-demo.jsonl \
  scripts/record-emulator-demo.sh target/fmtview-emulator-recordings/follow -- \
  target/debug/fmtview --follow target/follow-demo.jsonl
```

For release candidates, pass the release binary explicitly after building it:

```sh
scripts/record-emulator-demo.sh target/fmtview-emulator-recordings/release -- target/release/fmtview examples/chat.jsonl
```

If a developer machine already has a reachable display and you want to skip
Xvfb, reuse it explicitly:

```sh
FMTVIEW_EMULATOR_USE_EXISTING_DISPLAY=1 scripts/record-emulator-demo.sh target/fmtview-emulator-recordings/current-display
```

Artifacts stay under `target/` by default and should not be committed.

## Review Standard

Treat this as a final visual smoke test, not a replacement for unit tests,
benchmarks, or PTY checks. The recording proves that a real terminal emulator
composited the UI into a window; it does not prove every line of behavior.

Before accepting a visual change, inspect `contact-sheet.png` and at least a
few `keyframes/` yourself. Look for:

- blank or partially blank startup frames after the viewer should be visible;
- clipped line numbers, gutters, footer text, or title text;
- unreadable highlight colors in a dark terminal;
- stale cells after scrolling, search, or structure jumps;
- distracting full-window flicker during normal navigation;
- obvious wrap, selection, or viewport drift.

`metrics.json` contains only basic artifact checks. Failed checks mean the
recording did not capture useful evidence. Passing checks still require manual
inspection because color contrast and layout quality are visual judgments.

Required local tools are `Xvfb`, `kitty`, `xdotool`, `ffmpeg`, `xwininfo`,
`xdpyinfo`, and Python 3.

If Xvfb exits with `Cannot establish any listening sockets`, the current
environment is blocking local X socket creation. That is usually a sandbox or
container policy problem rather than a fmtview failure; rerun the visual smoke
on a normal developer machine or CI runner that permits Xvfb.
