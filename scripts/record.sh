#!/usr/bin/env bash
# Records the demo of the real app: a clip, the stills and a gif of the stills,
# all under packaging/repo.
#
#   scripts/record.sh --sign-in        # once: sign Claude Code in for the demo app (in the terminal)
#   scripts/record.sh --all            # record, then the gif and the clip
#   scripts/record.sh --record         # run the app and record the clip and stills
#   scripts/record.sh --gif            # the gif, from the last stills
#   scripts/record.sh --video [out]    # the clip, from the last recording
#
#   SKIP_BUILD=1 ...         reuse the current src-tauri/dist/
#   DEMO_ASSISTANT=0 ...     leave out the parts that need Claude Code
#   DEMO_HOLD=2.4 ...        a slower gif
#   DEMO_SPEED=1.1 ...       speed of the clip as a whole
#   DEMO_SKIP_SPEED=10 ...   speed of the waits (downloads, the assistant at work)
#
# The demo app runs under its own identifier, com.zalanalytics.znote.demo, so
# it starts from a first run without touching a real install. Its Claude Code
# sign-in is kept between runs; everything else it stored is cleared first.
# Playwright cannot reach the WebKitGTK window, so the interface runs in
# Chromium and talks to the real app through tools/demo/relay.ts.
# The example .znote file is encrypted with a new password on every run, so the
# demo always shows the password prompt even though the app remembers the last one.
#
# Needs ffmpeg (with libx264), the playwright chromium browser
# (npx playwright install chromium), python3, and a desktop session for the
# hidden app window. The run downloads the two models and the marketplace items.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

REPO=packaging/repo
IDENTIFIER=com.zalanalytics.znote.demo
work="$root/.demo"
hold="${DEMO_HOLD:-1.8}"
colors="${DEMO_COLORS:-128}"
backdrop=0xFAFAFA

usage() { awk 'NR>1 && !/^#/ {exit} NR>1 {sub(/^# ?/, ""); print}' "${BASH_SOURCE[0]}"; }

need() {
  command -v "$1" >/dev/null 2>&1 || { echo "missing $1, $2" >&2; exit 1; }
}

frame_size() {
  ffprobe -v error -select_streams v:0 -show_entries stream=width,height -of csv=p=0 "$1"
}

# Claude Code as the demo app runs it: the same binary, with the demo's own folder.
demo_claude() {
  CLAUDE_CONFIG_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/$IDENTIFIER/claude" \
    CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1 PATH="$HOME/.local/bin:$PATH" claude "$@"
}

sign_in() {
  PATH="$HOME/.local/bin:$PATH" need claude "install Claude Code first, see https://claude.com/claude-code"
  echo "==> signing Claude Code in for the demo app"
  demo_claude auth login
}

record() {
  need pnpm "install node 22 or newer and pnpm"
  need python3 "install it from your package manager"
  [ -d node_modules ] || { echo "run pnpm install first" >&2; exit 1; }
  if pgrep -x znote >/dev/null; then
    echo "close znote first, the demo app shares its single-instance lock" >&2
    exit 1
  fi
  # Asks Claude Code itself, the same way the app does, before the slow build.
  if [ "${DEMO_ASSISTANT:-1}" != "0" ] && ! demo_claude auth status --json 2>/dev/null | grep -q '"loggedIn": *true'; then
    echo "Claude Code is not signed in for the demo app: run $0 --sign-in, or DEMO_ASSISTANT=0 $0 --record" >&2
    exit 1
  fi
  if [ "${SKIP_BUILD:-0}" != "1" ]; then
    echo "==> building the frontend"
    pnpm build
  fi

  rm -rf "$work/raw" "$work/stills" "$work/cuts.txt"
  python3 tools/demo/fixtures.py "$work/files"
  DEMO_PASSWORD="larkspur-$(date +%s)"
  export DEMO_PASSWORD
  cargo run --quiet --manifest-path src-tauri/Cargo.toml --example demo_znote -- \
    tools/demo/examples/pages "$work/files/Larkspur notes.znote" "$DEMO_PASSWORD"

  echo "==> recording (the first run compiles the app)"
  npx playwright test --config tools/demo/demo.config.ts

