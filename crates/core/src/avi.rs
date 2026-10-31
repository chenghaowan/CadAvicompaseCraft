//! 最小 RIFF/AVI 读取器：只支持 CATIA 录屏产出的未压缩 DIB 视频流。
//!
//! 支持范围有意收窄：单视频流、biCompression == 0、24bpp BGR。
//! 遇到别的编码直接报错，交给调用方给出明确提示，而不是悄悄产出坏帧。

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

#[derive(Debug, Clone)]
pub struct AviInfo {
    pub width: u32,
    pub height: u32,
    pub fps_num: u32,
    pub fps_den: u32,
    /// DIB 行跨度（含 4 字节对齐填充）
    pub stride: usize,
    /// 正 biHeight = 自下而上存储
    pub bottom_up: bool,
}

pub struct AviReader {
    info: AviInfo,
    file: File,
    frames: Vec<(u64, usize)>,
    next: usize,
}

const FOURCC_DIB: u32 = 0;

impl AviReader {
    pub fn open(path: &Path) -> io::Result<Self> {
        let mut file = File::open(path)?;
        let len = file.metadata()?.len();
        let mut head = [0u8; 12];
        file.read_exact(&mut head)?;
        if &head[0..4] != b"RIFF" || &head[8..12] != b"AVI " {
            return Err(invalid("不是 RIFF/AVI 文件"));
        }
        let riff_end = 8u64.saturating_add(u32::from_le_bytes([
            head[4], head[5], head[6], head[7],
        ]) as u64)
            .min(len);

        let mut avih: Option<[u32; 9]> = None;
        let mut strf: Option<(i32, i32, u16, u32)> = None;
        let mut frames = Vec::new();

        scan(&mut file, 12, riff_end, &mut avih, &mut strf, &mut frames)?;

        let av = avih.ok_or_else(|| invalid("缺少 avih 头"))?;
        let (bi_w, bi_h, bits, comp) = strf.ok_or_else(|| invalid("缺少视频流格式头"))?;
        if comp != FOURCC_DIB {
            return Err(invalid(&format!(
                "视频流不是未压缩 DIB（compression=0x{comp:02X}），暂不支持"
            )));
        }
        if bits != 24 {
            return Err(invalid(&format!("只支持 24bpp DIB，实际 {bits}bpp")));
        }
        if bi_w <= 0 || bi_h == 0 {
            return Err(invalid("视频尺寸非法"));
        }

        let width = bi_w as u32;
        let height = bi_h.unsigned_abs();
        let stride = ((width as usize * 3 + 3) / 4) * 4;
        let usec = av[0].max(1);
        // 24 fps 的 AVI 头是 41667 µs，换算成 24/1 更干净
        let (fps_num, fps_den) = fps_from_usec(usec);

        let info = AviInfo {
            width,
            height,
            fps_num,
            fps_den,
            stride,
            bottom_up: bi_h > 0,
        };

        Ok(Self { info, file, frames, next: 0 })
    }

    pub fn info(&self) -> &AviInfo {
        &self.info
    }

    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// 读下一帧，返回 RGBA（alpha=255）。
    pub fn next_frame_rgba(&mut self) -> io::Result<Option<Vec<u8>>> {
        let Some(&(off, size)) = self.frames.get(self.next) else {
            return Ok(None);
        };
        self.next += 1;

        let w = self.info.width as usize;
        let h = self.info.height as usize;
        let stride = self.info.stride;
        // 有些写入器会在 00db 里带上 40 字节 BITMAPINFOHEADER，这里兼容两种布局
        let data_off = if size == stride * h + 40 {
            off + 40
        } else {
            off
        };
        if size < stride * h || w == 0 || h == 0 {
            return Err(invalid("帧数据长度与尺寸不符"));
        }

        self.file.seek(SeekFrom::Start(data_off))?;
        let mut raw = vec![0u8; stride * h];
        self.file.read_exact(&mut raw)?;

        let mut out = vec![0u8; w * h * 4];
        for y in 0..h {
            let src_y = if self.info.bottom_up { h - 1 - y } else { y };
            let src_row = &raw[src_y * stride..src_y * stride + w * 3];
            let dst_row = &mut out[y * w * 4..(y + 1) * w * 4];
            for x in 0..w {
                let s = x * 3;
                let d = x * 4;
                dst_row[d] = src_row[s + 2]; // BGR -> R
                dst_row[d + 1] = src_row[s + 1];
                dst_row[d + 2] = src_row[s];
                dst_row[d + 3] = 255;
            }
        }
        Ok(Some(out))
    }
}

fn fps_from_usec(usec: u32) -> (u32, u32) {
    // 常见录屏帧率直接命中，避免出现 23.99/1 这种别扭的分数
    for (n, d) in [(24u32, 1u32), (25, 1), (30, 1), (15, 1), (60, 1), (10, 1), (20, 1), (12, 1)] {
        let ideal = 1_000_000f64 / (n as f64 / d as f64);
        if (ideal - usec as f64).abs() < 200.0 {
            return (n, d);
        }
    }
    (1_000_000, usec)
}

fn invalid(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.to_string())
}

/// 顺序扫描 RIFF，收集 avih / strf / 帧索引。LIST 会递归进去。
fn scan(
    file: &mut File,
    start: u64,
    end: u64,
    avih: &mut Option<[u32; 9]>,
    strf: &mut Option<(i32, i32, u16, u32)>,
    frames: &mut Vec<(u64, usize)>,
) -> io::Result<()> {
    let mut pos = start;
    while pos + 8 <= end {
        file.seek(SeekFrom::Start(pos))?;
        let mut hdr = [0u8; 8];
        file.read_exact(&mut hdr)?;
        let id = [hdr[0], hdr[1], hdr[2], hdr[3]];
        let size = u32::from_le_bytes([hdr[4], hdr[5], hdr[6], hdr[7]]) as u64;
        let data = pos + 8;
        let next = data
            .saturating_add(size)
            .saturating_add(size & 1)
            .min(end)
            .max(data);

        if &id == b"LIST" {
            let mut four = [0u8; 4];
            file.read_exact(&mut four)?;
            scan(file, data + 4, (data + size).min(end), avih, strf, frames)?;
        } else if &id == b"avih" && size >= 48 {
            let mut b = [0u8; 40];
            file.read_exact(&mut b)?;
            let mut a = [0u32; 9];
            for (i, v) in a.iter_mut().enumerate() {
                let o = i * 4;
                *v = u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
            }
            *avih = Some(a);
        } else if &id == b"strf" && size >= 20 {
            let mut b = [0u8; 20];
            file.read_exact(&mut b)?;
            let w = i32::from_le_bytes([b[4], b[5], b[6], b[7]]);
            let h = i32::from_le_bytes([b[8], b[9], b[10], b[11]]);
            let bits = u16::from_le_bytes([b[14], b[15]]);
            let comp = u32::from_le_bytes([b[16], b[17], b[18], b[19]]);
            *strf = Some((w, h, bits, comp));
        } else if id[2] == b'd' && (id[3] == b'b' || id[3] == b'c') && &id[0..2] == b"00" {
            frames.push((data, size as usize));
        }
        pos = next;
    }
    Ok(())
}
