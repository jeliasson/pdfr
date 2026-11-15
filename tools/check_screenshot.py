"""Fails unless a PNG screenshot shows something: a real picture of a form
is large and varied; a blank or failed one is tiny and uniform.
Standard library only (runs in bare distribution containers)."""
import struct, sys, zlib

data = open(sys.argv[1], "rb").read()
assert data[:8] == b"\x89PNG\r\n\x1a\n", "not a PNG"
pos, idat, width, height = 8, b"", 0, 0
while pos < len(data):
    n, kind = struct.unpack(">I4s", data[pos:pos + 8])
    body = data[pos + 8:pos + 8 + n]
    if kind == b"IHDR":
        width, height = struct.unpack(">II", body[:8])
    elif kind == b"IDAT":
        idat += body
    pos += 12 + n
raw = zlib.decompress(idat)
distinct = len(set(raw[:: max(1, len(raw) // 200000)]))
print(f"{width}x{height}, {len(data)} bytes, {distinct} distinct byte values")
assert width >= 400 and height >= 300, "window too small"
assert len(data) > 20000 and distinct > 40, "the screenshot looks blank"
