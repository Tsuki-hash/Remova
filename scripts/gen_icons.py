"""Regenerate src-tauri icon set from a single square-ish source PNG.

Usage: python scripts/gen_icons.py <source.png> [icons_dir]
`icons_dir` defaults to <repo>/src-tauri/icons. Requires Pillow.
"""

import sys
from pathlib import Path

from PIL import Image

DEFAULT_ICONS = Path(__file__).resolve().parent.parent / "src-tauri" / "icons"

SIZES = {
    "icon.png": 512,
    "16x16.png": 16,
    "24x24.png": 24,
    "32x32.png": 32,
    "48x48.png": 48,
    "64x64.png": 64,
    "128x128.png": 128,
    "128x128@2x.png": 256,
    "256x256.png": 256,
    "Square310x310Logo.png": 310,
    "Square284x284Logo.png": 284,
    "Square150x150Logo.png": 150,
    "Square142x142Logo.png": 142,
    "Square107x107Logo.png": 107,
    "Square89x89Logo.png": 89,
    "Square71x71Logo.png": 71,
    "Square44x44Logo.png": 44,
    "Square30x30Logo.png": 30,
    "StoreLogo.png": 50,
    "icon_clean.png": 256,
}


def strip_near_white(im: Image.Image, threshold: int = 246) -> Image.Image:
    """Knock out flat near-white matte so tray/taskbar icons stay transparent."""
    px = im.load()
    w, h = im.size
    for y in range(h):
        for x in range(w):
            r, g, b, a = px[x, y]
            if a > 0 and r >= threshold and g >= threshold and b >= threshold:
                px[x, y] = (r, g, b, 0)
    return im


def fit_square(canvas: Image.Image, size: int) -> Image.Image:
    """Lanczos resize with a small safe margin so the glyph does not touch edges at 16px."""
    margin = max(1, size // 16)
    inner = size - margin * 2
    scaled = canvas.resize((inner, inner), Image.Resampling.LANCZOS)
    out = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    out.paste(scaled, (margin, margin), scaled)
    return out


def main(argv: list[str]) -> int:
    if len(argv) < 1:
        print(__doc__)
        return 2
    src = Path(argv[0])
    icons = Path(argv[1]) if len(argv) > 1 else DEFAULT_ICONS
    if not src.is_file():
        print(f"source image not found: {src}")
        return 1
    icons.mkdir(parents=True, exist_ok=True)

    im = Image.open(src).convert("RGBA")
    print("src", im.size)
    im = strip_near_white(im)
    w, h = im.size
    side = max(w, h)
    # Transparent canvas — opaque white boxed the taskbar/tray icon and softened edges.
    canvas = Image.new("RGBA", (side, side), (0, 0, 0, 0))
    canvas.paste(im, ((side - w) // 2, (side - h) // 2), im)
    for name, s in SIZES.items():
        fit_square(canvas, s).save(icons / name, "PNG", optimize=True)
        print("wrote", name, s)
    canvas.save(
        icons / "icon.ico",
        format="ICO",
        sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)],
    )
    print("wrote icon.ico")
    pub = icons.parent.parent / "public"
    pub.mkdir(parents=True, exist_ok=True)
    fit_square(canvas, 128).save(pub / "logo.png", "PNG")
    print("wrote public/logo.png")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
