#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["pillow>=10"]
# ///
"""Build the msix Start tiles from packaging/icon.png.

The app icon is a rounded dark plate on transparency, which looks wrong on a Windows tile: the
transparent corners show the tile colour around a smaller rounded square. The tiles instead fill
the whole canvas with the plate colour and centre the notebook on it, like the built-in Windows
tiles. Square44x44Logo and StoreLogo stay the rounded icon from src-tauri/icons, they are drawn
in the app list, taskbar and Store, not as tiles.

    uv run tools/icon/msix-tiles.py   # writes packaging/msix/Assets/*.png

Run it after tools/icon/generate.py. Keep PLATE equal to BackgroundColor in
packaging/msix/AppxManifest.xml.in.
"""

import pathlib

from PIL import Image

ROOT = pathlib.Path(__file__).resolve().parents[2]
OUT = ROOT / "packaging" / "msix" / "Assets"
PLATE = (14, 13, 16)  # the icon's plate, sampled from packaging/icon.png
GLYPH = 0.55  # notebook height as a share of the tile height

TILES = {
    "Square71x71Logo": (71, 71),
    "Square150x150Logo": (150, 150),
    "Square310x310Logo": (310, 310),
    "Wide310x150Logo": (310, 150),
}


def glyph():
    """The notebook cut out of the icon, on the plate colour."""
    icon = Image.open(ROOT / "packaging" / "icon.png").convert("RGBA")
    flat = Image.new("RGBA", icon.size, PLATE + (255,))
    flat.alpha_composite(icon)
    # everything clearly brighter than the plate is notebook; the transparent margin went dark
    box = flat.convert("L").point(lambda v: 255 if v > 60 else 0).getbbox()
    return flat.crop(box).convert("RGB")


def main():
    notebook = glyph()
    for name, (w, h) in TILES.items():
        gh = round(h * GLYPH)
        gw = round(notebook.width * gh / notebook.height)
        tile = Image.new("RGB", (w, h), PLATE)
        tile.paste(notebook.resize((gw, gh), Image.Resampling.LANCZOS), ((w - gw) // 2, (h - gh) // 2))
        tile.save(OUT / f"{name}.png")
        print(f"wrote {(OUT / name).relative_to(ROOT)}.png")


if __name__ == "__main__":
    main()
