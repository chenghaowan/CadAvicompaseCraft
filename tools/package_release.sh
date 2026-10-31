#!/usr/bin/env bash
# Build the Windows x64 release bundle and pack it as a 7z archive.
#
# Output:
#   dist/cadcompress-<version>-windows-x64/        staged, runnable files
#   release/cadcompress-<version>-windows-x64.7z   release asset
#   release/SHA256SUMS.txt
#
# Requires: Rust toolchain, 7z/7za (auto-detected, or set SEVEN_ZIP).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

VERSION="$(sed -n 's/^version = "\([^"]*\)".*/\1/p' crates/cli/Cargo.toml | head -n 1)"
[ -n "$VERSION" ] || { echo "error: cannot read version from crates/cli/Cargo.toml" >&2; exit 1; }

NAME="cadcompress-$VERSION-windows-x64"
DIST_ROOT="$ROOT/dist"
STAGE="$DIST_ROOT/$NAME"
RELEASE_DIR="$ROOT/release"
ARCHIVE="$RELEASE_DIR/$NAME.7z"

resolve_7z() {
    if [ -n "${SEVEN_ZIP:-}" ]; then
        printf '%s\n' "$SEVEN_ZIP"
        return 0
    fi
    for candidate in 7z 7za 7zr; do
        if command -v "$candidate" >/dev/null 2>&1; then
            command -v "$candidate"
            return 0
        fi
    done
    return 1
}

SEVEN_ZIP="$(resolve_7z)" || {
    echo "error: 7z not found. Install 7-Zip or set SEVEN_ZIP=/path/to/7z.exe" >&2
    exit 1
}

echo "==> version: $VERSION"
echo "==> building release binaries"
cargo build --release --workspace

for f in \
    target/release/cadcompress-cli.exe \
    target/release/cadcompress-gui.exe \
    third_party/ffmpeg/ffmpeg.exe \
    third_party/ffmpeg/COPYING.GPLv3 \
    third_party/ffmpeg/README.md \
    LICENSE NOTICE README.md CHANGELOG.md; do
    [ -f "$f" ] || { echo "error: missing $f" >&2; exit 1; }
done

echo "==> staging $STAGE"
rm -rf "$DIST_ROOT"
mkdir -p "$STAGE/third_party/ffmpeg"
cp target/release/cadcompress-cli.exe target/release/cadcompress-gui.exe "$STAGE/"
cp LICENSE NOTICE README.md CHANGELOG.md "$STAGE/"
cp third_party/ffmpeg/ffmpeg.exe \
   third_party/ffmpeg/COPYING.GPLv3 \
   third_party/ffmpeg/README.md \
   "$STAGE/third_party/ffmpeg/"

echo "==> packing $ARCHIVE"
mkdir -p "$RELEASE_DIR"
rm -f "$ARCHIVE"
ARCHIVE_ARG="$ARCHIVE"
if command -v cygpath >/dev/null 2>&1; then
    ARCHIVE_ARG="$(cygpath -w "$ARCHIVE")"
fi
(
    cd "$DIST_ROOT"
    "$SEVEN_ZIP" a -t7z -mx=9 "$ARCHIVE_ARG" "$NAME" >/dev/null
)

echo "==> checksum"
(
    cd "$RELEASE_DIR"
    sha256sum "$NAME.7z" > SHA256SUMS.txt
)

echo
echo "archive: $ARCHIVE"
echo "size:    $(du -h "$ARCHIVE" | cut -f1)"
echo "sha256:  $(cut -d' ' -f1 "$RELEASE_DIR/SHA256SUMS.txt")"
