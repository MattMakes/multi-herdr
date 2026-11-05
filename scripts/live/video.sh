#!/usr/bin/env bash
# Live check, video pipeline (spec: ai_docs/specs/video-pipeline/spec.md, Testing Strategy).
# Builds a synthetic 2-scene video project in a temp folder and runs every
# bundled skills/video-* script on it, with positive and negative fixtures:
#   1. static guards and compile checks over skills/video-* (no real media)
#   2. every script's --self-test
#   3. check_timeline.py, check_audio.py and compare_frames.py, each with a
#      passing and a failing fixture
#   4. the HTML scene template of skills/video-motion, with its real
#      seek-timeline.js, rendered by
#      render_frames.mjs in real headless Chrome, master.mp4 from
#      assemble_master.mjs, the publish pack, and qa_video.py with forward and
#      reverse probes (a passing master, a black master, a mismatched probe)
# Needs python3, node, ffmpeg and ffprobe. Steps 2 (render_frames, assemble_master,
# capture and materials self-tests) and 4 also need Chrome (CHROME_PATH, or the macOS app);
# without Chrome they print SKIP. Nothing is downloaded. Every file goes to a
# mktemp -d folder that a trap removes, so git status stays clean.
# Env: LIVE_VIDEO_KEEP=1 keeps the temp folder and prints its path.
set -euo pipefail
cd "$(dirname "$0")/../.."
unset ANTHROPIC_API_KEY 2>/dev/null || true

W=$(mktemp -d "${TMPDIR:-/tmp}/live-video.XXXXXX")
# shellcheck disable=SC2329 # run by the EXIT trap
cleanup() {
  if [ "${LIVE_VIDEO_KEEP:-0}" = 1 ]; then echo "kept $W"; else rm -rf "$W"; fi
}
trap cleanup EXIT
export PYTHONDONTWRITEBYTECODE=1 PYTHONPYCACHEPREFIX="$W/pycache"

FAILED=0
pass() { echo "PASS $1"; }
fail() { echo "FAIL $1: $2"; FAILED=1; }
skip() { echo "SKIP $1: $2"; }

# ok NAME CMD...: the command must exit 0. bad NAME CMD...: it must exit non-zero.
ok() {
  local name=$1 out; shift
  if out=$("$@" 2>&1); then pass "$name"; else fail "$name" "exit $?: $(printf '%s' "$out" | tail -n 6)"; fi
}
bad() {
  local name=$1 out; shift
  if out=$("$@" 2>&1); then fail "$name" "exited 0, a failure was expected"; else pass "$name (exit $?)"; fi
}

for tool in python3 node ffmpeg ffprobe; do
  command -v "$tool" >/dev/null 2>&1 || { echo "FAIL tools: $tool is not on PATH"; exit 1; }
done

CHROME=""
for c in "${CHROME_PATH:-}" "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"; do
  if [ -n "$c" ] && [ -x "$c" ]; then CHROME=$c; break; fi
done

S=skills
TL_CHECK=$S/video-storyboard/scripts/check_timeline.py
AUDIO_CHECK=$S/video-audio/scripts/check_audio.py
COMPARE=$S/video-reanimate/scripts/compare_frames.py
RENDER=$S/video-render/scripts/render_frames.mjs
ASSEMBLE=$S/video-render/scripts/assemble_master.mjs
QA=$S/video-qa/scripts/qa_video.py
SEEK_DOC=$S/video-motion/references/seek-contract.md
SEEK_LIB=$S/video-motion/scripts/seek-timeline.js

