#!/usr/bin/env bash
# 构建"只保留 H.264"的最小 FFmpeg（静态、便携，不安装到系统）。
# 依赖：能联网的 Git Bash；产物落在 $WORK/ffmpeg-out/bin/ffmpeg.exe
set -uo pipefail

WORK="${1:-/d/claw.forvia/_dl_ffmpeg_min}"
FFMPEG_VER="${FFMPEG_VER:-9.0.2}"
mkdir -p "$WORK"
cd "$WORK" || exit 1

log() { echo "[$(date +%H:%M:%S)] $*"; }

# ---------- 1. 便携 MSYS2 ----------
if [ ! -x "$WORK/msys64/usr/bin/bash.exe" ]; then
  log "下载 MSYS2 base"
  curl -L --retry 3 -o msys2-base.sfx.exe \
    "https://mirrors.tuna.tsinghua.edu.cn/msys2/distrib/msys2-x86_64-latest.sfx.exe" || exit 1
  log "解压 MSYS2"
  ./msys2-base.sfx.exe -y -o"$WORK" || exit 1
fi

BASH="$WORK/msys64/usr/bin/bash.exe"
[ -x "$BASH" ] || { log "MSYS2 解压失败"; exit 1; }

# ---------- 2. 工具链 ----------
log "切换 pacman 镜像到清华源"
"$BASH" -lc "for f in /etc/pacman.d/mirrorlist.msys; do echo 'Server = https://mirrors.tuna.tsinghua.edu.cn/msys2/msys/\$arch' > \$f; done;   for f in /etc/pacman.d/mirrorlist.mingw32 /etc/pacman.d/mirrorlist.mingw64; do echo 'Server = https://mirrors.tuna.tsinghua.edu.cn/msys2/mingw/\$arch' > \$f; done" || true

log "安装 mingw 工具链 + x264"
"$BASH" -lc "pacman -Sy --noconfirm --needed msys2-keyring >/dev/null 2>&1; \
  pacman -S --noconfirm --needed make diffutils \
    mingw-w64-x86_64-gcc mingw-w64-x86_64-nasm mingw-w64-x86_64-make \
    mingw-w64-x86_64-pkgconf mingw-w64-x86_64-x264" || { log "pacman 安装失败"; exit 1; }

# ---------- 3. 源码 ----------
if [ ! -d "ffmpeg-$FFMPEG_VER" ]; then
  log "下载 FFmpeg $FFMPEG_VER 源码"
  curl -L --retry 3 -o "ffmpeg-$FFMPEG_VER.tar.xz" \
    "https://ffmpeg.org/releases/ffmpeg-$FFMPEG_VER.tar.xz" || exit 1
  "$BASH" -lc "cd '$WORK' && tar xf 'ffmpeg-$FFMPEG_VER.tar.xz'" || exit 1
fi

# ---------- 4. 最小配置编译 ----------
log "configure + make（只留 rawvideo 输入 / libx264 / mp4 输出 / scale）"
"$BASH" -lc "
export PATH=/mingw64/bin:\$PATH
cd '$WORK/ffmpeg-$FFMPEG_VER' || exit 1
./configure \
  --prefix='$WORK/ffmpeg-out' \
  --disable-everything --disable-doc --disable-avdevice \
  --disable-network --disable-autodetect --disable-swresample \
  --enable-gpl --enable-libx264 \
  --enable-avcodec --enable-avformat --enable-avutil --enable-swscale --enable-avfilter \
  --enable-encoder=libx264 \
  --enable-decoder=rawvideo \
  --enable-demuxer=rawvideo \
  --enable-muxer=mp4 \
  --enable-protocol=file,pipe \
  --enable-filter=scale,format \
  --enable-ffmpeg \
  --pkg-config-flags=--static --extra-ldflags='-static'  \
  --extra-cflags='-O2' >'$WORK/configure.log' 2>&1 || { tail -20 '$WORK/configure.log'; exit 1; }
make -j12 >'$WORK/make.log' 2>&1 || { tail -30 '$WORK/make.log'; exit 1; }
" || { log "编译失败，见 $WORK/make.log"; exit 1; }

FF="$WORK/ffmpeg-$FFMPEG_VER/ffmpeg.exe"
if [ -f "$FF" ]; then
  log "产物: $FF  $(stat -c%s "$FF") bytes"
  cp "$FF" "$WORK/ffmpeg-min.exe"
else
  log "没找到 ffmpeg.exe"; exit 1
fi
log "完成"
