#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::borrow::Cow;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::Duration;

use aa_core::{
    AsciiConfig, AsciiResult, InputMode, PAPER_CHARACTER_TARGET, PlacementMode, StructureLineMode,
    ThinningMode, anime_sketch_paper_preset, color_illustration_preset, find_default_font,
    find_paper_font, paper_preset, save_ascii_png, save_ascii_text, save_stage_bundle,
    soft_grid_preset,
};
use aa_lineart::{
    DownloadProgress, LineCleanupPreset, LineartModel, LineartSession, ModelAvailability,
    ModelManager, ModelStatus,
};
use arboard::{Clipboard, ImageData};
use eframe::egui::{
    self, Color32, ComboBox, FontFamily, FontId, Frame, Margin, RichText, ScrollArea, Slider,
    Stroke, TextureHandle, TextureOptions, Vec2,
};
use image::{DynamicImage, GrayImage, Rgba, RgbaImage};

const SIDEBAR_WIDTH: f32 = 348.0;
const APP_ICON_PNG: &[u8] = include_bytes!("../../../assets/icons/aa-converter-icon.png");
const ACCENT: Color32 = Color32::from_rgb(43, 128, 148);
const ACCENT_STRONG: Color32 = Color32::from_rgb(34, 112, 133);
const SIDEBAR_BG: Color32 = Color32::from_rgb(29, 33, 33);
const SIDEBAR_PANEL: Color32 = Color32::from_rgb(37, 42, 42);
const SIDEBAR_TEXT: Color32 = Color32::from_rgb(219, 224, 214);
const SIDEBAR_MUTED: Color32 = Color32::from_rgb(159, 169, 160);
const CANVAS_BG: Color32 = Color32::from_rgb(236, 232, 223);
const CANVAS_PANEL: Color32 = Color32::from_rgb(228, 224, 214);
const ADVANCED_TUNING_HELP: &str =
    "Optional. Fine-tune how extracted lines are interpreted, detected, and thinned.";

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1180.0, 800.0])
            .with_min_inner_size([980.0, 660.0])
            .with_icon(load_app_icon()),
        ..Default::default()
    };

    eframe::run_native(
        "AA Converter",
        options,
        Box::new(|cc| Ok(Box::new(AaApp::new(cc)))),
    )
}

fn load_app_icon() -> egui::IconData {
    let icon = image::load_from_memory(APP_ICON_PNG)
        .expect("bundled app icon should be a valid PNG")
        .into_rgba8();

    egui::IconData {
        width: icon.width(),
        height: icon.height(),
        rgba: icon.into_raw(),
    }
}

struct AaApp {
    config: AsciiConfig,
    mode: WorkMode,
    profile: ProfilePreset,
    line_extractor: LineExtractorChoice,
    cleanup_preset: LineCleanupPreset,
    model_manager: Option<ModelManager>,
    model_availability: Vec<ModelAvailability>,
    image_path: Option<PathBuf>,
    batch_paths: Vec<PathBuf>,
    batch_output_dir: Option<PathBuf>,
    batch_pending: Option<Receiver<BatchMessage>>,
    batch_done: usize,
    batch_failed: usize,
    font_path: Option<PathBuf>,
    result: Option<AsciiResult>,
    pending: Option<Receiver<Result<ConversionOutput, String>>>,
    original_texture: Option<TextureHandle>,
    ai_lineart_texture: Option<TextureHandle>,
    ai_lineart_preview: Option<RgbaImage>,
    line_texture: Option<TextureHandle>,
    orientation_texture: Option<TextureHandle>,
    ascii_texture: Option<TextureHandle>,
    download_pending: Option<Receiver<DownloadMessage>>,
    download_progress: Option<ModelDownloadState>,
    preview_tab: PreviewTab,
    status: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PreviewTab {
    Compare,
    Original,
    AiLineart,
    Lines,
    Orientation,
    Ascii,
    Text,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProfilePreset {
    Paper,
    ColorIllustration,
    LineArt,
    SoftGrid,
    AiSketch,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkMode {
    Single,
    Batch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineExtractorChoice {
    Classic,
    Ai(LineartModel),
}

struct ConversionOutput {
    result: AsciiResult,
    ai_lineart: Option<RgbaImage>,
}

enum BatchMessage {
    ItemDone {
        index: usize,
        total: usize,
        path: PathBuf,
        result: Result<ConversionOutput, String>,
    },
    Finished {
        converted: usize,
        failed: usize,
        output_dir: PathBuf,
    },
}

enum DownloadMessage {
    Progress {
        model: LineartModel,
        downloaded: u64,
        total: u64,
    },
    Finished {
        model: LineartModel,
        result: Result<PathBuf, String>,
    },
}

#[derive(Debug, Clone)]
struct ModelDownloadState {
    model: LineartModel,
    downloaded: u64,
    total: u64,
}

impl AaApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        tune_style(&cc.egui_ctx);

        let model_manager = ModelManager::new().ok();
        let model_availability = model_manager
            .as_ref()
            .map(ModelManager::availability)
            .unwrap_or_default();

        let (config, font_path, profile, status) = match load_color_config() {
            Ok((config, path)) => (
                config,
                Some(path),
                ProfilePreset::ColorIllustration,
                "Illustration preset ready.".to_owned(),
            ),
            Err(_) => {
                let font_path = find_default_font();
                let status = font_path
                    .as_ref()
                    .map(|path| format!("Font ready: {}", compact_path(path)))
                    .unwrap_or_else(|| "Select a TTF or OTF font to begin.".to_owned());
                (
                    AsciiConfig::default(),
                    font_path,
                    ProfilePreset::Custom,
                    status,
                )
            }
        };

        Self {
            config,
            mode: WorkMode::Single,
            profile,
            line_extractor: LineExtractorChoice::Classic,
            cleanup_preset: LineCleanupPreset::Balanced,
            model_manager,
            model_availability,
            image_path: None,
            batch_paths: Vec::new(),
            batch_output_dir: None,
            batch_pending: None,
            batch_done: 0,
            batch_failed: 0,
            font_path,
            result: None,
            pending: None,
            original_texture: None,
            ai_lineart_texture: None,
            ai_lineart_preview: None,
            line_texture: None,
            orientation_texture: None,
            ascii_texture: None,
            download_pending: None,
            download_progress: None,
            preview_tab: PreviewTab::Compare,
            status,
        }
    }

    fn open_image_path(&mut self, ctx: &egui::Context, path: PathBuf) {
        match load_texture_from_path(ctx, &path, "original") {
            Ok(texture) => {
                self.image_path = Some(path.clone());
                self.original_texture = Some(texture);
                self.ai_lineart_texture = None;
                self.ai_lineart_preview = None;
                self.line_texture = None;
                self.orientation_texture = None;
                self.ascii_texture = None;
                self.result = None;
                self.status = format!("Loaded {}", compact_path(&path));
                self.preview_tab = PreviewTab::Compare;
            }
            Err(err) => {
                self.status = err;
            }
        }
    }

    fn open_image(&mut self, ctx: &egui::Context) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter(
                "Images",
                &["png", "jpg", "jpeg", "webp", "bmp", "tif", "tiff"],
            )
            .pick_file()
        else {
            return;
        };

        self.open_image_path(ctx, path);
    }

    fn add_batch_images(&mut self) {
        let Some(paths) = rfd::FileDialog::new()
            .add_filter(
                "Images",
                &["png", "jpg", "jpeg", "webp", "bmp", "tif", "tiff"],
            )
            .pick_files()
        else {
            return;
        };

        self.add_batch_paths(paths);
    }

    fn add_batch_folder(&mut self) {
        let Some(folder) = rfd::FileDialog::new().pick_folder() else {
            return;
        };

        let paths = collect_folder_images(&folder);
        if paths.is_empty() {
            self.status = format!("No supported images in {}", compact_path(&folder));
            return;
        }

        self.add_batch_paths(paths);
    }

    fn choose_batch_output(&mut self) {
        let Some(path) = rfd::FileDialog::new().pick_folder() else {
            return;
        };

        self.batch_output_dir = Some(path.clone());
        self.status = format!("Batch output: {}", compact_path(&path));
    }

    fn add_batch_paths(&mut self, paths: impl IntoIterator<Item = PathBuf>) {
        let before = self.batch_paths.len();
        for path in paths {
            if !is_supported_image(&path)
                || self.batch_paths.iter().any(|existing| existing == &path)
            {
                continue;
            }
            self.batch_paths.push(path);
        }

        let added = self.batch_paths.len().saturating_sub(before);
        self.status = format!(
            "Batch queue: {} image(s){}",
            self.batch_paths.len(),
            if added == 0 { "" } else { " ready" }
        );
    }

    fn refresh_model_availability(&mut self) {
        if let Ok(manager) = ModelManager::new() {
            self.model_availability = manager.availability();
            self.model_manager = Some(manager);
        }
    }

    fn selected_model_status(&self) -> Option<&ModelStatus> {
        let LineExtractorChoice::Ai(model) = self.line_extractor else {
            return None;
        };
        self.model_availability
            .iter()
            .find(|item| item.entry.id == model.id())
            .map(|item| &item.status)
    }

    fn selected_model_blocker(&self) -> Option<String> {
        let LineExtractorChoice::Ai(model) = self.line_extractor else {
            return None;
        };
        match self.selected_model_status() {
            Some(status) if status.is_available() => None,
            Some(ModelStatus::Corrupt { .. }) => Some(format!(
                "{} needs repair. Use Repair model before converting.",
                model.label()
            )),
            _ => Some(format!(
                "{} is not installed. Use Install model before converting.",
                model.label()
            )),
        }
    }

    fn start_model_download(&mut self, model: LineartModel) {
        if self.is_busy() {
            return;
        }

        let Some(manager) = self.model_manager.clone() else {
            self.status = "Model catalog is unavailable.".to_owned();
            return;
        };

        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let result = manager.download_model(model, |progress: DownloadProgress| {
                let _ = sender.send(DownloadMessage::Progress {
                    model,
                    downloaded: progress.downloaded,
                    total: progress.total,
                });
            });
            let _ = sender.send(DownloadMessage::Finished {
                model,
                result: result.map_err(|err| err.to_string()),
            });
        });

