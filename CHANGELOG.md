# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-10-31

### Added

- Initial public release.
- `cadcompress-gui`: drag-and-drop batch interface with quality presets, optional white
  background keying, per-file progress, and cancellation.
- `cadcompress-cli`: scriptable command-line interface with `--crf`, `--preset`,
  `--out`, and `--keep-bg` options.
- Minimal, strict RIFF/AVI reader for single-stream, uncompressed 24 bpp DIB captures.
- Background keying ported from PointToFix Craft, with soft edges and enclosed-region
  handling.
- Bundled minimal FFmpeg 9.0.2 static build (GPLv3) for H.264/libx264 encoding.
- Windows x64 release packaging as a 7z archive with SHA-256 checksum.