# ---------------------------------------------------------------- 1. guards
py_files=$(ls $S/video-*/scripts/*.py)
mjs_files=$(ls $S/video-*/scripts/*.mjs)
for f in $py_files; do ok "py_compile $f" python3 -m py_compile "$f"; done
for f in $mjs_files; do ok "node --check $f" node --check "$f"; done
ok "node --check $SEEK_LIB" node --check "$SEEK_LIB"

if hits=$(grep -rn "__renderAt" $S/video-* teammates/video-*.md); then
  fail "seek name" "found __renderAt (the name is __seekToFrame): $hits"
else pass "no __renderAt in skills/video-* or teammates/video-*"; fi
if hits=$(grep -rnEw '(start|end|duration|beats|downbeats|t)_s' $S/video-* teammates/video-*.md); then
  fail "field names" "found a _s field name (use _sec): $hits"
else pass "no _s field names in skills/video-* or teammates/video-*"; fi
if hits=$(grep -rnE '(^|[^A-Za-z0-9_-])(brief|direction|storyboard)\.md' $S/video-* teammates/video-*.md); then
  fail "hand-off names" "found a lowercase hand-off file name: $hits"
else pass "hand-off file names are uppercase"; fi

# ---------------------------------------------------------------- 2. self-tests
for f in $py_files; do ok "--self-test $f" python3 -I "$f" --self-test; done
ok "seek-timeline self-test" node $S/video-motion/scripts/seek-timeline.selftest.mjs
if [ -n "$CHROME" ]; then
  export CHROME_PATH=$CHROME
  ok "--self-test $RENDER" node "$RENDER" --self-test
  ok "--self-test $ASSEMBLE" node "$ASSEMBLE" --self-test
  ok "--self-test $S/video-capture/scripts/capture.mjs" node $S/video-capture/scripts/capture.mjs --self-test
  ok "--self-test $S/video-capture/scripts/materials.mjs" node $S/video-capture/scripts/materials.mjs --self-test
  ok "--self-test $S/video-qa/scripts/text_audit.mjs" node $S/video-qa/scripts/text_audit.mjs --self-test
else
  skip "render_frames, assemble_master, capture, materials and text_audit self-tests" "no Chrome: set CHROME_PATH"
fi

# ---------------------------------------------------------------- 3. checkers
P=$W/project
mkdir -p "$P"/{scenes/s01-hook,captures,audio,render,qa,ref,rendered,rendered-bad}
FPS=30 WIDTH=1920 HEIGHT=1080 DURATION=5.5
cat >"$P/timeline.json" <<JSON
{
  "meta": {"id": "live-video", "title": "Live video check", "aspect": "16:9", "fps": $FPS,
           "width": $WIDTH, "height": $HEIGHT, "duration_sec": $DURATION, "lufs_target": -14},
  "scenes": [
    {"id": "s01-hook", "kind": "hook", "start_sec": 0.0, "duration_sec": 2.5, "beat_in": 0, "beat_out": 5,
     "narration": "Ship every frame.", "caption": "Ship every frame", "title": "Hook",
     "plate": "scenes/s01-hook/index.html", "motion": {"template": "slam-in"},
     "transition_out": "fade",
     "critical_regions": [[0.1, 0.35, 0.6, 0.25]], "focal_point": [0.4, 0.5]},
    {"id": "s02-cta", "kind": "cta", "start_sec": 2.5, "duration_sec": 3.0, "beat_in": 5, "beat_out": 11,
     "narration": "Try it today.", "caption": "Try it today", "title": "Try it",
     "plate": "captures/s02-cta.mp4", "motion": {"template": "fade-up"}, "static_hold": false,
     "transition_out": "cut",
     "critical_regions": [[0.2, 0.3, 0.6, 0.3]], "focal_point": [0.5, 0.5]}
  ]
}
JSON
cat >"$P/audio/beats.json" <<'JSON'
{"bpm": 120, "beats_sec": [0.0, 0.5, 1.0, 1.5, 2.0, 2.5, 3.0, 3.5, 4.0, 4.5, 5.0],
 "downbeats_sec": [0.0, 2.0, 4.0], "source": "synthetic"}
JSON

# Negative timelines: a 12 s scene, and a hook that starts after 3 s.
python3 - "$P/timeline.json" "$W" <<'PY'
import copy, json, sys
t = json.load(open(sys.argv[1]))
long_scene = copy.deepcopy(t)
long_scene["scenes"][1]["duration_sec"] = 12.0
json.dump(long_scene, open(f"{sys.argv[2]}/timeline-long.json", "w"))
late_hook = copy.deepcopy(t)
late_hook["scenes"][0]["kind"] = "capture"
late_hook["scenes"][1]["kind"] = "hook"
json.dump(late_hook, open(f"{sys.argv[2]}/timeline-late-hook.json", "w"))
PY
ok "check_timeline passes the fixture" python3 "$TL_CHECK" "$P/timeline.json" --out "$P/qa/plan-check.json"
bad "check_timeline fails a 12 s scene" python3 "$TL_CHECK" "$W/timeline-long.json" --out "$W/plan-long.json"
bad "check_timeline fails a late hook" python3 "$TL_CHECK" "$W/timeline-late-hook.json" --out "$W/plan-late.json"

# audio/mix.wav: 2 sines, stereo, 48 kHz, gain set to -14 LUFS. hot.wav clips. silent.wav is empty.
sines="aevalsrc=exprs='0.5*sin(2*PI*220*t)+0.3*sin(2*PI*330*t)|0.5*sin(2*PI*220*t)+0.3*sin(2*PI*330*t)':s=48000:d=$DURATION"
ffmpeg -hide_banner -v error -y -f lavfi -i "$sines" -c:a pcm_s16le "$W/raw.wav"
lufs=$(ffmpeg -hide_banner -nostats -i "$W/raw.wav" -af ebur128 -f null - 2>&1 | awk '/^ +I: / {v=$2} END {print v}')
gain=$(awk -v l="$lufs" 'BEGIN {printf "%.3f", -14 - l}')
ffmpeg -hide_banner -v error -y -i "$W/raw.wav" -af "volume=${gain}dB" -ar 48000 -ac 2 -c:a pcm_s16le "$P/audio/mix.wav"
ffmpeg -hide_banner -v error -y -f lavfi -i "aevalsrc=exprs='3*sin(2*PI*220*t)|3*sin(2*PI*220*t)':s=48000:d=$DURATION" \
  -af "volume=0dB,alimiter=limit=1:level=disabled" -c:a pcm_s16le "$W/hot.wav"
ffmpeg -hide_banner -v error -y -f lavfi -i "anullsrc=r=48000:cl=stereo" -t "$DURATION" -c:a pcm_s16le "$W/silent.wav"
ok "check_audio passes -14 LUFS stereo" python3 "$AUDIO_CHECK" "$P/audio/mix.wav" --timeline "$P/timeline.json" --beats "$P/audio/beats.json" --out "$P/qa/audio-check.json"
bad "check_audio fails a clipped mix" python3 "$AUDIO_CHECK" "$W/hot.wav" --out "$W/audio-hot.json"
bad "check_audio fails a silent mix" python3 "$AUDIO_CHECK" "$W/silent.wav" --out "$W/audio-silent.json"

# Sync sheet: 1 voice file per scene (macOS say when present, else a tone), then
# sync_sheet.py, then the gate. A sheet with the words reversed must fail.
for scene in "s01-hook:Ship every frame." "s02-cta:Try it today."; do
  id=${scene%%:*} text=${scene#*:}
  if command -v say >/dev/null 2>&1; then
    say -o "$W/vo-$id.aiff" "$text"
    ffmpeg -hide_banner -v error -y -i "$W/vo-$id.aiff" -ar 48000 -ac 1 -c:a pcm_s16le "$P/audio/vo-$id.wav"
  else
    ffmpeg -hide_banner -v error -y -f lavfi -i "aevalsrc=exprs='0.4*sin(2*PI*220*t)':s=48000:d=1.2" \
      -c:a pcm_s16le "$P/audio/vo-$id.wav"
  fi
done
ok "sync_sheet writes audio/sync-sheet.json" python3 "$S/video-audio/scripts/sync_sheet.py" --timeline "$P/timeline.json" \
  --audio-dir "$P/audio" --beats "$P/audio/beats.json" --out "$P/audio/sync-sheet.json"
ok "check_audio passes the sync sheet" python3 "$AUDIO_CHECK" "$P/audio/mix.wav" --timeline "$P/timeline.json" \
  --sync-sheet "$P/audio/sync-sheet.json" --out "$W/audio-sync.json"
python3 - "$P/audio/sync-sheet.json" "$W/sync-bad.json" <<'PY'
import json, sys
sheet = json.load(open(sys.argv[1]))
sheet["vo_segments"][0]["words"].reverse()
json.dump(sheet, open(sys.argv[2], "w"))
PY
bad "check_audio fails words out of order" python3 "$AUDIO_CHECK" "$P/audio/mix.wav" --timeline "$P/timeline.json" \
  --sync-sheet "$W/sync-bad.json" --out "$W/audio-sync-bad.json"

# compare_frames: a reference set, the same set with 1 px of drift and noise, and a shifted set.
mkframes() { # DIR X_OFFSET Y NOISE
  local i=0 x
  for x in 40 70 100 130 160; do
    ffmpeg -hide_banner -v error -y -f lavfi -i "color=c=0x14213D:s=320x180:d=1" \
      -vf "drawbox=x=$((x + $2)):y=$3:w=80:h=60:color=0xF2C14E:t=fill$4" -frames:v 1 "$1/$(printf 'f%03d' $i).png"
    i=$((i + 1))
  done
}
mkframes "$P/ref" 0 60 ""
mkframes "$P/rendered" 1 60 ",noise=alls=3:allf=t"
mkframes "$P/rendered-bad" 120 100 ""
ok "compare_frames passes matching keyframes" python3 "$COMPARE" --ref "$P/ref" --rendered "$P/rendered" --out "$P/qa/reanimate-check.json"
bad "compare_frames fails mismatched keyframes" python3 "$COMPARE" --ref "$P/ref" --rendered "$P/rendered-bad" --out "$W/reanimate-bad.json"

# ---------------------------------------------------------------- 4. render and gate
if [ -z "$CHROME" ]; then
  skip "scene render, master, publish pack, qa_video" "no Chrome: set CHROME_PATH"
  [ "$FAILED" = 0 ] && echo "video live check: all run steps passed (Chrome steps skipped)" || echo "video live check: FAILED"
  exit "$FAILED"
fi

# The scene is the template of skills/video-motion, the first html block of
# seek-contract.md. It loads seek-timeline.js from its own folder, so the real
# file from skills/video-motion/scripts is copied there.
awk '/^```html$/ {f = 1; next} /^```$/ {if (f) exit} f' "$SEEK_DOC" >"$P/scenes/s01-hook/index.html"
test -s "$P/scenes/s01-hook/index.html" || { fail "template" "no html block in $SEEK_DOC"; exit 1; }
cp "$SEEK_LIB" "$P/scenes/s01-hook/seek-timeline.js"
# Fixture captured plate for s02-cta: testsrc2, 2 s of tail handle is not needed.
ffmpeg -hide_banner -v error -y -f lavfi -i "testsrc2=s=${WIDTH}x${HEIGHT}:r=$FPS:d=3.0" \
  -c:v libx264 -preset ultrafast -pix_fmt yuv420p "$P/captures/s02-cta.mp4"

# Tail handle: render the hook 0.3 s longer than its 2.5 s for the fade.
ok "render_frames renders the video-motion template to a clip" \
  node "$RENDER" --scene "$P/scenes/s01-hook/index.html" --out "$W/hook.mp4" --fps $FPS --width $WIDTH --height $HEIGHT --duration 2.8
mkdir -p "$P/render/scenes"
mv "$W/hook.mp4" "$P/render/scenes/s01-hook.mp4"
ok "assemble_master builds render/master.mp4" node "$ASSEMBLE" --master "$P/timeline.json" --out "$P/render/master.mp4"
ok "render_frames --package writes captions and the 3 aspect variants" \
  node "$RENDER" --package "$P/timeline.json" --master "$P/render/master.mp4" --out-dir "$P/render"
for f in captions.srt captions.vtt 16x9.mp4 9x16.mp4 1x1.mp4; do
  if test -s "$P/render/$f"; then pass "render/$f exists"; else fail "render/$f" "missing or empty"; fi
done

# Determinism probes: frames 0, 15, 30 forward, then the same frames in reverse.
ok "probe forward" node "$RENDER" --scene "$P/scenes/s01-hook/index.html" --out "$P/probe-forward" --fps $FPS --width $WIDTH --height $HEIGHT --duration 2.8 --frames 0,15,30
ok "probe reverse" node "$RENDER" --scene "$P/scenes/s01-hook/index.html" --out "$P/probe-reverse" --fps $FPS --width $WIDTH --height $HEIGHT --duration 2.8 --frames 0,15,30 --reverse

ok "qa_video passes the master" python3 "$QA" --video "$P/render/master.mp4" --timeline "$P/timeline.json" \
  --probe-forward "$P/probe-forward" --probe-reverse "$P/probe-reverse"
if python3 -c 'import json,sys; sys.exit(0 if json.load(open(sys.argv[1]))["passed"] is True else 1)' "$P/qa/render-check.json"; then
  pass "qa/render-check.json has passed: true"; else fail "qa/render-check.json" "passed is not true"; fi
for f in contact-sheet-360p.png hook-strip.png; do
  if test -s "$P/qa/$f"; then pass "qa/$f exists"; else fail "qa/$f" "missing or empty"; fi
done

# Negative: a black master, and a reverse probe that differs from the forward probe.
ffmpeg -hide_banner -v error -y -f lavfi -i "color=c=black:s=${WIDTH}x${HEIGHT}:r=$FPS:d=$DURATION" -i "$P/audio/mix.wav" \
  -c:v libx264 -preset ultrafast -pix_fmt yuv420p -c:a aac -shortest "$W/black.mp4"
bad "qa_video fails a black master" python3 "$QA" --video "$W/black.mp4" --timeline "$P/timeline.json" --out "$W/qa-black.json" \
  --contact-sheet "$W/black-sheet.png" --hook-strip "$W/black-strip.png" \
  --probe-forward "$P/probe-forward" --probe-reverse "$P/probe-reverse"
cp -R "$P/probe-reverse" "$W/probe-drift"
cp "$P/probe-forward/frame_000000.png" "$W/probe-drift/frame_000030.png"
bad "qa_video fails probes that differ" python3 "$QA" --video "$P/render/master.mp4" --timeline "$P/timeline.json" --out "$W/qa-drift.json" \
  --contact-sheet "$W/drift-sheet.png" --hook-strip "$W/drift-strip.png" \
  --probe-forward "$P/probe-forward" --probe-reverse "$W/probe-drift"

if [ "$FAILED" = 0 ]; then echo "video live check: all steps passed"; else echo "video live check: FAILED"; fi
exit "$FAILED"