        self.download_pending = Some(receiver);
        self.download_progress = Some(ModelDownloadState {
            model,
            downloaded: 0,
            total: 0,
        });
        self.status = format!("Starting {} install...", model.label());
    }

    fn open_font(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Fonts", &["ttf", "otf"])
            .pick_file()
        else {
            return;
        };

        self.font_path = Some(path.clone());
        self.profile = ProfilePreset::Custom;
        self.status = format!("Font ready: {}", compact_path(&path));
    }

    fn apply_paper_preset(&mut self) {
        match load_paper_config() {
            Ok((config, path, chars)) => {
                self.config = config;
                self.font_path = Some(path);
                self.profile = ProfilePreset::Paper;
                self.line_extractor = LineExtractorChoice::Classic;
                self.status = format!(
                    "Line Art preset ready: {} chars / target {}",
                    chars, PAPER_CHARACTER_TARGET
                );
            }
            Err(err) => {
                self.status = format!("Line Art preset failed: {err}");
            }
        }
    }

    fn apply_color_preset(&mut self) {
        let (config, path) = match load_color_config() {
            Ok((config, path)) => (config, Some(path)),
            Err(_) => (self.config.clone(), self.font_path.clone()),
        };

        self.config = config;
        if let Some(path) = path {
            self.font_path = Some(path);
        }
        self.profile = ProfilePreset::ColorIllustration;
        self.line_extractor = LineExtractorChoice::Classic;
        self.status = "Illustration preset ready.".to_owned();
    }

    fn apply_line_art_preset(&mut self) {
        let (mut config, path) = match load_paper_config() {
            Ok((config, path, _)) => (config, Some(path)),
            Err(_) => (self.config.clone(), self.font_path.clone()),
        };

        config.max_input_width = 640;
        config.font_px = 16.0;
        config.stripe_stride_px = 16;
        config.gaussian_sigma = 0.65;
        config.edge_threshold = 0.2;
        config.binary_threshold = 0.56;
        config.mismatch_weight = 0.65;
        config.match_weight = 1.05;
        config.score_cutoff = -4.0;
        config.glyph_alpha_threshold = 0.14;
        config.input_mode = InputMode::ExtractStructureLines;
        config.structure_line_mode = StructureLineMode::FlowDog;
        config.thinning_mode = ThinningMode::KmmK3mLookup;
        config.placement_mode = PlacementMode::PaperGreedy;

        self.config = config;
        if let Some(path) = path {
            self.font_path = Some(path);
        }
        self.profile = ProfilePreset::LineArt;
        self.line_extractor = LineExtractorChoice::Classic;
        self.status = "Fine Lines preset ready.".to_owned();
    }

    fn apply_soft_grid_preset(&mut self) {
        let (config, path) = match load_soft_grid_config() {
            Ok((config, path)) => (config, Some(path)),
            Err(_) => (self.config.clone(), self.font_path.clone()),
        };

        self.config = config;
        if let Some(path) = path {
            self.font_path = Some(path);
        }
        self.profile = ProfilePreset::SoftGrid;
        self.line_extractor = LineExtractorChoice::Classic;
        self.status = "B2 Soft Grid preset ready.".to_owned();
    }

    fn apply_ai_sketch_preset(&mut self) {
        let (config, path) = match load_ai_sketch_config() {
            Ok((config, path)) => (config, Some(path)),
            Err(_) => (self.config.clone(), self.font_path.clone()),
        };

        self.config = config;
        if let Some(path) = path {
            self.font_path = Some(path);
        }
        self.profile = ProfilePreset::AiSketch;
        self.line_extractor = LineExtractorChoice::Ai(LineartModel::Informative);
        self.cleanup_preset = LineCleanupPreset::Balanced;
        let status = self
            .selected_model_status()
            .map(ModelStatus::label)
            .unwrap_or("Not installed");
        self.status = format!("Informative + Balanced selected ({status}).");
    }

    fn run_conversion(&mut self) {
        if self.is_busy() {
            return;
        }

        let Some(image_path) = self.image_path.clone() else {
            self.status = "Open an image first.".to_owned();
            return;
        };

        let Some(font_path) = self.font_path.clone() else {
            self.status = "Select a font first.".to_owned();
            return;
        };

        if let Some(message) = self.selected_model_blocker() {
            self.status = message;
            return;
        }

        let config = self.config.clone();
        let line_extractor = self.line_extractor;
        let cleanup_preset = self.cleanup_preset;
        let (sender, receiver) = mpsc::channel();

        thread::spawn(move || {
            let message = convert_single_item(
                &image_path,
                &font_path,
                &config,
                line_extractor,
                cleanup_preset,
            );
            let _ = sender.send(message);
        });

        self.pending = Some(receiver);
        self.status = "Converting...".to_owned();
    }

    fn run_batch_conversion(&mut self) {
        if self.is_busy() {
            return;
        }

        if self.batch_paths.is_empty() {
            self.status = "Add images to the batch first.".to_owned();
            return;
        }

        let Some(font_path) = self.font_path.clone() else {
            self.status = "Select a font first.".to_owned();
            return;
        };

        if let Some(message) = self.selected_model_blocker() {
            self.status = message;
            return;
        }

        if self.batch_output_dir.is_none() {
            self.choose_batch_output();
        }

        let Some(output_dir) = self.batch_output_dir.clone() else {
            return;
        };

        let paths = self.batch_paths.clone();
        let config = self.config.clone();
        let line_extractor = self.line_extractor;
        let cleanup_preset = self.cleanup_preset;
        let (sender, receiver) = mpsc::channel();

        thread::spawn(move || {
            let total = paths.len();
            let mut converted = 0;
            let mut failed = 0;
            let font_bytes = match fs::read(&font_path) {
                Ok(bytes) => bytes,
                Err(err) => {
                    let _ = sender.send(BatchMessage::ItemDone {
                        index: 0,
                        total,
                        path: font_path,
                        result: Err(format!("Font load failed: {err}")),
                    });
                    let _ = sender.send(BatchMessage::Finished {
                        converted: 0,
                        failed: total,
                        output_dir,
                    });
                    return;
                }
            };
            let mut lineart_session = match load_lineart_session(line_extractor) {
                Ok(session) => session,
                Err(err) => {
                    for (index, path) in paths.into_iter().enumerate() {
                        failed += 1;
                        let _ = sender.send(BatchMessage::ItemDone {
                            index,
                            total,
                            path,
                            result: Err(err.clone()),
                        });
                    }
                    let _ = sender.send(BatchMessage::Finished {
                        converted,
                        failed,
                        output_dir,
                    });
                    return;
                }
            };

            for (index, path) in paths.into_iter().enumerate() {
                let result = convert_and_save_batch_item(
                    &path,
                    &font_bytes,
                    &config,
                    line_extractor,
                    cleanup_preset,
                    lineart_session.as_mut(),
                    &output_dir,
                    index,
                );
                if result.is_ok() {
                    converted += 1;
                } else {
                    failed += 1;
                }

                let _ = sender.send(BatchMessage::ItemDone {
                    index,
                    total,
                    path,
                    result,
                });
            }

            let _ = sender.send(BatchMessage::Finished {
                converted,
                failed,
                output_dir,
            });
        });

        self.batch_done = 0;
        self.batch_failed = 0;
        self.batch_pending = Some(receiver);
        self.status = format!("Batch converting 0/{}", self.batch_paths.len());
    }

    fn is_busy(&self) -> bool {
        self.pending.is_some() || self.batch_pending.is_some() || self.download_pending.is_some()
    }

    fn poll_conversion(&mut self, ctx: &egui::Context) {
        let Some(receiver) = self.pending.take() else {
            return;
        };

        match receiver.try_recv() {
            Ok(Ok(output)) => {
                if let Some(ai_lineart) = output.ai_lineart {
                    self.ai_lineart_texture = Some(load_texture_from_rgba(
                        ctx,
                        &ai_lineart,
                        "ai-lineart-preview",
                    ));
                    self.ai_lineart_preview = Some(ai_lineart);
                } else {
                    self.ai_lineart_texture = None;
                    self.ai_lineart_preview = None;
                }
                let result = output.result;
                self.line_texture = Some(load_texture_from_rgba(
                    ctx,
                    &result.line_preview,
                    "structure-lines",
                ));
                self.orientation_texture = Some(load_texture_from_rgba(
                    ctx,
                    &result.orientation_preview,
                    "orientation-preview",
                ));
                self.ascii_texture = Some(load_texture_from_rgba(
                    ctx,
                    &result.ascii_preview,
                    "ascii-preview",
                ));
                self.status = format!(
                    "Done in {:.2}s | {} glyphs | {} stripes",
                    result.timings.total.as_secs_f32(),
                    result.stats.placed_glyphs,
                    result.stats.stripes
                );
                self.result = Some(result);
                self.preview_tab = PreviewTab::Compare;
            }
            Ok(Err(err)) => {
                self.status = err;
            }
            Err(TryRecvError::Empty) => {
                self.pending = Some(receiver);
                ctx.request_repaint_after(Duration::from_millis(80));
            }
            Err(TryRecvError::Disconnected) => {
                self.status = "Conversion worker stopped unexpectedly.".to_owned();
            }
        }
    }

    fn poll_batch_conversion(&mut self, ctx: &egui::Context) {
        let Some(receiver) = self.batch_pending.take() else {
            return;
        };

        loop {
            match receiver.try_recv() {
                Ok(BatchMessage::ItemDone {
                    index,
                    total,
                    path,
                    result,
                }) => match result {
                    Ok(output) => {
                        self.batch_done += 1;
                        if let Ok(texture) = load_texture_from_path(ctx, &path, "batch-original") {
                            self.original_texture = Some(texture);
                        }
                        if let Some(ai_lineart) = output.ai_lineart {
                            self.ai_lineart_texture = Some(load_texture_from_rgba(
                                ctx,
                                &ai_lineart,
                                "batch-ai-lineart-preview",
                            ));
                            self.ai_lineart_preview = Some(ai_lineart);
                        } else {
                            self.ai_lineart_texture = None;
                            self.ai_lineart_preview = None;
                        }
                        let result = output.result;
                        self.line_texture = Some(load_texture_from_rgba(
                            ctx,
                            &result.line_preview,
                            "batch-structure-lines",
                        ));
                        self.orientation_texture = Some(load_texture_from_rgba(
                            ctx,
                            &result.orientation_preview,
                            "batch-orientation-preview",
                        ));
                        self.ascii_texture = Some(load_texture_from_rgba(
                            ctx,
                            &result.ascii_preview,
                            "batch-ascii-preview",
                        ));
                        self.image_path = Some(path.clone());
                        self.result = Some(result);
                        self.preview_tab = PreviewTab::Compare;
                        self.status = format!(
                            "Batch converting {}/{}: saved {}",
                            index + 1,
                            total,
                            compact_path(&path)
                        );
                    }
                    Err(err) => {
                        self.batch_failed += 1;
                        self.status = format!("Batch error {}/{}: {}", index + 1, total, err);
                    }
                },
                Ok(BatchMessage::Finished {
                    converted,
                    failed,
                    output_dir,
                }) => {
                    self.status = format!(
                        "Batch complete: {converted} saved, {failed} failed -> {}",
                        compact_path(&output_dir)
                    );
                    break;
                }
                Err(TryRecvError::Empty) => {
                    self.batch_pending = Some(receiver);
                    ctx.request_repaint_after(Duration::from_millis(120));
                    break;
                }
                Err(TryRecvError::Disconnected) => {
                    self.status = "Batch worker stopped unexpectedly.".to_owned();
                    break;
                }
            }
        }
    }

    fn poll_model_download(&mut self, ctx: &egui::Context) {
        let Some(receiver) = self.download_pending.take() else {
            return;
        };

        loop {
            match receiver.try_recv() {
                Ok(DownloadMessage::Progress {
                    model,
                    downloaded,
                    total,
                }) => {
                    self.download_progress = Some(ModelDownloadState {
                        model,
                        downloaded,
                        total,
                    });
                    self.status = format!(
                        "Installing {}: {} / {}",
                        model.label(),
                        format_bytes(downloaded),
                        format_bytes(total)
                    );
                }
                Ok(DownloadMessage::Finished { model, result }) => {
                    self.download_progress = None;
                    match result {
                        Ok(path) => {
                            self.refresh_model_availability();
                            self.status =
                                format!("{} ready: {}", model.label(), compact_path(&path));
                        }
                        Err(err) => {
                            self.refresh_model_availability();
                            self.status = format!("{} install failed: {err}", model.label());
                        }
                    }
                    break;
                }
                Err(TryRecvError::Empty) => {
                    self.download_pending = Some(receiver);
                    ctx.request_repaint_after(Duration::from_millis(120));
                    break;
                }
                Err(TryRecvError::Disconnected) => {
                    self.download_progress = None;
                    self.refresh_model_availability();
                    self.status = "Model install worker stopped unexpectedly.".to_owned();
                    break;
                }
            }
        }
    }

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped_files = ctx.input(|input| input.raw.dropped_files.clone());
        if self.mode == WorkMode::Batch {
            self.add_batch_paths(dropped_files.into_iter().filter_map(|file| file.path));
            return;
        }

        for file in dropped_files {
            let Some(path) = file.path else {
                continue;
            };
            if is_supported_image(&path) {
                self.open_image_path(ctx, path);
                break;
            }
        }
    }

    fn export_text(&mut self) {
        let Some(result) = &self.result else {
            self.status = "Nothing to export yet.".to_owned();
            return;
        };

        let Some(path) = rfd::FileDialog::new()
            .add_filter("Text", &["txt"])
            .set_file_name("ascii-art.txt")
            .save_file()
        else {
            return;
        };

        match save_ascii_text(result, &path) {
            Ok(()) => self.status = format!("Saved {}", compact_path(&path)),
            Err(err) => self.status = format!("Save failed: {err}"),
        }
    }

    fn export_png(&mut self) {
        let Some(result) = &self.result else {
            self.status = "Nothing to export yet.".to_owned();
            return;
        };

        let Some(path) = rfd::FileDialog::new()
            .add_filter("PNG", &["png"])
            .set_file_name("ascii-art.png")
            .save_file()
        else {
            return;
        };

        match save_ascii_png(result, &path) {
            Ok(()) => self.status = format!("Saved {}", compact_path(&path)),
            Err(err) => self.status = format!("Save failed: {err}"),
        }
    }

    fn export_stages(&mut self) {
        let Some(result) = &self.result else {
            self.status = "Nothing to export yet.".to_owned();
            return;
        };

        let Some(path) = rfd::FileDialog::new().pick_folder() else {
            return;
        };

        match save_stage_bundle(result, &path) {
            Ok(()) => {
                if let Some(ai_lineart) = &self.ai_lineart_preview {
                    let _ = ai_lineart.save(path.join("00-ai-lineart.png"));
                }
                self.status = format!("Saved stages to {}", compact_path(&path));
            }
            Err(err) => self.status = format!("Stage export failed: {err}"),
        }
    }

    fn copy_text(&mut self, ctx: &egui::Context) {
        let Some(result) = &self.result else {
            self.status = "Nothing to copy yet.".to_owned();
            return;
        };

        ctx.copy_text(result.text.clone());
        self.status = "Copied ASCII text.".to_owned();
    }

    fn copy_image(&mut self) {
        let Some(result) = &self.result else {
            self.status = "Nothing to copy yet.".to_owned();
            return;
        };

        let image = ImageData {
            width: result.ascii_preview.width() as usize,
            height: result.ascii_preview.height() as usize,
            bytes: Cow::Owned(result.ascii_preview.clone().into_raw()),
        };

        match Clipboard::new().and_then(|mut clipboard| clipboard.set_image(image)) {
            Ok(()) => self.status = "Copied rendered image.".to_owned(),
            Err(err) => self.status = format!("Image copy failed: {err}"),
        }
    }

    fn sidebar(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        let footer_height = 188.0;
        let controls_height = (ui.available_height() - footer_height).max(280.0);

        ScrollArea::vertical()
            .id_salt("controls-scroll")
            .auto_shrink([false, false])
            .max_height(controls_height)
            .show(ui, |ui| self.sidebar_controls(ctx, ui));

        ui.add_space(10.0);
        ui.separator();
        self.sidebar_footer(ui);
    }

    fn sidebar_controls(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        ui.label(RichText::new("AA Converter").size(23.0).strong())
            .on_hover_text("Start with Open Image, keep the Illustration preset, click Convert, then save or copy the result.");
        ui.label(
            RichText::new(self.profile.label())
                .small()
                .color(SIDEBAR_MUTED),
        );

        section_label(ui, "Mode");
        ui.horizontal_wrapped(|ui| {
            if mode_button(ui, "Single", self.mode == WorkMode::Single)
                .on_hover_text("Tune and convert one image.")
                .clicked()
            {
                self.mode = WorkMode::Single;
            }
            if mode_button(ui, "Batch", self.mode == WorkMode::Batch)
                .on_hover_text("Apply the current settings to many images.")
                .clicked()
            {
                self.mode = WorkMode::Batch;
            }
        });

        ui.add_space(10.0);
        match self.mode {
            WorkMode::Single => self.single_source_controls(ctx, ui),
            WorkMode::Batch => self.batch_source_controls(ui),
        }

        section_label(ui, "Preset");
        ui.horizontal_wrapped(|ui| {
            if preset_button(
                ui,
                "Illustration",
                self.profile == ProfilePreset::ColorIllustration,
            )
            .on_hover_text("Recommended first choice for color anime or character illustrations.")
            .clicked()
            {
                self.apply_color_preset();
            }
            if preset_button(ui, "Line Art", self.profile == ProfilePreset::Paper)
                .on_hover_text("Best starting point for clean black-and-white character line art.")
                .clicked()
            {
                self.apply_paper_preset();
            }
            if preset_button(ui, "Fine Lines", self.profile == ProfilePreset::LineArt)
                .on_hover_text("Picks up faint, thin, or detail-heavy line art more aggressively.")
                .clicked()
            {
                self.apply_line_art_preset();
            }
            if preset_button(ui, "B2 Soft Grid", self.profile == ProfilePreset::SoftGrid)
                .on_hover_text(
                    "Alternative sketch-style matcher. Useful for soft AI line art when the default looks too sparse.",
                )
                .clicked()
            {
                self.apply_soft_grid_preset();
            }
            if preset_button(ui, "AI 1px Lines", self.profile == ProfilePreset::AiSketch)
                .on_hover_text(
                    "Use an AI model to make line art first. Choose a model in Line Extraction, install it if needed, then Convert.",
                )
                .clicked()
            {
                self.apply_ai_sketch_preset();
            }
        });

        ui.add_space(10.0);
        if self.mode == WorkMode::Single {
            path_line(ui, "Image", self.image_path.as_deref());
        } else {
            path_line(ui, "Output", self.batch_output_dir.as_deref());
        }
        path_line(ui, "Font", self.font_path.as_deref());

        section_label(ui, "Line Extraction");
        self.line_extractor_controls(ui);
        egui::CollapsingHeader::new(
            RichText::new("Advanced tuning  ?")
                .small()
                .strong()
                .color(SIDEBAR_TEXT),
        )
        .id_salt("advanced-line-tuning")
        .default_open(false)
        .show(ui, |ui| {
            ui.add_space(4.0);
            self.advanced_line_tuning_controls(ui);
        })
        .header_response
        .on_hover_text(ADVANCED_TUNING_HELP);

        section_label(ui, "ASCII Rendering");
        ComboBox::from_id_salt("placement-mode")
            .width(ui.available_width())
            .selected_text(placement_mode_label(self.config.placement_mode))
            .show_ui(ui, |ui| {
                if ui
                    .selectable_value(
                        &mut self.config.placement_mode,
                        PlacementMode::PaperGreedy,
                        placement_mode_label(PlacementMode::PaperGreedy),
                    )
                    .on_hover_text(placement_mode_help(PlacementMode::PaperGreedy))
                    .changed()
                {
                    self.profile = ProfilePreset::Custom;
                }
                if ui
                    .selectable_value(
                        &mut self.config.placement_mode,
                        PlacementMode::LeftToRight,
                        placement_mode_label(PlacementMode::LeftToRight),
                    )
                    .on_hover_text(placement_mode_help(PlacementMode::LeftToRight))
                    .changed()
                {
                    self.profile = ProfilePreset::Custom;
                    if self.config.input_mode == InputMode::NormalizeAiLineart {
                        self.config.input_mode = InputMode::ExtractStructureLines;
                    }
                }
                if ui
                    .selectable_value(
                        &mut self.config.placement_mode,
                        PlacementMode::SoftGrid,
                        placement_mode_label(PlacementMode::SoftGrid),
                    )
                    .on_hover_text(placement_mode_help(PlacementMode::SoftGrid))
                    .changed()
                {
                    self.profile = ProfilePreset::Custom;
                }
            })
            .response
            .on_hover_text(placement_mode_help(self.config.placement_mode));

        if u32_slider(
            ui,
            &mut self.config.max_input_width,
            128..=1280,
            "max width",
        )
        .on_hover_text(
            "Working image width. Higher values preserve more detail and use more characters, but run slower.",
        )
        .changed()
        {
            self.profile = ProfilePreset::Custom;
        }
        if f32_slider(ui, &mut self.config.font_px, 8.0..=32.0, "font px")
            .on_hover_text("Glyph size. Smaller values make denser ASCII art; larger values make it simpler and chunkier.")
            .changed()
        {
            self.profile = ProfilePreset::Custom;
        }
        if u32_slider(ui, &mut self.config.stripe_stride_px, 10..=44, "stripe px")
            .on_hover_text("Vertical row spacing. Lower values pack rows tighter; higher values leave more space.")
            .changed()
        {
            self.profile = ProfilePreset::Custom;
        }

        if f32_slider(ui, &mut self.config.mismatch_weight, 0.0..=2.0, "mismatch")
            .on_hover_text("Penalty for glyph ink that does not match the extracted line image.")
            .changed()
        {
            self.profile = ProfilePreset::Custom;
        }
        if f32_slider(ui, &mut self.config.match_weight, 0.1..=2.5, "match")
            .on_hover_text("Reward for glyph strokes that align with the extracted image.")
            .changed()
        {
            self.profile = ProfilePreset::Custom;
        }
        if f32_slider(ui, &mut self.config.score_cutoff, -240.0..=60.0, "cutoff")
            .on_hover_text(
                "Minimum score for placing a glyph. Higher values reject more weak matches.",
            )
            .changed()
        {
            self.profile = ProfilePreset::Custom;
        }
        if f32_slider(
            ui,
            &mut self.config.glyph_alpha_threshold,
            0.02..=0.6,
            "glyph ink",
        )
        .on_hover_text(
            "How much font alpha counts as visible ink. Higher values make glyph masks stricter.",
        )
        .changed()
        {
            self.profile = ProfilePreset::Custom;
        }

        if ui
            .add(
                egui::TextEdit::multiline(&mut self.config.character_set)
                    .desired_rows(4)
                    .font(FontId::new(12.0, FontFamily::Monospace))
                    .desired_width(f32::INFINITY),
            )
            .on_hover_text(
                "Character set used for glyph placement. Changing it can strongly alter the final style.",
            )
            .changed()
        {
            self.profile = ProfilePreset::Custom;
        }
    }

    fn advanced_line_tuning_controls(&mut self, ui: &mut egui::Ui) {
        control_caption(
            ui,
            "Structure",
            "Which detector finds line candidates in the image.",
        );
        ComboBox::from_id_salt("structure-mode")
            .width(ui.available_width())
            .selected_text(structure_mode_label(self.config.structure_line_mode))
            .show_ui(ui, |ui| {
                if ui
                    .selectable_value(
                        &mut self.config.structure_line_mode,
                        StructureLineMode::FlowDog,
                        structure_mode_label(StructureLineMode::FlowDog),
                    )
                    .on_hover_text(structure_mode_help(StructureLineMode::FlowDog))
                    .changed()
                {
                    self.profile = ProfilePreset::Custom;
                }
                if ui
                    .selectable_value(
                        &mut self.config.structure_line_mode,
                        StructureLineMode::ScharrMagnitude,
                        structure_mode_label(StructureLineMode::ScharrMagnitude),
                    )
                    .on_hover_text(structure_mode_help(StructureLineMode::ScharrMagnitude))
                    .changed()
                {
                    self.profile = ProfilePreset::Custom;
                }
            })
            .response
            .on_hover_text(structure_mode_help(self.config.structure_line_mode));

        control_caption(
            ui,
            "Thinning",
            "How detected strokes are reduced into thinner structure lines.",
        );
        ComboBox::from_id_salt("thinning-mode")
            .width(ui.available_width())
            .selected_text(thinning_mode_label(self.config.thinning_mode))
            .show_ui(ui, |ui| {
                if ui
                    .selectable_value(
                        &mut self.config.thinning_mode,
                        ThinningMode::KmmK3mLookup,
                        thinning_mode_label(ThinningMode::KmmK3mLookup),
                    )
                    .on_hover_text(thinning_mode_help(ThinningMode::KmmK3mLookup))
                    .changed()
                {
                    self.profile = ProfilePreset::Custom;
                }
                if ui
                    .selectable_value(
                        &mut self.config.thinning_mode,
                        ThinningMode::ZhangSuen,
                        thinning_mode_label(ThinningMode::ZhangSuen),
                    )
                    .on_hover_text(thinning_mode_help(ThinningMode::ZhangSuen))
                    .changed()
                {
                    self.profile = ProfilePreset::Custom;
                }
                if ui
                    .selectable_value(
                        &mut self.config.thinning_mode,
                        ThinningMode::GuoHall,
                        thinning_mode_label(ThinningMode::GuoHall),
                    )
                    .on_hover_text(thinning_mode_help(ThinningMode::GuoHall))
                    .changed()
                {
                    self.profile = ProfilePreset::Custom;
                }
            })
            .response
            .on_hover_text(thinning_mode_help(self.config.thinning_mode));

        if f32_slider(ui, &mut self.config.gaussian_sigma, 0.3..=2.2, "blur")
            .on_hover_text("Smooths strokes before orientation scoring. Higher values reduce noise but can erase small detail.")
            .changed()
        {
            self.profile = ProfilePreset::Custom;
        }
        if f32_slider(ui, &mut self.config.edge_threshold, 0.04..=0.72, "edge")
            .on_hover_text("Edge sensitivity. Lower values keep faint edges; higher values keep only stronger contours.")
            .changed()
        {
            self.profile = ProfilePreset::Custom;
        }
        if f32_slider(ui, &mut self.config.binary_threshold, 0.05..=0.95, "binary")
            .on_hover_text(
                "Black/white cutoff for line-art input. Lower values keep lighter gray strokes.",
            )
            .changed()
        {
            self.profile = ProfilePreset::Custom;
        }
    }

    fn single_source_controls(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        ui.columns(2, |columns| {
            let open_width = columns[0].available_width();
            if columns[0]
                .add_sized([open_width, 34.0], egui::Button::new("Open Image"))
                .on_hover_text("Load one image to convert. Character illustrations and clean line art work best.")
                .clicked()
            {
                self.open_image(ctx);
            }
            let font_width = columns[1].available_width();
            if columns[1]
                .add_sized([font_width, 34.0], egui::Button::new("Select Font"))
                .on_hover_text("Optional. Choose a TTF or OTF font for the ASCII glyphs. The bundled font is used by default.")
                .clicked()
            {
                self.open_font();
            }
        });
    }

    fn batch_source_controls(&mut self, ui: &mut egui::Ui) {
        ui.columns(2, |columns| {
            let add_width = columns[0].available_width();
            if columns[0]
                .add_sized([add_width, 34.0], egui::Button::new("Add Images"))
                .on_hover_text("Add several images one by one for batch conversion.")
                .clicked()
            {
                self.add_batch_images();
            }
            let folder_width = columns[1].available_width();
            if columns[1]
                .add_sized([folder_width, 34.0], egui::Button::new("Add Folder"))
                .on_hover_text("Add every supported image in a folder to the batch queue.")
                .clicked()
            {
                self.add_batch_folder();
            }
        });

        ui.columns(2, |columns| {
            let output_width = columns[0].available_width();
            if columns[0]
                .add_sized([output_width, 34.0], egui::Button::new("Output Folder"))
                .on_hover_text("Choose where batch results will be written.")
                .clicked()
            {
                self.choose_batch_output();
            }
            let font_width = columns[1].available_width();
            if columns[1]
                .add_sized([font_width, 34.0], egui::Button::new("Select Font"))
                .on_hover_text("Optional. Choose a TTF or OTF font for the ASCII glyphs. The bundled font is used by default.")
                .clicked()
            {
                self.open_font();
            }
        });

        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(format!("{} image(s) queued", self.batch_paths.len()))
                    .small()
                    .color(SIDEBAR_MUTED),
            );
            if ui
                .add_enabled(
                    !self.is_busy() && !self.batch_paths.is_empty(),
                    egui::Button::new("Clear"),
                )
                .on_hover_text("Remove all queued batch images.")
                .clicked()
            {
                self.batch_paths.clear();
                self.batch_done = 0;
                self.batch_failed = 0;
                self.status = "Batch queue cleared.".to_owned();
            }
        });

        if !self.batch_paths.is_empty() {
            Frame::new()
                .fill(Color32::from_rgb(31, 36, 36))
                .stroke(Stroke::new(1.0, Color32::from_rgb(48, 55, 55)))
                .inner_margin(Margin::same(8))
                .show(ui, |ui| {
                    ScrollArea::vertical()
                        .id_salt("batch-list")
                        .max_height(116.0)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            for (index, path) in self.batch_paths.iter().take(80).enumerate() {
                                ui.label(
                                    RichText::new(format!("{:02}. {}", index + 1, file_name(path)))
                                        .small()
                                        .color(SIDEBAR_TEXT),
                                );
                            }
                            if self.batch_paths.len() > 80 {
                                ui.label(
                                    RichText::new(format!(
                                        "...and {} more",
                                        self.batch_paths.len() - 80
                                    ))
                                    .small()
                                    .color(SIDEBAR_MUTED),
                                );
                            }
                        });
                });
        }
    }

    fn line_extractor_controls(&mut self, ui: &mut egui::Ui) {
        let selected_text = match self.line_extractor {
            LineExtractorChoice::Classic => "Built-in extractor".to_owned(),
            LineExtractorChoice::Ai(model) => {
                let status = self
                    .selected_model_status()
                    .map(ModelStatus::label)
                    .unwrap_or("Not installed");
                format!("{} - {status}", model.label())
            }
        };

        control_caption(
            ui,
            "Extractor",
            "Choose the source of the line art: built-in extraction or an optional AI model.",
        );
        ComboBox::from_id_salt("line-extractor")
            .width(ui.available_width())
            .selected_text(selected_text)
            .show_ui(ui, |ui| {
                if ui
                    .selectable_value(
                        &mut self.line_extractor,
                        LineExtractorChoice::Classic,
                        "Built-in extractor",
                    )
                    .on_hover_text(line_extractor_help(LineExtractorChoice::Classic))
                    .changed()
                {
                    self.profile = ProfilePreset::Custom;
                }

                for model in LineartModel::ALL {
                    let status = self
                        .model_availability
                        .iter()
                        .find(|item| item.entry.id == model.id())
                        .map(|item| item.status.label())
                        .unwrap_or("Not installed");
                    if ui
                        .selectable_value(
                            &mut self.line_extractor,
                            LineExtractorChoice::Ai(model),
                            format!("{} - {status}", model.label()),
                        )
                        .on_hover_text(line_extractor_help(LineExtractorChoice::Ai(model)))
                        .changed()
                    {
                        self.profile = ProfilePreset::AiSketch;
                        self.config.input_mode = InputMode::NormalizeAiLineart;
                        apply_cleanup_preset_to_config(&mut self.config, self.cleanup_preset);
                    }
                }
            })
            .response
            .on_hover_text(line_extractor_help(self.line_extractor));

        if matches!(self.line_extractor, LineExtractorChoice::Classic) {
            if self.config.input_mode == InputMode::NormalizeAiLineart {
                self.config.input_mode = InputMode::ExtractStructureLines;
            }

            control_caption(
                ui,
                "Input mode",
                "How the current image should be interpreted before line detection.",
            );
            ComboBox::from_id_salt("input-mode")
                .width(ui.available_width())
                .selected_text(input_mode_label(self.config.input_mode))
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_value(
                            &mut self.config.input_mode,
                            InputMode::ExtractStructureLines,
                            input_mode_label(InputMode::ExtractStructureLines),
                        )
                        .on_hover_text(input_mode_help(InputMode::ExtractStructureLines))
                        .changed()
                    {
                        self.profile = ProfilePreset::Custom;
                    }
                    if ui
                        .selectable_value(
                            &mut self.config.input_mode,
                            InputMode::TreatAsBinaryLines,
                            input_mode_label(InputMode::TreatAsBinaryLines),
                        )
                        .on_hover_text(input_mode_help(InputMode::TreatAsBinaryLines))
                        .changed()
                    {
                        self.profile = ProfilePreset::Custom;
                    }
                    if ui
                        .selectable_value(
                            &mut self.config.input_mode,
                            InputMode::TreatAsSoftLines,
                            input_mode_label(InputMode::TreatAsSoftLines),
                        )
                        .on_hover_text(input_mode_help(InputMode::TreatAsSoftLines))
                        .changed()
                    {
                        self.profile = ProfilePreset::Custom;
                    }
                })
                .response
                .on_hover_text(input_mode_help(self.config.input_mode));
        } else {
            control_caption(
                ui,
                "1px cleanup",
                "Choose how strongly AI line art is cleaned and thinned before ASCII rendering.",
            );
            ComboBox::from_id_salt("line-cleanup")
                .width(ui.available_width())
                .selected_text(self.cleanup_preset.label())
                .show_ui(ui, |ui| {
                    for cleanup in LineCleanupPreset::ALL {
                        if ui
                            .selectable_value(&mut self.cleanup_preset, cleanup, cleanup.label())
                            .on_hover_text(cleanup.note())
                            .changed()
                        {
                            self.profile = ProfilePreset::AiSketch;
                            apply_cleanup_preset_to_config(&mut self.config, cleanup);
                        }
                    }
                })
                .response
                .on_hover_text(self.cleanup_preset.note());

            ui.horizontal_wrapped(|ui| {
                let status_label = self
                    .selected_model_status()
                    .map(ModelStatus::label)
                    .unwrap_or("Not installed");
                let model_label = match self.line_extractor {
                    LineExtractorChoice::Ai(model) => model.label(),
                    LineExtractorChoice::Classic => "Built-in extractor",
                };
                ui.label(
                    RichText::new(format!("{model_label} - {status_label}"))
                        .small()
                        .color(SIDEBAR_MUTED),
                );

                if let Some(progress) = &self.download_progress {
                    ui.label(
                        RichText::new(format!(
                            "{} {} / {}",
                            progress.model.label(),
                            format_bytes(progress.downloaded),
                            format_bytes(progress.total)
                        ))
                        .small()
                        .color(SIDEBAR_TEXT),
                    );
                }
            });

            if let LineExtractorChoice::Ai(model) = self.line_extractor {
                let needs_download = self
                    .selected_model_status()
                    .map(|status| !status.is_available())
                    .unwrap_or(true);
                if needs_download
                    && ui
                        .add_enabled(
                            !self.is_busy(),
                            egui::Button::new(match self.selected_model_status() {
                                Some(ModelStatus::Corrupt { .. }) => "Repair model",
                                _ => "Install model",
                            }),
                        )
                        .on_hover_text("Install this verified third-party model mirror into the models folder next to the app. See THIRD_PARTY_NOTICES.md for source and license details.")
                        .clicked()
                {
                    self.start_model_download(model);
                }
            }
        }
    }

    fn sidebar_footer(&mut self, ui: &mut egui::Ui) {
        let running = self.is_busy();
        let can_export = self.result.is_some();
        let can_start = match self.mode {
            WorkMode::Single => self.image_path.is_some(),
            WorkMode::Batch => !self.batch_paths.is_empty(),
        };

        Frame::new()
            .fill(SIDEBAR_PANEL)
            .stroke(Stroke::new(1.0, Color32::from_rgb(48, 55, 55)))
            .inner_margin(Margin::same(10))
            .show(ui, |ui| {
                ui.add(
                    egui::Label::new(RichText::new(&self.status).small().color(SIDEBAR_TEXT))
                        .wrap(),
                )
                .on_hover_text(
                    "Current app status. Errors and model install progress appear here.",
                );

                ui.add_space(8.0);
                let convert_fill = if running {
                    Color32::from_rgb(66, 76, 78)
                } else {
                    ACCENT
                };
                let idle_label = match self.mode {
                    WorkMode::Single => "Convert",
                    WorkMode::Batch => "Convert All",
                };
                let convert_button = egui::Button::new(
                    RichText::new(if running { "Converting..." } else { idle_label })
                        .strong()
                        .color(Color32::WHITE),
                )
                .fill(convert_fill)
                .min_size(Vec2::new(ui.available_width(), 40.0));
                if ui
                    .add_enabled(!running && can_start, convert_button)
                    .on_hover_text(match self.mode {
                        WorkMode::Single => {
                            "Convert the loaded image using the current preset and settings."
                        }
                        WorkMode::Batch => {
                            "Convert every queued image using the current preset and settings."
                        }
                    })
                    .clicked()
                {
                    match self.mode {
                        WorkMode::Single => self.run_conversion(),
                        WorkMode::Batch => self.run_batch_conversion(),
                    }
                }

                ui.add_space(6.0);
                let ctx = ui.ctx().clone();
                ui.columns(2, |columns| {
                    if footer_button(&mut columns[0], can_export, "Copy ASCII")
                        .on_hover_text(action_help("Copy ASCII"))
                        .clicked()
                    {
                        self.copy_text(&ctx);
                    }
                    if footer_button(&mut columns[1], can_export, "Copy Image")
                        .on_hover_text(action_help("Copy Image"))
                        .clicked()
                    {
                        self.copy_image();
                    }
                });
                ui.columns(3, |columns| {
                    if footer_button(&mut columns[0], can_export, "Save TXT")
                        .on_hover_text(action_help("Save TXT"))
                        .clicked()
                    {
                        self.export_text();
                    }
                    if footer_button(&mut columns[1], can_export, "Save PNG")
                        .on_hover_text(action_help("Save PNG"))
                        .clicked()
                    {
                        self.export_png();
                    }
                    if footer_button(&mut columns[2], can_export, "Stages")
                        .on_hover_text(action_help("Stages"))
                        .clicked()
                    {
                        self.export_stages();
                    }
                });
            });
    }

    fn preview(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            preview_tab(ui, &mut self.preview_tab, PreviewTab::Compare);
            preview_tab(ui, &mut self.preview_tab, PreviewTab::Original);
            if self.ai_lineart_texture.is_some() {
                preview_tab(ui, &mut self.preview_tab, PreviewTab::AiLineart);
            }
            preview_tab(ui, &mut self.preview_tab, PreviewTab::Lines);
            preview_tab(ui, &mut self.preview_tab, PreviewTab::Orientation);
            preview_tab(ui, &mut self.preview_tab, PreviewTab::Ascii);
            preview_tab(ui, &mut self.preview_tab, PreviewTab::Text);
        });

        self.preview_actions(ui);

        if self.is_busy() {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.spinner();
                let message = if let Some(progress) = &self.download_progress {
                    format!(
                        "Installing {}... {} / {}",
                        progress.model.label(),
                        format_bytes(progress.downloaded),
                        format_bytes(progress.total)
                    )
                } else if self.batch_pending.is_some() {
                    format!(
                        "Batch converting... {} saved, {} failed",
                        self.batch_done, self.batch_failed
                    )
                } else {
                    "Converting image...".to_owned()
                };
                ui.label(
                    RichText::new(message)
                        .small()
                        .color(Color32::from_rgb(92, 98, 88)),
                );
            });
        }

        if let Some(result) = &self.result {
            ui.add_space(4.0);
            ui.label(
                RichText::new(format!(
                    "{} x {} | preprocess {:.0}ms | score {:.0}ms",
                    result.width,
                    result.height,
                    result.timings.preprocess.as_secs_f32() * 1000.0,
                    result.timings.scoring.as_secs_f32() * 1000.0
                ))
                .small()
                .color(Color32::from_rgb(102, 108, 98)),
            );
        }

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(8.0);
        match self.preview_tab {
            PreviewTab::Compare => self.show_compare(ui),
            PreviewTab::Original => {
                if let Some(texture) = self.original_texture.as_ref() {
                    show_texture(ui, texture);
                } else {
                    self.empty_state(ui);
                }
            }
            PreviewTab::AiLineart => show_texture_or_stage_placeholder(
                ui,
                self.ai_lineart_texture.as_ref(),
                "AI lineart preview pending",
            ),
            PreviewTab::Lines => show_texture_or_stage_placeholder(
                ui,
                self.line_texture.as_ref(),
                "Structure preview pending",
            ),
            PreviewTab::Orientation => show_texture_or_stage_placeholder(
                ui,
                self.orientation_texture.as_ref(),
                "Direction preview pending",
            ),
            PreviewTab::Ascii => show_texture_or_stage_placeholder(
                ui,
                self.ascii_texture.as_ref(),
                "ASCII preview pending",
            ),
            PreviewTab::Text => self.show_text(ui),
        }
    }

    fn preview_actions(&mut self, ui: &mut egui::Ui) {
        let can_export = self.result.is_some();
        let ctx = ui.ctx().clone();

        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            if action_button(ui, can_export, "Copy ASCII")
                .on_hover_text(action_help("Copy ASCII"))
                .clicked()
            {
                self.copy_text(&ctx);
            }
            if action_button(ui, can_export, "Copy Image")
                .on_hover_text(action_help("Copy Image"))
                .clicked()
            {
                self.copy_image();
            }
            ui.add_space(6.0);
            if action_button(ui, can_export, "Save TXT")
                .on_hover_text(action_help("Save TXT"))
                .clicked()
            {
                self.export_text();
            }
            if action_button(ui, can_export, "Save PNG")
                .on_hover_text(action_help("Save PNG"))
                .clicked()
            {
                self.export_png();
            }
            if action_button(ui, can_export, "Save Stages")
                .on_hover_text(action_help("Stages"))
                .clicked()
            {
                self.export_stages();
            }
        });
    }

    fn show_compare(&mut self, ui: &mut egui::Ui) {
        if self.original_texture.is_none() && self.ascii_texture.is_none() {
            self.empty_state(ui);
            return;
        }

        let available = ui.available_size();
        let gap = 12.0;
        let pane_size = Vec2::new(
            ((available.x - gap) / 2.0).max(220.0),
            available.y.max(260.0),
        );

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            texture_pane(
                ui,
                Some("Original"),
                self.original_texture.as_ref(),
                "Original image pending",
                pane_size,
                false,
            );
            texture_pane(
                ui,
                Some("ASCII"),
                self.ascii_texture.as_ref(),
                "ASCII preview pending",
                pane_size,
                false,
            );
        });
    }

    fn show_text(&self, ui: &mut egui::Ui) {
        let Some(result) = &self.result else {
            stage_placeholder(ui, "Text output pending");
            return;
        };

        let mut text = result.text.clone();
        let available = ui.available_size();
        let editor_size = Vec2::new(available.x.max(320.0), available.y.max(260.0));

        Frame::new()
            .fill(Color32::from_rgb(6, 7, 7))
            .stroke(Stroke::new(1.0, Color32::from_rgb(65, 66, 62)))
            .inner_margin(Margin::same(12))
            .show(ui, |ui| {
                ui.set_min_size(editor_size - Vec2::splat(24.0));
                ScrollArea::both()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.add_sized(
                            ui.available_size().max(Vec2::new(480.0, 320.0)),
                            egui::TextEdit::multiline(&mut text)
                                .font(FontId::new(12.0, FontFamily::Monospace))
                                .desired_width(f32::INFINITY)
                                .interactive(false),
                        );
                    });
            });
    }

    fn empty_state(&mut self, ui: &mut egui::Ui) {
        let available = ui.available_size();
        let height = available.y.min(400.0).max(240.0);

        Frame::new()
            .fill(CANVAS_PANEL)
            .stroke(Stroke::new(1.0, Color32::from_rgb(188, 184, 172)))
            .inner_margin(Margin::same(28))
            .show(ui, |ui| {
                ui.set_min_size(Vec2::new(available.x.max(260.0), height));
                ui.vertical_centered(|ui| {
                    ui.add_space((height * 0.28).min(118.0));
                    ui.label(
                        RichText::new("No image loaded")
                            .size(24.0)
                            .strong()
                            .color(Color32::from_rgb(55, 57, 52)),
                    );
                    ui.label(
                        RichText::new("PNG, JPG, WEBP, BMP, TIFF")
                            .small()
                            .color(Color32::from_rgb(103, 101, 93)),
                    );
                    ui.add_space(14.0);

                    let open_button = egui::Button::new(
                        RichText::new("Open Image").strong().color(Color32::WHITE),
                    )
                    .fill(ACCENT)
                    .min_size(Vec2::new(150.0, 38.0));
                    if ui
                        .add(open_button)
                        .on_hover_text("Load an image and then press Convert in the left panel.")
                        .clicked()
                    {
                        let ctx = ui.ctx().clone();
                        self.open_image(&ctx);
                    }
                });
            });
    }
}

