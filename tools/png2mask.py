#!/usr/bin/env python3
"""Convert a (transparent) PNG logo into a voxel mask for eld-voxels.

Stdlib only. Coverage is taken from alpha (or luminance when the image has
no alpha). Each output cell is '#' when the area it covers is mostly filled,
so thin gaps between letterforms survive the downsample.

usage: png2mask.py logo.png [width] [threshold] > logo.mask
"""
import struct
import sys
import zlib


def read_png(path):
    data = open(path, "rb").read()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        sys.exit("not a PNG")
    pos, idat, plte, trns = 8, b"", None, None
    while pos < len(data):
        (length,) = struct.unpack(">I", data[pos:pos + 4])
        ctype = data[pos + 4:pos + 8]
        chunk = data[pos + 8:pos + 8 + length]
        pos += 12 + length
        if ctype == b"IHDR":
            w, h, depth, color, _, _, interlace = struct.unpack(">IIBBBBB", chunk)
        elif ctype == b"PLTE":
            plte = chunk
        elif ctype == b"tRNS":
            trns = chunk
        elif ctype == b"IDAT":
            idat += chunk
    if depth != 8 or interlace:
        sys.exit("only 8-bit non-interlaced PNGs are supported")
    channels = {0: 1, 2: 3, 3: 1, 4: 2, 6: 4}[color]
    raw = zlib.decompress(idat)
    stride = w * channels
    rows, prev, i = [], bytearray(stride), 0
    for _ in range(h):
        f = raw[i]
        line = bytearray(raw[i + 1:i + 1 + stride])
        i += 1 + stride
        for x in range(stride):
            a = line[x - channels] if x >= channels else 0
            b = prev[x]
            c = prev[x - channels] if x >= channels else 0
            if f == 1:
                line[x] = (line[x] + a) & 255
            elif f == 2:
                line[x] = (line[x] + b) & 255
            elif f == 3:
                line[x] = (line[x] + ((a + b) >> 1)) & 255
            elif f == 4:
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                line[x] = (line[x] + (a if pa <= pb and pa <= pc else b if pb <= pc else c)) & 255
        rows.append(line)
        prev = line

    def coverage(x, y):
        px = rows[y][x * channels:(x + 1) * channels]
        if color == 6:
            return px[3] / 255
        if color == 4:
            return px[1] / 255
        if color == 3:
            if trns and px[0] < len(trns):
                return trns[px[0]] / 255
            r, g, b = plte[px[0] * 3:px[0] * 3 + 3]
            return (r + g + b) / 765
        if color == 2:
            return sum(px) / 765
        return px[0] / 255

    return w, h, coverage


def main():
    path = sys.argv[1]
    width = int(sys.argv[2]) if len(sys.argv) > 2 else 112
    threshold = float(sys.argv[3]) if len(sys.argv) > 3 else 0.55
    w, h, cov = read_png(path)

    # Crop to the logo's bounding box.
    xs, ys = [], []
    for y in range(h):
        for x in range(w):
            if cov(x, y) > 0.5:
                xs.append(x)
                ys.append(y)
    x0, x1, y0, y1 = min(xs), max(xs) + 1, min(ys), max(ys) + 1
    cell = (x1 - x0) / width
    height = max(1, round((y1 - y0) / cell))

    out = []
    for gy in range(height):
        row = ""
        for gx in range(width):
            sx0, sx1 = x0 + gx * cell, x0 + (gx + 1) * cell
            sy0, sy1 = y0 + gy * cell, y0 + (gy + 1) * cell
            total = n = 0
            # supersample 4x4 inside each cell
            for sy in range(4):
                for sx in range(4):
                    px = int(sx0 + (sx + 0.5) * (sx1 - sx0) / 4)
                    py = int(sy0 + (sy + 0.5) * (sy1 - sy0) / 4)
                    if px < w and py < h:
                        total += cov(px, py)
                    n += 1
            row += "#" if total / n >= threshold else "."
        out.append(row)
    print("\n".join(out))


if __name__ == "__main__":
    main()
