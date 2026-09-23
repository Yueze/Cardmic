#!/usr/bin/env python3
"""Generate the Cardmic tray and app icons.

The tray mark is three dots in a triangle: compact and unlike any other
menu bar icon, so it is easy to spot among many.

    python3 make_icons.py        (needs Pillow; macOS iconutil for .icns)

Tray icons are raw RGBA (tray-*.rgba, square, size in the name) so the app
needs no image decoder. On macOS they are template images: only alpha
matters and the system tints them for light and dark menu bars. Windows gets
coloured variants that read on both light and dark taskbars.
"""
import os
import shutil
import subprocess

from PIL import Image, ImageDraw

HERE = os.path.dirname(os.path.abspath(__file__))
SS = 8  # supersampling

LIME = (153, 255, 0, 255)
INK = (24, 24, 24, 255)
GREY = (140, 140, 140, 255)
AMBER = (255, 176, 0, 255)
BLACK = (0, 0, 0, 255)


def mic(size_pt, px, fill, stroke, *, filled, slash=False, outline=None):
    """A microphone glyph on a size_pt x size_pt canvas rendered at px."""
    k = px * SS / size_pt  # supersampled pixels per point
    im = Image.new("RGBA", (px * SS, px * SS), (0, 0, 0, 0))
    d = ImageDraw.Draw(im)
    w = 1.5 * k  # stroke width

    def P(x, y):
        return (x * k, y * k)

    def glyph(col, extra=0.0):
        sw = w + extra * 2
        cap = [P(6.4 - extra / k, 1.4 - extra / k), P(11.6 + extra / k, 10.8 + extra / k)]
        r = 2.6 * k + extra
        if filled or extra:
            d.rounded_rectangle(cap, r, fill=col)
        else:
            d.rounded_rectangle(cap, r, outline=col, width=round(sw))
        # cradle: lower half of a circle around the capsule
        box = [P(3.6, 3.4), P(14.4, 13.4)]
        box = [(box[0][0] - extra, box[0][1] - extra), (box[1][0] + extra, box[1][1] + extra)]
        d.arc(box, 0, 180, fill=col, width=round(sw))
        d.line([P(9, 13.2), P(9, 16.0)], fill=col, width=round(sw))
        d.line([P(6.2, 16.2), P(11.8, 16.2)], fill=col, width=round(sw))

    if outline is not None:
        glyph(outline, extra=0.9 * k)
    glyph(fill if filled else stroke)
    if slash:
        # A gap in the colour of nothing, then the slash itself.
        d.line([P(2.6, 1.6), P(15.6, 16.8)], fill=(0, 0, 0, 0), width=round(w * 3.2))
        d.line([P(2.6, 1.6), P(15.6, 16.8)], fill=stroke, width=round(w))
    return im.resize((px, px), Image.LANCZOS)


def dots(px, fill, *, filled, alpha=255, keyline=None):
    """Cardmic's menu bar mark: three dots in a triangle, on an 18 pt canvas.

    Filled dots mean audio is flowing; rings mean waiting or off.
    """
    k = px * SS / 18.0
    im = Image.new("RGBA", (px * SS, px * SS), (0, 0, 0, 0))
    d = ImageDraw.Draw(im)
    r = 2.7  # dot radius, pt
    centres = [(9.0, 4.9), (4.3, 13.1), (13.7, 13.1)]  # equilateral, 9.4 pt side
    col = fill[:3] + (alpha,)
    for cx, cy in centres:
        if keyline is not None:
            e = r + 0.8
            d.ellipse([(cx - e) * k, (cy - e) * k, (cx + e) * k, (cy + e) * k], fill=keyline)
        box = [(cx - r) * k, (cy - r) * k, (cx + r) * k, (cy + r) * k]
        if filled:
            d.ellipse(box, fill=col)
        else:
            d.ellipse(box, outline=col, width=round(1.35 * k))
    return im.resize((px, px), Image.LANCZOS)


