use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::Duration;

use aa_core::{
    AsciiConfig, AsciiResult, InputMode, PAPER_CHARACTER_TARGET, PlacementMode, StructureLineMode,
    ThinningMode, color_illustration_preset, find_default_font, find_paper_font, paper_preset,
    save_ascii_png, save_ascii_text, save_stage_bundle,
};
use arboard::{Clipboard, ImageData};
use eframe::egui::{
    self, Color32, ComboBox, FontFamily, FontId, Frame, Margin, RichText, ScrollArea, Slider,
    Stroke, TextureHandle, TextureOptions, Vec2,
};
use image::RgbaImage;

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
    profile: ProfilePreset,
    image_path: Option<PathBuf>,
    font_path: Option<PathBuf>,
    result: Option<AsciiResult>,
    pending: Option<Receiver<Result<AsciiResult, String>>>,
    original_texture: Option<TextureHandle>,
    line_texture: Option<TextureHandle>,
    orientation_texture: Option<TextureHandle>,
    ascii_texture: Option<TextureHandle>,
    preview_tab: PreviewTab,
    status: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PreviewTab {
    Compare,
    Original,
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
    Custom,
}

impl AaApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        tune_style(&cc.egui_ctx);

        let (config, font_path, profile, status) = match load_paper_config() {
            Ok((config, path, chars)) => (
                config,
                Some(path),
                ProfilePreset::Paper,
                format!(
                    "Clean line-art preset ready: {chars} chars / target {PAPER_CHARACTER_TARGET}"
                ),
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
            profile,
            image_path: None,
            font_path,
            result: None,
            pending: None,
            original_texture: None,
            line_texture: None,
            orientation_texture: None,
            ascii_texture: None,
            preview_tab: PreviewTab::Compare,
            status,
        }
    }

    fn open_image_path(&mut self, ctx: &egui::Context, path: PathBuf) {
        match load_texture_from_path(ctx, &path, "original") {
            Ok(texture) => {
                self.image_path = Some(path.clone());
                self.original_texture = Some(texture);
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
                self.status = format!(
                    "Clean line-art preset ready: {} chars / target {}",
                    chars, PAPER_CHARACTER_TARGET
                );
            }
            Err(err) => {
                self.status = format!("Clean line-art preset failed: {err}");
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
        self.status = "Color edge preset ready.".to_owned();
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
        self.status = "Sensitive line preset ready.".to_owned();
    }

    fn run_conversion(&mut self) {
        if self.pending.is_some() {
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

        let config = self.config.clone();
        let (sender, receiver) = mpsc::channel();

        thread::spawn(move || {
            let message = aa_core::convert_path(&image_path, &font_path, &config)
                .map_err(|err| format!("Conversion failed: {err}"));
            let _ = sender.send(message);
        });

        self.pending = Some(receiver);
        self.status = "Converting...".to_owned();
    }

    fn poll_conversion(&mut self, ctx: &egui::Context) {
        let Some(receiver) = self.pending.take() else {
            return;
        };

        match receiver.try_recv() {
            Ok(Ok(result)) => {
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

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped_files = ctx.input(|input| input.raw.dropped_files.clone());
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
            Ok(()) => self.status = format!("Saved stages to {}", compact_path(&path)),
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
        ui.label(RichText::new("AA Converter").size(23.0).strong());
        ui.label(
            RichText::new(self.profile.label())
                .small()
                .color(SIDEBAR_MUTED),
        );

        ui.add_space(16.0);
        ui.columns(2, |columns| {
            let open_width = columns[0].available_width();
            if columns[0]
                .add_sized([open_width, 34.0], egui::Button::new("Open Image"))
                .clicked()
            {
                self.open_image(ctx);
            }
            let font_width = columns[1].available_width();
            if columns[1]
                .add_sized([font_width, 34.0], egui::Button::new("Select Font"))
                .clicked()
            {
                self.open_font();
            }
        });

        section_label(ui, "Preset");
        ui.horizontal_wrapped(|ui| {
            if preset_button(ui, "Clean lines", self.profile == ProfilePreset::Paper)
                .on_hover_text("Best starting point for clean black-and-white character line art.")
                .clicked()
            {
                self.apply_paper_preset();
            }
            if preset_button(
                ui,
                "Color edges",
                self.profile == ProfilePreset::ColorIllustration,
            )
            .on_hover_text("Experimental color-boundary extraction for color illustrations.")
            .clicked()
            {
                self.apply_color_preset();
            }
            if preset_button(ui, "Sensitive", self.profile == ProfilePreset::LineArt)
                .on_hover_text("Picks up faint, thin, or detail-heavy line art more aggressively.")
                .clicked()
            {
                self.apply_line_art_preset();
            }
        });

        ui.add_space(10.0);
        path_line(ui, "Image", self.image_path.as_deref());
        path_line(ui, "Font", self.font_path.as_deref());

        section_label(ui, "Pipeline");
        ComboBox::from_id_salt("input-mode")
            .width(ui.available_width())
            .selected_text(match self.config.input_mode {
                InputMode::ExtractStructureLines => "structure lines",
                InputMode::TreatAsBinaryLines => "binary lines",
            })
            .show_ui(ui, |ui| {
                if ui
                    .selectable_value(
                        &mut self.config.input_mode,
                        InputMode::ExtractStructureLines,
                        "structure lines",
                    )
                    .changed()
                {
                    self.profile = ProfilePreset::Custom;
                }
                if ui
                    .selectable_value(
                        &mut self.config.input_mode,
                        InputMode::TreatAsBinaryLines,
                        "binary lines",
                    )
                    .changed()
                {
                    self.profile = ProfilePreset::Custom;
                }
            })
            .response
            .on_hover_text(
                "Choose whether the app should extract structure lines or treat the image as already-clean line art.",
            );

        ComboBox::from_id_salt("structure-mode")
            .width(ui.available_width())
            .selected_text(match self.config.structure_line_mode {
                StructureLineMode::FlowDog => "ETF/FDoG-style",
                StructureLineMode::ScharrMagnitude => "Scharr",
            })
            .show_ui(ui, |ui| {
                if ui
                    .selectable_value(
                        &mut self.config.structure_line_mode,
                        StructureLineMode::FlowDog,
                        "ETF/FDoG-style",
                    )
                    .changed()
                {
                    self.profile = ProfilePreset::Custom;
                }
                if ui
                    .selectable_value(
                        &mut self.config.structure_line_mode,
                        StructureLineMode::ScharrMagnitude,
                        "Scharr",
                    )
                    .changed()
                {
                    self.profile = ProfilePreset::Custom;
                }
            })
            .response
            .on_hover_text("ETF/FDoG-style favors smoother coherent contours; Scharr is sharper but can catch more noise.");

        ComboBox::from_id_salt("thinning-mode")
            .width(ui.available_width())
            .selected_text(match self.config.thinning_mode {
                ThinningMode::KmmK3mLookup => "KMM/K3M lookup",
                ThinningMode::ZhangSuen => "Zhang-Suen",
            })
            .show_ui(ui, |ui| {
                if ui
                    .selectable_value(
                        &mut self.config.thinning_mode,
                        ThinningMode::KmmK3mLookup,
                        "KMM/K3M lookup",
                    )
                    .changed()
                {
                    self.profile = ProfilePreset::Custom;
                }
                if ui
                    .selectable_value(
                        &mut self.config.thinning_mode,
                        ThinningMode::ZhangSuen,
                        "Zhang-Suen",
                    )
                    .changed()
                {
                    self.profile = ProfilePreset::Custom;
                }
            })
            .response
            .on_hover_text("Reduces detected lines into thin strokes before glyph matching.");

        ComboBox::from_id_salt("placement-mode")
            .width(ui.available_width())
            .selected_text(match self.config.placement_mode {
                PlacementMode::PaperGreedy => "paper greedy",
                PlacementMode::LeftToRight => "left to right",
            })
            .show_ui(ui, |ui| {
                if ui
                    .selectable_value(
                        &mut self.config.placement_mode,
                        PlacementMode::PaperGreedy,
                        "paper greedy",
                    )
                    .changed()
                {
                    self.profile = ProfilePreset::Custom;
                }
                if ui
                    .selectable_value(
                        &mut self.config.placement_mode,
                        PlacementMode::LeftToRight,
                        "left to right",
                    )
                    .changed()
                {
                    self.profile = ProfilePreset::Custom;
                }
            })
            .response
            .on_hover_text("paper greedy is the recommended placement mode; left to right is mainly a comparison baseline.");

        section_label(ui, "Geometry");
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

        section_label(ui, "Features");
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

        section_label(ui, "Scoring");
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

        section_label(ui, "Characters");
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

    fn sidebar_footer(&mut self, ui: &mut egui::Ui) {
        let running = self.pending.is_some();
        let can_export = self.result.is_some();

        Frame::new()
            .fill(SIDEBAR_PANEL)
            .stroke(Stroke::new(1.0, Color32::from_rgb(48, 55, 55)))
            .inner_margin(Margin::same(10))
            .show(ui, |ui| {
                ui.add(
                    egui::Label::new(RichText::new(&self.status).small().color(SIDEBAR_TEXT))
                        .wrap(),
                );

                ui.add_space(8.0);
                let convert_fill = if running {
                    Color32::from_rgb(66, 76, 78)
                } else {
                    ACCENT
                };
                let convert_button = egui::Button::new(
                    RichText::new(if running { "Converting..." } else { "Convert" })
                        .strong()
                        .color(Color32::WHITE),
                )
                .fill(convert_fill)
                .min_size(Vec2::new(ui.available_width(), 40.0));
                if ui.add_enabled(!running, convert_button).clicked() {
                    self.run_conversion();
                }

                ui.add_space(6.0);
                let ctx = ui.ctx().clone();
                ui.columns(2, |columns| {
                    if footer_button(&mut columns[0], can_export, "Copy ASCII").clicked() {
                        self.copy_text(&ctx);
                    }
                    if footer_button(&mut columns[1], can_export, "Copy Image").clicked() {
                        self.copy_image();
                    }
                });
                ui.columns(3, |columns| {
                    if footer_button(&mut columns[0], can_export, "Save TXT").clicked() {
                        self.export_text();
                    }
                    if footer_button(&mut columns[1], can_export, "Save PNG").clicked() {
                        self.export_png();
                    }
                    if footer_button(&mut columns[2], can_export, "Stages").clicked() {
                        self.export_stages();
                    }
                });
            });
    }

    fn preview(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            preview_tab(ui, &mut self.preview_tab, PreviewTab::Compare);
            preview_tab(ui, &mut self.preview_tab, PreviewTab::Original);
            preview_tab(ui, &mut self.preview_tab, PreviewTab::Lines);
            preview_tab(ui, &mut self.preview_tab, PreviewTab::Orientation);
            preview_tab(ui, &mut self.preview_tab, PreviewTab::Ascii);
            preview_tab(ui, &mut self.preview_tab, PreviewTab::Text);
        });

        self.preview_actions(ui);

        if self.pending.is_some() {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(
                    RichText::new("Converting image...")
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
            if action_button(ui, can_export, "Copy ASCII").clicked() {
                self.copy_text(&ctx);
            }
            if action_button(ui, can_export, "Copy Image").clicked() {
                self.copy_image();
            }
            ui.add_space(6.0);
            if action_button(ui, can_export, "Save TXT").clicked() {
                self.export_text();
            }
            if action_button(ui, can_export, "Save PNG").clicked() {
                self.export_png();
            }
            if action_button(ui, can_export, "Save Stages").clicked() {
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
                    if ui.add(open_button).clicked() {
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
            Self::Paper => "Clean line-art preset",
            Self::ColorIllustration => "Color edge preset",
            Self::LineArt => "Sensitive line preset",
            Self::Custom => "Custom profile",
        }
    }
}

fn section_label(ui: &mut egui::Ui, label: &str) {
    ui.add_space(14.0);
    ui.label(RichText::new(label).strong().color(SIDEBAR_TEXT));
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

fn preset_button(ui: &mut egui::Ui, label: &str, selected: bool) -> egui::Response {
    let fill = if selected {
        ACCENT
    } else {
        Color32::from_rgb(45, 49, 52)
    };
    ui.add(egui::Button::new(label).fill(fill))
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
            Self::Lines => "Lines",
            Self::Orientation => "Direction",
            Self::Ascii => "ASCII",
            Self::Text => "Text",
        }
    }

    fn button_width(self) -> f32 {
        match self {
            Self::Compare | Self::Original => 88.0,
            Self::Orientation => 96.0,
            Self::Lines | Self::Ascii | Self::Text => 72.0,
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
    let response = ui.add(
        egui::Button::new(RichText::new(label).small().color(text_color))
            .fill(fill)
            .min_size(Vec2::new(tab.button_width(), 28.0)),
    );
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
