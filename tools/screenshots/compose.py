"""The README's hero: the Cardputer's screen (a pixel-exact capture from
`cardmic screenshot --scale 3`, in a plain bezel) beside the app's window
(a capture from render.swift, with its system shadow).

    python3 tools/screenshots/compose.py WINDOW.png DEVICE_SCREEN.png OUT.png
"""
import sys
from PIL import Image, ImageDraw, ImageFilter

BLUR, DY = 46, 34


def rounded(size, r):
    m = Image.new("L", size, 0)
    ImageDraw.Draw(m).rounded_rectangle((0, 0, size[0] - 1, size[1] - 1), r, fill=255)
    return m


def device_panel(path, scale=4, pad=44, radius=44):
    raw = Image.open(path).convert("RGB")
    raw = raw.resize((raw.width // 3, raw.height // 3), Image.NEAREST)        # back to 240 x 135
    screen = raw.resize((raw.width * scale, raw.height * scale), Image.NEAREST)
    w, h = screen.width + 2 * pad, screen.height + 2 * pad
    body = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    body.paste((20, 20, 20, 255), (0, 0, w, h), rounded((w, h), radius))
    edge = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    ImageDraw.Draw(edge).rounded_rectangle((1, 1, w - 2, h - 2), radius - 1, outline=(58, 58, 58, 255), width=2)
    body.alpha_composite(edge)
    body.paste(screen, (pad, pad), rounded(screen.size, 10))
    return body


def shadow(img, alpha=0.55):
    a = img.getchannel("A").point(lambda v: int(v * alpha))
    sh = Image.new("RGBA", (img.width + 6 * BLUR, img.height + 6 * BLUR + DY), (0, 0, 0, 0))
    m = Image.new("L", sh.size, 0)
    m.paste(a, (3 * BLUR, 3 * BLUR + DY))
    sh.putalpha(m.filter(ImageFilter.GaussianBlur(BLUR)))
    return sh


win = Image.open(sys.argv[1]).convert("RGBA")
panel = device_panel(sys.argv[2])
# The opaque window inside the capture; the rest is its shadow.
wx0, wy0, wx1, wy1 = win.getchannel("A").point(lambda v: 255 if v >= 250 else 0).getbbox()
margin, gap = 140, 96
canvas = Image.new("RGBA", (margin + panel.width + gap + (win.width - wx0), win.height), (0, 0, 0, 0))
py = wy0 + (wy1 - wy0 - panel.height) // 2
canvas.alpha_composite(shadow(panel), (margin - 3 * BLUR, py - 3 * BLUR))
canvas.alpha_composite(panel, (margin, py))
canvas.alpha_composite(win, (margin + panel.width + gap - wx0, 0))
canvas.save(sys.argv[3], optimize=True)
print(canvas.size, "->", sys.argv[3])
