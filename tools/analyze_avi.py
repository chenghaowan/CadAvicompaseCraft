"""Analyze CATIA AVI capture: RIFF structure, stream info, frame extraction, stats."""
import struct
import sys
import os
from collections import Counter

def parse_riff(path):
    data = open(path, 'rb').read()
    assert data[:4] == b'RIFF' and data[8:12] == b'AVI ', data[:12]
    riff_size = struct.unpack('<I', data[4:8])[0]
    print(f"file: {path}")
    print(f"size: {len(data):,} bytes (header says {riff_size + 8:,})")

    def walk(buf, start, end, depth=0):
        out = []
        pos = start
        while pos + 8 <= end:
            cid = buf[pos:pos + 4]
            csz = struct.unpack('<I', buf[pos + 4:pos + 8])[0]
            if csz > len(buf) - pos - 8:
                break
            cend = pos + 8 + csz
            if cid == b'LIST':
                ltype = buf[pos + 8:pos + 12]
                out.append((pos, cid + b'/' + ltype, csz, depth, cend))
                out.extend(walk(buf, pos + 12, cend, depth + 1))
            else:
                out.append((pos, cid, csz, depth, cend))
            pos = cend + (csz & 1)
        return out

    chunks = walk(data, 12, len(data))
    # top-level + a summary
    tops = [c for c in chunks if c[3] == 0]
    for pos, cid, csz, depth, _ in tops:
        print(f"  chunk {cid!r} @{pos:#x} size={csz:,}")
    return data, chunks

def find(data, chunks, needle):
    for pos, cid, csz, depth, cend in chunks:
        if needle in cid:
            return pos, csz, cend
    return None

def analyze(path):
    data, chunks = parse_riff(path)

    idx = find(data, chunks, b'avih')
    if idx:
        pos, csz, cend = idx
        f = struct.unpack('<IIIIIIIIIIIII', data[pos + 8:pos + 8 + 52])
        names = ['usecPerFrame', 'maxBytesPerSec', 'padding', 'flags', 'totalFrames',
                 'initialFrames', 'streams', 'suggestedBuf', 'width', 'height',
                 'reserved0', 'reserved1', 'reserved2']
        print("\navih:")
        for n, v in zip(names, f):
            if n.startswith('reserved'):
                continue
            print(f"  {n:16s} = {v:,}")
        print(f"  -> fps = {1e6 / f[0]:.3f}, duration = {f[0]*f[4]/1e6:.3f} s, "
              f"raw rate = {f[1]/1048576:.1f} MiB/s")

    # video stream header
    idx = find(data, chunks, b'strh')
    if idx:
        pos, csz, cend = idx
        fcc_type, fcc_handler = data[pos + 8:pos + 12], data[pos + 12:pos + 16]
        (flags, prio, lang, init_frames, scale, rate, start,
         length, buf, quality, samplesize) = struct.unpack('<IHHIIIIIIII', data[pos + 20:pos + 20 + 40])
        print(f"\nstrh: type={fcc_type!r} handler={fcc_handler!r} rate={rate} scale={scale} "
              f"length={length} samplesize={samplesize}")

    idx = find(data, chunks, b'strf')
    if idx:
        pos, csz, cend = idx
        hdr = struct.unpack('<IiiHHIIiiII', data[pos + 8:pos + 8 + 40])
        (bsize, w, h, planes, bits, comp, size_img, xppm, yppm, clrused, clrimp) = hdr
        print(f"strf(BITMAPINFOHEADER): {w}x{h} planes={planes} bits={bits} "
              f"comp={comp} sizeImage={size_img:,} bpp_row={size_img // abs(h):,}")

    # stream count per LIST strl
    n_strl = sum(1 for c in chunks if c[1] == b'LIST/strl')
    print(f"\nstream lists (LIST/strl): {n_strl}")

    has_idx1 = any(c[1] == b'idx1' for c in chunks)
    print(f"idx1 index: {has_idx1}")

    # frames inside LIST movi
    movi = [c for c in chunks if c[1] == b'LIST/movi']
    if movi:
        mpos, _, mcsz, _, mend = movi[0]
        frames = [(p, cid, s) for p, cid, s, d, _ in chunks
                  if mpos < p < mend and cid[:2] == b'00']
        print(f"frames in movi: {len(frames)} (first={frames[0][1]!r} last={frames[-1][1]!r})")
        sizes = [s for _, _, s in frames]
        print(f"frame chunk sizes: min={min(sizes):,} max={max(sizes):,} avg={sum(sizes)//len(sizes):,}")
        return data, frames, (w, h, bits)
    return data, [], None

def frame_rgb(data, pos, csz, w, h, bits):
    off = pos + 8  # AVI DIB frames store bitmap bits only
    stride = ((w * bits // 8) + 3) & ~3
    raw = data[off:off + stride * abs(h)]
    import numpy as np
    rows = np.frombuffer(raw, dtype=np.uint8).reshape(abs(h), stride)[:, :w * bits // 8]
    img = rows.reshape(abs(h), w, 3)[:, :, ::-1]  # BGR -> RGB, top-down already since DIB bottom-up stored reversed
    return img[::-1]  # flip vertical

def main():
    path = sys.argv[1]
    data, frames, fmt = analyze(path)
    outdir = sys.argv[2]
    os.makedirs(outdir, exist_ok=True)
    import numpy as np
    from PIL import Image

    if not frames or not fmt:
        print("no frames parsed")
        return
    w, h, bits = fmt
    picks = [0, 1, 2, 30, 60, 90, 119]
    prev = None
    for i, (pos, cid, csz) in enumerate(frames):
        img = frame_rgb(data, pos, csz, w, h, bits)
        if i in picks:
            Image.fromarray(img).save(os.path.join(outdir, f"frame_{i:03d}.png"))
        if i in (0, 1, 2, 30, 60, 90, 119):
            u = np.unique(img.reshape(-1, 3), axis=0)
            print(f"frame {i:3d}: {csz:,} B  unique colors={len(u):,}")
            if prev is not None:
                diff = np.abs(img.astype(np.int16) - prev.astype(np.int16))
                changed = (diff.max(axis=2) > 0).mean()
                print(f"         vs prev: changed px {changed*100:.1f}%  mean|d| {diff.mean():.1f}")
        prev = img

    # background analysis on frame 0
    img0 = frame_rgb(data, frames[0][0], frames[0][2], w, h, bits)
    border = np.concatenate([img0[0], img0[-1], img0[:, 0], img0[:, -1]])
    cnt = Counter(map(tuple, border.tolist()))
    print("\nborder top colors:", cnt.most_common(6))
    # vertical gradient check on the far-left column (should be pure background)
    col = img0[:, w // 2 - 40].astype(np.int16)
    print("center column bg sample at y=5,50,100,200,300,400,500:",
          [tuple(col[y]) for y in (5, 50, 100, 200, 300, 400, 500) if y < h])
    # noise: standard deviation inside a flat background patch
    patch = img0[10:60, 10:60].astype(np.int16)
    print("flat bg patch std per channel:", patch.reshape(-1, 3).std(axis=0).round(2))

if __name__ == '__main__':
    main()
