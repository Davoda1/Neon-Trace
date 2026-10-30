"""Rebuild the original NeonTrace globe icon. Requires Pillow for development only."""
from pathlib import Path
from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parents[1]
SCALE = 4
im = Image.new("RGBA", (256 * SCALE, 256 * SCALE))
d = ImageDraw.Draw(im)

def box(values):
    return tuple(round(x * SCALE) for x in values)

d.rounded_rectangle(box((4, 4, 252, 252)), radius=55 * SCALE, fill="#10151c")
d.ellipse(box((40, 40, 216, 216)), outline="#419dff", width=8 * SCALE)
d.ellipse(box((86, 40, 170, 216)), outline="#3171ad", width=4 * SCALE)
for a, b, c, e in [(41, 128, 215, 128), (56, 79, 200, 79), (56, 177, 200, 177)]:
    d.line(box((a, b, c, e)), fill="#3171ad", width=4 * SCALE)
points = []
for i in range(201):
    t = i / 200
    x = (1-t)**2 * 48 + 2*(1-t)*t * 114 + t*t * 204
    y = (1-t)**2 * 166 + 2*(1-t)*t * 43 + t*t * 99
    points.append((round(x*SCALE), round(y*SCALE)))
d.line(points, fill="#9bffc1", width=11*SCALE, joint="curve")
d.ellipse(box((194, 89, 214, 109)), fill="#baffd3")
im = im.resize((256,256), Image.Resampling.LANCZOS)
im.save(ROOT / "assets/neontrace.png")
im.save(ROOT / "assets/neontrace.ico", sizes=[(16,16),(24,24),(32,32),(48,48),(64,64),(128,128),(256,256)])
im.resize((64,64),Image.Resampling.LANCZOS).tobytes()
(ROOT / "assets/neontrace-64.rgba").write_bytes(im.resize((64,64),Image.Resampling.LANCZOS).tobytes())
