# shellcheck shell=bash
# Tinta Office identity for the Linux packages. Sourced by package.sh after env.sh.
#
# The desktop entry, AppStream metadata, MIME file and icons in this directory keep the release's
# names (ai.storyteller.*), so upstream changes to them merge cleanly. package.sh stages them as
# they are, and tinta_stage renames and rewrites the staged copies to this build's identity, which
# comes from the brand values in .cargo/config.toml: the same values the binaries are compiled
# with, so the window class (brand::APP_ID) always matches the desktop entry's StartupWMClass.
#
# Before sourcing, set:
#   BRAND_PREFIX    env prefix of the brand values, e.g. PDFCRAFT (PDFCRAFT_BRAND_NAME, ...)
#   UPSTREAM_ID     the release's app id, e.g. ai.storyteller.pdfcraft
#   UPSTREAM_NAME   the release's product name, e.g. PdfCraft
#   UPSTREAM_REPO   the release's repository, e.g. storytold/pdfcraft
#
# Sets BRAND_NAME, BRAND_SLUG, BRAND_ID, BRAND_REPO, BRAND_HOMEPAGE and BRAND_PUBLISHER.

# One brand value from .cargo/config.toml ([env] KEY = "value"); empty when absent.
brand_value() {
  awk -v key="$1" '
    $1 == key && $2 == "=" { line = $0; sub(/^[^=]*=[ \t]*"/, "", line); sub(/".*$/, "", line); print line; exit }
  ' "$ROOT/.cargo/config.toml"
  return
}

BRAND_NAME="$(brand_value "${BRAND_PREFIX}_BRAND_NAME")"
BRAND_SLUG="$(brand_value "${BRAND_PREFIX}_BRAND_SLUG")"
BRAND_ID="$(brand_value "${BRAND_PREFIX}_BRAND_APP_ID")"
BRAND_PUBLISHER="$(brand_value "${BRAND_PREFIX}_BRAND_PUBLISHER")"
BRAND_GITHUB="$(brand_value "${BRAND_PREFIX}_BRAND_GITHUB")"
for v in BRAND_NAME BRAND_SLUG BRAND_ID BRAND_PUBLISHER BRAND_GITHUB; do
  if [[ -z "${!v}" ]]; then
    echo "error: ${BRAND_PREFIX}_${v} is not set in $ROOT/.cargo/config.toml" >&2
    exit 1
  fi
done
BRAND_REPO="${BRAND_GITHUB#https://github.com/}"
BRAND_HOMEPAGE="https://tintaoffice.com"
BRAND_DEVELOPER_ID="${BRAND_ID%.*}"
export BRAND_NAME BRAND_SLUG BRAND_ID BRAND_REPO BRAND_HOMEPAGE BRAND_PUBLISHER

# Rewrite one text file from the release's identity to this build's.
brand_rewrite() {
  sed -i \
    -e "s|https://getartcraft.com/apps/[a-z]*|$BRAND_HOMEPAGE|g" \
    -e "s|https://getartcraft.com[a-z/]*|$BRAND_HOMEPAGE|g" \
    -e "s|https://discord.gg/artcraft|$BRAND_HOMEPAGE|g" \
    -e "s|$UPSTREAM_REPO|$BRAND_REPO|g" \
    -e "s|$UPSTREAM_ID|$BRAND_ID|g" \
    -e "s|developer id=\"ai.storyteller\"|developer id=\"$BRAND_DEVELOPER_ID\"|g" \
    -e "s|Learning Machines LLC|$BRAND_PUBLISHER|g" \
    -e "s|$UPSTREAM_NAME|$BRAND_NAME|g" \
    "$1"
  return
}

# Rename and rewrite the staged desktop entry, AppStream metadata, MIME file and icons, then fail
# if anything in the staged share/ tree still names the release's identity or the ArtCraft brand.
tinta_stage() {
  local share="$1/usr/share" f
  mv "$share/applications/$UPSTREAM_ID.desktop" "$share/applications/$BRAND_ID.desktop"
  mv "$share/mime/packages/$UPSTREAM_ID.xml" "$share/mime/packages/$BRAND_ID.xml"
  mv "$share/metainfo/$UPSTREAM_ID.metainfo.xml" "$share/metainfo/$BRAND_ID.metainfo.xml"
  brand_rewrite "$share/applications/$BRAND_ID.desktop"
  brand_rewrite "$share/mime/packages/$BRAND_ID.xml"
  brand_rewrite "$share/metainfo/$BRAND_ID.metainfo.xml"
  while IFS= read -r -d '' f; do
    mv "$f" "$(dirname "$f")/$BRAND_ID.${f##*.}"
  done < <(find "$share/icons" -name "$UPSTREAM_ID.*" -print0)
  if grep -rIil -e 'storyteller' -e 'artcraft' -e 'storytold' -e 'Learning Machines' \
    "$share/applications" "$share/mime" "$share/metainfo" >/dev/null; then
    grep -rIin -e 'storyteller' -e 'artcraft' -e 'storytold' -e 'Learning Machines' \
      "$share/applications" "$share/mime" "$share/metainfo" >&2
    echo "error: the staged Linux files still name the release's identity (above)" >&2
    exit 1
  fi
  if find "$share/icons" -name "$UPSTREAM_ID.*" | grep -q .; then
    echo "error: icons still named $UPSTREAM_ID" >&2
    exit 1
  fi
  return
}

# The nfpm config with this build's package name, vendor and homepage. The binaries keep their
# release names, so the package conflicts with the release's own package.
tinta_nfpm() {
  local upstream_pkg
  upstream_pkg="$(awk '$1 == "name:" { print $2; exit }' "$1")"
  sed \
    -e "s|^name: .*|name: $BRAND_SLUG\nconflicts:\n  - $upstream_pkg\nreplaces:\n  - $upstream_pkg|" \
    -e "s|^vendor: .*|vendor: $BRAND_PUBLISHER|" \
    -e "s|^homepage: .*|homepage: $BRAND_HOMEPAGE|" \
    "$1" >"$2"
  return
}
