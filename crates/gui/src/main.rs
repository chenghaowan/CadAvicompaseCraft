#![cfg_attr(windows, windows_subsystem = "windows")]
//! 图形界面：拖入若干 AVI，选质量档，批量压缩并显示前后体积。

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;

use cadcompress_core::pipeline::default_output;
use cadcompress_core::{compress, resolve_ffmpeg, Options, Outcome};
use eframe::egui;

fn main() -> eframe::Result<()> {
    let native = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([820.0, 600.0])
            .with_min_inner_size([640.0, 420.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        concat!("CATIA 录屏压缩 v", env!("CARGO_PKG_VERSION")),
        native,
        Box::new(|_cc| Ok(Box::new(App::new()))),
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Quality {
    High,
    Standard,
    Small,
}

impl Quality {
    fn label(self) -> &'static str {
        match self {
            Quality::High => "高画质  CRF 18",
            Quality::Standard => "标准  CRF 20",
            Quality::Small => "小体积  CRF 24",
        }
    }
    fn opts(self) -> (u32, &'static str) {
        match self {
            Quality::High => (18, "slow"),
            Quality::Standard => (20, "medium"),
            Quality::Small => (24, "medium"),
        }
    }
}

enum Status {
    Pending,
    Running(f32),
    Done(Outcome, PathBuf),
    Failed(String),
}

struct Item {
    path: PathBuf,
    size: u64,
    status: Status,
}

impl Item {
    fn new(path: PathBuf) -> Self {
        let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        Self { path, size, status: Status::Pending }
    }
}

enum Msg {
    Progress(usize, f32),
    Done(usize, Outcome, PathBuf),
    Failed(usize, String),
    Finished,
    DlProgress(Option<f32>),
    DlDone(PathBuf),
    DlFailed(String),
}

/// 下载 ffmpeg 的界面状态
struct Downloading {
    pct: Option<f32>,
}

struct App {
    items: Vec<Item>,
    quality: Quality,
    white_bg: bool,
    running: bool,
    downloading: Option<Downloading>,
    ffmpeg: Option<PathBuf>,
    cancel: Arc<AtomicBool>,
    rx: Option<Receiver<Msg>>,
    error: Option<String>,
    note: Option<String>,
}

impl App {
    fn new() -> Self {
        Self {
            items: Vec::new(),
            quality: Quality::Standard,
            white_bg: true,
            running: false,
            downloading: None,
            ffmpeg: None,
            cancel: Arc::new(AtomicBool::new(false)),
            rx: None,
            error: None,
            note: None,
        }
    }

    fn add_files(&mut self, paths: Vec<PathBuf>) {
        for p in paths {
            let is_avi = p
                .extension()
                .map(|e| e.eq_ignore_ascii_case("avi"))
                .unwrap_or(false);
            if is_avi && p.is_file() && !self.items.iter().any(|i| i.path == p) {
                self.items.push(Item::new(p));
            }
        }
    }

    fn start(&mut self) {
        if self.running || self.downloading.is_some() {
            return;
        }
        if self.ffmpeg.is_none() {
            match resolve_ffmpeg() {
                Ok(p) => self.ffmpeg = Some(p),
                Err(e) => {
                    self.begin_download(e);
                    return;
                }
            }
        }
        let ffmpeg = self.ffmpeg.clone().expect("ffmpeg 已解析");
        let (crf, preset) = self.quality.opts();
        let mut opts = Options::default();
        opts.crf = crf;
        opts.preset = preset.to_string();
        opts.white_bg = self.white_bg;

        let jobs: Vec<(usize, PathBuf)> = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, i)| matches!(i.status, Status::Pending | Status::Failed(_)))
            .map(|(idx, i)| (idx, i.path.clone()))
            .collect();
        if jobs.is_empty() {
            return;
        }

        for (idx, _) in &jobs {
            self.items[*idx].status = Status::Running(0.0);
        }
        self.running = true;
        self.error = None;
        self.note = None;
        self.cancel = Arc::new(AtomicBool::new(false));
        let cancel = self.cancel.clone();
        let (tx, rx) = channel();
        self.rx = Some(rx);

        std::thread::spawn(move || {
            for (idx, path) in jobs {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                let out = default_output(&path);
                let mut last = f32::NEG_INFINITY;
                let res = compress(
                    &path,
                    &out,
                    &opts,
                    &ffmpeg,
                    |p| {
                        let pct = if p.total > 0 {
                            p.done as f32 / p.total as f32 * 100.0
                        } else {
                            0.0
                        };
                        if pct - last >= 0.5 {
                            last = pct;
                            let _ = tx.send(Msg::Progress(idx, pct));
                        }
                    },
                    &cancel,
                );
                match res {
                    Ok(o) => {
                        let _ = tx.send(Msg::Done(idx, o, out));
                    }
                    Err(e) => {
                        let _ = tx.send(Msg::Failed(idx, e.to_string()));
                    }
                }
            }
            let _ = tx.send(Msg::Finished);
        });
    }

    /// 找不到 ffmpeg 时先下载，下载完自动开始压缩
    fn begin_download(&mut self, why: String) {
        self.error = Some(format!("未找到 ffmpeg（{why}），开始自动下载，约 110 MB"));
        self.downloading = Some(Downloading { pct: None });
        self.cancel = Arc::new(AtomicBool::new(false));
        let cancel = self.cancel.clone();
        let (tx, rx) = channel();
        self.rx = Some(rx);

        std::thread::spawn(move || {
            let res = cadcompress_core::ensure_ffmpeg(
                |pr| {
                    let pct = pr.total.map(|t| pr.downloaded as f32 / t as f32 * 100.0);
                    let _ = tx.send(Msg::DlProgress(pct));
                },
                &cancel,
            );
            match res {
                Ok(p) => {
                    let _ = tx.send(Msg::DlDone(p));
                }
                Err(e) => {
                    let _ = tx.send(Msg::DlFailed(e));
                }
            }
        });
    }

    fn poll(&mut self) {
        let mut msgs = Vec::new();
        if let Some(rx) = &self.rx {
            while let Ok(msg) = rx.try_recv() {
                msgs.push(msg);
            }
        }
        for msg in msgs {
            match msg {
                Msg::Progress(i, pct) => {
                    if let Some(item) = self.items.get_mut(i) {
                        item.status = Status::Running(pct);
                    }
                }
                Msg::Done(i, o, out) => {
                    if let Some(item) = self.items.get_mut(i) {
                        item.status = Status::Done(o, out);
                    }
                }
                Msg::Failed(i, e) => {
                    if let Some(item) = self.items.get_mut(i) {
                        item.status = Status::Failed(e);
                    }
                }
                Msg::Finished => {
                    // 取消后没跑完的条目退回等待状态，允许再次开始
                    for item in &mut self.items {
                        if matches!(item.status, Status::Running(_)) {
                            item.status = Status::Pending;
                        }
                    }
                    self.running = false;
                    self.rx = None;
                }
                Msg::DlProgress(pct) => {
                    if let Some(d) = &mut self.downloading {
                        d.pct = pct;
                    }
                }
                Msg::DlDone(path) => {
                    self.downloading = None;
                    self.ffmpeg = Some(path);
                    self.error = None;
                    self.start();
                }
                Msg::DlFailed(e) => {
                    self.downloading = None;
                    self.error = Some(format!("下载 ffmpeg 失败：{e}"));
                }
            }
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll();

        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect()
        });
        if !dropped.is_empty() && !self.running {
            self.add_files(dropped);
        }
        if self.running || self.downloading.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.heading("CATIA 录屏压缩");
            ui.label(egui::RichText::new("未压缩 AVI → H.264 MP4，约 200 倍").weak());
        });
        ui.add_space(6.0);
        ui.separator();
        {
            ui.horizontal(|ui| {
                ui.label("质量档");
                egui::ComboBox::from_id_salt("quality")
                    .selected_text(self.quality.label())
                    .show_ui(ui, |ui| {
                        for q in [Quality::High, Quality::Standard, Quality::Small] {
                            ui.selectable_value(&mut self.quality, q, q.label());
                        }
                    });
                ui.checkbox(&mut self.white_bg, "背景抹白");
                ui.separator();
                let pending = self
                    .items
                    .iter()
                    .filter(|i| !matches!(i.status, Status::Done(..)))
                    .count();
                if ui
                    .add_enabled(
                        !self.running && self.downloading.is_none() && pending > 0,
                        egui::Button::new("开始压缩"),
                    )
                    .clicked()
                {
                    self.start();
                }
                if self.running && ui.button("取消").clicked() {
                    self.cancel.store(true, Ordering::Relaxed);
                }
                if self.downloading.is_some() && ui.button("取消下载").clicked() {
                    self.cancel.store(true, Ordering::Relaxed);
                }
                if !self.running && self.downloading.is_none() && ui.button("清空").clicked() {
                    self.items.clear();
                }
                if let Some(d) = &self.downloading {
                    ui.separator();
                    ui.label("首次运行：正在下载 ffmpeg（约 110 MB）");
                    match d.pct {
                        Some(p) => ui.add(
                            egui::ProgressBar::new((p / 100.0).clamp(0.0, 1.0))
                                .desired_width(200.0)
                                .text(format!("{p:.0}%")),
                        ),
                        None => ui.add(egui::Spinner::new()),
                    };
                }
            });
            if let Some(err) = &self.error {
                ui.colored_label(egui::Color32::from_rgb(200, 60, 60), err);
            }
            if let Some(note) = &self.note {
                ui.label(note);
            }
        }
        ui.separator();

        if self.items.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label(
                        egui::RichText::new("把 CATIA 录屏生成的 .avi 拖到这里\n支持多选，批量处理")
                            .size(16.0)
                            .weak(),
                    );
                });
                return;
            }

    egui::ScrollArea::vertical()
        .max_height(ui.available_height())
        .show(ui, |ui| {
                let mut remove: Option<usize> = None;
                for (idx, item) in self.items.iter().enumerate() {
                    ui.add_space(4.0);
                    egui::Frame::group(ui.style()).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(
                                    item.path
                                        .file_name()
                                        .map(|s| s.to_string_lossy().to_string())
                                        .unwrap_or_default(),
                                )
                                .strong(),
                            );
                            ui.label(format!("{:.1} MB", item.size as f64 / 1_048_576.0));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if !self.running && ui.small_button("移除").clicked() {
                                    remove = Some(idx);
                                }
                                match &item.status {
                                    Status::Done(o, out) => {
                                        ui.label(format!(
                                            "→ {:.2} MB（{:.0} 倍）",
                                            o.out_size as f64 / 1_048_576.0,
                                            o.ratio()
                                        ));
                                        if ui.small_button("目录").clicked() {
                                            if let Some(dir) = out.parent() {
                                                let _ = std::process::Command::new("explorer")
                                                    .arg(dir)
                                                    .spawn();
                                            }
                                        }
                                    }
                                    Status::Running(p) => {
                                        ui.add(
                                            egui::ProgressBar::new(p / 100.0)
                                                .desired_width(220.0)
                                                .text(format!("{p:.0}%")),
                                        );
                                    }
                                    Status::Failed(e) => {
                                        ui.colored_label(
                                            egui::Color32::from_rgb(200, 60, 60),
                                            format!("失败: {e}"),
                                        );
                                    }
                                    Status::Pending => {
                                        ui.label(egui::RichText::new("等待").weak());
                                    }
                                }
                            });
                        });
                    });
                }
                if let Some(i) = remove {
                    self.items.remove(i);
                }
            });
    }
}
