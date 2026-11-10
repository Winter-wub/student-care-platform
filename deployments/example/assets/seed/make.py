"""Draws the example seed's demo files (fictional Seal Bay University):
  hall-map.png     a 640x360 plain map of the orientation hall
  orientation.pdf  a one-page orientation schedule

Usage: python3 make.py <output dir>   (here: deployments/example/assets/seed)
No third-party packages: the PNG and the PDF are written by hand."""
import struct
import sys
import zlib

out = sys.argv[1]

W, H = 640, 360
SEA = (11, 92, 173)
SAND = (238, 226, 196)
HALL = (31, 78, 121)
WHITE = (255, 255, 255)


def pixel(x, y):
    if y > 300:
        return SEA
    if 220 <= x <= 420 and 110 <= y <= 250:
        if 300 <= x <= 340 and y >= 200:
            return WHITE
        return HALL
    if 300 <= x <= 340 and y > 250:
        return (200, 180, 140)
    return SAND


raw = bytearray()
for y in range(H):
    raw.append(0)
    for x in range(W):
        raw.extend(pixel(x, y))


def chunk(tag, data):
    c = struct.pack(">I", len(data)) + tag + data
    return c + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)


png = b"\x89PNG\r\n\x1a\n"
png += chunk(b"IHDR", struct.pack(">IIBBBBB", W, H, 8, 2, 0, 0, 0))
png += chunk(b"IDAT", zlib.compress(bytes(raw), 9))
png += chunk(b"IEND", b"")
open(f"{out}/hall-map.png", "wb").write(png)

lines = [
    "Seal Bay University (fictional demo)",
    "New student orientation, academic year 2569",
    "",
    "09:00  Welcome, Seal Bay Hall",
    "10:00  Campus tour",
    "12:00  Lunch with your faculty",
    "13:30  Registration help desk",
    "",
    "Demo file: not a real university.",
]
text = "BT /F1 16 Tf 72 760 Td 22 TL\n"
for line in lines:
    text += "(" + line.replace("(", "\\(").replace(")", "\\)") + ") Tj T*\n"
text += "ET\n"
objs = [
    b"<< /Type /Catalog /Pages 2 0 R >>",
    b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
    b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] "
    b"/Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>",
    b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
    b"<< /Length %d >>\nstream\n" % len(text) + text.encode() + b"endstream",
]
pdf = b"%PDF-1.4\n"
offsets = []
for i, o in enumerate(objs, 1):
    offsets.append(len(pdf))
    pdf += b"%d 0 obj\n" % i + o + b"\nendobj\n"
xref = len(pdf)
pdf += b"xref\n0 %d\n0000000000 65535 f \n" % (len(objs) + 1)
for off in offsets:
    pdf += b"%010d 00000 n \n" % off
pdf += b"trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (len(objs) + 1, xref)
open(f"{out}/orientation.pdf", "wb").write(pdf)
