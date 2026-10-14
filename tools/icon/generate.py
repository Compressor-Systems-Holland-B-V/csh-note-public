#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["requests>=2"]
# ///
"""Generate the znote app icon with the Recraft API.

Three steps: render a raster icon, cut away its background so only the rounded tile is left on
full transparency, then vectorize that PNG into an SVG. Both files are kept.

The look follows the ZAL Analytics project art: a dark tile with white, grey and one muted mauve
accent, no text. The shape follows the Zed editor icon: a flat rounded square. Gradients are
avoided because the vectorizer turns them into bands.

    export RECRAFT_API_KEY=...
    uv run tools/icon/generate.py                 # writes packaging/icon.png and packaging/icon.svg
    uv run tools/icon/generate.py --out other.svg # writes other.png and other.svg
    uv run tools/icon/generate.py --dry-run       # print the request, send nothing

scripts/generate_icon does the same with the key pulled from Bitwarden. Then rebuild the icon set
with `pnpm tauri icon packaging/icon.png` and the msix tiles with tools/icon/msix-tiles.py.
"""

import argparse
import json
import os
import pathlib
import sys
import time

ROOT = pathlib.Path(__file__).resolve().parents[2]
API = "https://external.api.recraft.ai/v1"
MODEL = os.environ.get("RECRAFT_MODEL", "recraftv4_1")

BLACK = [18, 18, 20]
WHITE = [255, 255, 255]
GREY = [153, 153, 153]
MAUVE = [145, 111, 111]

# The backdrop only exists to be removed. It must not match any icon colour, or background
# removal cuts holes into the tile (a white backdrop took the white notebook cover with it).
BACKDROP = [120, 200, 120]

PROMPT = (
    "Professional desktop app icon for znote, a document editor with an AI assistant, in the style "
    "of the Zed editor and modern macOS icons. One rounded square tile with smooth continuous "
    "corners, filling most of the canvas with a small even margin, on a plain flat light green "
    "background. The tile is one flat solid neutral charcoal, almost black, no gradient, no "
    "highlight, no red or brown tint. Centered on the tile, large and bold, filling about sixty "
    "percent of it, a minimal closed notebook: an off-white cover joined to a slim grey spine on the "
    "left, and in the middle of the cover one four-pointed sparkle star in muted mauve with a "
    "smaller grey sparkle at its lower right. Clean geometric shapes, "
    "precise symmetric proportions, crisp edges, refined and premium, no outlines, no shadow "
    "outside the tile, no letters, no text, no numbers. Must stay readable at 32 pixels."
)


def request():
    return {
        "prompt": PROMPT,
        "model": MODEL,
        "size": "1:1",
        "image_format": "png",
        "response_format": "url",
        "controls": {
            "colors": [{"rgb": rgb} for rgb in (BLACK, WHITE, GREY, MAUVE)],
            "background_color": {"rgb": BACKDROP},
        },
    }


def call(session, path, **kwargs):
    """POST to a Recraft endpoint and download the image it points to."""
    for attempt in range(4):
        response = session.post(f"{API}/{path}", timeout=180, **kwargs)
        if response.status_code in (429, 500, 502, 503, 504) and attempt < 3:
            time.sleep(5 * 2**attempt)
            continue
        if not response.ok:
            sys.exit(f"recraft {path} said {response.status_code}: {response.text}")
        body = response.json()
        url = body["data"][0]["url"] if "data" in body else body["image"]["url"]
        return download(session, url)
    raise RuntimeError("unreachable")


def download(session, url):
    import requests

    for attempt in range(4):
        try:
            image = session.get(url, timeout=60)
            image.raise_for_status()
            return image.content
        except requests.RequestException:
            if attempt == 3:
                raise
            time.sleep(5 * 2**attempt)
    raise RuntimeError("unreachable")


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--out", type=pathlib.Path, default=ROOT / "packaging" / "icon.svg")
    parser.add_argument("--dry-run", action="store_true", help="print the request, send nothing")
    args = parser.parse_args()

    body = request()
    if args.dry_run:
        print(json.dumps(body, indent=2))
        return
    if not (key := os.environ.get("RECRAFT_API_KEY")):
        sys.exit("RECRAFT_API_KEY is not set")

    import requests

    session = requests.Session()
    session.headers["Authorization"] = f"Bearer {key}"

    raster = call(session, "images/generations", json=body)
    png = call(session, "images/removeBackground", files={"file": ("icon.png", raster)}, data={"image_format": "png"})
    if not png.startswith(b"\x89PNG"):
        sys.exit("recraft did not return a png")
    svg = call(session, "images/vectorize", files={"file": ("icon.png", png)})
    if b"<svg" not in svg[:2048]:
        sys.exit("recraft did not return an svg")

    args.out.parent.mkdir(parents=True, exist_ok=True)
    for path, data in ((args.out.with_suffix(".png"), png), (args.out.with_suffix(".svg"), svg)):
        path.write_bytes(data)
        print(f"wrote {path.relative_to(ROOT) if path.is_relative_to(ROOT) else path}")


if __name__ == "__main__":
    main()
