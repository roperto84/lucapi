"""LucAPI brand mark — regenerates assets/logo.png.

Run with Pillow:  python3 design/logo.py


Geometry on a 24-unit grid, supersampled for clean edges:
  tile   24 x 24, corner radius 6.84 (28.5%)
  glyph  "L" stroke 5.7, round terminals, outer elbow softened (r 0.8)
  colour vertical gradient #5B7CF5 -> #1E3AB4 with a faint inner rim
"""
import math
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter

OUT = Path(__file__).resolve().parent.parent / "assets" / "logo.png"

N = 512
SS = 8
G = N * SS
TOP, BOTTOM = (0x5B, 0x7C, 0xF5), (0x1E, 0x3A, 0xB4)

grad = Image.new("RGBA", (G, G))
px = grad.load()
for y in range(G):
    for x in range(G):
        t = (x * 0.22 + y * 0.78) / G
        px[x, y] = tuple(round(a + (b - a) * t) for a, b in zip(TOP, BOTTOM)) + (255,)

tile_mask = Image.new("L", (G, G), 0)
ImageDraw.Draw(tile_mask).rounded_rectangle([0, 0, G - 1, G - 1], radius=int(0.285 * G), fill=255)
logo = Image.new("RGBA", (G, G), (0, 0, 0, 0))
logo.paste(grad, (0, 0), tile_mask)

rim = Image.new("RGBA", (G, G), (0, 0, 0, 0))
w = int(0.011 * G)
ImageDraw.Draw(rim).rounded_rectangle([w, w, G - 1 - w, G - 1 - w], radius=int(0.284 * G - 1.0 * SS),
                                      outline=(255, 255, 255, 48), width=w)
logo = Image.alpha_composite(logo, rim.filter(ImageFilter.GaussianBlur(0.7 * SS)))

# ---- glyph
X0, Y0, X1, Y1 = 0.213, 0.186, 0.792, 0.810
STROKE = 0.238
ELBOW = 0.8 / 24.0
d = ImageDraw.Draw(logo)
def P(v):
    return int(v * G)
pts = [(X0, Y0 + STROKE / 2), (X0, Y1 - ELBOW)]
for i in range(9):
    a = math.radians(180 - 90 * i / 8)
    pts.append((X0 + ELBOW + ELBOW * math.cos(a), Y1 - ELBOW + ELBOW * math.sin(a)))
pts += [
    (X1 - STROKE / 2, Y1),
    (X1 - STROKE / 2, Y1 - STROKE),
    (X0 + STROKE, Y1 - STROKE),
    (X0 + STROKE, Y0 + STROKE / 2),
]
d.polygon([(P(x), P(y)) for x, y in pts], fill=(255, 255, 255, 255))
d.ellipse([P(X0), P(Y0), P(X0 + STROKE), P(Y0 + STROKE)], fill=(255, 255, 255, 255))
d.ellipse([P(X1 - STROKE), P(Y1 - STROKE), P(X1), P(Y1)], fill=(255, 255, 255, 255))

logo.resize((N, N), Image.LANCZOS).save(OUT)
print(f"wrote {OUT}")
