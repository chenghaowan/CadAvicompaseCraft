//! 压缩流水线：AVI 读帧 →（可选）键控白底 → ffmpeg 编码。
//!
//! CLI 和 GUI 共用这一份实现，进度和取消都通过参数注入。

use std::env;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use crate::avi::AviReader;
use crate::encode::{EncConfig, Encoder};
use crate::keying::{keying_whiten, Shot};

#[derive(Clone, Debug)]
pub struct Options {
    /// 把背景抹成纯白（贴文档用；对体积没有影响）
    pub white_bg: bool,
    pub crf: u32,
    pub preset: String,
    pub codec: String,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            white_bg: true,
            crf: 20,
            preset: "medium".into(),
            codec: "libx264".into(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Progress {
    pub done: usize,
    pub total: usize,
}

#[derive(Clone, Debug)]
pub struct Outcome {
    pub frames: usize,
    pub in_size: u64,
    pub out_size: u64,
    pub bg_fraction: f32,
    pub elapsed: f64,
}

impl Outcome {
    pub fn ratio(&self) -> f64 {
        self.in_size as f64 / self.out_size.max(1) as f64
    }
}

/// 把 `input` 压成 `out`。`progress` 每帧调用一次；`cancel` 置位后尽快中止并返回错误。
pub fn compress(
    input: &Path,
    out: &Path,
    opts: &Options,
    ffmpeg: &Path,
    mut progress: impl FnMut(Progress),
    cancel: &AtomicBool,
) -> Result<Outcome, Box<dyn Error>> {
    let mut reader = AviReader::open(input)?;
    let info = reader.info().clone();
    let total = reader.frame_count();

    let cfg = EncConfig {
        width: info.width,
        height: info.height,
        fps_num: info.fps_num,
        fps_den: info.fps_den,
        out: out.to_path_buf(),
        crf: opts.crf,
        preset: opts.preset.clone(),
        codec: opts.codec.clone(),
    };
    let mut encoder = Encoder::spawn(ffmpeg, &cfg)?;

    let started = Instant::now();
    let mut rgb = vec![0u8; info.width as usize * info.height as usize * 3];
    let mut done = 0usize;
    let mut bg_sum = 0f64;

    while let Some(rgba) = reader.next_frame_rgba()? {
        if cancel.load(Ordering::Relaxed) {
            encoder.abort();
            return Err("已取消".into());
        }
        if opts.white_bg {
            let shot = Shot {
                width: info.width as i32,
                height: info.height as i32,
                rgba,
            };
            let keyed = keying_whiten(&shot, false);
            bg_sum += keyed.bg_fraction as f64;
            pack_rgb(&keyed.rgba, &mut rgb);
        } else {
            pack_rgb(&rgba, &mut rgb);
        }
        encoder.write_frame(&rgb)?;
        done += 1;
        progress(Progress { done, total });
    }
    encoder.finish()?;

    Ok(Outcome {
        frames: done,
        in_size: fs::metadata(input)?.len(),
        out_size: fs::metadata(out)?.len(),
        bg_fraction: if done > 0 {
            (bg_sum / done as f64) as f32
        } else {
            0.0
        },
        elapsed: started.elapsed().as_secs_f64(),
    })
}

fn pack_rgb(rgba: &[u8], rgb: &mut [u8]) {
    for (i, px) in rgba.chunks_exact(4).enumerate() {
        let o = i * 3;
        rgb[o] = px[0];
        rgb[o + 1] = px[1];
        rgb[o + 2] = px[2];
    }
}

/// 查找 ffmpeg：环境变量 → 程序目录 → third_party → PATH。
pub fn resolve_ffmpeg() -> Result<PathBuf, String> {
    if let Ok(p) = env::var("CADCOMPRESS_FFMPEG") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Ok(p);
        }
    }

    let exe_dir = env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf));
    let cwd = env::current_dir().ok();
    let local = env::var("LOCALAPPDATA")
        .ok()
        .map(|d| PathBuf::from(d).join("cadcompress"));
    let mut candidates = Vec::new();
    for base in [exe_dir, cwd, local].into_iter().flatten() {
        candidates.push(base.join("ffmpeg.exe"));
        candidates.push(base.join("third_party").join("ffmpeg").join("ffmpeg.exe"));
    }
    for p in &candidates {
        if p.is_file() {
            return Ok(p.clone());
        }
    }

    for dir in env::split_paths(&env::var("PATH").unwrap_or_default()) {
        let p = dir.join("ffmpeg.exe");
        if p.is_file() {
            return Ok(p);
        }
    }
    Err("找不到 ffmpeg.exe：放到程序目录、third_party/ffmpeg/ 或 PATH，或设置 CADCOMPRESS_FFMPEG".into())
}

/// 默认输出路径：源文件旁边 `<名字>_compressed.mp4`，重名时加序号，不覆盖已有文件。
pub fn default_output(input: &Path) -> PathBuf {
    let stem = input
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "output".into());
    let dir = input.parent().unwrap_or_else(|| Path::new("."));
    for n in 1..1000 {
        let name = if n == 1 {
            format!("{stem}_compressed.mp4")
        } else {
            format!("{stem}_compressed_{n}.mp4")
        };
        let p = dir.join(name);
        if !p.exists() {
            return p;
        }
    }
    dir.join(format!("{stem}_compressed.mp4"))
}
