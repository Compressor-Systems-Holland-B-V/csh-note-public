#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

usage() {
  cat <<'USAGE'
Usage:
  scripts/publish.sh                  build the frontend, then publish the crate to crates.io
  scripts/publish.sh --dry-run        package and compile it, upload nothing
  scripts/publish.sh --pages          regenerate packaging/repo/html from this version's release
  scripts/publish.sh --pages v1.2.0   from that tag instead

Any other flag goes to cargo publish, e.g. --no-verify. Publishing needs `cargo login`
or CARGO_REGISTRY_TOKEN, and a version on crates.io can never be replaced, so run
scripts/update_version.sh first. --pages needs jq and the gh cli, logged in or with GH_TOKEN.
USAGE
}

VERSION="$(node -p "require('./package.json').version")"
OUT_DIR=packaging/repo/html
TEMPLATE=tools/download-page/index.template.html
REPO_URL=https://github.com/ZAL-Analytics/znote

pages() {
  local TAG="${1:-v$VERSION}"

  for tool in gh jq; do
    command -v "$tool" >/dev/null || { echo "$tool is not installed, stopping" >&2; exit 1; }
  done

  if ! gh release view "$TAG" --repo "$REPO_URL" >/dev/null 2>&1; then
    echo "==> no $TAG release yet, leaving $OUT_DIR alone."
    echo "    the release workflow regenerates it once the packages are attached,"
    echo "    or run scripts/publish.sh --pages $TAG by hand afterwards"
    return 0
  fi

  echo "==> Reading the assets on $TAG"
  release="$(gh release view "$TAG" --repo "$REPO_URL" --json tagName,publishedAt,assets)"

  local VERSION="${TAG#v}"
  RELEASED_ON="$(jq -r '.publishedAt | split("T")[0]' <<<"$release")"
  RELEASE_URL="$REPO_URL/releases/tag/$TAG"

  catalog='
windows|*_x64-setup.exe|windows|Windows|10 and 11, 64-bit|Installer. Adds znote to the start menu and opens .znote files.
macos|*_universal.dmg|apple|macOS|Apple silicon and Intel|Disk image. Drag the app into your Applications folder.
linux|*_amd64.deb|debian|Debian and Ubuntu|.deb, 64-bit|For Debian 13 and later, Ubuntu 24.04 and later, Mint and Pop!_OS.
linux|*.x86_64.rpm|fedora|Fedora and openSUSE|.rpm, 64-bit|For Fedora, openSUSE and other rpm based systems.
linux|*.pkg.tar.zst|arch|Arch Linux|.pkg.tar.zst, 64-bit|For Arch, Manjaro and EndeavourOS.
'

  icon() {
    case "$1" in
      windows) echo '<path d="M0 3.4 9.8 2v9.4H0zm10.9-1.6L24 0v11.4H10.9zM0 12.6h9.8V22L0 20.6zm10.9 0H24V24l-13.1-1.8z"/>' ;;
      apple)   echo '<path d="M16.4 12.7c0-2.7 2.2-4 2.3-4.1-1.2-1.8-3.2-2-3.9-2.1-1.6-.2-3.2.9-4 .9s-2.1-.9-3.5-.9c-1.8 0-3.4 1-4.3 2.6-1.9 3.2-.5 8 1.3 10.6.9 1.3 1.9 2.7 3.3 2.6 1.3-.1 1.8-.8 3.4-.8s2 .8 3.4.8 2.3-1.3 3.2-2.5c1-1.5 1.4-2.9 1.4-3-.1 0-2.7-1-2.7-4.1zM13.8 4.3c.7-.9 1.2-2.1 1.1-3.3-1 0-2.3.7-3.1 1.6-.7.8-1.3 2-1.1 3.2 1.1.1 2.3-.6 3.1-1.5z"/>' ;;
      debian)  echo '<circle cx="12" cy="12" r="5.2" fill="none" stroke="currentColor" stroke-width="2"/><circle cx="12" cy="3.2" r="2.6"/><circle cx="4.4" cy="16.4" r="2.6"/><circle cx="19.6" cy="16.4" r="2.6"/>' ;;
      fedora)  echo '<path d="M12 0a12 12 0 0 0 0 24h5.7a6.3 6.3 0 0 0 6.3-6.3V12A12 12 0 0 0 12 0zm1.6 5.6a3.6 3.6 0 0 1 3.6 3.6 1.2 1.2 0 1 1-2.4 0 1.2 1.2 0 0 0-1.2-1.2 1.2 1.2 0 0 0-1.2 1.2v2.2h2a1.2 1.2 0 1 1 0 2.4h-2v1.4a3.6 3.6 0 1 1-3.6-3.6h1.2V9.2a3.6 3.6 0 0 1 3.6-3.6zM9.2 13.6a1.2 1.2 0 1 0 1.2 1.2v-1.2z"/>' ;;
      arch)    echo '<path d="M12 .8c.8 1.6 1.3 2.7 2.2 4.3-.6-.6-1.2-1-1.8-1.4.9 2.3 1.4 4.7 1.2 7.1-.1 2.6-1.1 5-2.7 7 1.6-.4 3.2-1.3 4.4-2.5-.1.7-.4 1.4-.9 2.1 2.1-1.4 3.2-3.4 3.4-5.4l4.6 10.2H1.6L12 .8zm.3 15.4c1 .6 1.9 1.4 2.5 2.4H9.2c.6-1 1.5-1.8 2.5-2.4z"/>' ;;
    esac
  }

  human_size() {
    awk -v b="$1" 'BEGIN { printf (b < 1048576) ? "%.0f KB\n" : "%.1f MB\n", (b < 1048576) ? b / 1024 : b / 1048576 }'
  }

  escape() {
    sed -e 's/&/\&amp;/g' -e 's/</\&lt;/g' -e 's/>/\&gt;/g' -e 's/"/\&quot;/g' <<<"$1"
  }

  card() {
    local os="$1" icon_name="$2" name="$3" note="$4" blurb="$5" button="$6"
    printf '%s\n' "            <li class=\"card\" data-os=\"$os\">
              <div class=\"card-head\">
                <svg viewBox=\"0 0 24 24\" aria-hidden=\"true\">$(icon "$icon_name")</svg>
                <div>
                  <div class=\"card-name\">$(escape "$name")</div>
                  <div class=\"card-note\">$(escape "$note")</div>
                </div>
                <span class=\"yours-flag\">Your system</span>
              </div>
              <p class=\"card-body\">$(escape "$blurb")</p>
              $button
            </li>"
  }

  cards=""
  missing=()

  while IFS='|' read -r os glob icon_name name note blurb; do
    [ -n "${os:-}" ] || continue

    asset="$(jq -c --arg glob "$glob" 'first(.assets[] | select(.name | test("^" + ($glob | gsub("\\."; "\\.") | gsub("\\*"; ".*")) + "$"))) // empty' <<<"$release")"
    if [ -z "$asset" ]; then
      missing+=("$name ($glob)")
      continue
    fi

    url="$(jq -r .url <<<"$asset")"
    size="$(human_size "$(jq -r .size <<<"$asset")")"
    cards+="$(card "$os" "$icon_name" "$name" "$note" "$blurb" \
      "<a class=\"button\" href=\"$url\" download>Download <span class=\"size\">$size</span></a>")
