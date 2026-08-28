"""Generates assets/topclock.ico from the clock's own colors and font.

Run it after changing the palette to keep the icon in step with the app:

    python tools/make_icon.py

Each size is drawn at its own scale rather than downsampled from one bitmap, so
the 16px entry stays legible instead of turning to mush. Sizes at or above 32px
show two rows of digits; 16px only has room for one.
"""

import os
import struct
import zlib
from PIL import Image, ImageDraw, ImageFont

BG = (0x0C, 0x0C, 0x10, 255)  # near-black, matches the ini default
FG = (0x00, 0xE6, 0xAA, 255)  # mint green
FONT = r"C:\Windows\Fonts\consolab.ttf"  # Consolas Bold

SIZES = [256, 128, 64, 48, 32, 16]
SS = 4  # supersample factor; the whole thing is drawn big and shrunk down

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(ROOT, "assets", "topclock.ico")


def fit_font(lines, target_w):
    """Largest font size whose widest line fits target_w.

    Measured on the ink of the real digits, not on a sample like "88": glyphs
    such as 1 carry wide side bearings, and fitting the advance width instead
    of the ink leaves the small sizes looking shrunken inside the tile.
    """
    lo, hi = 4, 4000
    while lo < hi:
        mid = (lo + hi + 1) // 2
        font = ImageFont.truetype(FONT, mid)
        widest = max(font.getbbox(t)[2] - font.getbbox(t)[0] for t in lines)
        if widest <= target_w:
            lo = mid
        else:
            hi = mid - 1
    return ImageFont.truetype(FONT, lo)


def ink(text, font, canvas):
    """The text cropped to its own ink, so centering ignores font metrics."""
    tmp = Image.new("RGBA", (canvas * 2, canvas * 2), (0, 0, 0, 0))
    ImageDraw.Draw(tmp).text((canvas // 2, canvas // 2), text, font=font, fill=FG)
    return tmp.crop(tmp.getbbox())


def render(size):
    s = size * SS
    img = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    ImageDraw.Draw(img).rounded_rectangle(
        [0, 0, s - 1, s - 1], radius=int(s * 0.22), fill=BG
    )

    if size >= 32:
        lines, width, gap = ["12", "34"], 0.62, int(s * 0.06)
    else:
        lines, width, gap = ["12"], 0.78, 0

    font = fit_font(lines, int(s * width))
    inks = [ink(t, font, s) for t in lines]

    total_h = sum(i.height for i in inks) + gap * (len(inks) - 1)
    y = (s - total_h) // 2
    for layer in inks:
        img.alpha_composite(layer, ((s - layer.width) // 2, y))
        y += layer.height + gap

    return img.resize((size, size), Image.LANCZOS)


def png_bytes(img):
    def chunk(tag, data):
        c = tag + data
        return struct.pack(">I", len(data)) + c + struct.pack(">I", zlib.crc32(c))

    w, h = img.size
    raw = b"".join(
        b"\x00" + img.crop((0, y, w, y + 1)).tobytes() for y in range(h)
    )
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


def bmp_bytes(img):
    """A DIB for the icon directory: 32bpp bottom-up, plus an empty AND mask."""
    w, h = img.size
    header = struct.pack(
        "<IiiHHIIiiII", 40, w, h * 2, 1, 32, 0, 0, 0, 0, 0, 0
    )
    px = img.load()
    xor = b"".join(
        bytes(v for x in range(w) for v in _bgra(px[x, y]))
        for y in range(h - 1, -1, -1)
    )
    # Alpha carries the transparency, so the legacy mask is all zeros.
    stride = ((w + 31) // 32) * 4
    return header + xor + b"\x00" * (stride * h)


def _bgra(p):
    r, g, b, a = p
    return (b, g, r, a)


def main():
    entries = []
    for size in SIZES:
        img = render(size)
        # Uncompressed DIBs get expensive fast -- a 128px entry is 67 KB of
        # them, more than the executable it decorates. Windows has read
        # PNG-compressed entries since Vista, so the large sizes use those and
        # only the small ones, where the saving is negligible, stay BMP.
        blob = png_bytes(img) if size >= 64 else bmp_bytes(img)
        entries.append((size, blob))

    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    offset = 6 + 16 * len(entries)
    head = struct.pack("<HHH", 0, 1, len(entries))
    dirs, blobs = b"", b""
    for size, blob in entries:
        dim = 0 if size == 256 else size
        dirs += struct.pack("<BBBBHHII", dim, dim, 0, 0, 1, 32, len(blob), offset)
        blobs += blob
        offset += len(blob)

    with open(OUT, "wb") as f:
        f.write(head + dirs + blobs)
    print(f"wrote {OUT} ({len(head + dirs + blobs):,} bytes, {len(entries)} sizes)")


if __name__ == "__main__":
    main()
