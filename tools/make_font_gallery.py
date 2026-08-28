"""Renders every installed font the way topclock would, as sheets for the README.

    python tools/make_font_gallery.py

Calls the same GDI entry points the app does -- CreateFontW with the same
height, weight and quality, TextOutW into a 32bpp DIB -- so a row in the gallery
is what you actually get from setting `font` to that name. Driving the real app
once per font would mean roughly 1500 window launches; this takes seconds.

Each font gets four renders: antialiasing off and on, each composited over white
and over black, since a transparent clock looks quite different against each.
"""

import ctypes
import os
from ctypes import wintypes

from PIL import Image, ImageDraw, ImageFont

gdi32 = ctypes.WinDLL("gdi32", use_last_error=True)
user32 = ctypes.WinDLL("user32", use_last_error=True)

# Must match the app's defaults, or the gallery is a picture of something else.
FONT_SIZE = 26
WEIGHT = 700
FG = (0x00, 0xE6, 0xAA)
SAMPLE = "88:88"  # measured, as in the app
SHOWN = "12:34"  # drawn

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(ROOT, "docs", "fonts")
PER_SHEET = 48
ZOOM = 2

# The shortlist that gets its own compact sheet for the README. The full
# gallery is too tall to embed, so this is what a reader actually sees.
SUGGESTED = [
    "OCR A Extended",
    "MS Gothic",
    "NSimSun",
    "Consolas",
    "Cascadia Mono SemiBold",
    "Segoe UI Semibold",
    "Arial Black",
    "Franklin Gothic Demi",
]


class BITMAPINFOHEADER(ctypes.Structure):
    _fields_ = [
        ("biSize", wintypes.DWORD),
        ("biWidth", ctypes.c_long),
        ("biHeight", ctypes.c_long),
        ("biPlanes", wintypes.WORD),
        ("biBitCount", wintypes.WORD),
        ("biCompression", wintypes.DWORD),
        ("biSizeImage", wintypes.DWORD),
        ("biXPelsPerMeter", ctypes.c_long),
        ("biYPelsPerMeter", ctypes.c_long),
        ("biClrUsed", wintypes.DWORD),
        ("biClrImportant", wintypes.DWORD),
    ]


class BITMAPINFO(ctypes.Structure):
    _fields_ = [("bmiHeader", BITMAPINFOHEADER), ("bmiColors", wintypes.DWORD * 3)]


def coverage(face, antialias, text, canvas):
    """Rasterizes `text` and returns (pixels, width, height) of a coverage mask.

    White on black, exactly as the app does it, so the value in any channel is
    how much of that pixel the glyph covers.
    """
    w = h = canvas
    hdc = gdi32.CreateCompatibleDC(None)
    bmi = BITMAPINFO()
    bmi.bmiHeader.biSize = ctypes.sizeof(BITMAPINFOHEADER)
    bmi.bmiHeader.biWidth = w
    bmi.bmiHeader.biHeight = -h  # top-down
    bmi.bmiHeader.biPlanes = 1
    bmi.bmiHeader.biBitCount = 32
    bmi.bmiHeader.biCompression = 0

    bits = ctypes.c_void_p()
    bmp = gdi32.CreateDIBSection(hdc, ctypes.byref(bmi), 0, ctypes.byref(bits), None, 0)
    old_bmp = gdi32.SelectObject(hdc, bmp)

    # 4 = ANTIALIASED_QUALITY, 3 = NONANTIALIASED_QUALITY.
    font = gdi32.CreateFontW(
        FONT_SIZE, 0, 0, 0, WEIGHT, 0, 0, 0, 1, 0, 0,
        4 if antialias else 3, 0, face,
    )
    old_font = gdi32.SelectObject(hdc, font)
    gdi32.SetBkMode(hdc, 1)  # TRANSPARENT
    gdi32.SetTextColor(hdc, 0x00FFFFFF)
    origin = FONT_SIZE
    gdi32.TextOutW(hdc, origin, origin, text, len(text))
    gdi32.GdiFlush()

    buf = (ctypes.c_uint32 * (w * h)).from_address(bits.value)
    px = list(buf)

    gdi32.SelectObject(hdc, old_font)
    gdi32.DeleteObject(font)
    gdi32.SelectObject(hdc, old_bmp)
    gdi32.DeleteObject(bmp)
    gdi32.DeleteDC(hdc)
    return px, w, h


def ink_box(px, w, h):
    """Bounds of the drawn pixels. Row slices first: most rows are empty, and
    skipping them whole is far cheaper than testing every pixel."""
    l, t, r, b = w, h, -1, -1
    for y in range(h):
        row = px[y * w:(y + 1) * w]
        if not any(row):
            continue
        t = min(t, y)
        b = max(b, y)
        if l > 0:
            for x in range(l):
                if row[x]:
                    l = x
                    break
        if r < w - 1:
            for x in range(w - 1, r, -1):
                if row[x]:
                    r = x
                    break
    return None if r < l else (l, t, r, b)


