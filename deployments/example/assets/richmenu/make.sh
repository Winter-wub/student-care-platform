#!/usr/bin/env bash
# Draws the example's LINE rich menu images (fictional Seal Bay University):
#   locked.png    the locked Menu: 1 cell
#   unlocked.png  the unlocked Menu: 3 cells (default Locale's entries)
#
# Run it again whenever the Menu changes, and update LOCKED_CELLS /
# UNLOCKED_CELLS in deployments/example/deployment/src/menu_images.rs to
# match: a test fails when the cell count and the Menu's entries differ.
#
#   deployments/example/assets/richmenu/make.sh
#
# Needs `rsvg-convert` (librsvg, which lays the text out with Pango, so Thai
# vowels and tone marks are shaped) and ImageMagick 7 (`magick`, to measure
# and assemble the cells). Plain ImageMagick text (`label:`, `-annotate`)
# does no complex text layout: leading vowels float away from their
# consonants. `brew install librsvg imagemagick`, or apt `librsvg2-bin
# imagemagick fonts-noto-core`.
# Fonts: FONT_TH (default Thonburi, macOS; falls back to Noto Sans Thai) and
# FONT_EN are CSS font-family lists for the SVG.
#
# The grid is the Core's (`harpseal_channel_line::render`): 1-3 entries in
# one row, 4 as 2 x 2, 5-6 as three columns, more as four columns; the
# image is 2500 px wide and 843 px high for one row, 1686 for more. Each
# cell is the image divided by the grid, the way `menu_sized` places the
# tap areas.
set -euo pipefail

cd "$(dirname "$0")"

for tool in rsvg-convert magick; do
  command -v "$tool" >/dev/null || { echo "$tool not found (see the top of this script)" >&2; exit 1; }
done

FONT_TH="${FONT_TH:-Thonburi, 'Noto Sans Thai', Sarabun, sans-serif}"
FONT_EN="${FONT_EN:-'Helvetica Neue', Helvetica, Arial, sans-serif}"

WIDTH=2500
BLUE='#0b5cad'      # branding.primary_color in deployment.toml
GREY='#5b6b7d'      # the quiet cell: leaving the service
LINE_COLOR='#ffffff'
EN_COLOR='#cfe2f7'
THAI_SIZE=130       # largest Thai label size; shrinks to fit a cell

# The Core's grid: columns for n entries.
cols_for() {
  case "$1" in
    1|2|3) echo "$1" ;;
    4) echo 2 ;;
    5|6) echo 3 ;;
    *) echo 4 ;;
  esac
}

# Width in px of "$1" at THAI_SIZE, as the SVG renderer lays it out.
text_width() {
  local tmp
  tmp=$(mktemp -d)
  cat >"$tmp/t.svg" <<SVG
<svg xmlns="http://www.w3.org/2000/svg" width="3000" height="300">
<text x="50" y="200" font-family="$FONT_TH" font-weight="bold" font-size="$THAI_SIZE" fill="#000">$1</text></svg>
SVG
  rsvg-convert "$tmp/t.svg" -o "$tmp/t.png"
  magick "$tmp/t.png" -background white -alpha remove -trim -format '%w' info:
  rm -rf "$tmp"
}

# cell OUT WIDTH HEIGHT FILL THAI ENGLISH
cell() {
  local out=$1 w=$2 h=$3 fill=$4 th=$5 en=$6
  local natural size
  natural=$(text_width "$th")
  size=$THAI_SIZE
  if [ "$natural" -gt $((w - 120)) ]; then
    size=$((THAI_SIZE * (w - 120) / natural))
  fi
  cat >"$out.svg" <<SVG
<svg xmlns="http://www.w3.org/2000/svg" width="$w" height="$h">
<rect width="$w" height="$h" fill="$fill"/>
<text x="$((w / 2))" y="$((h / 2 - 5))" text-anchor="middle" font-family="$FONT_TH" font-weight="bold" font-size="$size" fill="#fff">$th</text>
<text x="$((w / 2))" y="$((h / 2 + 90))" text-anchor="middle" font-family="$FONT_EN" font-size="56" fill="$EN_COLOR">$en</text>
<rect width="$w" height="$h" fill="none" stroke="$LINE_COLOR" stroke-width="12"/>
</svg>
SVG
  rsvg-convert "$out.svg" -o "$out"
}

# image OUT "thai|english|fill" ...
image() {
  local out=$1; shift
  local n=$# cols rows height cw ch i r c
  cols=$(cols_for "$n")
  rows=$(((n + cols - 1) / cols))
  height=$((rows == 1 ? 843 : 1686))
  cw=$((WIDTH / cols))
  ch=$((height / rows))
  local tmp
  tmp=$(mktemp -d)
  i=0
  for spec in "$@"; do
    IFS='|' read -r th en fill <<<"$spec"
    cell "$tmp/c$i.png" "$cw" "$ch" "$fill" "$th" "$en"
    i=$((i + 1))
  done
  local row_files=()
  for ((r = 0; r < rows; r++)); do
    local files=()
    for ((c = 0; c < cols; c++)); do
      i=$((r * cols + c))
      [ "$i" -lt "$n" ] && files+=("$tmp/c$i.png")
    done
    magick "${files[@]}" +append "$tmp/r$r.png"
    row_files+=("$tmp/r$r.png")
  done
  # `cols * cw` can be 1 px short of 2500 (three columns): pad to the exact
  # size LINE requires. -strip and no dates keep the output reproducible.
  magick "${row_files[@]}" -append \
    -background "$LINE_COLOR" -alpha remove -gravity west -extent "${WIDTH}x${height}" \
    -strip +set date:create +set date:modify "PNG24:$out"
  rm -rf "$tmp"
  echo "$out: $n cells, ${cols} x ${rows}, ${WIDTH} x ${height}"
}

# The labels are the example's Menu entries (th, with English small).
image locked.png \
  "เริ่มใช้งาน|Start|$BLUE"

image unlocked.png \
  "ข่าวล่าสุด|Latest news|$BLUE" \
  "ติดต่อเจ้าหน้าที่|Contact Staff|$BLUE" \
  "ยกเลิกการเชื่อมบัญชี|Unlink my account|$GREY"
