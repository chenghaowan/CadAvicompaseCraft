//! ffmpeg 缺失时的自动下载：用 Windows 自带的 curl.exe + tar.exe 完成，零新增依赖。
//!
//! 流程：下载 zip（.part）→ SHA-256 校验 → 解压出 ffmpeg.exe → 试运行验证。
//! 安装位置优先程序目录（便携），不可写时退回 %LOCALAPPDATA%\cadcompress。

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::sha256;

/// 版本化 URL：字节稳定，可以钉哈希。换版本时同步更新 URL 和 SHA256。
pub const FFMPEG_URL: &str =
    "https://www.gyan.dev/ffmpeg/builds/packages/ffmpeg-9.0.2-essentials_build.zip";
/// 官方公布的包摘要（gyan.dev 同名 .sha256 文件），置空则跳过校验（不推荐）
pub const FFMPEG_SHA256: &str = "60f467265b1e312373dbcd92200c2618a74850f98d3d078e94296bb3fa2047ba";
/// 下载提示大小（字节），拿不到 Content-Length 时兜底显示
pub const FFMPEG_APPROX_BYTES: u64 = 114_768_076;

const ZIP_ENTRY: &str = "ffmpeg-9.0.2-essentials_build/bin/ffmpeg.exe";

#[derive(Clone, Copy, Debug)]
pub struct DlProgress {
    pub downloaded: u64,
    pub total: Option<u64>,
}

/// 下载并安装 ffmpeg，返回可执行文件路径。已在安装位置则直接返回。
pub fn ensure_ffmpeg(
    mut progress: impl FnMut(DlProgress),
    cancel: &AtomicBool,
) -> Result<PathBuf, String> {
    let dir = install_dir()?;
    let target = dir.join("ffmpeg.exe");
    if target.is_file() {
        return Ok(target);
    }

    let url = std::env::var("CADCOMPRESS_FFMPEG_URL").unwrap_or_else(|_| FFMPEG_URL.to_string());
    let curl = find_tool("curl.exe").ok_or("找不到 curl.exe（Windows 10 以上系统自带）")?;
    let tar = find_tool("tar.exe").ok_or("找不到 tar.exe（Windows 10 以上系统自带）")?;

    let zip_path = dir.join("ffmpeg-download.zip.part");
    let _ = std::fs::remove_file(&zip_path);

    let total = head_content_length(&curl, &url);
    let mut child = Command::new(&curl)
        .args(["-L", "--fail", "--retry", "3", "--connect-timeout", "20"])
        .arg("-o")
        .arg(&zip_path)
        .arg(&url)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("启动 curl 失败: {e}"))?;

    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_file(&zip_path);
            return Err("下载已取消".into());
        }
        if let Ok(meta) = std::fs::metadata(&zip_path) {
            progress(DlProgress { downloaded: meta.len(), total });
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    let _ = std::fs::remove_file(&zip_path);
                    return Err(format!(
                        "下载失败（curl 退出码 {}）",
                        status.code().unwrap_or(-1)
                    ));
                }
                break;
            }
            Ok(None) => {}
            Err(e) => return Err(format!("等待 curl 失败: {e}")),
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let got_size = std::fs::metadata(&zip_path).map(|m| m.len()).unwrap_or(0);
    progress(DlProgress { downloaded: got_size, total });

    // 完整性校验：哈希不符直接删，绝不运行来源不明的可执行文件
    let expect = std::env::var("CADCOMPRESS_FFMPEG_SHA256")
        .unwrap_or_else(|_| FFMPEG_SHA256.to_string());
    if !expect.trim().is_empty() {
        let got = sha256::file_hex(&zip_path).map_err(|e| format!("读取下载文件失败: {e}"))?;
        if !got.eq_ignore_ascii_case(expect.trim()) {
            let _ = std::fs::remove_file(&zip_path);
            return Err(format!(
                "下载文件 SHA-256 校验不符（{got}），已删除，没有运行它"
            ));
        }
    }

    // 解压出 ffmpeg.exe
    let tmp = dir.join("ffmpeg-extract.tmp");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(|e| format!("创建临时目录失败: {e}"))?;

    let mut extracted = tmp.join(ZIP_ENTRY);
    if !extract_zip(&tar, &zip_path, &tmp, Some(ZIP_ENTRY)) || !extracted.is_file() {
        // 布局和预期不同：全部解开再找
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).map_err(|e| format!("创建临时目录失败: {e}"))?;
        if !extract_zip(&tar, &zip_path, &tmp, None) {
            let _ = std::fs::remove_dir_all(&tmp);
            return Err("解压下载包失败".into());
        }
        extracted = find_ffmpeg_in(&tmp).ok_or_else(|| {
            let _ = std::fs::remove_dir_all(&tmp);
            "解压后没有找到 bin/ffmpeg.exe"
        })?;
    }

    std::fs::copy(&extracted, &target).map_err(|e| format!("安装 ffmpeg 失败: {e}"))?;
    let _ = std::fs::remove_dir_all(&tmp);
    let _ = std::fs::remove_file(&zip_path);

    verify_runs(&target)?;
    Ok(target)
}