impl eframe::App for AaApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_dropped_files(ctx);
        self.poll_conversion(ctx);
        self.poll_batch_conversion(ctx);
        self.poll_model_download(ctx);

        egui::SidePanel::left("controls")
            .resizable(false)
            .exact_width(SIDEBAR_WIDTH)
            .frame(Frame::new().fill(SIDEBAR_BG).inner_margin(Margin::same(18)))
            .show(ctx, |ui| self.sidebar(ctx, ui));

        egui::CentralPanel::default()
            .frame(Frame::new().fill(CANVAS_BG).inner_margin(Margin::same(18)))
            .show(ctx, |ui| self.preview(ui));
    }
}

fn tune_style(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = Vec2::new(8.0, 8.0);
    style.spacing.button_padding = Vec2::new(10.0, 6.0);
    style.spacing.slider_width = 154.0;
    style.visuals = egui::Visuals::dark();
    style.visuals.panel_fill = SIDEBAR_BG;
    style.visuals.window_fill = SIDEBAR_BG;
    style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(45, 49, 52);
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(61, 70, 74);
    style.visuals.widgets.active.bg_fill = ACCENT;
    style.visuals.selection.bg_fill = ACCENT;
    style.visuals.faint_bg_color = Color32::from_rgb(38, 41, 43);
    ctx.set_style(style);
}

