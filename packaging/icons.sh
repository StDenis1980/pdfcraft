#!/usr/bin/env bash
# Regenerate the app icons and the Windows PDF file icon.
#
# The app icon (the IberOffice azulejo tile: pdfcraft.svg, pdfcraft-small.svg, the PNGs, .ico and
# .icns) is drawn in code by packaging/iberoffice-icons.py, which needs python3 with cairosvg and
# Pillow. The PDF file icon is rendered from its master vector assets/app-icon/pdfcraft-document.svg
# with resvg. The outputs are committed, so building and packaging never need these tools. After
# running it, update the sha256 values in ATTRIBUTION.toml, then `cargo xtask assets --write`.
#
#   packaging/icons.sh
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIR="$ROOT/assets/app-icon"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

python3 "$ROOT/packaging/iberoffice-icons.py" apply "$ROOT" pdf pdfcraft

command -v resvg >/dev/null || { echo "error: resvg not found (brew install resvg)" >&2; exit 1; }
render() { resvg -w "$2" -h "$2" "$1" "$3" </dev/null; }

# Windows PDF file icon: PNG-compressed entries, 16-256 px. It has its own resource in
# pdfcraft.exe; the app icon remains the first/default icon.
ICO_PNGS=()
for s in 16 20 24 32 40 48 64 128 256; do
  render "$DIR/pdfcraft-document.svg" "$s" "$TMP/pdfcraft-document-$s.png"
  ICO_PNGS+=("$TMP/pdfcraft-document-$s.png")
done
python3 - "$DIR/pdfcraft-document.ico" "${ICO_PNGS[@]}" <<'PY'
import struct, sys
out, pngs = sys.argv[1], sys.argv[2:]
blobs = [open(p, "rb").read() for p in pngs]
head = struct.pack("<HHH", 0, 1, len(blobs))
entries, data, offset = b"", b"", 6 + 16 * len(blobs)
for b in blobs:
    w, h = struct.unpack(">II", b[16:24])  # IHDR
    entries += struct.pack("<BBBBHHII", w % 256, h % 256, 0, 0, 1, 32, len(b), offset)
    data += b
    offset += len(b)
open(out, "wb").write(head + entries + data)
PY

echo "icons written to $DIR"
