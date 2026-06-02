use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::Duration;

use aa_core::{
    AsciiConfig, AsciiResult, InputMode, PAPER_CHARACTER_TARGET, PlacementMode, StructureLineMode,
    ThinningMode, color_illustration_preset, find_default_font, find_paper_font, paper_preset,
    save_ascii_png, save_ascii_text, save_stage_bundle,
};
use eframe::egui::{
    self, Color32, ComboBox, FontFamily, FontId, Frame, Image, Margin, RichText, ScrollArea,
    Slider, Stroke, TextureHandle, TextureOptions, Vec2,
};
use image::RgbaImage;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1120.0, 780.0])
            .with_min_inner_size([920.0, 640.0]),
        ..Default::default()
    };

    eframe::run_native(
        "AA Converter",
        options,
        Box::new(|cc| Ok(Box::new(AaApp::new(cc)))),
    )
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
                format!("Paper profile ready: {chars} chars / target {PAPER_CHARACTER_TARGET}"),
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
                    "Paper preset ready: {} chars / target {}",
                    chars, PAPER_CHARACTER_TARGET
                );
            }
            Err(err) => {
                self.status = format!("Paper preset failed: {err}");
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
        self.status = "Color illustration preset ready.".to_owned();
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
        self.status = "Line art preset ready.".to_owned();
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

    fn sidebar(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        let footer_height = 146.0;
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
        ui.label(RichText::new("AA Converter").size(22.0).strong());
        ui.label(
            RichText::new(self.profile.label())
                .small()
                .color(Color32::from_rgb(166, 176, 172)),
        );

        ui.add_space(14.0);
        ui.horizontal(|ui| {
            if ui
                .add_sized(
                    [ui.available_width() * 0.58, 32.0],
                    egui::Button::new("Open Image"),
                )
                .clicked()
            {
                self.open_image(ctx);
            }
            if ui
                .add_sized([ui.available_width(), 32.0], egui::Button::new("Font"))
                .clicked()
            {
                self.open_font();
            }
        });

        ui.add_space(8.0);
        ui.label(RichText::new("Preset").strong());
        ui.horizontal_wrapped(|ui| {
            if preset_button(ui, "Paper", self.profile == ProfilePreset::Paper).clicked() {
                self.apply_paper_preset();
            }
            if preset_button(
                ui,
                "Color",
                self.profile == ProfilePreset::ColorIllustration,
            )
            .clicked()
            {
                self.apply_color_preset();
            }
            if preset_button(ui, "Line", self.profile == ProfilePreset::LineArt).clicked() {
                self.apply_line_art_preset();
            }
        });

        ui.add_space(8.0);
        path_line(ui, "Image", self.image_path.as_deref());
        path_line(ui, "Font", self.font_path.as_deref());

        ui.add_space(14.0);
        ui.label(RichText::new("Pipeline").strong());
        ComboBox::from_id_salt("input-mode")
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
            });

        ComboBox::from_id_salt("structure-mode")
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
            });

        ComboBox::from_id_salt("thinning-mode")
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
            });

        ComboBox::from_id_salt("placement-mode")
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
            });

        ui.add_space(10.0);
        ui.label(RichText::new("Geometry").strong());
        if compact_slider(
            ui,
            &mut self.config.max_input_width,
            128..=1280,
            "max width",
        )
        .changed()
        {
            self.profile = ProfilePreset::Custom;
        }
        if ui
            .add(Slider::new(&mut self.config.font_px, 8.0..=32.0).text("font px"))
            .changed()
        {
            self.profile = ProfilePreset::Custom;
        }
        if compact_slider(ui, &mut self.config.stripe_stride_px, 10..=44, "stripe px").changed() {
            self.profile = ProfilePreset::Custom;
        }

        ui.add_space(10.0);
        ui.label(RichText::new("Features").strong());
        if ui
            .add(Slider::new(&mut self.config.gaussian_sigma, 0.3..=2.2).text("blur"))
            .changed()
        {
            self.profile = ProfilePreset::Custom;
        }
        if ui
            .add(Slider::new(&mut self.config.edge_threshold, 0.04..=0.72).text("edge"))
            .changed()
        {
            self.profile = ProfilePreset::Custom;
        }
        if ui
            .add(Slider::new(&mut self.config.binary_threshold, 0.05..=0.95).text("binary"))
            .changed()
        {
            self.profile = ProfilePreset::Custom;
        }

        ui.add_space(10.0);
        ui.label(RichText::new("Scoring").strong());
        if ui
            .add(Slider::new(&mut self.config.mismatch_weight, 0.0..=2.0).text("mismatch"))
            .changed()
        {
            self.profile = ProfilePreset::Custom;
        }
        if ui
            .add(Slider::new(&mut self.config.match_weight, 0.1..=2.5).text("match"))
            .changed()
        {
            self.profile = ProfilePreset::Custom;
        }
        if ui
            .add(Slider::new(&mut self.config.score_cutoff, -240.0..=60.0).text("cutoff"))
            .changed()
        {
            self.profile = ProfilePreset::Custom;
        }
        if ui
            .add(Slider::new(&mut self.config.glyph_alpha_threshold, 0.02..=0.6).text("glyph ink"))
            .changed()
        {
            self.profile = ProfilePreset::Custom;
        }

        ui.add_space(10.0);
        ui.label(RichText::new("Characters").strong());
        if ui
            .add(
                egui::TextEdit::multiline(&mut self.config.character_set)
                    .desired_rows(4)
                    .font(FontId::new(12.0, FontFamily::Monospace))
                    .desired_width(f32::INFINITY),
            )
            .changed()
        {
            self.profile = ProfilePreset::Custom;
        }
    }

    fn sidebar_footer(&mut self, ui: &mut egui::Ui) {
        let running = self.pending.is_some();
        let can_export = self.result.is_some();

        ui.add_space(10.0);
        ui.add(
            egui::Label::new(
                RichText::new(&self.status)
                    .small()
                    .color(Color32::from_rgb(210, 214, 205)),
            )
            .wrap(),
        );

        ui.add_space(8.0);
        let convert_fill = if running {
            Color32::from_rgb(66, 76, 78)
        } else {
            Color32::from_rgb(49, 126, 147)
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
        ui.columns(4, |columns| {
            if footer_button(&mut columns[0], can_export, "TXT").clicked() {
                self.export_text();
            }
            if footer_button(&mut columns[1], can_export, "PNG").clicked() {
                self.export_png();
            }
            if footer_button(&mut columns[2], can_export, "Stages").clicked() {
                self.export_stages();
            }
            if footer_button(&mut columns[3], can_export, "Copy").clicked() {
                if let Some(result) = &self.result {
                    ctx.copy_text(result.text.clone());
                    self.status = "Copied text.".to_owned();
                }
            }
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

    fn show_compare(&mut self, ui: &mut egui::Ui) {
        if self.original_texture.is_none() && self.ascii_texture.is_none() {
            self.empty_state(ui);
            return;
        }

        ui.columns(2, |columns| {
            labeled_texture(&mut columns[0], "Original", self.original_texture.as_ref());
            labeled_texture(&mut columns[1], "ASCII", self.ascii_texture.as_ref());
        });
    }

    fn show_text(&self, ui: &mut egui::Ui) {
        let Some(result) = &self.result else {
            stage_placeholder(ui, "Text output pending");
            return;
        };

        let mut text = result.text.clone();
        ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut text)
                        .font(FontId::new(12.0, FontFamily::Monospace))
                        .desired_width(f32::INFINITY)
                        .interactive(false),
                );
            });
    }

    fn empty_state(&mut self, ui: &mut egui::Ui) {
        let available = ui.available_size();
        let height = available.y.min(420.0).max(240.0);

        Frame::new()
            .fill(Color32::from_rgb(228, 225, 216))
            .stroke(Stroke::new(1.0, Color32::from_rgb(190, 186, 174)))
            .inner_margin(Margin::same(24))
            .show(ui, |ui| {
                ui.set_min_size(Vec2::new(available.x.max(260.0), height));
                ui.vertical_centered(|ui| {
                    ui.add_space((height * 0.30).min(130.0));
                    ui.label(
                        RichText::new("No image loaded")
                            .size(22.0)
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
                    .fill(Color32::from_rgb(49, 126, 147))
                    .min_size(Vec2::new(132.0, 36.0));
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
            .exact_width(320.0)
            .frame(
                Frame::new()
                    .fill(Color32::from_rgb(31, 34, 34))
                    .inner_margin(Margin::same(18)),
            )
            .show(ctx, |ui| self.sidebar(ctx, ui));

        egui::CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(Color32::from_rgb(235, 232, 224))
                    .inner_margin(Margin::same(18)),
            )
            .show(ctx, |ui| self.preview(ui));
    }
}

fn tune_style(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = Vec2::new(8.0, 8.0);
    style.spacing.button_padding = Vec2::new(10.0, 6.0);
    style.visuals = egui::Visuals::dark();
    style.visuals.panel_fill = Color32::from_rgb(27, 29, 31);
    style.visuals.window_fill = Color32::from_rgb(27, 29, 31);
    style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(45, 49, 52);
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(61, 70, 74);
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(49, 126, 147);
    style.visuals.selection.bg_fill = Color32::from_rgb(49, 126, 147);
    style.visuals.faint_bg_color = Color32::from_rgb(38, 41, 43);
    ctx.set_style(style);
}

impl ProfilePreset {
    fn label(self) -> &'static str {
        match self {
            Self::Paper => "Paper greedy profile",
            Self::ColorIllustration => "Color illustration profile",
            Self::LineArt => "Line art profile",
            Self::Custom => "Custom profile",
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

fn preset_button(ui: &mut egui::Ui, label: &str, selected: bool) -> egui::Response {
    let fill = if selected {
        Color32::from_rgb(49, 126, 147)
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

fn path_line(ui: &mut egui::Ui, label: &str, path: Option<&Path>) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(label)
                .small()
                .color(Color32::from_rgb(154, 164, 154)),
        );
        let value = path.map(compact_path).unwrap_or_else(|| "none".to_owned());
        ui.label(RichText::new(value).small());
    });
}

fn compact_slider(
    ui: &mut egui::Ui,
    value: &mut u32,
    range: std::ops::RangeInclusive<u32>,
    text: &str,
) -> egui::Response {
    ui.add(Slider::new(value, range).text(text))
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
        Color32::from_rgb(49, 126, 147)
    } else {
        Color32::from_rgb(219, 216, 207)
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
    let Some(texture) = texture else {
        stage_placeholder(ui, placeholder);
        return;
    };

    show_texture(ui, texture);
}

fn show_texture(ui: &mut egui::Ui, texture: &TextureHandle) {
    ScrollArea::both()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let available = ui.available_size();
            let size = texture.size_vec2();
            let scale = (available.x / size.x)
                .min(available.y / size.y)
                .clamp(0.25, 8.0);
            let image_size = size * scale;
            ui.add(Image::new(texture).fit_to_exact_size(image_size));
        });
}

fn labeled_texture(ui: &mut egui::Ui, label: &str, texture: Option<&TextureHandle>) {
    ui.label(
        RichText::new(label)
            .small()
            .strong()
            .color(Color32::from_rgb(75, 76, 72)),
    );
    ui.add_space(6.0);
    show_fit_texture(ui, texture);
}

fn show_fit_texture(ui: &mut egui::Ui, texture: Option<&TextureHandle>) {
    let Some(texture) = texture else {
        stage_placeholder(ui, "ASCII preview pending");
        return;
    };

    let available = ui.available_size();
    let size = texture.size_vec2();
    let scale = (available.x / size.x)
        .min(available.y / size.y)
        .clamp(0.25, 8.0);
    ui.add(Image::new(texture).fit_to_exact_size(size * scale));
}

fn stage_placeholder(ui: &mut egui::Ui, title: &str) {
    let available = ui.available_size();
    let height = available.y.min(360.0).max(180.0);

    Frame::new()
        .fill(Color32::from_rgb(229, 226, 216))
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
