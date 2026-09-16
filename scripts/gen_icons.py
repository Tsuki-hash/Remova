from PIL import Image
from pathlib import Path

src = Path(r"D:\迅雷下载\绘制应用图标高清版.png")
im = Image.open(src).convert("RGBA")
print("src", im.size)
w, h = im.size
side = max(w, h)
canvas = Image.new("RGBA", (side, side), (255, 255, 255, 255))
canvas.paste(im, ((side - w) // 2, (side - h) // 2), im)
icons = Path(r"D:\Agent-Project\XiaomiMiMoProjects\Remova-next\src-tauri\icons")
sizes = {
    "icon.png": 512,
    "32x32.png": 32,
    "128x128.png": 128,
    "128x128@2x.png": 256,
    "64x64.png": 64,
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
for name, s in sizes.items():
    canvas.resize((s, s), Image.Resampling.LANCZOS).save(icons / name, "PNG", optimize=True)
    print("wrote", name, s)
canvas.save(
    icons / "icon.ico",
    format="ICO",
    sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)],
)
print("wrote icon.ico")
pub = Path(r"D:\Agent-Project\XiaomiMiMoProjects\Remova-next\public")
pub.mkdir(exist_ok=True)
canvas.resize((128, 128), Image.Resampling.LANCZOS).save(pub / "logo.png", "PNG")
print("wrote public/logo.png")
