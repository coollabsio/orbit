#!/usr/bin/env bash
# Renders the Promo composition.
#   STYLE=synthwave|futurebass|bach scripts/render-promo.sh   → out/orbit-promo-<style>.mp4 (synthesized; see make-beat.mjs)
#   STYLE=none scripts/render-promo.sh                       → out/orbit-promo-silent.mp4 (no audio track)
#   MUSIC=out/interstellar.mp3 scripts/render-promo.sh        → out/orbit-promo-interstellar.mp4
# Mixing uses the system ffmpeg (Remotion's build has no afade). With MUSIC, the video is rendered silently and the track is added from MUSIC_START seconds (default 341: the
# climax of interstellar.mp3 lands on the end card at 39 s), with a short fade in and a 2-second fade out.
set -euo pipefail
cd "$(dirname "$0")/.."
if [ "${STYLE:-}" = none ]; then
  bun x remotion render Promo out/orbit-promo-silent.mp4 --codec=h264 --crf=16 --muted
  exit 0
fi
if [ -z "${MUSIC:-}" ]; then
  bun scripts/make-beat.mjs
  bun x remotion render Promo "out/orbit-promo-${STYLE:-synthwave}.mp4" --codec=h264 --crf=16 --audio-codec=aac --audio-bitrate=256k
  exit 0
fi
start=${MUSIC_START:-341}
out=out/orbit-promo-$(basename "${MUSIC%.*}").mp4
bun x remotion render Promo out/orbit-promo.silent.mp4 --codec=h264 --crf=16 --muted
duration=$(ffprobe -v error -show_entries format=duration -of csv=p=0 out/orbit-promo.silent.mp4)
ffmpeg -v error -y -i out/orbit-promo.silent.mp4 -ss "$start" -t "$duration" -i "$MUSIC" \
  -map 0:v -map 1:a -c:v copy -af "afade=t=in:d=0.4,afade=t=out:st=$(echo "$duration - 2" | bc):d=2" \
  -c:a aac -b:a 256k -shortest -movflags +faststart "$out"
rm out/orbit-promo.silent.mp4
echo "$out (music from ${start}s of $MUSIC)"
