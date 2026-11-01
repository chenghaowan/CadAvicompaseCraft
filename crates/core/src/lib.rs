//! CadAvicompaseCraft 核心库：AVI 读取 + 背景键控 + ffmpeg 编码。

pub mod avi;
pub mod encode;
pub mod fetch;
pub mod keying;
pub mod pipeline;
pub mod sha256;

pub use fetch::{ensure_ffmpeg, install_dir, DlProgress};
pub use pipeline::{compress, default_output, resolve_ffmpeg, Options, Outcome, Progress};
