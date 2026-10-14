#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["requests>=2"]
# ///
"""Generate the znote app icon as a square SVG with the Recraft API.

The look follows the ZAL Analytics project art: flat shapes on black, white and
grey with one muted mauve accent, no gradients and no text.

    export RECRAFT_API_KEY=...
    uv run tools/icon/generate.py                 # writes packaging/icon.svg
    uv run tools/icon/generate.py --out other.svg
    uv run tools/icon/generate.py --dry-run       # print the request, send nothing

scripts/generate_icon does the same with the key pulled from Bitwarden. Then
rebuild the icon set with `pnpm tauri icon packaging/icon.svg`.
"""

import argparse
import json
import os
import pathlib
import sys
import time

ROOT = pathlib.Path(__file__).resolve().parents[2]
API = "https://external.api.recraft.ai/v1/images/generations"
MODEL = os.environ.get("RECRAFT_MODEL", "recraftv4_1_vector")

BLACK = [0, 0, 0]
WHITE = [255, 255, 255]
GREY = [153, 153, 153]
MAUVE = [145, 111, 111]

PROMPT = (
    "App icon for znote, a note-taking and document editor with an AI assistant. A rounded square "
    "black tile filling the whole canvas. On it, a closed notebook seen from the front, white cover "
    "with a grey spine and grey binding rings on the left side, and on the cover one four-pointed "
    "AI sparkle star in muted mauve with a smaller grey sparkle beside it. Flat minimal vector, "
    "thick rounded shapes, generous spacing, centered, no gradients, no shadows, no outlines, no "
    "letters, no text, no numbers. Must stay readable at 32 pixels."
)


def request():
    return {
        "prompt": PROMPT,
        "model": MODEL,
        "size": "1:1",
        "response_format": "url",
        "controls": {
            "colors": [{"rgb": rgb} for rgb in (WHITE, GREY, MAUVE)],
            "background_color": {"rgb": BLACK},
        },
    }


def generate(body, session):
    for attempt in range(4):
        response = session.post(API, json=body, timeout=180)
        if response.status_code in (429, 500, 502, 503, 504) and attempt < 3:
            time.sleep(5 * 2**attempt)
            continue
        if not response.ok:
            sys.exit(f"recraft said {response.status_code}: {response.text}")
        image = session.get(response.json()["data"][0]["url"], timeout=60)
        image.raise_for_status()
        return image.content
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
    svg = generate(body, session)
    if b"<svg" not in svg[:2048]:
        sys.exit(f"recraft did not return an svg, model {MODEL}")
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_bytes(svg)
    print(f"wrote {args.out.relative_to(ROOT) if args.out.is_relative_to(ROOT) else args.out}")


if __name__ == "__main__":
    main()