"
    echo "    $name -> $(jq -r .name <<<"$asset") ($size)"
  done <<<"$catalog"

  if [ ${#missing[@]} -gt 0 ]; then
    printf "::warning::$TAG has no asset for %s, that card is left off the page\n" "${missing[@]}" >&2
  fi

  if [ -z "$cards" ]; then
    echo "$TAG has no assets matching any known package, stopping" >&2
    exit 1
  fi

  echo "==> Writing $OUT_DIR"
  mkdir -p "$OUT_DIR"
  touch "$OUT_DIR/.nojekyll"
  cp src-tauri/icons/128x128.png "$OUT_DIR/favicon.png"

  cards_file="$(mktemp)"
  trap 'rm -f "$cards_file"' EXIT
  printf '%s' "$cards" > "$cards_file"

  sed -e "s|{{VERSION}}|$VERSION|g" \
      -e "s|{{RELEASED_ON}}|$RELEASED_ON|g" \
      -e "s|{{RELEASE_URL}}|$RELEASE_URL|g" \
      -e "s|{{REPO_URL}}|$REPO_URL|g" \
      -e "/{{CARDS}}/{ r $cards_file
d }" \
      "$TEMPLATE" > "$OUT_DIR/index.html"

  echo "==> $OUT_DIR/index.html now points at $TAG"
  echo "    commit $OUT_DIR to publish it, the pages workflow deploys from there"
}

case "${1:-}" in
  -h | --help) usage; exit 0 ;;
  --pages) pages "${2:-}"; exit 0 ;;
esac

for file in src-tauri/Cargo.toml src-tauri/tauri.conf.json; do
  grep -q "\"\\?$VERSION\"\\?" "$file" || {
    echo "$file does not carry version $VERSION, stopping" >&2
    exit 1
  }
done

echo "==> Building the frontend into src-tauri/dist"
[ -d node_modules ] || pnpm install --frozen-lockfile
pnpm build

echo "==> Publishing znote $VERSION to crates.io"
cargo publish --manifest-path src-tauri/Cargo.toml --allow-dirty "$@"