def render_pair(face, antialias):
    """The clock for one font and antialias setting, over light and over dark.

    The two backdrops share a single rasterization: with bg_alpha at 0 the
    premultiplied color is just fg*cov and the alpha is cov, so compositing is
    one lerp per channel and needs no further GDI work.
    """
    canvas = FONT_SIZE * 8
    mask, w, h = coverage(face, antialias, SAMPLE, canvas)
    box = ink_box(mask, w, h)
    if not box:
        return None
    l, t, r, b = box
    cw, ch = r - l + 1, b - t + 1
    if not (2 <= cw <= canvas and 2 <= ch <= canvas):
        return None

    shown, _, _ = coverage(face, antialias, SHOWN, canvas)
    covs = [
        [(shown[(y + t) * w + (x + l)] >> 8) & 0xFF for x in range(cw)]
        for y in range(ch)
    ]
    out = []
    for backdrop in ((255, 255, 255), (0, 0, 0)):
        im = Image.new("RGB", (cw, ch))
        px = im.load()
        for y in range(ch):
            row = covs[y]
            for x in range(cw):
                cov = row[x]
                px[x, y] = (
                    (FG[0] * cov + backdrop[0] * (255 - cov)) // 255,
                    (FG[1] * cov + backdrop[1] * (255 - cov)) // 255,
                    (FG[2] * cov + backdrop[2] * (255 - cov)) // 255,
                )
        out.append(im)
    return out


def families():
    import subprocess
    ps = (
        "Add-Type -AssemblyName System.Drawing;"
        "(New-Object System.Drawing.Text.InstalledFontCollection).Families |"
        " ForEach-Object { $_.Name }"
    )
    out = subprocess.run(
        ["powershell", "-NoProfile", "-Command", ps],
        capture_output=True, text=True, encoding="utf-8",
    ).stdout
    skip = ("Wingdings", "Webdings", "Symbol", "MT Extra", "Marlett", "MDL2",
            "Fluent Icons", "Emoji")
    names = [n.strip() for n in out.splitlines() if n.strip()]
    return [n for n in names if not any(s in n for s in skip)]


def suggested_sheet():
    """A small sheet of the shortlist at default settings, for embedding.

    Two columns rather than four: antialiasing is off by default, so this shows
    what you actually get, over a light and a dark desktop.
    """
    z = 4
    name_font = ImageFont.truetype(r"C:\Windows\Fonts\segoeuib.ttf", 17)
    note_font = ImageFont.truetype(r"C:\Windows\Fonts\segoeui.ttf", 13)

    rendered = []
    for face in SUGGESTED:
        pair = render_pair(face, False)
        if pair:
            rendered.append((face, pair))

    labelw = 250
    cellw = max(p[0].width for _, p in rendered) * z + 28
    rowh = max(p[0].height for _, p in rendered) * z + 30
    gap = 26  # the top pick sits apart from the rest
    W = labelw + cellw * 2
    H = 46 + rowh * len(rendered) + gap

    sheet = Image.new("RGB", (W, H), (32, 32, 36))
    d = ImageDraw.Draw(sheet)
    for i, head in enumerate(("over a light desktop", "over a dark desktop")):
        d.text((labelw + cellw * i, 16), head, font=note_font, fill=(150, 150, 158))

    for r, (face, pair) in enumerate(rendered):
        y = 46 + rowh * r + (gap if r else 0)
        if r == 1:
            d.line([(12, y - gap // 2), (W - 12, y - gap // 2)], fill=(70, 70, 78))
        d.text((14, y + rowh // 2 - 14), face, font=name_font, fill=(230, 230, 236))
        d.text((14, y + rowh // 2 + 6),
               f"{pair[0].width}x{pair[0].height} px", font=note_font, fill=(140, 140, 148))
        for c, v in enumerate(pair):
            big = v.resize((v.width * z, v.height * z), Image.NEAREST)
            sheet.paste(big, (labelw + cellw * c, y + (rowh - big.height) // 2))

    path = os.path.join(OUT, "suggested.png")
    sheet.convert("P", palette=Image.ADAPTIVE, colors=256).save(path, optimize=True)
    print(f"  {path}  {sheet.size[0]}x{sheet.size[1]}  "
          f"{os.path.getsize(path) // 1024} KB")


def main():
    os.makedirs(OUT, exist_ok=True)
    suggested_sheet()
    label = ImageFont.truetype(r"C:\Windows\Fonts\segoeuib.ttf", 14)
    small = ImageFont.truetype(r"C:\Windows\Fonts\segoeui.ttf", 12)

    rows = []
    for face in families():
        off = render_pair(face, False)
        on = render_pair(face, True)
        if off and on:
            rows.append((face, off + on))
    print(f"rendered {len(rows)} families")

    headers = ["antialias off / light", "antialias off / dark",
               "antialias on / light", "antialias on / dark"]
    colw = 230
    rowh = 74
    sheets = (len(rows) + PER_SHEET - 1) // PER_SHEET
    for s in range(sheets):
        chunk = rows[s * PER_SHEET:(s + 1) * PER_SHEET]
        W = 250 + colw * 4
        H = 44 + rowh * len(chunk)
        sheet = Image.new("RGB", (W, H), (32, 32, 36))
        d = ImageDraw.Draw(sheet)
        for c, head in enumerate(headers):
            d.text((250 + colw * c + 8, 16), head, font=small, fill=(150, 150, 158))
        for i, (face, variants) in enumerate(chunk):
            y = 44 + rowh * i
            d.text((12, y + rowh // 2 - 10), face, font=label, fill=(226, 226, 232))
            for c, v in enumerate(variants):
                big = v.resize((v.width * ZOOM, v.height * ZOOM), Image.NEAREST)
                sheet.paste(big, (250 + colw * c + 8, y + (rowh - big.height) // 2))
        path = os.path.join(OUT, f"gallery-{s + 1:02d}.png")
        sheet.convert("P", palette=Image.ADAPTIVE, colors=256).save(path, optimize=True)
        print(f"  {path}  {sheet.size[0]}x{sheet.size[1]}  "
              f"{os.path.getsize(path) // 1024} KB")


if __name__ == "__main__":
    main()