  mkdir -p "$REPO/demo"
  rm -f "$REPO/demo"/*.png
  cp "$work/stills"/*.png "$REPO/demo/"
  echo "==> $(ls "$REPO/demo" | wc -l) stills in $REPO/demo"
}

# Turns the stills into one gif, each held for a moment.
gif() {
  need ffmpeg "install it from your package manager"
  local dir="$REPO/demo" out="$REPO/demo.gif" width="${DEMO_WIDTH:-1000}"

  shopt -s nullglob
  local frames=("$dir"/*.png)
  shopt -u nullglob
  [ "${#frames[@]}" -gt 0 ] || { echo "no stills in $dir, run --record first" >&2; exit 1; }

  local first size
  first="$(frame_size "${frames[0]}")"
  for frame in "${frames[@]}"; do
    size="$(frame_size "$frame")"
    [ "$size" = "$first" ] || { echo "$frame is ${size/,/x}, expected ${first/,/x}" >&2; exit 1; }
  done

  mkdir -p "$work"
  local list="$work/gif.txt" palette="$work/gif-palette.png" last index=0 seconds
  last=$(( ${#frames[@]} - 1 ))
  : > "$list"
  for frame in "${frames[@]}"; do
    seconds="$hold"
    if [ "$index" -eq 0 ] || [ "$index" -eq "$last" ]; then
      seconds="$(awk -v h="$hold" 'BEGIN { printf "%.2f", h * 1.7 }')"
    fi
    printf "file '%s'\nduration %s\n" "$root/$frame" "$seconds" >> "$list"
    index=$(( index + 1 ))
  done
  # concat drops the duration of its last entry, so list that frame twice
  printf "file '%s'\n" "$root/${frames[$last]}" >> "$list"

  local scale="scale=$width:-2:flags=lanczos"
  ffmpeg -hide_banner -loglevel error -y -f concat -safe 0 -i "$list" \
    -vf "$scale,palettegen=max_colors=$colors:stats_mode=diff" "$palette"
  ffmpeg -hide_banner -loglevel error -y -f concat -safe 0 -i "$list" -i "$palette" \
    -lavfi "$scale[frames];[frames][1:v]paletteuse=dither=bayer:bayer_scale=3:diff_mode=rectangle" \
    -fps_mode vfr -loop 0 "$out"
  echo "==> ${#frames[@]} frames into $out ($(du -h "$out" | cut -f1))"
}

# The filter that plays the recording from the trim point, with every wait
# listed in cuts.txt sped up.
cut_filter() {
  local cuts="$1" skip="$2" speed="$3"
  awk -v skip="$skip" -v speed="$speed" '
    $1 == "trim" { at = $2; next }
    {
      if ($1 > at) parts[n++] = sprintf("%s %s 1", at, $1)
      parts[n++] = sprintf("%s %s %s", $1, $2, skip)
      at = $2
    }
    END {
      parts[n++] = sprintf("%s end 1", at)
      printf "[0:v]split=%d", n
      for (i = 0; i < n; i++) printf "[s%d]", i
      printf ";"
      for (i = 0; i < n; i++) {
        split(parts[i], p, " ")
        end = p[2] == "end" ? "" : ":end=" p[2]
        printf "[s%d]trim=start=%s%s,setpts=(PTS-STARTPTS)/%s/%s[p%d];", i, p[1], end, p[3], speed, i
      }
      for (i = 0; i < n; i++) printf "[p%d]", i
      printf "concat=n=%d:v=1:a=0[out]", n
    }' "$cuts"
}

video() {
  local out="${1:-$REPO/demo.mp4}"
  need ffmpeg "install it from your package manager"
  local webm cuts="$work/cuts.txt"
  webm="$(find "$work/raw" -name '*.webm' -print -quit 2>/dev/null || true)"
  [ -n "$webm" ] || { echo "no recording in $work/raw, run --record first" >&2; exit 1; }
  [ -f "$cuts" ] || { echo "$cuts is missing, the last recording did not finish" >&2; exit 1; }

  # Store listings reject the baseline stream openh264 makes. Held in a variable
  # because grep would close the pipe early and pipefail would read that as a failure.
  local encoders
  encoders="$(ffmpeg -hide_banner -encoders 2>/dev/null)"
  grep -q ' libx264 ' <<<"$encoders" || { echo "ffmpeg has no libx264 (dnf install ffmpeg)" >&2; exit 1; }

  local fit="scale=1920:1080:force_original_aspect_ratio=decrease:flags=lanczos"
  fit="$fit,pad=1920:1080:(ow-iw)/2:(oh-ih)/2:color=$backdrop"

  echo "==> encoding $out"
  mkdir -p "$(dirname "$out")"
  ffmpeg -hide_banner -loglevel error -y -i "$webm" \
    -filter_complex "$(cut_filter "$cuts" "${DEMO_SKIP_SPEED:-10}" "${DEMO_SPEED:-1.1}");[out]fps=30,$fit,format=yuv420p[v]" \
    -map "[v]" -an -c:v libx264 -preset slow -crf 20 -profile:v high -level:v 4.0 -movflags +faststart \
    "$out"

  local seconds
  seconds="$(ffprobe -v error -show_entries format=duration -of csv=p=0 "$out" 2>/dev/null || echo '?')"
  echo "==> done: $out ($(frame_size "$out" | tr ',' 'x'), ${seconds}s, $(du -h "$out" | cut -f1))"
}

(($#)) || { usage; exit 1; }

do_sign_in=0 do_record=0 do_gif=0 do_video=0
video_out=""

while (($#)); do
  case "$1" in
    --all) do_record=1 do_gif=1 do_video=1 ;;
    --sign-in) do_sign_in=1 ;;
    --record) do_record=1 ;;
    --gif) do_gif=1 ;;
    --video) do_video=1 ;;
    -h | --help) usage; exit 0 ;;
    -*) echo "unknown flag $1" >&2; usage >&2; exit 1 ;;
    *) video_out="$1" ;;
  esac
  shift
done

((do_sign_in)) && sign_in
((do_record)) && record
((do_gif)) && gif
((do_video)) && video "$video_out"
exit 0
