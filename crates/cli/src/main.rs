//! 命令行入口：参数解析、进度输出、结果汇总。

use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use cadcompress_core::pipeline::default_output;
use cadcompress_core::{compress, resolve_ffmpeg, Options};

struct Args {
    input: PathBuf,
    out: PathBuf,
    opts: Options,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("错误: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_args()?;
    let ffmpeg = resolve_ffmpeg()?;

    let in_size = fs::metadata(&args.input)?.len();
    println!(
        "输入: {}  {:.1} MB",
        args.input.display(),
        in_size as f64 / 1_048_576.0
    );
    println!(
        "输出: {}  白底={}  crf={}  preset={}",
        args.out.display(),
        if args.opts.white_bg { "是" } else { "否" },
        args.opts.crf,
        args.opts.preset
    );

    let cancel = AtomicBool::new(false);
    let mut last_pct = 0u32;
    let result = compress(
        &args.input,
        &args.out,
        &args.opts,
        &ffmpeg,
        |p| {
            let pct = if p.total > 0 {
                (p.done * 100 / p.total) as u32
            } else {
                0
            };
            if pct != last_pct || p.done == p.total {
                last_pct = pct;
                print!("\r[{}/{}] {:3}%", p.done, p.total, pct);
                let _ = io::stdout().flush();
            }
        },
        &cancel,
    )?;

    println!(
        "\n完成: {:.2} MB → {:.2} MB（{:.0} 倍），{:.1} 秒，背景占比 {:.1}%",
        result.in_size as f64 / 1_048_576.0,
        result.out_size as f64 / 1_048_576.0,
        result.ratio(),
        result.elapsed,
        result.bg_fraction * 100.0
    );
    Ok(())
}

fn parse_args() -> Result<Args, Box<dyn std::error::Error>> {
    let mut input: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut opts = Options::default();

    let mut it = env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "-h" | "--help" => {
                println!("用法: cadcompress-cli <input.avi> [-o out.mp4] [--crf N] [--preset P] [--keep-bg]");
                println!("  -o, --out <file>  输出路径（默认 <原文件名>_compressed.mp4）");
                println!("      --crf <N>     x264 CRF，默认 20");
                println!("      --preset <P>  x264 preset，默认 medium");
                println!("      --keep-bg     保留原始背景（不做白底）");
                println!("  -V, --version     显示版本");
                std::process::exit(0);
            }
            "-V" | "--version" => {
                println!("cadcompress-cli {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "-o" | "--out" => out = Some(PathBuf::from(it.next().ok_or("--out 需要参数")?)),
            "--crf" => opts.crf = it.next().ok_or("--crf 需要参数")?.parse()?,
            "--preset" => opts.preset = it.next().ok_or("--preset 需要参数")?,
            "--keep-bg" => opts.white_bg = false,
            _ if a.starts_with("--crf=") => opts.crf = a[6..].parse()?,
            _ if a.starts_with("--preset=") => opts.preset = a[9..].to_string(),
            _ if a.starts_with('-') => return Err(format!("未知参数: {a}").into()),
            _ => input = Some(PathBuf::from(a)),
        }
    }

    let input = input.ok_or("缺少输入文件")?;
    if !input.is_file() {
        return Err(format!("找不到文件: {}", input.display()).into());
    }
    let out = out.unwrap_or_else(|| default_output(&input));
    Ok(Args { input, out, opts })
}