def save_rgba(im, name):
    with open(os.path.join(HERE, name), "wb") as f:
        f.write(im.tobytes())
    im.save(os.path.join(HERE, name.replace(".rgba", ".png")))


def tray_icons():
    # macOS template images: 18 pt tall menu bar icon, drawn at 2x. Only
    # alpha counts; the system tints them for light and dark menu bars.
    save_rgba(dots(36, BLACK, filled=False), "tray-mac-idle-36.rgba")
    save_rgba(dots(36, BLACK, filled=True), "tray-mac-live-36.rgba")
    save_rgba(dots(36, BLACK, filled=False, alpha=110), "tray-mac-off-36.rgba")
    # Windows: 32 px, coloured, with a dark keyline so lime reads on light taskbars.
    save_rgba(dots(32, GREY, filled=False), "tray-win-idle-32.rgba")
    save_rgba(dots(32, LIME, filled=True, keyline=INK), "tray-win-live-32.rgba")
    save_rgba(dots(32, GREY, filled=False, alpha=120), "tray-win-off-32.rgba")
    save_rgba(dots(32, AMBER, filled=False), "tray-win-warn-32.rgba")


def app_icon(px):
    """Black squircle, lime microphone: the device's own palette."""
    S = px * SS
    im = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    d = ImageDraw.Draw(im)
    inset = S * 0.098  # macOS icon grid: 824 of 1024 is the tile
    tile = [inset, inset, S - inset, S - inset]
    d.rounded_rectangle(tile, (S - 2 * inset) * 0.225, fill=(14, 14, 14, 255))
    # faint top highlight, fading out by mid-height
    fade = Image.linear_gradient("L").resize((S, S)).point(lambda v: max(0, 22 - v * 44 // 255))
    shape = Image.new("L", (S, S), 0)
    ImageDraw.Draw(shape).rounded_rectangle(tile, (S - 2 * inset) * 0.225, fill=255)
    hl = Image.new("RGBA", (S, S), (255, 255, 255, 0))
    hl.putalpha(Image.composite(fade, Image.new("L", (S, S), 0), shape))
    im.alpha_composite(hl)
    d = ImageDraw.Draw(im)
    glyph = mic(18, px, LIME, LIME, filled=True).resize((S, S), Image.LANCZOS)
    g = glyph.resize((int(S * 0.56), int(S * 0.56)), Image.LANCZOS)
    im.alpha_composite(g, (int(S * 0.22), int(S * 0.21)))
    # sound waves either side
    w = S * 0.022
    cx, cy = S * 0.5, S * 0.40
    for r, a in ((S * 0.27, 255), (S * 0.34, 150)):
        box = [cx - r, cy - r, cx + r, cy + r]
        d.arc(box, -38, 38, fill=LIME[:3] + (a,), width=round(w))
        d.arc(box, 142, 218, fill=LIME[:3] + (a,), width=round(w))
    return im.resize((px, px), Image.LANCZOS)


def app_icons():
    big = app_icon(1024)
    big.save(os.path.join(HERE, "app-1024.png"))
    ico_sizes = [16, 24, 32, 48, 64, 128, 256]
    big.save(os.path.join(HERE, "Cardmic.ico"), sizes=[(s, s) for s in ico_sizes])
    if shutil.which("iconutil"):
        iconset = os.path.join(HERE, "Cardmic.iconset")
        os.makedirs(iconset, exist_ok=True)
        for s in (16, 32, 128, 256, 512):
            big.resize((s, s), Image.LANCZOS).save(os.path.join(iconset, f"icon_{s}x{s}.png"))
            big.resize((s * 2, s * 2), Image.LANCZOS).save(os.path.join(iconset, f"icon_{s}x{s}@2x.png"))
        subprocess.run(["iconutil", "-c", "icns", iconset, "-o", os.path.join(HERE, "Cardmic.icns")], check=True)
        shutil.rmtree(iconset)


if __name__ == "__main__":
    tray_icons()
    app_icons()
    print("icons written to", HERE)