/// 安装目录：程序目录可写就用它（便携），否则 %LOCALAPPDATA%\cadcompress。
pub fn install_dir() -> Result<PathBuf, String> {
    if let Some(d) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
    {
        if dir_writable(&d) {
            return Ok(d);
        }
    }
    let base = std::env::var("LOCALAPPDATA").map_err(|_| "找不到 LOCALAPPDATA 目录")?;
    let d = Path::new(&base).join("cadcompress");
    std::fs::create_dir_all(&d).map_err(|e| format!("创建目录失败: {e}"))?;
    Ok(d)
}

fn dir_writable(dir: &Path) -> bool {
    let probe = dir.join(".cadcompress_probe");
    match std::fs::File::create(&probe) {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

fn find_tool(name: &str) -> Option<PathBuf> {
    let sys = Path::new(r"C:\Windows\System32").join(name);
    if sys.is_file() {
        return Some(sys);
    }
    for dir in std::env::split_paths(&std::env::var("PATH").unwrap_or_default()) {
        let p = dir.join(name);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

fn head_content_length(curl: &Path, url: &str) -> Option<u64> {
    let out = Command::new(curl)
        .args(["-sIL", "--max-time", "15"])
        .arg(url)
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines()
        .filter(|l| l.to_ascii_lowercase().starts_with("content-length:"))
        .last()
        .and_then(|l| l.split(':').nth(1))
        .and_then(|v| v.trim().parse::<u64>().ok())
}

fn extract_zip(tar: &Path, zip: &Path, dir: &Path, entry: Option<&str>) -> bool {
    let mut cmd = Command::new(tar);
    cmd.arg("-xf").arg(zip).arg("-C").arg(dir);
    if let Some(e) = entry {
        cmd.arg(e);
    }
    cmd.stdout(Stdio::null()).stderr(Stdio::null());
    cmd.status().map(|s| s.success()).unwrap_or(false)
}

fn find_ffmpeg_in(dir: &Path) -> Option<PathBuf> {
    let mut stack = vec![dir.to_path_buf()];
    let mut best: Option<PathBuf> = None;
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.file_name().map(|n| n == "ffmpeg.exe").unwrap_or(false) {
                let better = best
                    .as_ref()
                    .map(|b: &PathBuf| !b.components().any(|c| c.as_os_str() == "bin"))
                    .unwrap_or(true);
                let in_bin = p.components().any(|c| c.as_os_str() == "bin");
                if in_bin || better {
                    best = Some(p);
                }
                if in_bin {
                    return best;
                }
            }
        }
    }
    best
}

fn verify_runs(path: &Path) -> Result<(), String> {
    let out = Command::new(path)
        .arg("-version")
        .output()
        .map_err(|e| format!("下载的 ffmpeg 无法运行: {e}"))?;
    if !out.status.success() || !out.stdout.starts_with(b"ffmpeg") {
        return Err("下载的 ffmpeg 自检失败（-version 输出异常）".into());
    }
    Ok(())
}
