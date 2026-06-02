use std::collections::HashSet;
use std::f32::consts::PI;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use fontdue::{Font, FontSettings};
use image::imageops::FilterType;
use image::{DynamicImage, GrayImage, ImageBuffer, Luma, Rgba, RgbaImage};
use imageproc::filter;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use ttf_parser::Face;

pub mod benchmark;

#[derive(Debug, thiserror::Error)]
pub enum AaError {
    #[error("image load failed: {0}")]
    Image(#[from] image::ImageError),
    #[error("font load failed: {0}")]
    Font(String),
    #[error("file operation failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("json operation failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("benchmark failed: {0}")]
    Benchmark(String),
    #[error("no usable characters were found in the selected character set")]
    EmptyCharacterSet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlacementMode {
    PaperGreedy,
    LeftToRight,
}

impl Default for PlacementMode {
    fn default() -> Self {
        Self::PaperGreedy
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InputMode {
    ExtractStructureLines,
    TreatAsBinaryLines,
}

impl Default for InputMode {
    fn default() -> Self {
        Self::ExtractStructureLines
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StructureLineMode {
    FlowDog,
    ScharrMagnitude,
}

impl Default for StructureLineMode {
    fn default() -> Self {
        Self::FlowDog
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThinningMode {
    KmmK3mLookup,
    ZhangSuen,
}

impl Default for ThinningMode {
    fn default() -> Self {
        Self::KmmK3mLookup
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AsciiConfig {
    pub max_input_width: u32,
    pub font_px: f32,
    pub stripe_stride_px: u32,
    pub gaussian_sigma: f32,
    pub edge_threshold: f32,
    pub binary_threshold: f32,
    pub mismatch_weight: f32,
    pub match_weight: f32,
    pub score_cutoff: f32,
    pub glyph_alpha_threshold: f32,
    pub input_mode: InputMode,
    pub structure_line_mode: StructureLineMode,
    pub thinning_mode: ThinningMode,
    pub placement_mode: PlacementMode,
    #[serde(default)]
    pub color_edge_boost: bool,
    #[serde(default)]
    pub stroke_tolerance: bool,
    #[serde(default = "default_flowdog_etf_radius")]
    pub flowdog_etf_radius: u32,
    #[serde(default = "default_flowdog_etf_iterations")]
    pub flowdog_etf_iterations: usize,
    #[serde(default = "default_flowdog_sigma_c")]
    pub flowdog_sigma_c: f32,
    #[serde(default = "default_flowdog_sigma_s")]
    pub flowdog_sigma_s: f32,
    #[serde(default = "default_flowdog_sigma_m")]
    pub flowdog_sigma_m: f32,
    #[serde(default = "default_flowdog_rho")]
    pub flowdog_rho: f32,
    pub character_set: String,
}

impl Default for AsciiConfig {
    fn default() -> Self {
        Self {
            max_input_width: 640,
            font_px: 16.0,
            stripe_stride_px: 18,
            gaussian_sigma: 0.7,
            edge_threshold: 0.22,
            binary_threshold: 0.58,
            mismatch_weight: 0.65,
            match_weight: 1.0,
            score_cutoff: 0.0,
            glyph_alpha_threshold: 0.16,
            input_mode: InputMode::ExtractStructureLines,
            structure_line_mode: StructureLineMode::FlowDog,
            thinning_mode: ThinningMode::KmmK3mLookup,
            placement_mode: PlacementMode::PaperGreedy,
            color_edge_boost: false,
            stroke_tolerance: false,
            flowdog_etf_radius: default_flowdog_etf_radius(),
            flowdog_etf_iterations: default_flowdog_etf_iterations(),
            flowdog_sigma_c: default_flowdog_sigma_c(),
            flowdog_sigma_s: default_flowdog_sigma_s(),
            flowdog_sigma_m: default_flowdog_sigma_m(),
            flowdog_rho: default_flowdog_rho(),
            character_set: DEFAULT_CHARACTER_SET.to_owned(),
        }
    }
}

fn default_flowdog_etf_radius() -> u32 {
    3
}

fn default_flowdog_etf_iterations() -> usize {
    3
}

fn default_flowdog_sigma_c() -> f32 {
    0.8
}

fn default_flowdog_sigma_s() -> f32 {
    1.6
}

fn default_flowdog_sigma_m() -> f32 {
    2.0
}

fn default_flowdog_rho() -> f32 {
    0.98
}

pub const DEFAULT_CHARACTER_SET: &str = " \u{3000}!\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdefghijklmnopqrstuvwxyz{|}~";
pub const PAPER_CHARACTER_TARGET: usize = 752;

#[derive(Debug, Clone)]
pub struct AsciiResult {
    pub text: String,
    pub width: u32,
    pub height: u32,
    pub line_preview: RgbaImage,
    pub orientation_preview: RgbaImage,
    pub ascii_preview: RgbaImage,
    pub placements: Vec<PlacedGlyph>,
    pub timings: PipelineTimings,
    pub stats: PipelineStats,
}

#[derive(Debug, Clone, Default)]
pub struct PipelineTimings {
    pub preprocess: Duration,
    pub feature_extraction: Duration,
    pub glyph_analysis: Duration,
    pub scoring: Duration,
    pub placement: Duration,
    pub rendering: Duration,
    pub total: Duration,
}

#[derive(Debug, Clone, Default)]
pub struct PipelineStats {
    pub input_size: (u32, u32),
    pub working_size: (u32, u32),
    pub stripes: usize,
    pub glyphs: usize,
    pub placed_glyphs: usize,
    pub foreground_pixels: usize,
}

#[derive(Debug, Clone)]
pub struct PlacedGlyph {
    pub ch: char,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone)]
struct InkImage {
    width: u32,
    height: u32,
    ink: Vec<f32>,
}

impl InkImage {
    fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            ink: vec![0.0; (width * height) as usize],
        }
    }

    fn idx(&self, x: u32, y: u32) -> usize {
        (y * self.width + x) as usize
    }

    fn get(&self, x: i32, y: i32) -> f32 {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return 0.0;
        }
        self.ink[self.idx(x as u32, y as u32)]
    }

    fn set(&mut self, x: u32, y: u32, value: f32) {
        let idx = self.idx(x, y);
        self.ink[idx] = value.clamp(0.0, 1.0);
    }

    fn foreground_count(&self) -> usize {
        self.ink.iter().filter(|v| **v > 0.5).count()
    }
}

#[derive(Debug, Clone)]
struct FeatureImage {
    width: u32,
    height: u32,
    source_ink: Vec<f32>,
    value: Vec<f32>,
    orientation: Vec<Option<f32>>,
}

impl FeatureImage {
    fn idx(&self, x: u32, y: u32) -> usize {
        (y * self.width + x) as usize
    }

    fn value_at(&self, x: i32, y: i32) -> f32 {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return 0.0;
        }
        self.value[self.idx(x as u32, y as u32)]
    }

    fn orientation_at(&self, x: i32, y: i32) -> Option<f32> {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return None;
        }
        self.orientation[self.idx(x as u32, y as u32)]
    }

    fn source_at(&self, x: i32, y: i32) -> f32 {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return 0.0;
        }
        self.source_ink[self.idx(x as u32, y as u32)]
    }
}

#[derive(Debug, Clone)]
struct GlyphImage {
    ch: char,
    advance: u32,
    width: u32,
    height: u32,
    alpha: Vec<f32>,
    foreground: Vec<(u32, u32)>,
    feature: FeatureImage,
    is_blank: bool,
}

impl GlyphImage {
    fn alpha_at(&self, x: u32, y: u32) -> f32 {
        self.alpha[(y * self.width + x) as usize]
    }
}

#[derive(Debug, Clone)]
struct Candidate {
    x: u32,
    glyph_index: usize,
    score: f32,
}

#[derive(Debug, Clone)]
struct StripeScore {
    width: u32,
    candidates: Vec<Candidate>,
}

pub fn find_default_font() -> Option<PathBuf> {
    if let Some(path) = find_paper_font() {
        return Some(path);
    }

    let candidates = [
        r"C:\Windows\Fonts\seguiemj.ttf",
        r"C:\Windows\Fonts\segoeui.ttf",
        r"C:\Windows\Fonts\arial.ttf",
        r"C:\Windows\Fonts\consola.ttf",
        "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
        "/System/Library/Fonts/Supplemental/Arial.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/truetype/liberation2/LiberationSans-Regular.ttf",
    ];

    candidates
        .iter()
        .map(PathBuf::from)
        .find(|path| path.exists())
}

pub fn find_paper_font() -> Option<PathBuf> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let candidates = [
        manifest_dir.join("../../assets/fonts/Saitamaar-Regular.ttf"),
        PathBuf::from("assets/fonts/Saitamaar-Regular.ttf"),
        PathBuf::from("../assets/fonts/Saitamaar-Regular.ttf"),
        PathBuf::from("../../assets/fonts/Saitamaar-Regular.ttf"),
    ];

    candidates
        .into_iter()
        .find(|path| path.exists())
        .and_then(|path| path.canonicalize().ok().or(Some(path)))
}

pub fn paper_preset(font_bytes: &[u8]) -> Result<AsciiConfig, AaError> {
    Ok(AsciiConfig {
        max_input_width: 640,
        font_px: 16.0,
        stripe_stride_px: 16,
        gaussian_sigma: 0.7,
        edge_threshold: 0.22,
        binary_threshold: 0.58,
        mismatch_weight: 0.65,
        match_weight: 1.0,
        score_cutoff: 0.0,
        glyph_alpha_threshold: 0.16,
        input_mode: InputMode::ExtractStructureLines,
        structure_line_mode: StructureLineMode::FlowDog,
        thinning_mode: ThinningMode::KmmK3mLookup,
        placement_mode: PlacementMode::PaperGreedy,
        color_edge_boost: false,
        stroke_tolerance: false,
        flowdog_etf_radius: default_flowdog_etf_radius(),
        flowdog_etf_iterations: default_flowdog_etf_iterations(),
        flowdog_sigma_c: default_flowdog_sigma_c(),
        flowdog_sigma_s: default_flowdog_sigma_s(),
        flowdog_sigma_m: default_flowdog_sigma_m(),
        flowdog_rho: default_flowdog_rho(),
        character_set: paper_character_set(font_bytes)?,
    })
}

pub fn color_illustration_preset(font_bytes: &[u8]) -> Result<AsciiConfig, AaError> {
    let mut config = paper_preset(font_bytes)?;
    config.max_input_width = 720;
    config.edge_threshold = 0.2;
    config.binary_threshold = 0.56;
    config.match_weight = 1.05;
    config.score_cutoff = -2.0;
    config.color_edge_boost = true;
    config.stroke_tolerance = true;
    Ok(config)
}

pub fn paper_character_set(font_bytes: &[u8]) -> Result<String, AaError> {
    let face = Face::parse(font_bytes, 0).map_err(|err| AaError::Font(err.to_string()))?;
    let mut seen = HashSet::new();
    let mut chars = Vec::with_capacity(PAPER_CHARACTER_TARGET);

    for ch in [' ', '\u{3000}'] {
        if seen.insert(ch) && face.glyph_index(ch).is_some() {
            chars.push(ch);
        }
    }

    for &(start, end) in PAPER_CHARACTER_RANGES {
        for codepoint in start..=end {
            let Some(ch) = char::from_u32(codepoint) else {
                continue;
            };
            if seen.insert(ch) && face.glyph_index(ch).is_some() {
                chars.push(ch);
                if chars.len() == PAPER_CHARACTER_TARGET {
                    return Ok(chars.into_iter().collect());
                }
            }
        }
    }

    Ok(chars.into_iter().collect())
}

const PAPER_CHARACTER_RANGES: &[(u32, u32)] = &[
    (0x0020, 0x007E),
    (0x00A0, 0x00FF),
    (0x2010, 0x205E),
    (0x2190, 0x21FF),
    (0x2500, 0x257F),
    (0x2580, 0x259F),
    (0x25A0, 0x25FF),
    (0x2600, 0x26FF),
    (0x3000, 0x303F),
    (0x3040, 0x309F),
    (0x30A0, 0x30FF),
    (0x31F0, 0x31FF),
    (0xFF00, 0xFFEF),
];

pub fn convert_path(
    image_path: impl AsRef<Path>,
    font_path: impl AsRef<Path>,
    config: &AsciiConfig,
) -> Result<AsciiResult, AaError> {
    let image = image::open(image_path)?;
    let font_bytes = std::fs::read(font_path)?;
    convert_image(&image, &font_bytes, config)
}

pub fn convert_image(
    image: &DynamicImage,
    font_bytes: &[u8],
    config: &AsciiConfig,
) -> Result<AsciiResult, AaError> {
    let total_start = Instant::now();
    let mut timings = PipelineTimings::default();
    let input_size = (image.width(), image.height());

    let started = Instant::now();
    let line_image = preprocess_image(image, config);
    timings.preprocess = started.elapsed();

    let foreground_pixels = line_image.foreground_count();

    let started = Instant::now();
    let features = extract_features(&line_image, config.gaussian_sigma);
    timings.feature_extraction = started.elapsed();

    let font = Font::from_bytes(font_bytes, FontSettings::default())
        .map_err(|err| AaError::Font(err.to_owned()))?;

    let started = Instant::now();
    let glyphs = build_glyphs(&font, config)?;
    timings.glyph_analysis = started.elapsed();

    let stripe_count = line_image.height.div_ceil(config.stripe_stride_px) as usize;

    let started = Instant::now();
    let stripe_scores: Vec<StripeScore> = (0..stripe_count)
        .into_par_iter()
        .map(|stripe| score_stripe(&features, &glyphs, stripe as u32, config))
        .collect();
    timings.scoring = started.elapsed();

    let started = Instant::now();
    let stripe_results: Vec<Vec<PlacedGlyph>> = stripe_scores
        .par_iter()
        .enumerate()
        .map(|(stripe, scores)| place_stripe(scores, &glyphs, stripe as u32, config))
        .collect();
    let mut placements: Vec<PlacedGlyph> = stripe_results.into_iter().flatten().collect();
    placements.sort_by_key(|p| (p.y, p.x));
    let text = build_text(
        &placements,
        &glyphs,
        line_image.width,
        config.stripe_stride_px,
    );
    timings.placement = started.elapsed();

    let started = Instant::now();
    let line_preview = render_line_preview(&line_image);
    let orientation_preview = render_orientation_preview(&features);
    let ascii_preview = render_ascii_preview(
        line_image.width,
        stripe_count as u32 * config.stripe_stride_px,
        &placements,
        &glyphs,
    );
    timings.rendering = started.elapsed();
    timings.total = total_start.elapsed();

    Ok(AsciiResult {
        text,
        width: line_image.width,
        height: stripe_count as u32 * config.stripe_stride_px,
        line_preview,
        orientation_preview,
        ascii_preview,
        placements,
        timings,
        stats: PipelineStats {
            input_size,
            working_size: (line_image.width, line_image.height),
            stripes: stripe_count,
            glyphs: glyphs.len(),
            placed_glyphs: 0,
            foreground_pixels,
        },
    }
    .with_placed_count())
}

impl AsciiResult {
    fn with_placed_count(mut self) -> Self {
        self.stats.placed_glyphs = self
            .placements
            .iter()
            .filter(|placement| !placement.ch.is_whitespace())
            .count();
        self
    }
}

pub fn save_ascii_png(result: &AsciiResult, path: impl AsRef<Path>) -> Result<(), AaError> {
    result.ascii_preview.save(path)?;
    Ok(())
}

pub fn save_ascii_text(result: &AsciiResult, path: impl AsRef<Path>) -> Result<(), AaError> {
    std::fs::write(path, &result.text)?;
    Ok(())
}

pub fn save_stage_bundle(result: &AsciiResult, directory: impl AsRef<Path>) -> Result<(), AaError> {
    let directory = directory.as_ref();
    std::fs::create_dir_all(directory)?;
    result
        .line_preview
        .save(directory.join("01-structure-lines.png"))?;
    result
        .orientation_preview
        .save(directory.join("02-orientation-map.png"))?;
    result
        .ascii_preview
        .save(directory.join("03-ascii-render.png"))?;
    std::fs::write(directory.join("04-ascii.txt"), &result.text)?;
    std::fs::write(directory.join("metrics.txt"), result_metrics(result))?;
    Ok(())
}

pub fn result_metrics(result: &AsciiResult) -> String {
    format!(
        "input_size: {}x{}\nworking_size: {}x{}\noutput_size: {}x{}\nstripes: {}\nglyphs: {}\nplaced_glyphs: {}\nforeground_pixels: {}\npreprocess_ms: {:.3}\nfeature_ms: {:.3}\nglyph_analysis_ms: {:.3}\nscoring_ms: {:.3}\nplacement_ms: {:.3}\nrendering_ms: {:.3}\ntotal_ms: {:.3}\n",
        result.stats.input_size.0,
        result.stats.input_size.1,
        result.stats.working_size.0,
        result.stats.working_size.1,
        result.width,
        result.height,
        result.stats.stripes,
        result.stats.glyphs,
        result.stats.placed_glyphs,
        result.stats.foreground_pixels,
        result.timings.preprocess.as_secs_f64() * 1000.0,
        result.timings.feature_extraction.as_secs_f64() * 1000.0,
        result.timings.glyph_analysis.as_secs_f64() * 1000.0,
        result.timings.scoring.as_secs_f64() * 1000.0,
        result.timings.placement.as_secs_f64() * 1000.0,
        result.timings.rendering.as_secs_f64() * 1000.0,
        result.timings.total.as_secs_f64() * 1000.0,
    )
}

fn preprocess_image(image: &DynamicImage, config: &AsciiConfig) -> InkImage {
    let resized = resize_to_working_size(image, config.max_input_width);
    let gray = resized.to_luma8();
    match config.input_mode {
        InputMode::ExtractStructureLines => {
            let mut edges = match config.structure_line_mode {
                StructureLineMode::FlowDog => extract_structure_edges(&gray, config),
                StructureLineMode::ScharrMagnitude => {
                    extract_scharr_edges(&gray, config.edge_threshold)
                }
            };
            if config.color_edge_boost && colorfulness(&resized) > 0.035 {
                let color_edges =
                    extract_color_structure_edges(&resized, config.edge_threshold * 0.9);
                edges = merge_ink_images(&edges, &color_edges);
            }
            let cleaned = pre_thin_denoise(&edges);
            thin_image(&cleaned, config.thinning_mode)
        }
        InputMode::TreatAsBinaryLines => {
            let binary = threshold_binary(&gray, config.binary_threshold);
            let cleaned = pre_thin_denoise(&binary);
            thin_image(&cleaned, config.thinning_mode)
        }
    }
}

fn merge_ink_images(left: &InkImage, right: &InkImage) -> InkImage {
    let mut output = InkImage::new(left.width, left.height);
    for y in 0..left.height {
        for x in 0..left.width {
            output.set(
                x,
                y,
                left.get(x as i32, y as i32)
                    .max(right.get(x as i32, y as i32)),
            );
        }
    }
    output
}

fn colorfulness(image: &DynamicImage) -> f32 {
    let rgba = image.to_rgba8();
    let pixel_count = (rgba.width() * rgba.height()).max(1) as f32;
    rgba.pixels()
        .map(|pixel| {
            let r = pixel[0] as f32 / 255.0;
            let g = pixel[1] as f32 / 255.0;
            let b = pixel[2] as f32 / 255.0;
            r.max(g).max(b) - r.min(g).min(b)
        })
        .sum::<f32>()
        / pixel_count
}

fn thin_image(image: &InkImage, mode: ThinningMode) -> InkImage {
    match mode {
        ThinningMode::KmmK3mLookup => k3m_lookup_thinning(image),
        ThinningMode::ZhangSuen => zhang_suen_thinning(image),
    }
}

fn resize_to_working_size(image: &DynamicImage, max_width: u32) -> DynamicImage {
    if image.width() <= max_width {
        return image.clone();
    }
    let ratio = max_width as f32 / image.width() as f32;
    let height = (image.height() as f32 * ratio).round().max(1.0) as u32;
    image.resize(max_width, height, FilterType::Lanczos3)
}

fn threshold_binary(gray: &GrayImage, threshold: f32) -> InkImage {
    let mut output = InkImage::new(gray.width(), gray.height());
    for y in 0..gray.height() {
        for x in 0..gray.width() {
            let luma = gray.get_pixel(x, y)[0] as f32 / 255.0;
            if luma < threshold {
                output.set(x, y, 1.0);
            }
        }
    }
    output
}

fn extract_structure_edges(gray: &GrayImage, config: &AsciiConfig) -> InkImage {
    let flow = edge_tangent_flow(
        gray,
        config.flowdog_etf_iterations,
        config.flowdog_etf_radius,
    );
    let dog = flow_based_dog(gray, &flow, config);
    let (width, height) = gray.dimensions();
    let mut strengths = vec![0.0f32; (width * height) as usize];
    let mut max_strength = 0.0f32;

    for y in 0..height {
        for x in 0..width {
            let idx = (y * width + x) as usize;
            let strength = (-dog[idx]).max(0.0);
            strengths[idx] = strength;
            max_strength = max_strength.max(strength);
        }
    }

    let cutoff = max_strength * config.edge_threshold.clamp(0.03, 0.92);
    let mut output = InkImage::new(width, height);
    for y in 0..height {
        for x in 0..width {
            let idx = (y * width + x) as usize;
            if strengths[idx] >= cutoff && dog[idx] < 0.0 {
                output.set(x, y, 1.0);
            }
        }
    }

    output
}

fn extract_scharr_edges(gray: &GrayImage, threshold: f32) -> InkImage {
    let blurred = filter::gaussian_blur_f32(gray, 1.0);
    let (width, height) = blurred.dimensions();
    let mut magnitudes = vec![0.0f32; (width * height) as usize];
    let mut max_magnitude = 0.0f32;

    for y in 0..height {
        for x in 0..width {
            let gx = scharr_x(&blurred, x as i32, y as i32);
            let gy = scharr_y(&blurred, x as i32, y as i32);
            let magnitude = (gx * gx + gy * gy).sqrt();
            magnitudes[(y * width + x) as usize] = magnitude;
            max_magnitude = max_magnitude.max(magnitude);
        }
    }

    let cutoff = max_magnitude * threshold.clamp(0.02, 0.95);
    let mut output = InkImage::new(width, height);
    for y in 0..height {
        for x in 0..width {
            let idx = (y * width + x) as usize;
            if magnitudes[idx] >= cutoff {
                output.set(x, y, 1.0);
            }
        }
    }

    output
}

fn extract_color_structure_edges(image: &DynamicImage, threshold: f32) -> InkImage {
    let rgba = image.to_rgba8();
    let (width, height) = rgba.dimensions();
    let mut strengths = vec![0.0f32; (width * height) as usize];
    let mut max_strength = 0.0f32;

    for y in 0..height {
        for x in 0..width {
            let idx = (y * width + x) as usize;
            let (gx_r, gy_r) = scharr_rgba_channel(&rgba, x as i32, y as i32, 0);
            let (gx_g, gy_g) = scharr_rgba_channel(&rgba, x as i32, y as i32, 1);
            let (gx_b, gy_b) = scharr_rgba_channel(&rgba, x as i32, y as i32, 2);

            let gx_l = 0.2126 * gx_r + 0.7152 * gx_g + 0.0722 * gx_b;
            let gy_l = 0.2126 * gy_r + 0.7152 * gy_g + 0.0722 * gy_b;
            let luma_mag = (gx_l * gx_l + gy_l * gy_l).sqrt();
            let chroma_mag = ((gx_r - gx_g).powi(2)
                + (gx_r - gx_b).powi(2)
                + (gx_g - gx_b).powi(2)
                + (gy_r - gy_g).powi(2)
                + (gy_r - gy_b).powi(2)
                + (gy_g - gy_b).powi(2))
            .sqrt()
                * 0.5;
            let darkness = 1.0 - rgba_luma(&rgba, x, y);
            let strength = luma_mag + 0.65 * chroma_mag + 0.18 * darkness * luma_mag;
            strengths[idx] = strength;
            max_strength = max_strength.max(strength);
        }
    }

    let cutoff = max_strength * threshold.clamp(0.02, 0.95);
    let mut output = InkImage::new(width, height);
    for y in 0..height {
        for x in 0..width {
            let idx = (y * width + x) as usize;
            if strengths[idx] >= cutoff {
                output.set(x, y, 1.0);
            }
        }
    }
    output
}

fn edge_tangent_flow(gray: &GrayImage, iterations: usize, radius: u32) -> Vec<(f32, f32)> {
    let blurred = filter::gaussian_blur_f32(gray, 1.0);
    let (width, height) = blurred.dimensions();
    let mut flow = vec![(1.0f32, 0.0f32); (width * height) as usize];
    let mut magnitude = vec![0.0f32; (width * height) as usize];
    let mut max_magnitude = 0.0f32;

    for y in 0..height {
        for x in 0..width {
            let gx = scharr_x(&blurred, x as i32, y as i32);
            let gy = scharr_y(&blurred, x as i32, y as i32);
            let mag = (gx * gx + gy * gy).sqrt();
            let idx = (y * width + x) as usize;
            magnitude[idx] = mag;
            max_magnitude = max_magnitude.max(mag);

            if mag > 0.0001 {
                let tx = -gy / mag;
                let ty = gx / mag;
                flow[idx] = (tx, ty);
            }
        }
    }

    if max_magnitude > 0.0 {
        for value in &mut magnitude {
            *value /= max_magnitude;
        }
    }

    for _ in 0..iterations {
        flow = smooth_tangent_flow(width, height, &flow, &magnitude, radius);
    }

    flow
}

fn smooth_tangent_flow(
    width: u32,
    height: u32,
    flow: &[(f32, f32)],
    magnitude: &[f32],
    radius: u32,
) -> Vec<(f32, f32)> {
    let mut next = flow.to_vec();
    let radius = radius.clamp(1, 12) as i32;
    let sigma2 = 2.0 * (radius as f32).powi(2);

    for y in 0..height {
        for x in 0..width {
            let idx = (y * width + x) as usize;
            let center = flow[idx];
            let mut sx = 0.0f32;
            let mut sy = 0.0f32;

            for oy in -radius..=radius {
                for ox in -radius..=radius {
                    let nx = (x as i32 + ox).clamp(0, width as i32 - 1) as u32;
                    let ny = (y as i32 + oy).clamp(0, height as i32 - 1) as u32;
                    let nidx = (ny * width + nx) as usize;
                    let neighbor = flow[nidx];
                    let dot = center.0 * neighbor.0 + center.1 * neighbor.1;
                    let sign = if dot < 0.0 { -1.0 } else { 1.0 };
                    let dist2 = (ox * ox + oy * oy) as f32;
                    let spatial = (-dist2 / sigma2).exp();
                    let weight = spatial * dot.abs() * magnitude[nidx].max(0.05);
                    sx += sign * neighbor.0 * weight;
                    sy += sign * neighbor.1 * weight;
                }
            }

            let len = (sx * sx + sy * sy).sqrt();
            if len > 0.0001 {
                next[idx] = (sx / len, sy / len);
            }
        }
    }

    next
}

fn flow_based_dog(gray: &GrayImage, flow: &[(f32, f32)], config: &AsciiConfig) -> Vec<f32> {
    let (width, height) = gray.dimensions();
    let mut dog = vec![0.0f32; (width * height) as usize];
    let tau = config.flowdog_rho.clamp(0.85, 1.05);
    let sigma_c = config.flowdog_sigma_c.max(0.1);
    let sigma_s = config.flowdog_sigma_s.max(sigma_c + 0.1);
    let sigma_m = config.flowdog_sigma_m.max(0.1);
    let radius_c = gaussian_radius(sigma_c);
    let radius_s = gaussian_radius(sigma_s);
    let radius_m = gaussian_radius(sigma_m);

    for y in 0..height {
        for x in 0..width {
            let idx = (y * width + x) as usize;
            let tangent = flow[idx];
            let normal = (-tangent.1, tangent.0);
            let narrow = gaussian_sample_along(gray, x as f32, y as f32, normal, sigma_c, radius_c);
            let wide = gaussian_sample_along(gray, x as f32, y as f32, normal, sigma_s, radius_s);
            dog[idx] = narrow - tau * wide;
        }
    }

    let mut smoothed = dog.clone();
    for y in 0..height {
        for x in 0..width {
            let idx = (y * width + x) as usize;
            let tangent = flow[idx];
            smoothed[idx] = gaussian_sample_values_along(
                &dog, width, height, x as f32, y as f32, tangent, sigma_m, radius_m,
            );
        }
    }

    smoothed
}

fn gaussian_radius(sigma: f32) -> i32 {
    (sigma.max(0.1) * 2.5).ceil().clamp(1.0, 16.0) as i32
}

fn gaussian_sample_along(
    image: &GrayImage,
    x: f32,
    y: f32,
    direction: (f32, f32),
    sigma: f32,
    radius: i32,
) -> f32 {
    let mut total = 0.0f32;
    let mut weight_sum = 0.0f32;
    let sigma2 = 2.0 * sigma * sigma;

    for step in -radius..=radius {
        let distance = step as f32;
        let weight = (-(distance * distance) / sigma2).exp();
        total += gray_value_at(
            image,
            x + direction.0 * distance,
            y + direction.1 * distance,
        ) * weight;
        weight_sum += weight;
    }

    total / weight_sum.max(0.0001)
}

fn gaussian_sample_values_along(
    values: &[f32],
    width: u32,
    height: u32,
    x: f32,
    y: f32,
    direction: (f32, f32),
    sigma: f32,
    radius: i32,
) -> f32 {
    let mut total = 0.0f32;
    let mut weight_sum = 0.0f32;
    let sigma2 = 2.0 * sigma * sigma;

    for step in -radius..=radius {
        let distance = step as f32;
        let weight = (-(distance * distance) / sigma2).exp();
        let sx = (x + direction.0 * distance)
            .round()
            .clamp(0.0, (width - 1) as f32) as u32;
        let sy = (y + direction.1 * distance)
            .round()
            .clamp(0.0, (height - 1) as f32) as u32;
        total += values[(sy * width + sx) as usize] * weight;
        weight_sum += weight;
    }

    total / weight_sum.max(0.0001)
}

fn pre_thin_denoise(input: &InkImage) -> InkImage {
    let mut output = input.clone();
    if input.width < 3 || input.height < 3 {
        return output;
    }

    for y in 1..input.height - 1 {
        for x in 1..input.width - 1 {
            let diagonals = input.get(x as i32 - 1, y as i32 - 1)
                + input.get(x as i32 + 1, y as i32 - 1)
                + input.get(x as i32 + 1, y as i32 + 1)
                + input.get(x as i32 - 1, y as i32 + 1);
            if diagonals < 2.0 {
                output.set(x, y, 0.0);
            } else if diagonals > 2.0 {
                output.set(x, y, 1.0);
            } else {
                output.set(x, y, input.get(x as i32, y as i32));
            }
        }
    }

    output
}

fn k3m_lookup_thinning(input: &InkImage) -> InkImage {
    let mut image = input.clone();
    if image.width < 3 || image.height < 3 {
        return image;
    }

    loop {
        let mut border = Vec::new();
        for y in 1..image.height - 1 {
            for x in 1..image.width - 1 {
                if image.get(x as i32, y as i32) > 0.0 && K3M_A0.contains(&k3m_weight(&image, x, y))
                {
                    border.push((x, y));
                }
            }
        }

        if border.is_empty() {
            break;
        }

        let border_len = border.len();
        let mut remaining = border;
        for phase in [K3M_A1, K3M_A2, K3M_A3, K3M_A4, K3M_A5] {
            let mut survivors = Vec::new();
            for (x, y) in remaining {
                if phase.contains(&k3m_weight(&image, x, y)) {
                    image.set(x, y, 0.0);
                } else {
                    survivors.push((x, y));
                }
            }
            remaining = survivors;
        }

        if remaining.len() == border_len {
            break;
        }
    }

    for y in 1..image.height - 1 {
        for x in 1..image.width - 1 {
            if image.get(x as i32, y as i32) > 0.0 && K3M_A1PIX.contains(&k3m_weight(&image, x, y))
            {
                image.set(x, y, 0.0);
            }
        }
    }

    image
}

fn zhang_suen_thinning(input: &InkImage) -> InkImage {
    let mut image = input.clone();
    if image.width < 3 || image.height < 3 {
        return image;
    }

    let mut changed = true;
    while changed {
        changed = false;
        let mut to_clear = Vec::new();

        for y in 1..image.height - 1 {
            for x in 1..image.width - 1 {
                if image.get(x as i32, y as i32) <= 0.0 {
                    continue;
                }
                let p = eight_neighbors(&image, x as i32, y as i32);
                let bp = p.iter().filter(|v| **v > 0).count();
                let ap = transitions(&p);
                if (2..=6).contains(&bp)
                    && ap == 1
                    && p[0] * p[2] * p[4] == 0
                    && p[2] * p[4] * p[6] == 0
                {
                    to_clear.push((x, y));
                }
            }
        }

        if !to_clear.is_empty() {
            changed = true;
            for (x, y) in to_clear.drain(..) {
                image.set(x, y, 0.0);
            }
        }

        for y in 1..image.height - 1 {
            for x in 1..image.width - 1 {
                if image.get(x as i32, y as i32) <= 0.0 {
                    continue;
                }
                let p = eight_neighbors(&image, x as i32, y as i32);
                let bp = p.iter().filter(|v| **v > 0).count();
                let ap = transitions(&p);
                if (2..=6).contains(&bp)
                    && ap == 1
                    && p[0] * p[2] * p[6] == 0
                    && p[0] * p[4] * p[6] == 0
                {
                    to_clear.push((x, y));
                }
            }
        }

        if !to_clear.is_empty() {
            changed = true;
            for (x, y) in to_clear.drain(..) {
                image.set(x, y, 0.0);
            }
        }
    }

    image
}

fn eight_neighbors(image: &InkImage, x: i32, y: i32) -> [u8; 8] {
    [
        (image.get(x, y - 1) > 0.0) as u8,
        (image.get(x + 1, y - 1) > 0.0) as u8,
        (image.get(x + 1, y) > 0.0) as u8,
        (image.get(x + 1, y + 1) > 0.0) as u8,
        (image.get(x, y + 1) > 0.0) as u8,
        (image.get(x - 1, y + 1) > 0.0) as u8,
        (image.get(x - 1, y) > 0.0) as u8,
        (image.get(x - 1, y - 1) > 0.0) as u8,
    ]
}

fn transitions(neighbors: &[u8; 8]) -> usize {
    let mut count = 0;
    for idx in 0..8 {
        if neighbors[idx] == 0 && neighbors[(idx + 1) % 8] == 1 {
            count += 1;
        }
    }
    count
}

fn k3m_weight(image: &InkImage, x: u32, y: u32) -> u16 {
    const WEIGHTS: [[u16; 3]; 3] = [[32, 64, 128], [16, 0, 1], [8, 4, 2]];
    let mut weight = 0u16;
    for ox in -1..=1 {
        for oy in -1..=1 {
            if image.get(x as i32 + ox, y as i32 + oy) > 0.0 {
                weight += WEIGHTS[(ox + 1) as usize][(oy + 1) as usize];
            }
        }
    }
    weight
}

const K3M_A0: &[u16] = &[
    3, 6, 7, 12, 14, 15, 24, 28, 30, 31, 48, 56, 60, 62, 63, 96, 112, 120, 124, 126, 127, 129, 131,
    135, 143, 159, 191, 192, 193, 195, 199, 207, 223, 224, 225, 227, 231, 239, 240, 241, 243, 247,
    248, 249, 251, 252, 253, 254,
];
const K3M_A1: &[u16] = &[7, 14, 28, 56, 112, 131, 193, 224];
const K3M_A2: &[u16] = &[
    7, 14, 15, 28, 30, 56, 60, 112, 120, 131, 135, 193, 195, 224, 225, 240,
];
const K3M_A3: &[u16] = &[
    7, 14, 15, 28, 30, 31, 56, 60, 62, 112, 120, 124, 131, 135, 143, 193, 195, 199, 224, 225, 227,
    240, 241, 248,
];
const K3M_A4: &[u16] = &[
    7, 14, 15, 28, 30, 31, 56, 60, 62, 63, 112, 120, 124, 126, 131, 135, 143, 159, 193, 195, 199,
    207, 224, 225, 227, 231, 240, 241, 243, 248, 249, 252,
];
const K3M_A5: &[u16] = &[
    7, 14, 15, 28, 30, 31, 56, 60, 62, 63, 112, 120, 124, 126, 131, 135, 143, 159, 191, 193, 195,
    199, 207, 224, 225, 227, 231, 239, 240, 241, 243, 248, 249, 251, 252, 254,
];
const K3M_A1PIX: &[u16] = K3M_A0;

fn extract_features(image: &InkImage, sigma: f32) -> FeatureImage {
    let gray = ink_to_gray(image);
    let blurred = filter::gaussian_blur_f32(&gray, sigma);
    let (width, height) = blurred.dimensions();
    let mut gx_values = vec![0.0f32; (width * height) as usize];
    let mut gy_values = vec![0.0f32; (width * height) as usize];

    for y in 0..height {
        for x in 0..width {
            let idx = (y * width + x) as usize;
            gx_values[idx] = scharr_x(&blurred, x as i32, y as i32);
            gy_values[idx] = scharr_y(&blurred, x as i32, y as i32);
        }
    }

    let mut source_ink = vec![0.0f32; (width * height) as usize];
    let mut value = vec![0.0f32; (width * height) as usize];
    let mut orientation = vec![None; (width * height) as usize];

    for y in 0..height {
        for x in 0..width {
            let idx = (y * width + x) as usize;
            source_ink[idx] = image.get(x as i32, y as i32);
            value[idx] = blurred.get_pixel(x, y)[0] as f32 / 255.0;

            if source_ink[idx] <= 0.0 && value[idx] < 0.05 {
                continue;
            }

            let mut vx = 0.0f32;
            let mut vy = 0.0f32;
            for wy in -2..=2 {
                for wx in -2..=2 {
                    let sx = (x as i32 + wx).clamp(0, width as i32 - 1) as u32;
                    let sy = (y as i32 + wy).clamp(0, height as i32 - 1) as u32;
                    let sidx = (sy * width + sx) as usize;
                    let gx = gx_values[sidx];
                    let gy = gy_values[sidx];
                    vx += gx * gx - gy * gy;
                    vy += 2.0 * gx * gy;
                }
            }

            let theta_prime = 0.5 * vy.atan2(vx);
            let theta = if vx >= 0.0 {
                theta_prime + PI / 2.0
            } else {
                theta_prime
            };
            orientation[idx] = Some(normalize_pi(theta));
        }
    }

    FeatureImage {
        width,
        height,
        source_ink,
        value,
        orientation,
    }
}

fn build_glyphs(font: &Font, config: &AsciiConfig) -> Result<Vec<GlyphImage>, AaError> {
    let mut seen = HashSet::new();
    let mut chars: Vec<char> = config
        .character_set
        .chars()
        .filter(|ch| seen.insert(*ch))
        .collect();

    if !chars.contains(&' ') {
        chars.insert(0, ' ');
    }

    let mut glyphs = Vec::new();
    for ch in chars {
        let (metrics, bitmap) = font.rasterize(ch, config.font_px);
        let advance = metrics.advance_width.ceil().max(1.0) as u32;
        let width = advance.max(metrics.width as u32).max(1);
        let height = config.stripe_stride_px.max(metrics.height as u32).max(1);
        let top = ((height as i32 - metrics.height as i32) / 2).max(0);
        let mut alpha = vec![0.0f32; (width * height) as usize];

        for by in 0..metrics.height as u32 {
            for bx in 0..metrics.width as u32 {
                let target_x = bx.min(width - 1);
                let target_y = (top as u32 + by).min(height - 1);
                let src = bitmap[(by * metrics.width as u32 + bx) as usize] as f32 / 255.0;
                alpha[(target_y * width + target_x) as usize] = src;
            }
        }

        let mut ink = InkImage::new(width, height);
        let mut foreground = Vec::new();
        for y in 0..height {
            for x in 0..width {
                let a = alpha[(y * width + x) as usize];
                if a >= config.glyph_alpha_threshold {
                    ink.set(x, y, a);
                    foreground.push((x, y));
                }
            }
        }

        let feature = extract_features(&ink, config.gaussian_sigma);
        glyphs.push(GlyphImage {
            ch,
            advance,
            width,
            height,
            alpha,
            foreground,
            feature,
            is_blank: ch.is_whitespace(),
        });
    }

    if glyphs.iter().all(|glyph| glyph.is_blank) {
        return Err(AaError::EmptyCharacterSet);
    }

    glyphs.sort_by_key(|glyph| (glyph.is_blank, glyph.advance, glyph.ch));
    Ok(glyphs)
}

fn score_stripe(
    features: &FeatureImage,
    glyphs: &[GlyphImage],
    stripe: u32,
    config: &AsciiConfig,
) -> StripeScore {
    let stripe_y = stripe * config.stripe_stride_px;
    let width = features.width;
    let candidates: Vec<Candidate> = (0..width)
        .into_par_iter()
        .flat_map_iter(|x| {
            glyphs
                .iter()
                .enumerate()
                .filter_map(move |(glyph_index, glyph)| {
                    if x + glyph.advance > width {
                        return None;
                    }

                    let score = if glyph.is_blank {
                        0.0
                    } else {
                        score_glyph_at(features, glyph, x, stripe_y, config)
                    };

                    Some(Candidate {
                        x,
                        glyph_index,
                        score,
                    })
                })
        })
        .collect();

    StripeScore { width, candidates }
}

fn score_glyph_at(
    features: &FeatureImage,
    glyph: &GlyphImage,
    x: u32,
    stripe_y: u32,
    config: &AsciiConfig,
) -> f32 {
    let mut match_score = 0.0f32;
    let mut mismatch_score = 0.0f32;

    for &(gx, gy) in &glyph.foreground {
        let ix = x as i32 + gx as i32;
        let iy = stripe_y as i32 + gy as i32;
        let (source_support, support_orientation) = if config.stroke_tolerance {
            source_support_at(features, ix, iy)
        } else {
            (features.source_at(ix, iy), None)
        };
        let input_value = if config.stroke_tolerance {
            features.value_at(ix, iy).max(source_support * 0.65)
        } else {
            features.value_at(ix, iy)
        };
        let input_foreground = source_support > 0.0;
        let glyph_value = glyph.feature.value_at(gx as i32, gy as i32);
        let glyph_orientation = glyph.feature.orientation_at(gx as i32, gy as i32);
        let input_orientation = features.orientation_at(ix, iy).or(support_orientation);

        if input_foreground {
            let proximity = source_support.clamp(0.0, 1.0);
            let mismatch_scale = if config.stroke_tolerance {
                1.0 - 0.35 * proximity
            } else {
                1.0
            };
            mismatch_score += (1.0 - input_value) * mismatch_scale;
            if let (Some(glyph_theta), Some(input_theta)) = (glyph_orientation, input_orientation) {
                let delta = angle_delta(glyph_theta, input_theta);
                let pixel_affinity = 1.0 - (input_value - glyph_value).abs();
                let orientation_scale = if config.stroke_tolerance {
                    proximity
                } else {
                    1.0
                };
                match_score += pixel_affinity.max(0.0) * (delta.cos() + 1.0) * orientation_scale;
                mismatch_score += delta.sin() * orientation_scale;
            }
        } else {
            mismatch_score += 1.0;
        }
    }

    config.mismatch_weight * mismatch_score - config.match_weight * match_score
}

fn source_support_at(features: &FeatureImage, x: i32, y: i32) -> (f32, Option<f32>) {
    const OFFSETS: &[(i32, i32, f32)] = &[
        (0, 0, 1.0),
        (-1, 0, 0.68),
        (1, 0, 0.68),
        (0, -1, 0.68),
        (0, 1, 0.68),
        (-1, -1, 0.48),
        (1, -1, 0.48),
        (-1, 1, 0.48),
        (1, 1, 0.48),
    ];

    let mut best_support = 0.0f32;
    let mut best_orientation = None;
    for &(ox, oy, weight) in OFFSETS {
        let source = features.source_at(x + ox, y + oy);
        let support = source * weight;
        if support > best_support {
            best_support = support;
            best_orientation = features.orientation_at(x + ox, y + oy);
        }
    }
    (best_support, best_orientation)
}

fn place_stripe(
    scores: &StripeScore,
    glyphs: &[GlyphImage],
    stripe: u32,
    config: &AsciiConfig,
) -> Vec<PlacedGlyph> {
    match config.placement_mode {
        PlacementMode::PaperGreedy => {
            let sorted_candidates = sorted_candidate_indices(scores, glyphs);
            let blank_glyphs = sorted_blank_glyphs(glyphs);
            let mut failed = HashSet::new();
            gen_paper_segment(
                0,
                scores.width,
                true,
                stripe * config.stripe_stride_px,
                scores,
                glyphs,
                config,
                &blank_glyphs,
                &sorted_candidates,
                &mut failed,
            )
            .unwrap_or_else(|| place_left_to_right(scores, glyphs, stripe, config))
        }
        PlacementMode::LeftToRight => place_left_to_right(scores, glyphs, stripe, config),
    }
}

fn gen_paper_segment(
    start: u32,
    end: u32,
    boundary_right: bool,
    y: u32,
    scores: &StripeScore,
    glyphs: &[GlyphImage],
    config: &AsciiConfig,
    blank_glyphs: &[usize],
    sorted_candidates: &[usize],
    failed: &mut HashSet<(u32, u32, bool)>,
) -> Option<Vec<PlacedGlyph>> {
    if start >= end {
        return Some(Vec::new());
    }

    let key = (start, end, boundary_right);
    if failed.contains(&key) {
        return None;
    }

    for &candidate_index in sorted_candidates {
        let candidate = &scores.candidates[candidate_index];
        let glyph = &glyphs[candidate.glyph_index];
        let glyph_end = candidate.x + glyph.advance;

        if candidate.x < start || candidate.x >= end || glyph_end > end {
            continue;
        }

        if candidate.score >= config.score_cutoff {
            break;
        }

        let Some(mut left) = gen_paper_segment(
            start,
            candidate.x,
            false,
            y,
            scores,
            glyphs,
            config,
            blank_glyphs,
            sorted_candidates,
            failed,
        ) else {
            continue;
        };

        let right = gen_paper_segment(
            glyph_end,
            end,
            boundary_right,
            y,
            scores,
            glyphs,
            config,
            blank_glyphs,
            sorted_candidates,
            failed,
        );

        if right.is_none() && !boundary_right {
            continue;
        }

        left.push(PlacedGlyph {
            ch: glyph.ch,
            x: candidate.x,
            y,
            width: glyph.advance,
            height: glyph.height,
        });

        if let Some(mut right) = right {
            left.append(&mut right);
        }

        return Some(left);
    }

    let blank = fill_blank_segment(start, end, boundary_right, y, glyphs, blank_glyphs);
    if blank.is_none() {
        failed.insert(key);
    }
    blank
}

fn sorted_candidate_indices(scores: &StripeScore, glyphs: &[GlyphImage]) -> Vec<usize> {
    let mut indices: Vec<usize> = scores
        .candidates
        .iter()
        .enumerate()
        .filter_map(|(index, candidate)| (!glyphs[candidate.glyph_index].is_blank).then_some(index))
        .collect();
    indices.sort_by(|a, b| {
        scores.candidates[*a]
            .score
            .total_cmp(&scores.candidates[*b].score)
    });
    indices
}

fn sorted_blank_glyphs(glyphs: &[GlyphImage]) -> Vec<usize> {
    let mut blanks: Vec<usize> = glyphs
        .iter()
        .enumerate()
        .filter_map(|(index, glyph)| glyph.is_blank.then_some(index))
        .collect();
    blanks.sort_by_key(|index| std::cmp::Reverse(glyphs[*index].advance));
    blanks
}

fn fill_blank_segment(
    start: u32,
    end: u32,
    boundary_right: bool,
    y: u32,
    glyphs: &[GlyphImage],
    blank_glyphs: &[usize],
) -> Option<Vec<PlacedGlyph>> {
    if start >= end {
        return Some(Vec::new());
    }

    if blank_glyphs.is_empty() {
        return boundary_right.then(Vec::new);
    }

    let width = (end - start) as usize;
    let mut previous: Vec<Option<(usize, usize)>> = vec![None; width + 1];
    previous[0] = Some((0, usize::MAX));

    for pos in 0..=width {
        if previous[pos].is_none() {
            continue;
        }

        for &glyph_index in blank_glyphs {
            let advance = glyphs[glyph_index].advance as usize;
            if advance == 0 || pos + advance > width {
                continue;
            }

            if previous[pos + advance].is_none() {
                previous[pos + advance] = Some((pos, glyph_index));
            }
        }
    }

    let target = if previous[width].is_some() {
        width
    } else if boundary_right {
        (0..=width)
            .rev()
            .find(|pos| previous[*pos].is_some())
            .unwrap_or(0)
    } else {
        return None;
    };

    let mut placements = Vec::new();
    let mut pos = target;
    while pos > 0 {
        let Some((prev, glyph_index)) = previous[pos] else {
            break;
        };
        let glyph = &glyphs[glyph_index];
        placements.push(PlacedGlyph {
            ch: glyph.ch,
            x: start + prev as u32,
            y,
            width: glyph.advance,
            height: glyph.height,
        });
        pos = prev;
    }

    placements.reverse();
    Some(placements)
}

fn place_left_to_right(
    scores: &StripeScore,
    glyphs: &[GlyphImage],
    stripe: u32,
    config: &AsciiConfig,
) -> Vec<PlacedGlyph> {
    let mut output = Vec::new();
    let y = stripe * config.stripe_stride_px;
    let mut x = 0;
    while x < scores.width {
        let best = scores
            .candidates
            .iter()
            .filter(|candidate| {
                candidate.x == x
                    && !glyphs[candidate.glyph_index].is_blank
                    && candidate.x + glyphs[candidate.glyph_index].advance <= scores.width
            })
            .min_by(|a, b| a.score.total_cmp(&b.score));

        let Some(best) = best else {
            x += 1;
            continue;
        };

        let glyph = &glyphs[best.glyph_index];
        if best.score < config.score_cutoff {
            output.push(PlacedGlyph {
                ch: glyph.ch,
                x,
                y,
                width: glyph.advance,
                height: glyph.height,
            });
        }
        x += glyph.advance.max(1);
    }
    output
}

fn build_text(
    placements: &[PlacedGlyph],
    glyphs: &[GlyphImage],
    width: u32,
    stride: u32,
) -> String {
    let space_advance = glyphs
        .iter()
        .find(|glyph| glyph.ch == ' ')
        .map(|glyph| glyph.advance)
        .unwrap_or(4)
        .max(1);
    let rows = placements
        .iter()
        .map(|placement| placement.y / stride)
        .max()
        .unwrap_or(0)
        + 1;

    let mut lines = Vec::with_capacity(rows as usize);
    for row in 0..rows {
        let mut row_glyphs: Vec<&PlacedGlyph> = placements
            .iter()
            .filter(|placement| placement.y / stride == row)
            .collect();
        row_glyphs.sort_by_key(|placement| placement.x);

        let mut cursor = 0;
        let mut line = String::new();
        for placement in row_glyphs {
            let gap = placement.x.saturating_sub(cursor);
            let spaces = (gap as f32 / space_advance as f32).round() as usize;
            line.extend(std::iter::repeat_n(' ', spaces));
            line.push(placement.ch);
            cursor = placement.x + placement.width;
        }

        let trailing = width.saturating_sub(cursor);
        let spaces = (trailing as f32 / space_advance as f32).round() as usize;
        line.extend(std::iter::repeat_n(' ', spaces));
        lines.push(line.trim_end().to_owned());
    }

    lines.join("\n")
}

fn render_line_preview(image: &InkImage) -> RgbaImage {
    let mut output = RgbaImage::from_pixel(image.width, image.height, Rgba([250, 250, 246, 255]));
    for y in 0..image.height {
        for x in 0..image.width {
            let value = image.get(x as i32, y as i32);
            if value > 0.0 {
                output.put_pixel(x, y, Rgba([14, 14, 14, 255]));
            }
        }
    }
    output
}

fn render_orientation_preview(features: &FeatureImage) -> RgbaImage {
    let mut output =
        RgbaImage::from_pixel(features.width, features.height, Rgba([250, 250, 246, 255]));
    for y in 0..features.height {
        for x in 0..features.width {
            let idx = features.idx(x, y);
            let Some(theta) = features.orientation[idx] else {
                continue;
            };

            let strength = features.value[idx].clamp(0.0, 1.0);
            if strength < 0.03 {
                continue;
            }

            let hue = theta / PI;
            let color = hue_to_rgb(hue);
            let mix = strength.sqrt().clamp(0.0, 1.0);
            output.put_pixel(
                x,
                y,
                Rgba([
                    lerp_u8(250, color[0], mix),
                    lerp_u8(250, color[1], mix),
                    lerp_u8(246, color[2], mix),
                    255,
                ]),
            );
        }
    }
    output
}

fn render_ascii_preview(
    width: u32,
    height: u32,
    placements: &[PlacedGlyph],
    glyphs: &[GlyphImage],
) -> RgbaImage {
    let mut output = RgbaImage::from_pixel(width, height, Rgba([250, 250, 246, 255]));

    for placement in placements {
        let Some(glyph) = glyphs.iter().find(|glyph| glyph.ch == placement.ch) else {
            continue;
        };
        for y in 0..glyph.height {
            for x in 0..glyph.width {
                let alpha = glyph.alpha_at(x, y);
                if alpha <= 0.01 {
                    continue;
                }
                let tx = placement.x + x;
                let ty = placement.y + y;
                if tx >= width || ty >= height {
                    continue;
                }
                let shade = (250.0 * (1.0 - alpha)).round().clamp(0.0, 250.0) as u8;
                output.put_pixel(tx, ty, Rgba([shade, shade, shade, 255]));
            }
        }
    }

    output
}

fn ink_to_gray(image: &InkImage) -> GrayImage {
    let mut gray: GrayImage = ImageBuffer::from_pixel(image.width, image.height, Luma([0]));
    for y in 0..image.height {
        for x in 0..image.width {
            let value = (image.get(x as i32, y as i32) * 255.0).round() as u8;
            gray.put_pixel(x, y, Luma([value]));
        }
    }
    gray
}

fn gray_value_at(image: &GrayImage, x: f32, y: f32) -> f32 {
    let sx = x.round().clamp(0.0, (image.width() - 1) as f32) as u32;
    let sy = y.round().clamp(0.0, (image.height() - 1) as f32) as u32;
    image.get_pixel(sx, sy)[0] as f32 / 255.0
}

fn hue_to_rgb(hue: f32) -> [u8; 3] {
    let hue = hue.rem_euclid(1.0) * 6.0;
    let sector = hue.floor();
    let f = hue - sector;
    let q = 1.0 - f;
    let (r, g, b) = match sector as u32 {
        0 => (1.0, f, 0.0),
        1 => (q, 1.0, 0.0),
        2 => (0.0, 1.0, f),
        3 => (0.0, q, 1.0),
        4 => (f, 0.0, 1.0),
        _ => (1.0, 0.0, q),
    };

    [
        (r * 220.0).round() as u8,
        (g * 220.0).round() as u8,
        (b * 220.0).round() as u8,
    ]
}

fn lerp_u8(a: u8, b: u8, t: f32) -> u8 {
    (a as f32 + (b as f32 - a as f32) * t)
        .round()
        .clamp(0.0, 255.0) as u8
}

fn scharr_rgba_channel(image: &RgbaImage, x: i32, y: i32, channel: usize) -> (f32, f32) {
    let kernel_x = [[3.0, 0.0, -3.0], [10.0, 0.0, -10.0], [3.0, 0.0, -3.0]];
    let kernel_y = [[3.0, 10.0, 3.0], [0.0, 0.0, 0.0], [-3.0, -10.0, -3.0]];
    let width = image.width() as i32;
    let height = image.height() as i32;
    let mut gx = 0.0f32;
    let mut gy = 0.0f32;

    for ky in 0..3 {
        for kx in 0..3 {
            let sx = (x + kx - 1).clamp(0, width - 1) as u32;
            let sy = (y + ky - 1).clamp(0, height - 1) as u32;
            let value = image.get_pixel(sx, sy)[channel] as f32 / 255.0;
            gx += value * kernel_x[ky as usize][kx as usize];
            gy += value * kernel_y[ky as usize][kx as usize];
        }
    }

    (gx, gy)
}

fn rgba_luma(image: &RgbaImage, x: u32, y: u32) -> f32 {
    let pixel = image.get_pixel(x, y);
    let r = pixel[0] as f32 / 255.0;
    let g = pixel[1] as f32 / 255.0;
    let b = pixel[2] as f32 / 255.0;
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

fn scharr_x(image: &GrayImage, x: i32, y: i32) -> f32 {
    let kernel = [[3.0, 0.0, -3.0], [10.0, 0.0, -10.0], [3.0, 0.0, -3.0]];
    convolve3(image, x, y, &kernel)
}

fn scharr_y(image: &GrayImage, x: i32, y: i32) -> f32 {
    let kernel = [[3.0, 10.0, 3.0], [0.0, 0.0, 0.0], [-3.0, -10.0, -3.0]];
    convolve3(image, x, y, &kernel)
}

fn convolve3(image: &GrayImage, x: i32, y: i32, kernel: &[[f32; 3]; 3]) -> f32 {
    let width = image.width() as i32;
    let height = image.height() as i32;
    let mut sum = 0.0;

    for ky in 0..3 {
        for kx in 0..3 {
            let sx = (x + kx - 1).clamp(0, width - 1) as u32;
            let sy = (y + ky - 1).clamp(0, height - 1) as u32;
            let value = image.get_pixel(sx, sy)[0] as f32 / 255.0;
            sum += value * kernel[ky as usize][kx as usize];
        }
    }

    sum
}

fn normalize_pi(angle: f32) -> f32 {
    let mut normalized = angle % PI;
    if normalized < 0.0 {
        normalized += PI;
    }
    normalized
}

fn angle_delta(a: f32, b: f32) -> f32 {
    let delta = (a - b).abs() % PI;
    delta.min(PI - delta)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn angle_delta_treats_opposite_lines_as_equal() {
        assert!(angle_delta(0.0, PI) < 0.0001);
        assert!(angle_delta(0.1, PI + 0.1) < 0.0001);
    }

    #[test]
    fn pre_thinning_removes_lonely_diagonal_noise() {
        let mut input = InkImage::new(3, 3);
        input.set(1, 1, 1.0);
        let output = pre_thin_denoise(&input);
        assert_eq!(output.get(1, 1), 0.0);
    }

    #[test]
    fn pre_thinning_fills_when_diagonal_majority_exists() {
        let mut input = InkImage::new(3, 3);
        input.set(0, 0, 1.0);
        input.set(2, 0, 1.0);
        input.set(0, 2, 1.0);
        let output = pre_thin_denoise(&input);
        assert_eq!(output.get(1, 1), 1.0);
    }

    #[test]
    fn color_structure_edges_detect_color_boundaries() {
        let mut image = RgbaImage::from_pixel(24, 12, Rgba([220, 72, 72, 255]));
        for y in 0..12 {
            for x in 12..24 {
                image.put_pixel(x, y, Rgba([72, 180, 96, 255]));
            }
        }

        let edges = extract_color_structure_edges(&DynamicImage::ImageRgba8(image), 0.2);
        assert!(edges.foreground_count() > 0);
        assert!((0..12).any(|y| edges.get(11, y) > 0.0 || edges.get(12, y) > 0.0));
    }

    #[test]
    fn source_support_matches_nearby_strokes() {
        let mut input = InkImage::new(5, 5);
        input.set(3, 2, 1.0);
        let features = extract_features(&input, 0.7);

        let (center_support, _) = source_support_at(&features, 3, 2);
        let (near_support, _) = source_support_at(&features, 2, 2);

        assert!(center_support > 0.99);
        assert!(near_support > 0.4);
        assert!(near_support < center_support);
    }

    #[test]
    fn converts_simple_binary_cross_when_a_system_font_exists() {
        let Some(font_path) = find_default_font() else {
            return;
        };

        let font_bytes = std::fs::read(font_path).unwrap();
        let mut image = RgbaImage::from_pixel(96, 96, Rgba([255, 255, 255, 255]));
        for i in 12..84 {
            image.put_pixel(i, i, Rgba([0, 0, 0, 255]));
            image.put_pixel(95 - i, i, Rgba([0, 0, 0, 255]));
        }

        let config = AsciiConfig {
            max_input_width: 96,
            font_px: 14.0,
            stripe_stride_px: 16,
            input_mode: InputMode::TreatAsBinaryLines,
            character_set: " /\\|-_".to_owned(),
            score_cutoff: 200.0,
            ..AsciiConfig::default()
        };

        let result = convert_image(&DynamicImage::ImageRgba8(image), &font_bytes, &config).unwrap();
        assert_eq!(result.width, 96);
        assert!(!result.placements.is_empty());
        assert!(!result.text.trim().is_empty());
    }

    #[test]
    fn paper_preset_uses_bundled_saitamaar_when_present() {
        let Some(font_path) = find_paper_font() else {
            return;
        };

        let font_bytes = std::fs::read(font_path).unwrap();
        let config = paper_preset(&font_bytes).unwrap();
        assert_eq!(config.font_px, 16.0);
        assert_eq!(config.placement_mode, PlacementMode::PaperGreedy);
        assert_eq!(config.thinning_mode, ThinningMode::KmmK3mLookup);
        assert_eq!(config.character_set.chars().count(), PAPER_CHARACTER_TARGET);
    }
}
