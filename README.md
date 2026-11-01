<div align="center">

# CadAvicompaseCraft

**Compress CATIA screen recordings by ~240× — from gigabytes of raw AVI to a few megabytes of MP4.**

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Platform: Windows x64](https://img.shields.io/badge/platform-Windows%20x64-0078D6.svg)](#windows-x64)
[![Rust 2021](https://img.shields.io/badge/rust-2021%20edition-orange.svg)](https://www.rust-lang.org/)
[![Version](https://img.shields.io/badge/version-0.1.0-brightgreen.svg)](CHANGELOG.md)

[Download](#download) · [Features](#features) · [Usage](#usage) · [How it works](#how-it-works) · [Build](#build-from-source)

</div>

---

CATIA's built-in screen recorder writes **uncompressed 24-bit DIB AVI**. It is lossless,
but enormous: the 5-second sample used for testing is 237 MB (47.4 MiB/s), so one minute
takes roughly 2.8 GB. CadAvicompaseCraft reads those frames directly, optionally replaces
the viewport background with pure white, and pipes RGB frames into a minimal FFmpeg build.
The same clip becomes a **1.00 MB** H.264 MP4 — about **238× smaller**.

## Features

- **~238× smaller** — H.264 High profile, `yuv420p`, `+faststart` for instant playback.
- **Document-ready white background** — border flood-fill keying with feathered edges and
  enclosed-region handling, designed for pasting captures into Word or PowerPoint.
  Can be disabled with one checkbox or flag.
- **Batch GUI** — drag and drop multiple `.avi` files, pick a quality preset, watch
  per-file progress, cancel at any time.
- **Scriptable CLI** — stable flags for CRF, preset, output path, and background handling.
- **Self-contained** — ships a 5.6 MB minimal FFmpeg static build. No install, no PATH
  setup, no admin rights.
- **Auto-download fallback** — if no FFmpeg is found at runtime, a full build is
  downloaded automatically, SHA-256-verified before it is ever executed, and reused from
  then on.
- **No console flash** — the GUI suppresses FFmpeg's console window.
- **Strict, safe input handling** — unsupported AVIs are rejected with a clear error instead
  of producing corrupt frames; existing files are never overwritten.
- **No ML model** — keying is a fast, single-threaded color algorithm that runs at
  interactive speed.

## Benchmark

| Metric | Value |
|---|---|
| Input | 1357 × 509, 24 fps, 120 frames (5.00 s), **237 MB** |
| Output (CRF 20, `medium`, white background) | **1.00 MB** — H.264 High, `yuv420p`, 1.67 Mbps |
| Compression ratio | **~238×** |
| Throughput | ~50 frames/s (≈2.3 s for the 120-frame clip) |

> Background whitening changes appearance, not size (±0.1% in our measurements); all
> savings come from re-encoding.

> The 237 MB sample recording is not included in this repository.

## Download

### Windows x64

Grab `cadcompress-0.1.0-windows-x64.7z` from the [Releases](../../releases) page, extract
it anywhere, and run:

- **`cadcompress-gui.exe`** — drag-and-drop GUI.
- **`cadcompress-cli.exe`** — command line.

FFmpeg 9.0.2 is bundled, so there is nothing else to install. The archive also contains
`LICENSE`, `NOTICE`, `README.md`, `CHANGELOG.md`, and the FFmpeg license/readme.

## Usage

### GUI

1. Double-click `cadcompress-gui.exe`.
2. Drag one or more `.avi` files into the window (multi-select is supported).
3. Choose a quality preset:

   | Preset | CRF | x264 preset | When to use |
   |---|---|---|---|
   | 高画质 (High) | 18 | `slow` | Archival quality |
   | 标准 (Standard) | 20 | `medium` | Recommended default |
   | 小体积 (Small) | 24 | `medium` | Smallest files for chat/email |

4. Leave **背景抹白** (whiten background) checked for document-ready output, or uncheck it
   to keep the original gradient background.
5. Click **开始压缩** (Start). Each file shows its own progress; **取消** (Cancel) stops
   the batch.
6. Output is written next to each source file as `<name>_compressed.mp4`. If that name
   exists, a numeric suffix is added — existing files are never overwritten.

> The GUI and CLI messages are currently in Chinese.

### CLI

```bat
cadcompress-cli.exe <input.avi> [-o <output.mp4>] [--crf N] [--preset P] [--keep-bg]
```

```bat
:: Default: white background, CRF 20, medium preset
cadcompress-cli.exe capture.avi

:: Keep the original background and target a smaller file
cadcompress-cli.exe capture.avi -o small.mp4 --crf 24 --keep-bg
```

| Option | Default | Description |
|---|---|---|
| `<input.avi>` | — | Input CATIA recording (required) |
| `-o, --out <file>` | `<name>_compressed.mp4` | Output path |
| `--crf <0-51>` | `20` | x264 quality/size trade-off; 18–23 are safe, 26+ softens thin lines |
| `--preset <name>` | `medium` | x264 speed preset; `slow` is ~1.4% smaller but ~50% slower |
| `--keep-bg` | off | Keep the original background instead of whitening it |
| `--no-download` | off | Fail with an error instead of auto-downloading FFmpeg |
| `-h, --help` | — | Show usage |
| `-V, --version` | — | Show version |

`ffmpeg.exe` is resolved in this order:

1. `CADCOMPRESS_FFMPEG` environment variable
2. `ffmpeg.exe` next to the executable
3. `third_party\ffmpeg\ffmpeg.exe` relative to the executable
4. The same two locations relative to the working directory
5. `%LOCALAPPDATA%\cadcompress\ffmpeg.exe`
6. `PATH`

### First-run auto-download

When none of those locations has an FFmpeg, both programs download the official
**essentials build (~110 MB)** instead of failing:

- The archive is **SHA-256-verified** against a hash pinned in the source; a mismatch is
  deleted and never executed.
- It installs next to the executable when that directory is writable (portable), otherwise
  to `%LOCALAPPDATA%\cadcompress\`, and is reused on every later run.
- **GUI:** triggered by clicking 开始压缩; a progress bar with a 取消下载 button appears at
  the bottom, and the batch starts automatically once the download finishes.
- **CLI:** shows a percentage on the console; `--no-download` opts out, `Ctrl+C` aborts
  and cleans up the partial file.

Two environment variables retarget the download (e.g. for a mirror):

```bat
set CADCOMPRESS_FFMPEG_URL=https://example.com/ffmpeg-build.zip
set CADCOMPRESS_FFMPEG_SHA256=<sha256 of that archive>
```

> The auto-downloaded build is the full essentials package, because it needs a stable
> public download source. The trimmed 5.6 MB build in `third_party\ffmpeg\` always takes
> precedence — downloading is only the fallback when it is missing.

## Input requirements

CadAvicompaseCraft intentionally accepts only the format CATIA's recorder produces:

- AVI container with a single video stream
- Uncompressed 24 bpp DIB frames (`biCompression = 0`)
- Top-down or bottom-up storage
- Odd dimensions are fine (FFmpeg scales them down to even for `yuv420p`)

Anything else is rejected when the file is opened, with an explicit error.

## How it works

```
capture.avi ──► AviReader ──► keying (optional) ──► RGB24 pipe ──► ffmpeg/libx264 ──► .mp4
```

1. **Read** — a minimal RIFF/AVI parser extracts frame offsets, dimensions, frame rate,
   and stride, then converts BGR DIB rows to RGBA. No decoder and no temporary files.
2. **Key** — the background palette is grown by flood fill from the frame borders; pixels
   within tolerance are turned white, with soft edges blended to avoid fringes. Enclosed
   background regions (holes, table cells) are judged separately so part walls are kept.
3. **Encode** — RGB24 frames are streamed through stdin to FFmpeg, which encodes H.264
   with libx264 and writes an MP4 with `+faststart`.

The keying algorithm is ported from [PointToFix Craft](https://github.com/chenghaowan/pointtofixcraft) (MIT).

## Build from source

Prerequisites: stable Rust with the MSVC toolchain, Windows x64.

```bat
cargo build --release
```

Binaries are written to:

```
target\release\cadcompress-cli.exe
target\release\cadcompress-gui.exe
```

The repository includes `third_party\ffmpeg\ffmpeg.exe`, so a release build runs as-is.
If you use a different FFmpeg, point `CADCOMPRESS_FFMPEG` at it or place it next to the
executable.

### Rebuilding the minimal FFmpeg

`tools\build_ffmpeg_min.sh` bootstraps a portable MSYS2 toolchain, downloads FFmpeg 9.0.2,
and builds only what the encoder needs (`libx264`, `rawvideo`, `mp4`, `scale`). The result
is the ~5.6 MB `ffmpeg.exe` in `third_party\ffmpeg\`.

```bash
bash tools/build_ffmpeg_min.sh
```

### Packaging a release

```bash
SEVEN_ZIP=/path/to/7z.exe bash tools/package_release.sh
```

The script builds both binaries, stages them with licenses and docs under
`dist/cadcompress-<version>-windows-x64/`, then writes
`release/cadcompress-<version>-windows-x64.7z` plus `release/SHA256SUMS.txt`.
`7z`/`7za` is auto-detected on `PATH`; set `SEVEN_ZIP` if it lives elsewhere.

## Project layout

```
crates/core/src/avi.rs        Minimal RIFF/AVI reader (DIB → RGBA)
crates/core/src/keying.rs     Background keying (ported from PointToFix Craft)
crates/core/src/encode.rs     FFmpeg child-process encoder (rawvideo pipe)
crates/core/src/pipeline.rs   Pipeline, progress, cancellation, output naming
crates/cli/src/main.rs        Argument parsing and terminal progress
crates/gui/src/main.rs        egui drag-and-drop interface
third_party/ffmpeg/           Minimal FFmpeg 9.0.2 (GPLv3) + license/readme
tools/                        AVI analysis, FFmpeg build, release packaging
```

## License

The source code of this project is licensed under the [MIT License](LICENSE).

The distributed FFmpeg binary is a separate, minimized **GPLv3** program
(`third_party/ffmpeg/`), invoked as a child process over pipes and not linked with this
project's code. See [NOTICE](NOTICE) and `third_party/ffmpeg/README.md` for details,
including source links and the requirement to ship `COPYING.GPLv3`.

H.264 encoding may also be subject to third-party **patent** licensing, independent of
the GPL. Evaluate this before commercial distribution.

## Acknowledgements

- Background keying is ported from [PointToFix Craft](https://github.com/chenghaowan/pointtofixcraft) (MIT).
- Encoding is performed by a custom minimal build of [FFmpeg](https://ffmpeg.org/) and
  [x264](https://code.videolan.org/videolan/x264).
