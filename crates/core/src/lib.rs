//! CadAvicompaseCraft 核心库：AVI 读取 + 背景键控 + ffmpeg 编码。

pub mod avi;
pub mod encode;
pub mod keying;
pub mod pipeline;

pub use pipeline::{compress, resolve_ffmpeg, Options, Outcome, Progress};