impl ProfilePreset {
    fn label(self) -> &'static str {
        match self {
            Self::Paper => "Line Art preset",
            Self::ColorIllustration => "Illustration preset",
            Self::LineArt => "Fine Lines preset",
            Self::SoftGrid => "B2 Soft Grid preset",
            Self::AiSketch => "AI 1px Lines preset",
            Self::Custom => "Custom profile",
        }
    }
}

fn section_label(ui: &mut egui::Ui, label: &str) {
    ui.add_space(14.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.label(RichText::new(label).strong().color(SIDEBAR_TEXT));
        if let Some(help) = section_help(label) {
            ui.add(
                egui::Button::new(RichText::new("?").small().color(SIDEBAR_MUTED))
                    .fill(Color32::from_rgb(38, 42, 44))
                    .min_size(Vec2::new(20.0, 20.0)),
            )
            .on_hover_text(help);
        }
    });
}

fn section_help(label: &str) -> Option<&'static str> {
    match label {
        "Mode" => Some(
            "Single converts one image for tuning. Batch applies the same settings to many images.",
        ),
        "Preset" => Some(
            "Start with Illustration for color character art. Try AI 1px Lines when built-in line extraction is not clean enough.",
        ),
        "Line Extraction" => Some(
            "Choose the line-art source first. Open Advanced tuning only when you want to adjust internal detection settings.",
        ),
        "ASCII Rendering" => {
            Some("Tune how glyphs are chosen, placed, spaced, and rendered into ASCII output.")
        }
        _ => None,
    }
}

