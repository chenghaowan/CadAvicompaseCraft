# third_party/ffmpeg

This directory contains **FFmpeg as an independently distributed third-party program**.
It is used only by `cadcompress-cli.exe` / `cadcompress-gui.exe` through command-line
pipes. It is not part of this project's source code and is not linked with it.

- Version: FFmpeg 9.0.2 (custom minimal static build, Windows x64)
- Size: about 5.6 MB (a full "essentials" build is 101 MB)
- License: **GNU General Public License v3** (see `COPYING.GPLv3`), because it links
  the GPL-licensed libx264
- Build script: `tools/build_ffmpeg_min.sh`

## Components kept

| Component | Purpose |
|---|---|
| `libx264` (static) | H.264 encoding |
| `rawvideo` demuxer / decoder | Receive RGB frames from stdin |
| `mp4` muxer | Write MP4 (with `+faststart`) |
| `scale` / `format` filters | Round odd dimensions down to even (required by `yuv420p`) |
| `file` / `pipe` protocols | Pipe input and output |

All other codecs, containers, filters, and network protocols are disabled. As a result,
this ffmpeg build **cannot** read or inspect MP4 files; it only encodes. That is
intentional — it keeps the binary as small as possible.

## Sources

- FFmpeg: https://ffmpeg.org/releases/ffmpeg-9.0.2.tar.xz
- x264: https://code.videolan.org/videolan/x264 (MSYS2 `mingw-w64-x86_64-x264`)
- Rebuild: run `bash tools/build_ffmpeg_min.sh` from Git Bash

## Distribution notes

1. `ffmpeg.exe` in this directory is a **separate GPL program**. This project (MIT)
   invokes it as a child process with a stdin pipe. This is "aggregate distribution"
   and does not change the license of this project's source code.
2. When distributing, you must include `COPYING.GPLv3` unchanged and keep the origin and
   source links in this file.
3. Do not modify `ffmpeg.exe`; if you modify or rebuild it, you must also make the
   corresponding GPL source available.
4. H.264 encoding also involves third-party **patent** licensing, which is independent of
   the GPL. Evaluate this before commercial distribution.
