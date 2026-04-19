#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

input="${1:-}"
output="${2:-$repo_root/resources/screenshot.jpg}"
blur_region="${BLUR_REGION:-68x24+28+630}"

if ! command -v convert >/dev/null 2>&1; then
  echo "error: ImageMagick 'convert' is required" >&2
  exit 1
fi

if [[ -z "$input" ]]; then
  input="$(
    find "$HOME/Pictures" -type f \
      \( -iname '*.png' -o -iname '*.jpg' -o -iname '*.jpeg' -o -iname '*.webp' \) \
      -printf '%T@ %p\n' \
      | sort -nr \
      | awk 'NR == 1 { sub(/^[^ ]+ /, ""); print }'
  )"
fi

if [[ -z "$input" || ! -f "$input" ]]; then
  echo "error: screenshot not found: ${input:-<newest image under ~/Pictures>}" >&2
  exit 1
fi

mkdir -p "$(dirname "$output")"

if [[ "$blur_region" != *+*+* ]]; then
  echo "error: BLUR_REGION must look like WIDTHxHEIGHT+X+Y" >&2
  exit 1
fi

blur_offset="+${blur_region#*+}"

convert "$input" \
  \( -clone 0 -crop "$blur_region" +repage -blur 0x9 \) \
  -geometry "$blur_offset" -composite \
  -strip -quality 92 "$output"

echo "updated $output"
echo "source: $input"
echo "blur region: $blur_region"
