"""Render the animated Orbit pixel "O" logo as GIF and MP4.

Usage: python3 render.py  (needs ffmpeg on PATH; writes files next to this script)
"""
import math
import pathlib
import subprocess

OUT = pathlib.Path(__file__).parent
FPS, SECONDS = 25, 5.0
UNIT, SIZE = 32, 512  # px per logo grid cell, canvas size
ORIGIN = (SIZE - 11 * UNIT) // 2

PINK = (0xF2, 0x45, 0x8F)
SHADOW = (0x74, 0x2F, 0x4D)
SPARK = (0xFF, 0xE3, 0xEF)
BG = (0x0B, 0x0B, 0x0E)

# Same bars as apps/web/public/logo.svg (pink layer; the shadow is this shape offset by +1,+1).
BARS = [(0, 2, 2, 6), (2, 0, 6, 2), (8, 2, 2, 6), (2, 8, 6, 2)]
CELLS = sorted(
    {(x, y) for bx, by, w, h in BARS for x in range(bx, bx + w) for y in range(by, by + h)},
    key=lambda c: math.atan2(c[0] + 0.5 - 5, -(c[1] + 0.5 - 5)) % (2 * math.pi),
)


def clamp(v):
    return max(0.0, min(1.0, v))


def ease_out_back(v):
    c = 1.70158
    return 1 + (c + 1) * (v - 1) ** 3 + c * (v - 1) ** 2


def ease_in_out(v):
    return v * v * (3 - 2 * v)


def mix(a, b, k):
    return tuple(round(x + (y - x) * k) for x, y in zip(a, b))


def frame(t):
    img = bytearray(bytes(BG) * SIZE * SIZE)

    def rect(cx, cy, scale, color):
        half = scale * UNIT / 2
        x0, x1 = round(ORIGIN + cx * UNIT - half), round(ORIGIN + cx * UNIT + half)
        y0, y1 = round(ORIGIN + cy * UNIT - half), round(ORIGIN + cy * UNIT + half)
        if x1 <= x0 or y1 <= y0:
            return
        row = bytes(color) * (x1 - x0)
        for y in range(y0, y1):
            i = (y * SIZE + x0) * 3
            img[i : i + len(row)] = row

    n = len(CELLS)
    # Shadow slides out after the ring is built and slides back before it unwinds.
    shadow = ease_out_back(clamp((t - 1.3) / 0.45)) if t < 3.6 else 1 - ease_in_out(clamp((t - 3.6) / 0.3))
    glint = -4 + 30 * clamp((t - 2.0) / 0.7)

    cells = []
    for i, (x, y) in enumerate(CELLS):
        born = 1.1 * i / n
        age = t - born
        grow = ease_out_back(clamp(age / 0.18))
        shrink = 1 - ease_in_out(clamp((t - 3.9 - 0.8 * i / n) / 0.15))
        scale = min(grow, shrink)
        if scale <= 0:
            continue
        color = mix(SPARK, PINK, clamp(age / 0.35))
        d = abs(x + y - glint)
        if d < 1.5:
            color = mix(color, SPARK, 1 - d / 1.5)
        cells.append((x + 0.5, y + 0.5, scale, color))

    for cx, cy, scale, _ in cells:
        rect(cx + shadow, cy + shadow, scale, SHADOW)
    for cell in cells:
        rect(*cell)
    return bytes(img)


def main():
    frames = [frame(i / FPS) for i in range(int(FPS * SECONDS))]
    raw = ["-f", "rawvideo", "-pix_fmt", "rgb24", "-s", f"{SIZE}x{SIZE}", "-r", str(FPS), "-i", "-"]
    jobs = {
        "orbit-logo.gif": ["-vf", "split[a][b];[a]palettegen=stats_mode=full[p];[b][p]paletteuse=dither=none", "-loop", "0"],
        "orbit-logo.mp4": ["-c:v", "libx264", "-pix_fmt", "yuv420p", "-crf", "16", "-movflags", "+faststart"],
    }
    for name, args in jobs.items():
        subprocess.run(["ffmpeg", "-y", "-loglevel", "error", *raw, *args, str(OUT / name)], input=b"".join(frames), check=True)
        print(OUT / name)


if __name__ == "__main__":
    main()
