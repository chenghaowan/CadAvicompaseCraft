//! 调用 ffmpeg.exe 作为编码器：stdin 收 rawvideo，stdout 不需要，stderr 直接透传。

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};

/// 当前进程有没有控制台：CLI 在终端里运行为 true，GUI（windows 子系统）为 false。
#[cfg(windows)]
fn has_console() -> bool {
    extern "system" {
        fn GetConsoleWindow() -> *mut core::ffi::c_void;
    }
    // 只查询系统状态，不涉及内存所有权
    unsafe { !GetConsoleWindow().is_null() }
}

#[cfg(not(windows))]
fn has_console() -> bool {
    true
}

pub struct EncConfig {
    pub width: u32,
    pub height: u32,
    pub fps_num: u32,
    pub fps_den: u32,
    pub out: PathBuf,
    pub crf: u32,
    pub preset: String,
    pub codec: String,
}

pub struct Encoder {
    child: Child,
    stdin: Option<ChildStdin>,
}

impl Encoder {
    pub fn spawn(ffmpeg: &Path, cfg: &EncConfig) -> io::Result<Self> {
        let mut cmd = Command::new(ffmpeg);
        cmd.arg("-hide_banner")
            .arg("-v")
            .arg("error")
            .arg("-nostats")
            .arg("-y")
            .arg("-f")
            .arg("rawvideo")
            .arg("-pix_fmt")
            .arg("rgb24")
            .arg("-s")
            .arg(format!("{}x{}", cfg.width, cfg.height))
            .arg("-r")
            .arg(format!("{}/{}", cfg.fps_num, cfg.fps_den))
            .arg("-i")
            .arg("pipe:0");

        // yuv420p 要求偶数边：CATIA 录像常见奇数宽（如 1357），这里少 1 像素即可
        if cfg.width % 2 != 0 || cfg.height % 2 != 0 {
            cmd.arg("-vf")
                .arg("scale=trunc(iw/2)*2:trunc(ih/2)*2");
        }

        cmd.arg("-an")
            .arg("-c:v")
            .arg(&cfg.codec)
            .arg("-crf")
            .arg(cfg.crf.to_string())
            .arg("-preset")
            .arg(&cfg.preset)
            .arg("-pix_fmt")
            .arg("yuv420p")
            .arg("-movflags")
            .arg("+faststart")
            .arg(&cfg.out)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(if has_console() { Stdio::inherit() } else { Stdio::null() });

        // GUI 是 Windows 子系统程序，没有控制台；此时 ffmpeg 作为控制台程序
        // 启动会自己弹出一个黑窗，必须显式禁止。
        #[cfg(windows)]
        if !has_console() {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        let mut child = cmd.spawn()?;
        let stdin = child.stdin.take();
        Ok(Self { child, stdin })
    }

    pub fn write_frame(&mut self, rgb: &[u8]) -> io::Result<()> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "编码器已关闭"))?;
        stdin.write_all(rgb)
    }

    pub fn finish(mut self) -> io::Result<()> {
        drop(self.stdin.take());
        let status = self.child.wait()?;
        if !status.success() {
            let log_hint = if has_console() {
                "错误信息见上方"
            } else {
                "图形界面下不显示 ffmpeg 日志"
            };
            return Err(io::Error::other(format!(
                "ffmpeg 退出码 {}（{log_hint}）",
                status.code().unwrap_or(-1)
            )));
        }
        Ok(())
    }

    /// 取消时直接杀掉编码器，不等它把剩余数据写完。
    pub fn abort(mut self) {
        drop(self.stdin.take());
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
