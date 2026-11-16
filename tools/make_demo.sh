#!/usr/bin/env bash
# Records a scripted demo scene and encodes it for the README.
#   tools/make_demo.sh fill
#   tools/make_demo.sh remember
# Needs a release build and ffmpeg. Uses made-up data only, on the
# fictional scanned form from tools/gen_media_form.py.
set -euo pipefail
scene=${1:?usage: tools/make_demo.sh <fill|remember>}
root=$(cd "$(dirname "$0")/.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/data" "$root/docs/media"
if [[ $scene == remember ]]; then
  # Two fictional people, so the suggestions have something to offer.
  cat > "$work/data/memory.json" <<'JSON'
{"values":{"full_name":[{"value":"Anna Lindqvist","count":5,"last_used":5},{"value":"Erik Lindqvist","count":2,"last_used":3}],
"email":[{"value":"anna@example.se","count":4,"last_used":5}],
"street_address":[{"value":"Storgatan 12","count":3,"last_used":4}]},
"profiles":[{"id":1,"values":{"full_name":"Anna Lindqvist","email":"anna@example.se","street_address":"Storgatan 12"},"roles":["applicant"],"count":3,"last_used":5},
{"id":2,"values":{"full_name":"Erik Lindqvist"},"roles":["child"],"count":1,"last_used":3}],
"settings":{"ai_enabled":false,"ai_consent":false,"model":"","remember":"ask"}}
JSON
fi
# Suggestions need the printed labels, which a scan doesn't have as text yet:
# the remember scene uses the same form before scanning.
form=household_scanned.pdf
[[ $scene == remember ]] && form=household_form.pdf
"$root/target/release/pdfr" "$root/samples/$form" --data-dir "$work/data" --record "$work/frames" --script "$scene"
crop="crop='min(iw,941)':'min(ih,640)':0:0"
ffmpeg -loglevel error -y -f concat -safe 0 -i "$work/frames/frames.ffconcat" \
  -vf "$crop,fps=30,scale=820:-2:flags=lanczos,format=yuv420p" -c:v libx264 -crf 22 -preset slow -movflags +faststart \
  "$root/docs/media/$scene.mp4"
ffmpeg -loglevel error -y -f concat -safe 0 -i "$work/frames/frames.ffconcat" \
  -vf "$crop,fps=15,scale=720:-1:flags=lanczos,split[a][b];[a]palettegen=max_colors=128:stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=4:diff_mode=rectangle" \
  "$root/docs/media/$scene.gif"
echo "docs/media/$scene.gif and .mp4 written"
