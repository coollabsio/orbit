#!/usr/bin/env bash
# Renders the video silently, then adds the music straight from the source file as AAC-LC at the source sample
# rate (no resampling or volume processing). AAC, not MP3-in-MP4: QuickTime, Safari, iOS and X play MP4 audio only as AAC.
# MUSIC_START (seconds into the track): 296 puts the 6:20 climax on the end card and ends as the track fades out.
set -euo pipefail
cd "$(dirname "$0")/.."
music=${MUSIC:-out/interstellar.mp3}
start=${MUSIC_START:-296}
bun x remotion render LaunchVideo out/orbit-launch.silent.mp4 --codec=h264 --crf=16 --muted
bun x remotion ffmpeg -v error -y -i out/orbit-launch.silent.mp4 -ss "$start" -i "$music" \
  -map 0:v -map 1:a -c:v copy -c:a libfdk_aac -b:a 320k -shortest -movflags +faststart out/orbit-launch.mp4
rm out/orbit-launch.silent.mp4
echo "out/orbit-launch.mp4 (music from ${start}s of $music)"