fn control_caption(ui: &mut egui::Ui, label: &str, help: &str) {
    ui.add_space(6.0);
    ui.label(RichText::new(label).small().strong().color(SIDEBAR_MUTED))
        .on_hover_text(help);
}

fn action_help(label: &str) -> &'static str {
    match label {
        "Copy ASCII" => "Copy the approximate text version to the clipboard.",
        "Copy Image" => "Copy the rendered PNG-style ASCII preview to the clipboard.",
        "Save TXT" => "Save the approximate ASCII text output as a .txt file.",
        "Save PNG" => "Save the rendered ASCII image as a PNG file.",
        "Stages" | "Save Stages" => {
            "Export intermediate previews such as source lines, direction map, AI line art, and final ASCII."
        }
        _ => "Run this action on the current result.",
    }
}

fn input_mode_label(mode: InputMode) -> &'static str {
    match mode {
        InputMode::ExtractStructureLines => "structure lines",
        InputMode::TreatAsBinaryLines => "binary lines",
        InputMode::TreatAsSoftLines => "soft lines",
        InputMode::NormalizeAiLineart => "AI lineart 1px",
    }
}

fn input_mode_help(mode: InputMode) -> &'static str {
    match mode {
        InputMode::ExtractStructureLines => {
            "Use this for normal color illustrations or photos. The app first extracts structure lines from the image."
        }
        InputMode::TreatAsBinaryLines => {
            "Use this when the input is already clean black-and-white line art. The app treats dark pixels as the target lines."
        }
        InputMode::TreatAsSoftLines => {
            "Use this for gray, antialiased, or sketch-like line art. The app preserves soft stroke strength instead of forcing a hard binary cutoff."
        }
        InputMode::NormalizeAiLineart => {
            "Use this after an AI line extractor. The AI line art is thresholded, cleaned, thinned to 1px structure, then matched with glyphs."
        }
    }
}

