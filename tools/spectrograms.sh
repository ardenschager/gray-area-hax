#!/bin/sh
# Spectrogram PNG next to every WAV in the given directories.
#   tools/spectrograms.sh DIR [DIR ...]
for d in "$@"; do
  for f in "$d"/*.wav; do
    ffmpeg -hide_banner -loglevel error -y -i "$f" \
      -lavfi "showspectrumpic=s=720x240:mode=combined:color=intensity:scale=log:fscale=log:legend=1" \
      "${f%.wav}.png"
  done
done