fn structure_mode_label(mode: StructureLineMode) -> &'static str {
    match mode {
        StructureLineMode::FlowDog => "ETF/FDoG-style",
        StructureLineMode::ScharrMagnitude => "Scharr",
    }
}

fn structure_mode_help(mode: StructureLineMode) -> &'static str {
    match mode {
        StructureLineMode::FlowDog => {
            "Smoother contour extraction inspired by ETF/FDoG line drawing. Better for coherent anime outlines and hair flow."
        }
        StructureLineMode::ScharrMagnitude => {
            "Sharper gradient edge detection. It reacts strongly to local contrast, so it can preserve crisp edges but may catch more texture noise."
        }
    }
}

fn thinning_mode_label(mode: ThinningMode) -> &'static str {
    match mode {
        ThinningMode::KmmK3mLookup => "KMM/K3M lookup",
        ThinningMode::ZhangSuen => "Zhang-Suen",
        ThinningMode::GuoHall => "Guo-Hall",
    }
}

fn thinning_mode_help(mode: ThinningMode) -> &'static str {
    match mode {
        ThinningMode::KmmK3mLookup => {
            "Paper-style thinning path used by the default line-art pipeline. Good first choice for clean line art."
        }
        ThinningMode::ZhangSuen => {
            "Simple skeletonization baseline. Useful for comparison, but diagonal and curved lines can look more brittle."
        }
        ThinningMode::GuoHall => {
            "Thinning used by the AI cleanup preset. Often behaves better on AI line-art masks with small gaps and branches."
        }
    }
}

fn placement_mode_label(mode: PlacementMode) -> &'static str {
    match mode {
        PlacementMode::PaperGreedy => "paper greedy",
        PlacementMode::LeftToRight => "left to right",
        PlacementMode::SoftGrid => "soft grid",
    }
}

fn placement_mode_help(mode: PlacementMode) -> &'static str {
    match mode {
        PlacementMode::PaperGreedy => {
            "Recommended placement mode. It recursively places high-scoring glyphs so the text follows the extracted line structure."
        }
        PlacementMode::LeftToRight => {
            "Baseline comparison mode. It scans in reading order and is mostly useful for checking whether paper greedy is helping."
        }
        PlacementMode::SoftGrid => {
            "B2-style grid matcher for soft sketch or AI line-art input. Try it when paper greedy looks too sparse or fragmented."
        }
    }
}

fn line_extractor_help(extractor: LineExtractorChoice) -> &'static str {
    match extractor {
        LineExtractorChoice::Classic => {
            "Built-in line extraction. No model install required; best first choice for a portable, immediate conversion."
        }
        LineExtractorChoice::Ai(model) => model_help(model),
    }
}

fn model_help(model: LineartModel) -> &'static str {
    match model {
        LineartModel::Informative => {
            "Balanced AI line extraction. Good default when color illustrations have soft outlines."
        }
        LineartModel::Anime2Sketch => {
            "Sketch-focused extractor. Often good for anime faces, hair, and character line art."
        }
        LineartModel::AnilinesBasic => {
            "Cleaner AniLines option. Try this when Anime2Sketch keeps too much sketch noise."
        }
        LineartModel::AnilinesDetail => {
            "More detailed AniLines option. Try this when thin hair or clothing lines disappear."
        }
    }
}

fn load_paper_config() -> Result<(AsciiConfig, PathBuf, usize), String> {
    let path = find_paper_font().ok_or_else(|| "Saitamaar font asset was not found.".to_owned())?;
    let bytes = std::fs::read(&path).map_err(|err| err.to_string())?;
    let config = paper_preset(&bytes).map_err(|err| err.to_string())?;
    let chars = config.character_set.chars().count();
    Ok((config, path, chars))
}

fn load_color_config() -> Result<(AsciiConfig, PathBuf), String> {
    let path = find_paper_font().ok_or_else(|| "Saitamaar font asset was not found.".to_owned())?;
    let bytes = std::fs::read(&path).map_err(|err| err.to_string())?;
    let config = color_illustration_preset(&bytes).map_err(|err| err.to_string())?;
    Ok((config, path))
}

fn load_soft_grid_config() -> Result<(AsciiConfig, PathBuf), String> {
    let path = find_paper_font().ok_or_else(|| "Saitamaar font asset was not found.".to_owned())?;
    let bytes = std::fs::read(&path).map_err(|err| err.to_string())?;
    let config = soft_grid_preset(&bytes).map_err(|err| err.to_string())?;
    Ok((config, path))
}

fn load_ai_sketch_config() -> Result<(AsciiConfig, PathBuf), String> {
    let path = find_paper_font().ok_or_else(|| "Saitamaar font asset was not found.".to_owned())?;
    let bytes = std::fs::read(&path).map_err(|err| err.to_string())?;
    let config = anime_sketch_paper_preset(&bytes).map_err(|err| err.to_string())?;
    Ok((config, path))
}

fn convert_and_save_batch_item(
    image_path: &Path,
    font_bytes: &[u8],
    config: &AsciiConfig,
    line_extractor: LineExtractorChoice,
    cleanup_preset: LineCleanupPreset,
    lineart_session: Option<&mut LineartSession>,
    output_dir: &Path,
    index: usize,
) -> Result<ConversionOutput, String> {
    let image =
        image::open(image_path).map_err(|err| format!("{}: {err}", compact_path(image_path)))?;
    let output = convert_loaded_image(
        &image,
        font_bytes,
        config,
        line_extractor,
        cleanup_preset,
        lineart_session,
    )
    .map_err(|err| format!("{}: {err}", compact_path(image_path)))?;
    fs::create_dir_all(output_dir).map_err(|err| err.to_string())?;

    let prefix = format!("{:03}-{}", index + 1, safe_file_stem(image_path));
    save_ascii_png(
        &output.result,
        output_dir.join(format!("{prefix}-ascii.png")),
    )
    .map_err(|err| err.to_string())?;
    save_ascii_text(
        &output.result,
        output_dir.join(format!("{prefix}-ascii.txt")),
    )
    .map_err(|err| err.to_string())?;
    if let Some(ai_lineart) = &output.ai_lineart {
        ai_lineart
            .save(output_dir.join(format!("{prefix}-ai-lineart.png")))
            .map_err(|err| err.to_string())?;
    }

    Ok(output)
}

fn convert_single_item(
    image_path: &Path,
    font_path: &Path,
    config: &AsciiConfig,
    line_extractor: LineExtractorChoice,
    cleanup_preset: LineCleanupPreset,
) -> Result<ConversionOutput, String> {
    let image = image::open(image_path).map_err(|err| format!("Image load failed: {err}"))?;
    let font_bytes = fs::read(font_path).map_err(|err| format!("Font load failed: {err}"))?;
    let mut lineart_session = load_lineart_session(line_extractor)?;
    convert_loaded_image(
        &image,
        &font_bytes,
        config,
        line_extractor,
        cleanup_preset,
        lineart_session.as_mut(),
    )
    .map_err(|err| format!("Conversion failed: {err}"))
}

fn convert_loaded_image(
    image: &DynamicImage,
    font_bytes: &[u8],
    config: &AsciiConfig,
    line_extractor: LineExtractorChoice,
    cleanup_preset: LineCleanupPreset,
    lineart_session: Option<&mut LineartSession>,
) -> Result<ConversionOutput, String> {
    match line_extractor {
        LineExtractorChoice::Classic => {
            let result =
                aa_core::convert_image(image, font_bytes, config).map_err(|err| err.to_string())?;
            Ok(ConversionOutput {
                result,
                ai_lineart: None,
            })
        }
        LineExtractorChoice::Ai(_) => {
            let session =
                lineart_session.ok_or_else(|| "Lineart model was not loaded.".to_owned())?;
            let lineart = session.extract(image).map_err(|err| err.to_string())?;
            let ai_lineart = gray_to_rgba(&lineart);
            let mut config = config.clone();
            apply_cleanup_preset_to_config(&mut config, cleanup_preset);
            let result =
                aa_core::convert_image(&DynamicImage::ImageLuma8(lineart), font_bytes, &config)
                    .map_err(|err| err.to_string())?;
            Ok(ConversionOutput {
                result,
                ai_lineart: Some(ai_lineart),
            })
        }
    }
}

fn load_lineart_session(
    line_extractor: LineExtractorChoice,
) -> Result<Option<LineartSession>, String> {
    let LineExtractorChoice::Ai(model) = line_extractor else {
        return Ok(None);
    };
    let manager = ModelManager::new().map_err(|err| err.to_string())?;
    let path = manager
        .path_for_model(model)
        .map_err(|err| err.to_string())?;
    LineartSession::new(model, &path)
        .map(Some)
        .map_err(|err| err.to_string())
}

fn apply_cleanup_preset_to_config(config: &mut AsciiConfig, cleanup: LineCleanupPreset) {
    config.input_mode = InputMode::NormalizeAiLineart;
    config.thinning_mode = ThinningMode::GuoHall;
    config.placement_mode = PlacementMode::PaperGreedy;
    config.stroke_tolerance = false;
    match cleanup {
        LineCleanupPreset::Balanced => {
            config.edge_threshold = 0.14;
            config.binary_threshold = 0.42;
            config.min_component_pixels = 4;
            config.short_branch_prune_px = 4;
        }
        LineCleanupPreset::Delicate => {
            config.edge_threshold = 0.08;
            config.binary_threshold = 0.30;
            config.min_component_pixels = 1;
            config.short_branch_prune_px = 2;
        }
        LineCleanupPreset::Clean => {
            config.edge_threshold = 0.20;
            config.binary_threshold = 0.58;
            config.min_component_pixels = 8;
            config.short_branch_prune_px = 8;
        }
    }
}

fn gray_to_rgba(gray: &GrayImage) -> RgbaImage {
    RgbaImage::from_fn(gray.width(), gray.height(), |x, y| {
        let value = gray.get_pixel(x, y)[0];
        Rgba([value, value, value, 255])
    })
}

fn collect_folder_images(folder: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(folder) else {
        return Vec::new();
    };

    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| is_supported_image(path))
        .collect();
    paths.sort();
    paths
}

fn safe_file_stem(path: &Path) -> String {
    let stem = path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("image");
    let sanitized: String = stem
        .chars()
        .map(|ch| match ch {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            ch if ch.is_control() => '_',
            ch => ch,
        })
        .collect();
    let trimmed = sanitized.trim();
    if trimmed.is_empty() {
        "image".to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn preset_button(ui: &mut egui::Ui, label: &str, selected: bool) -> egui::Response {
    let fill = if selected {
        ACCENT
    } else {
        Color32::from_rgb(45, 49, 52)
    };
    ui.add(egui::Button::new(label).fill(fill))
}

fn mode_button(ui: &mut egui::Ui, label: &str, selected: bool) -> egui::Response {
    preset_button(ui, label, selected)
}

fn footer_button(ui: &mut egui::Ui, enabled: bool, label: &str) -> egui::Response {
    ui.add_enabled(
        enabled,
        egui::Button::new(label).min_size(Vec2::new(ui.available_width(), 30.0)),
    )
}

fn action_button(ui: &mut egui::Ui, enabled: bool, label: &str) -> egui::Response {
    let fill = if enabled {
        Color32::from_rgb(238, 235, 225)
    } else {
        Color32::from_rgb(218, 214, 205)
    };
    ui.add_enabled(
        enabled,
        egui::Button::new(
            RichText::new(label)
                .small()
                .color(Color32::from_rgb(51, 57, 55)),
        )
        .fill(fill)
        .min_size(Vec2::new(96.0, 28.0)),
    )
}

fn path_line(ui: &mut egui::Ui, label: &str, path: Option<&Path>) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.label(RichText::new(label).small().color(SIDEBAR_MUTED));
        let value = path.map(compact_path).unwrap_or_else(|| "none".to_owned());
        ui.add(egui::Label::new(RichText::new(value).small().color(SIDEBAR_TEXT)).wrap());
    });
}

fn u32_slider(
    ui: &mut egui::Ui,
    value: &mut u32,
    range: std::ops::RangeInclusive<u32>,
    text: &str,
) -> egui::Response {
    ui.add_sized(
        [ui.available_width(), 22.0],
        Slider::new(value, range).text(text),
    )
}

fn f32_slider(
    ui: &mut egui::Ui,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    text: &str,
) -> egui::Response {
    ui.add_sized(
        [ui.available_width(), 22.0],
        Slider::new(value, range).text(text),
    )
}

impl PreviewTab {
    fn label(self) -> &'static str {
        match self {
            Self::Compare => "Compare",
            Self::Original => "Original",
            Self::AiLineart => "AI Lineart",
            Self::Lines => "Lines",
            Self::Orientation => "Direction",
            Self::Ascii => "ASCII",
            Self::Text => "Text",
        }
    }

    fn button_width(self) -> f32 {
        match self {
            Self::Compare | Self::Original => 88.0,
            Self::AiLineart => 98.0,
            Self::Orientation => 96.0,
            Self::Lines | Self::Ascii | Self::Text => 72.0,
        }
    }

    fn help(self) -> &'static str {
        match self {
            Self::Compare => "Show the original image next to the rendered ASCII result.",
            Self::Original => "Show only the loaded source image.",
            Self::AiLineart => {
                "Show the AI-extracted line art before 1px cleanup and ASCII placement."
            }
            Self::Lines => "Show the line image that the glyph matcher tries to follow.",
            Self::Orientation => "Show estimated stroke directions used for glyph scoring.",
            Self::Ascii => "Show the final rendered ASCII image.",
            Self::Text => "Show the approximate plain-text ASCII output.",
        }
    }
}

fn preview_tab(ui: &mut egui::Ui, selected: &mut PreviewTab, tab: PreviewTab) {
    let is_selected = *selected == tab;
    let label = tab.label();
    let fill = if is_selected {
        ACCENT_STRONG
    } else {
        Color32::from_rgb(222, 218, 208)
    };
    let text_color = if is_selected {
        Color32::WHITE
    } else {
        Color32::from_rgb(68, 70, 66)
    };
    let response = ui
        .add(
            egui::Button::new(RichText::new(label).small().color(text_color))
                .fill(fill)
                .min_size(Vec2::new(tab.button_width(), 28.0)),
        )
        .on_hover_text(tab.help());
    if response.clicked() {
        *selected = tab;
    }
}

fn show_texture_or_stage_placeholder(
    ui: &mut egui::Ui,
    texture: Option<&TextureHandle>,
    placeholder: &str,
) {
    let available = ui.available_size();
    texture_pane(
        ui,
        None,
        texture,
        placeholder,
        Vec2::new(available.x.max(260.0), available.y.max(260.0)),
        true,
    );
}

fn show_texture(ui: &mut egui::Ui, texture: &TextureHandle) {
    let available = ui.available_size();
    texture_pane(
        ui,
        None,
        Some(texture),
        "Preview pending",
        Vec2::new(available.x.max(260.0), available.y.max(260.0)),
        true,
    );
}

fn texture_pane(
    ui: &mut egui::Ui,
    label: Option<&str>,
    texture: Option<&TextureHandle>,
    placeholder: &str,
    pane_size: Vec2,
    allow_upscale: bool,
) {
    let (rect, _) = ui.allocate_exact_size(pane_size, egui::Sense::hover());
    let painter = ui.painter_at(rect);

    painter.rect_filled(rect, 0.0, CANVAS_PANEL);
    painter.rect_stroke(
        rect,
        0.0,
        Stroke::new(1.0, Color32::from_rgb(196, 192, 181)),
        egui::StrokeKind::Inside,
    );

    let mut content_rect = rect.shrink(12.0);
    if let Some(label) = label {
        painter.text(
            content_rect.left_top(),
            egui::Align2::LEFT_TOP,
            label,
            FontId::new(12.0, FontFamily::Proportional),
            Color32::from_rgb(75, 76, 72),
        );
        content_rect.min.y += 24.0;
    }

    if let Some(texture) = texture {
        let image_size = fitted_size(texture.size_vec2(), content_rect.size(), allow_upscale);
        let image_rect = egui::Rect::from_center_size(content_rect.center(), image_size);
        painter.image(
            texture.id(),
            image_rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    } else {
        let placeholder_rect = content_rect.shrink(12.0);
        painter.rect_filled(placeholder_rect, 0.0, Color32::from_rgb(226, 222, 212));
        painter.rect_stroke(
            placeholder_rect,
            0.0,
            Stroke::new(1.0, Color32::from_rgb(198, 194, 183)),
            egui::StrokeKind::Inside,
        );
        painter.text(
            placeholder_rect.center(),
            egui::Align2::CENTER_CENTER,
            placeholder,
            FontId::new(12.0, FontFamily::Proportional),
            Color32::from_rgb(89, 88, 81),
        );
    }
}

fn fitted_size(source: Vec2, bounds: Vec2, allow_upscale: bool) -> Vec2 {
    if source.x <= 0.0 || source.y <= 0.0 {
        return bounds;
    }

    let max_scale = if allow_upscale { 8.0 } else { 1.0 };
    let scale = (bounds.x / source.x)
        .min(bounds.y / source.y)
        .clamp(0.05, max_scale);
    source * scale
}

fn stage_placeholder(ui: &mut egui::Ui, title: &str) {
    let available = ui.available_size();
    let height = available.y.min(360.0).max(180.0);

    Frame::new()
        .fill(CANVAS_PANEL)
        .stroke(Stroke::new(1.0, Color32::from_rgb(196, 192, 181)))
        .inner_margin(Margin::same(18))
        .show(ui, |ui| {
            ui.set_min_size(Vec2::new(available.x.max(220.0), height));
            ui.vertical_centered(|ui| {
                ui.add_space((height * 0.38).min(120.0));
                ui.label(
                    RichText::new(title)
                        .small()
                        .strong()
                        .color(Color32::from_rgb(89, 88, 81)),
                );
            });
        });
}

fn load_texture_from_path(
    ctx: &egui::Context,
    path: &Path,
    name: &str,
) -> Result<TextureHandle, String> {
    let image = image::open(path).map_err(|err| format!("Image load failed: {err}"))?;
    let rgba = image.to_rgba8();
    Ok(load_texture_from_rgba(ctx, &rgba, name))
}

fn load_texture_from_rgba(ctx: &egui::Context, rgba: &RgbaImage, name: &str) -> TextureHandle {
    let color_image = egui::ColorImage::from_rgba_unmultiplied(
        [rgba.width() as usize, rgba.height() as usize],
        rgba.as_raw(),
    );
    ctx.load_texture(name, color_image, TextureOptions::LINEAR)
}

fn is_supported_image(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "webp" | "bmp" | "tif" | "tiff"
            )
        })
        .unwrap_or(false)
}

fn compact_path(path: &Path) -> String {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("unknown");
    let parent = path
        .parent()
        .and_then(|parent| parent.file_name())
        .and_then(|name| name.to_str());

    match parent {
        Some(parent) => format!("{parent}/{file_name}"),
        None => file_name.to_owned(),
    }
}

fn format_bytes(bytes: u64) -> String {
    if bytes == 0 {
        return "0 MB".to_owned();
    }
    format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("unknown")
        .to_owned()
}
