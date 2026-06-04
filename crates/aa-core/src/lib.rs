use std::collections::{HashSet, VecDeque};
use std::f32::consts::PI;
use std::path::{Path, PathBuf};

use fontdue::{Font, FontSettings};
use image::imageops::FilterType;
use image::{DynamicImage, GrayImage, ImageBuffer, Luma, Rgba, RgbaImage};
use imageproc::filter;
#[cfg(feature = "parallel")]
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use ttf_parser::Face;
use web_time::{Duration, Instant};

#[cfg(feature = "benchmark")]
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
    SoftGrid,
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
    TreatAsSoftLines,
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
    #[serde(default)]
    pub target_edge_density: f32,
    #[serde(default)]
    pub min_component_pixels: u32,
    #[serde(default)]
    pub short_branch_prune_px: u32,
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
            target_edge_density: 0.0,
            min_component_pixels: 0,
            short_branch_prune_px: 0,
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
pub const SOFT_GRID_CHARACTER_SET: &str = " .,:;'`_-~^/|()[]{}<>!7JL1lIrvx╲";
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
        target_edge_density: 0.0,
        min_component_pixels: 0,
        short_branch_prune_px: 0,
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

pub fn anime_sketch_paper_preset(font_bytes: &[u8]) -> Result<AsciiConfig, AaError> {
    let mut config = paper_preset(font_bytes)?;
    config.input_mode = InputMode::TreatAsBinaryLines;
    config.binary_threshold = 0.72;
    config.thinning_mode = ThinningMode::KmmK3mLookup;
    config.placement_mode = PlacementMode::PaperGreedy;
    config.edge_threshold = 0.2;
    config.score_cutoff = 0.0;
    config.stroke_tolerance = false;
    Ok(config)
}

pub fn soft_grid_preset(_font_bytes: &[u8]) -> Result<AsciiConfig, AaError> {
    Ok(AsciiConfig {
        max_input_width: 384,
        font_px: 16.0,
        stripe_stride_px: 16,
        gaussian_sigma: 0.65,
        edge_threshold: 0.2,
        binary_threshold: 0.58,
        mismatch_weight: 0.65,
        match_weight: 1.0,
        score_cutoff: 0.0,
        glyph_alpha_threshold: 0.14,
        input_mode: InputMode::TreatAsSoftLines,
        structure_line_mode: StructureLineMode::FlowDog,
        thinning_mode: ThinningMode::KmmK3mLookup,
        placement_mode: PlacementMode::SoftGrid,
        color_edge_boost: false,
        stroke_tolerance: false,
        target_edge_density: 0.0,
        min_component_pixels: 0,
        short_branch_prune_px: 0,
        flowdog_etf_radius: default_flowdog_etf_radius(),
        flowdog_etf_iterations: default_flowdog_etf_iterations(),
        flowdog_sigma_c: default_flowdog_sigma_c(),
        flowdog_sigma_s: default_flowdog_sigma_s(),
        flowdog_sigma_m: default_flowdog_sigma_m(),
        flowdog_rho: default_flowdog_rho(),
        character_set: SOFT_GRID_CHARACTER_SET.to_owned(),
    })
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

    let mut placements: Vec<PlacedGlyph> = if config.placement_mode == PlacementMode::SoftGrid {
        let started = Instant::now();
        let placements = place_soft_grid(&features, &glyphs, config);
        timings.scoring = started.elapsed();
        placements
    } else {
        let started = Instant::now();
        #[cfg(feature = "parallel")]
        let stripe_scores: Vec<StripeScore> = (0..stripe_count)
            .into_par_iter()
            .map(|stripe| score_stripe(&features, &glyphs, stripe as u32, config))
            .collect();
        #[cfg(not(feature = "parallel"))]
        let stripe_scores: Vec<StripeScore> = (0..stripe_count)
            .map(|stripe| score_stripe(&features, &glyphs, stripe as u32, config))
            .collect();
        timings.scoring = started.elapsed();

        let started = Instant::now();
        #[cfg(feature = "parallel")]
        let stripe_results: Vec<Vec<PlacedGlyph>> = stripe_scores
            .par_iter()
            .enumerate()
            .map(|(stripe, scores)| place_stripe(scores, &glyphs, stripe as u32, config))
            .collect();
        #[cfg(not(feature = "parallel"))]
        let stripe_results: Vec<Vec<PlacedGlyph>> = stripe_scores
            .iter()
            .enumerate()
            .map(|(stripe, scores)| place_stripe(scores, &glyphs, stripe as u32, config))
            .collect();
        timings.placement = started.elapsed();
        stripe_results.into_iter().flatten().collect()
    };
    placements.sort_by_key(|p| (p.y, p.x));
    let text = if config.placement_mode == PlacementMode::SoftGrid {
        build_soft_grid_text(
            &placements,
            line_image.width,
            line_image.height,
            ((config.font_px * 0.5).round() as u32).clamp(4, 24),
            config.stripe_stride_px.max(1),
        )
    } else {
        build_text(
            &placements,
            &glyphs,
            line_image.width,
            config.stripe_stride_px,
        )
    };

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
    let thinned = match config.input_mode {
        InputMode::ExtractStructureLines => {
            let mut edges = match config.structure_line_mode {
                StructureLineMode::FlowDog => extract_structure_edges(&gray, config),
                StructureLineMode::ScharrMagnitude => {
                    extract_scharr_edges(&gray, config.edge_threshold, config.target_edge_density)
                }
            };
            if config.color_edge_boost && colorfulness(&resized) > 0.035 {
                let color_density = if config.target_edge_density > 0.0 {
                    config.target_edge_density * 0.45
                } else {
                    0.0
                };
                let color_edges = extract_color_structure_edges(
                    &resized,
                    config.edge_threshold * 0.9,
                    color_density,
                );
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
        InputMode::TreatAsSoftLines => soft_line_probability(&gray),
    };
    postprocess_line_image(&thinned, config)
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

fn soft_line_probability(gray: &GrayImage) -> InkImage {
    let width = gray.width();
    let height = gray.height();
    let mut values = vec![0.0f32; (width * height) as usize];

    for y in 0..height {
        for x in 0..width {
            let luma = gray.get_pixel(x, y)[0] as f32 / 255.0;
            values[(y * width + x) as usize] = 1.0 - luma;
        }
    }

    let mut probability = bilateral_filter_values(&values, width, height, 2, 0.05, 3.0);
    for value in &mut probability {
        *value = ((*value - 0.035) / 0.42).clamp(0.0, 1.0).powf(0.92);
    }

    let blurred = gaussian_blur_values(&probability, width, height, 0.65);
    let mut output = InkImage::new(width, height);
    for y in 0..height {
        for x in 0..width {
            let idx = (y * width + x) as usize;
            let gx = sobel_x_values(&blurred, width, height, x as i32, y as i32) / 8.0;
            let gy = sobel_y_values(&blurred, width, height, x as i32, y as i32) / 8.0;
            let gradient = (gx * gx + gy * gy).sqrt();
            let edge_boost = (gradient * 3.6).clamp(0.0, 1.0);
            let value = (blurred[idx] * 0.88 + edge_boost * 0.12).clamp(0.0, 1.0);
            if value > 0.015 {
                output.set(x, y, value);
            }
        }
    }

    output
}

fn bilateral_filter_values(
    values: &[f32],
    width: u32,
    height: u32,
    radius: i32,
    sigma_color: f32,
    sigma_space: f32,
) -> Vec<f32> {
    let mut output = vec![0.0f32; values.len()];
    let color_denominator = 2.0 * sigma_color * sigma_color;
    let space_denominator = 2.0 * sigma_space * sigma_space;

    for y in 0..height as i32 {
        for x in 0..width as i32 {
            let center = sample_values(values, width, height, x, y);
            let mut weighted = 0.0;
            let mut weight_sum = 0.0;

            for oy in -radius..=radius {
                for ox in -radius..=radius {
                    let sample = sample_values(values, width, height, x + ox, y + oy);
                    let color_delta = sample - center;
                    let space_distance = (ox * ox + oy * oy) as f32;
                    let color_weight = (-(color_delta * color_delta) / color_denominator).exp();
                    let space_weight = (-space_distance / space_denominator).exp();
                    let weight = color_weight * space_weight;
                    weighted += sample * weight;
                    weight_sum += weight;
                }
            }

            output[(y as u32 * width + x as u32) as usize] = if weight_sum > 0.0 {
                weighted / weight_sum
            } else {
                center
            };
        }
    }

    output
}

fn gaussian_blur_values(values: &[f32], width: u32, height: u32, sigma: f32) -> Vec<f32> {
    if sigma <= 0.0 || width == 0 || height == 0 {
        return values.to_vec();
    }

    let radius = (sigma * 3.0).ceil() as i32;
    let mut kernel = Vec::with_capacity((radius * 2 + 1) as usize);
    let mut kernel_sum = 0.0;
    for i in -radius..=radius {
        let value = (-(i * i) as f32 / (2.0 * sigma * sigma)).exp();
        kernel.push(value);
        kernel_sum += value;
    }
    for value in &mut kernel {
        *value /= kernel_sum;
    }

    let mut horizontal = vec![0.0f32; values.len()];
    for y in 0..height as i32 {
        for x in 0..width as i32 {
            let mut sum = 0.0;
            for (offset, weight) in (-radius..=radius).zip(kernel.iter()) {
                sum += sample_values(values, width, height, x + offset, y) * weight;
            }
            horizontal[(y as u32 * width + x as u32) as usize] = sum;
        }
    }

    let mut output = vec![0.0f32; values.len()];
    for y in 0..height as i32 {
        for x in 0..width as i32 {
            let mut sum = 0.0;
            for (offset, weight) in (-radius..=radius).zip(kernel.iter()) {
                sum += sample_values(&horizontal, width, height, x, y + offset) * weight;
            }
            output[(y as u32 * width + x as u32) as usize] = sum;
        }
    }

    output
}

fn sample_values(values: &[f32], width: u32, height: u32, x: i32, y: i32) -> f32 {
    let sx = x.clamp(0, width.saturating_sub(1) as i32) as u32;
    let sy = y.clamp(0, height.saturating_sub(1) as i32) as u32;
    values[(sy * width + sx) as usize]
}

fn sobel_x_values(values: &[f32], width: u32, height: u32, x: i32, y: i32) -> f32 {
    let kernel = [[-1.0, 0.0, 1.0], [-2.0, 0.0, 2.0], [-1.0, 0.0, 1.0]];
    convolve_values3(values, width, height, x, y, &kernel)
}

fn sobel_y_values(values: &[f32], width: u32, height: u32, x: i32, y: i32) -> f32 {
    let kernel = [[-1.0, -2.0, -1.0], [0.0, 0.0, 0.0], [1.0, 2.0, 1.0]];
    convolve_values3(values, width, height, x, y, &kernel)
}

fn convolve_values3(
    values: &[f32],
    width: u32,
    height: u32,
    x: i32,
    y: i32,
    kernel: &[[f32; 3]; 3],
) -> f32 {
    let mut sum = 0.0;
    for ky in 0..3 {
        for kx in 0..3 {
            let sample = sample_values(values, width, height, x + kx - 1, y + ky - 1);
            sum += sample * kernel[ky as usize][kx as usize];
        }
    }
    sum
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

    let cutoff = strength_cutoff(
        &strengths,
        max_strength,
        config.edge_threshold.clamp(0.03, 0.92),
        config.target_edge_density,
    );
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

fn extract_scharr_edges(gray: &GrayImage, threshold: f32, target_density: f32) -> InkImage {
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

    let cutoff = strength_cutoff(
        &magnitudes,
        max_magnitude,
        threshold.clamp(0.02, 0.95),
        target_density,
    );
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

fn extract_color_structure_edges(
    image: &DynamicImage,
    threshold: f32,
    target_density: f32,
) -> InkImage {
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

    let cutoff = strength_cutoff(
        &strengths,
        max_strength,
        threshold.clamp(0.02, 0.95),
        target_density,
    );
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

fn strength_cutoff(
    strengths: &[f32],
    max_strength: f32,
    relative_threshold: f32,
    target_density: f32,
) -> f32 {
    let target_density = target_density.clamp(0.0, 0.45);
    if target_density <= 0.0 {
        return max_strength * relative_threshold;
    }

    let mut positives: Vec<f32> = strengths
        .iter()
        .copied()
        .filter(|value| *value > 0.0 && value.is_finite())
        .collect();
    if positives.is_empty() {
        return f32::INFINITY;
    }

    positives.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    let target_count = ((strengths.len() as f32) * target_density)
        .round()
        .clamp(1.0, positives.len() as f32) as usize;
    positives[positives.len() - target_count]
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

fn postprocess_line_image(input: &InkImage, config: &AsciiConfig) -> InkImage {
    let mut output = input.clone();
    if config.min_component_pixels > 0 {
        output = remove_small_components(&output, config.min_component_pixels as usize);
    }
    if config.short_branch_prune_px > 0 {
        output = prune_short_branches(&output, config.short_branch_prune_px as usize);
        if config.min_component_pixels > 0 {
            output = remove_small_components(&output, config.min_component_pixels as usize);
        }
    }
    output
}

fn remove_small_components(input: &InkImage, min_pixels: usize) -> InkImage {
    if min_pixels <= 1 || input.width == 0 || input.height == 0 {
        return input.clone();
    }

    let mut output = input.clone();
    let mut visited = vec![false; input.ink.len()];
    let mut queue = VecDeque::new();

    for y in 0..input.height {
        for x in 0..input.width {
            let idx = input.idx(x, y);
            if visited[idx] || input.ink[idx] <= 0.5 {
                continue;
            }

            let mut component = Vec::new();
            visited[idx] = true;
            queue.push_back((x, y));
            while let Some((cx, cy)) = queue.pop_front() {
                component.push((cx, cy));
                for (nx, ny) in foreground_neighbors(input, cx, cy) {
                    let nidx = input.idx(nx, ny);
                    if !visited[nidx] {
                        visited[nidx] = true;
                        queue.push_back((nx, ny));
                    }
                }
            }

            if component.len() < min_pixels {
                for (cx, cy) in component {
                    output.set(cx, cy, 0.0);
                }
            }
        }
    }

    output
}

fn prune_short_branches(input: &InkImage, max_branch_pixels: usize) -> InkImage {
    if max_branch_pixels == 0 || input.width < 3 || input.height < 3 {
        return input.clone();
    }

    let mut remove = vec![false; input.ink.len()];
    let mut endpoints = Vec::new();
    for y in 0..input.height {
        for x in 0..input.width {
            if input.get(x as i32, y as i32) > 0.5 && foreground_degree(input, x, y) <= 1 {
                endpoints.push((x, y));
            }
        }
    }

    for endpoint in endpoints {
        let mut path = vec![endpoint];
        let mut previous = None;
        let mut current = endpoint;
        let mut terminal_degree = foreground_degree(input, current.0, current.1);
        let mut reached_terminal = false;

        while path.len() <= max_branch_pixels {
            let next_candidates: Vec<(u32, u32)> =
                foreground_neighbors(input, current.0, current.1)
                    .into_iter()
                    .filter(|point| Some(*point) != previous)
                    .collect();
            if next_candidates.len() != 1 {
                reached_terminal = true;
                break;
            }

            let next = next_candidates[0];
            previous = Some(current);
            current = next;
            terminal_degree = foreground_degree(input, current.0, current.1);
            path.push(current);
            if terminal_degree != 2 {
                reached_terminal = true;
                break;
            }
        }

        if reached_terminal && path.len() <= max_branch_pixels + 1 {
            let removable_len = if terminal_degree > 2 {
                path.len().saturating_sub(1)
            } else {
                path.len()
            };
            for &(x, y) in path.iter().take(removable_len) {
                remove[input.idx(x, y)] = true;
            }
        }
    }

    let mut output = input.clone();
    for y in 0..input.height {
        for x in 0..input.width {
            if remove[input.idx(x, y)] {
                output.set(x, y, 0.0);
            }
        }
    }
    output
}

fn foreground_neighbors(input: &InkImage, x: u32, y: u32) -> Vec<(u32, u32)> {
    let mut neighbors = Vec::with_capacity(8);
    for oy in -1..=1 {
        for ox in -1..=1 {
            if ox == 0 && oy == 0 {
                continue;
            }
            let nx = x as i32 + ox;
            let ny = y as i32 + oy;
            if input.get(nx, ny) > 0.5 {
                neighbors.push((nx as u32, ny as u32));
            }
        }
    }
    neighbors
}

fn foreground_degree(input: &InkImage, x: u32, y: u32) -> usize {
    foreground_neighbors(input, x, y).len()
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
    #[cfg(feature = "parallel")]
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
    #[cfg(not(feature = "parallel"))]
    let candidates: Vec<Candidate> = (0..width)
        .flat_map(|x| {
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
        PlacementMode::SoftGrid => Vec::new(),
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

#[derive(Debug, Clone)]
struct SoftGridGlyph {
    glyph_index: usize,
    mask: Vec<f32>,
    ink: f32,
    orientation_hist: [f32; 8],
    ports: [f32; 4],
    heavy: bool,
    is_blank: bool,
}

#[derive(Debug, Clone)]
struct SoftGridCellChoice {
    score: f32,
    candidate_index: usize,
}

#[derive(Debug, Clone)]
struct SoftGridPatch {
    values: Vec<f32>,
    sum: f32,
    max: f32,
    orientation_hist: [f32; 8],
    ports: [f32; 4],
}

fn place_soft_grid(
    features: &FeatureImage,
    glyphs: &[GlyphImage],
    config: &AsciiConfig,
) -> Vec<PlacedGlyph> {
    let cell_width = ((config.font_px * 0.5).round() as u32).clamp(4, 24);
    let cell_height = config.stripe_stride_px.max(1);
    let candidates = build_soft_grid_glyphs(glyphs, config, cell_width, cell_height);
    if candidates.is_empty() {
        return Vec::new();
    }

    let rows = features.height.div_ceil(cell_height);
    let cols = features.width.div_ceil(cell_width);
    let mut top_cells = vec![Vec::<SoftGridCellChoice>::new(); (rows * cols) as usize];
    let mut choices = vec![0usize; (rows * cols) as usize];

    for row in 0..rows {
        let y = row * cell_height;
        for col in 0..cols {
            let x = col * cell_width;
            let patch = soft_grid_patch(features, x, y, cell_width, cell_height);
            let mut scored: Vec<SoftGridCellChoice> = candidates
                .iter()
                .enumerate()
                .map(|(candidate_index, candidate)| SoftGridCellChoice {
                    score: soft_grid_score(candidate, &patch, 1.0),
                    candidate_index,
                })
                .collect();
            scored.sort_by(|left, right| {
                right
                    .score
                    .partial_cmp(&left.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            scored.truncate(9);

            let cell_index = (row * cols + col) as usize;
            choices[cell_index] = scored
                .first()
                .map(|choice| choice.candidate_index)
                .unwrap_or(0);
            top_cells[cell_index] = scored;
        }
    }

    for _ in 0..2 {
        for row in 0..rows {
            for col in 0..cols {
                let cell_index = (row * cols + col) as usize;
                let mut best_score = f32::NEG_INFINITY;
                let mut best_candidate = choices[cell_index];

                for option in &top_cells[cell_index] {
                    let candidate = &candidates[option.candidate_index];
                    let mut score = option.score;

                    if col > 0 {
                        let left = &candidates[choices[(row * cols + col - 1) as usize]];
                        score += soft_grid_compat(left, candidate, true) * 0.50;
                    }
                    if col + 1 < cols {
                        let right = &candidates[choices[(row * cols + col + 1) as usize]];
                        score += soft_grid_compat(candidate, right, true) * 0.50;
                    }
                    if row > 0 {
                        let above = &candidates[choices[((row - 1) * cols + col) as usize]];
                        score += soft_grid_compat(above, candidate, false) * 0.30;
                    }
                    if row + 1 < rows {
                        let below = &candidates[choices[((row + 1) * cols + col) as usize]];
                        score += soft_grid_compat(candidate, below, false) * 0.30;
                    }

                    if score > best_score {
                        best_score = score;
                        best_candidate = option.candidate_index;
                    }
                }

                choices[cell_index] = best_candidate;
            }
        }
    }

    let mut output = Vec::new();
    for row in 0..rows {
        for col in 0..cols {
            let candidate = &candidates[choices[(row * cols + col) as usize]];
            if candidate.is_blank {
                continue;
            }
            let glyph = &glyphs[candidate.glyph_index];
            output.push(PlacedGlyph {
                ch: glyph.ch,
                x: col * cell_width,
                y: row * cell_height,
                width: cell_width,
                height: cell_height,
            });
        }
    }

    output
}

fn build_soft_grid_glyphs(
    glyphs: &[GlyphImage],
    config: &AsciiConfig,
    cell_width: u32,
    cell_height: u32,
) -> Vec<SoftGridGlyph> {
    let mut seen = HashSet::new();
    let mut ordered_chars: Vec<char> = config
        .character_set
        .chars()
        .filter(|ch| seen.insert(*ch))
        .collect();
    if !ordered_chars.contains(&' ') {
        ordered_chars.insert(0, ' ');
    }

    ordered_chars
        .into_iter()
        .filter_map(|ch| {
            let glyph_index = glyphs.iter().position(|glyph| glyph.ch == ch)?;
            let glyph = &glyphs[glyph_index];
            let mut mask = vec![0.0f32; (cell_width * cell_height) as usize];
            let x_offset = (cell_width as i32 - glyph.width as i32).max(0) / 2;
            let y_offset = (cell_height as i32 - glyph.height as i32) / 2 - 1;
            let mut ink = 0.0f32;

            for gy in 0..glyph.height.min(cell_height) {
                for gx in 0..glyph.width.min(cell_width) {
                    let alpha = ((glyph.alpha_at(gx, gy) - 0.14) / 0.86).clamp(0.0, 1.0);
                    if alpha <= 0.01 {
                        continue;
                    }
                    let tx = x_offset + gx as i32;
                    let ty = y_offset + gy as i32;
                    if tx < 0 || ty < 0 || tx >= cell_width as i32 || ty >= cell_height as i32 {
                        continue;
                    }
                    let idx = (ty as u32 * cell_width + tx as u32) as usize;
                    mask[idx] = alpha;
                    ink += alpha;
                }
            }

            if ink < 0.2 && !glyph.is_blank {
                return None;
            }

            Some(SoftGridGlyph {
                glyph_index,
                orientation_hist: soft_grid_orientation_hist(&mask, cell_width, cell_height),
                ports: soft_grid_ports(&mask, cell_width, cell_height),
                heavy: ink > 22.0,
                mask,
                ink,
                is_blank: glyph.is_blank,
            })
        })
        .collect()
}

fn soft_grid_patch(
    features: &FeatureImage,
    x: u32,
    y: u32,
    cell_width: u32,
    cell_height: u32,
) -> SoftGridPatch {
    let mut values = vec![0.0f32; (cell_width * cell_height) as usize];
    let mut sum = 0.0f32;
    let mut max = 0.0f32;

    for cy in 0..cell_height {
        for cx in 0..cell_width {
            let idx = (cy * cell_width + cx) as usize;
            let patch = features
                .source_at((x + cx) as i32, (y + cy) as i32)
                .clamp(0.0, 1.0);
            values[idx] = patch;
            sum += patch;
            max = max.max(patch);
        }
    }

    SoftGridPatch {
        orientation_hist: soft_grid_orientation_hist(&values, cell_width, cell_height),
        ports: soft_grid_ports(&values, cell_width, cell_height),
        values,
        sum,
        max,
    }
}

fn soft_grid_score(glyph: &SoftGridGlyph, patch: &SoftGridPatch, strictness: f32) -> f32 {
    let patch_sum = patch.sum;
    let patch_max = patch.max;

    if glyph.is_blank {
        return -patch_sum * 1.55;
    }

    let mut overlap = 0.0f32;
    let mut overdraw = 0.0f32;
    let mut miss = 0.0f32;
    for (ink, patch) in glyph.mask.iter().zip(&patch.values) {
        let ink = ink.clamp(0.0, 1.0);
        let patch = patch.clamp(0.0, 1.0);
        overlap += ink * patch;
        overdraw += ink * (1.0 - patch).powf(1.25);
        miss += (1.0 - (ink * 1.2).clamp(0.0, 1.0)) * patch;
    }

    let orientation = dot8(&glyph.orientation_hist, &patch.orientation_hist);
    let port_delta = glyph
        .ports
        .iter()
        .zip(patch.ports.iter())
        .map(|(left, right)| (left - right).abs())
        .sum::<f32>();

    let mut score = overlap * 4.55
        - overdraw * (1.42 * strictness)
        - miss * 0.20
        - (glyph.ink - patch_sum * 0.94).abs() * 0.24
        + orientation * patch_sum.min(7.0) * 0.82
        - port_delta * 0.42;

    if glyph.heavy && patch_sum < 5.2 {
        score -= 3.4 * strictness;
    }
    if glyph.ink > 18.0 && patch_max < 0.35 {
        score -= 1.5 * strictness;
    }
    if patch_sum < 0.82 || patch_max < 0.19 {
        score -= glyph.ink * 0.35 + 1.4;
    }

    score
}

fn soft_grid_compat(left: &SoftGridGlyph, right: &SoftGridGlyph, horizontal: bool) -> f32 {
    if left.is_blank || right.is_blank {
        return 0.0;
    }

    let (left_port, right_port) = if horizontal {
        (left.ports[1], right.ports[0])
    } else {
        (left.ports[3], right.ports[2])
    };

    left_port.min(right_port) * 1.7 - (left_port - right_port).abs() * 0.5
        + dot8(&left.orientation_hist, &right.orientation_hist) * 0.16
}

fn soft_grid_orientation_hist(values: &[f32], width: u32, height: u32) -> [f32; 8] {
    let mut hist = [0.0f32; 8];
    let mut norm = 0.0;

    for y in 0..height as i32 {
        for x in 0..width as i32 {
            let value = sample_values(values, width, height, x, y);
            let gx = sobel_x_values(values, width, height, x, y) / 8.0;
            let gy = sobel_y_values(values, width, height, x, y) / 8.0;
            let magnitude = (gx * gx + gy * gy).sqrt() * value.max(0.15);
            if magnitude <= 0.0 {
                continue;
            }
            let theta = (gy.atan2(gx) + PI / 2.0).rem_euclid(PI);
            let bin = ((theta / PI * 8.0).floor() as usize).min(7);
            hist[bin] += magnitude;
        }
    }

    for value in hist {
        norm += value * value;
    }
    norm = norm.sqrt();
    if norm > 1e-6 {
        for value in &mut hist {
            *value /= norm;
        }
    }

    hist
}

fn soft_grid_ports(values: &[f32], width: u32, height: u32) -> [f32; 4] {
    [
        soft_grid_region_mean(values, width, height, 0, width.min(2), 0, height),
        soft_grid_region_mean(
            values,
            width,
            height,
            width.saturating_sub(2),
            width,
            0,
            height,
        ),
        soft_grid_region_mean(values, width, height, 0, width, 0, height.min(2)),
        soft_grid_region_mean(
            values,
            width,
            height,
            0,
            width,
            height.saturating_sub(2),
            height,
        ),
    ]
}

fn soft_grid_region_mean(
    values: &[f32],
    width: u32,
    _height: u32,
    x_start: u32,
    x_end: u32,
    y_start: u32,
    y_end: u32,
) -> f32 {
    if x_start >= x_end || y_start >= y_end {
        return 0.0;
    }

    let mut sum = 0.0;
    let mut count = 0;
    for y in y_start..y_end {
        for x in x_start..x_end {
            sum += values[(y * width + x) as usize];
            count += 1;
        }
    }

    if count > 0 { sum / count as f32 } else { 0.0 }
}

fn dot8(left: &[f32; 8], right: &[f32; 8]) -> f32 {
    left.iter()
        .zip(right.iter())
        .map(|(left, right)| left * right)
        .sum()
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

fn build_soft_grid_text(
    placements: &[PlacedGlyph],
    width: u32,
    height: u32,
    cell_width: u32,
    cell_height: u32,
) -> String {
    let rows = height.div_ceil(cell_height) as usize;
    let cols = width.div_ceil(cell_width) as usize;
    let mut grid = vec![vec![' '; cols]; rows];

    for placement in placements {
        let row = (placement.y / cell_height) as usize;
        let col = (placement.x / cell_width) as usize;
        if row < rows && col < cols {
            grid[row][col] = placement.ch;
        }
    }

    grid.into_iter()
        .map(|row| row.into_iter().collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_line_preview(image: &InkImage) -> RgbaImage {
    let mut output = RgbaImage::from_pixel(image.width, image.height, Rgba([250, 250, 246, 255]));
    for y in 0..image.height {
        for x in 0..image.width {
            let value = image.get(x as i32, y as i32);
            if value > 0.0 {
                let shade = (250.0 * (1.0 - value.clamp(0.0, 1.0)))
                    .round()
                    .clamp(14.0, 250.0) as u8;
                output.put_pixel(x, y, Rgba([shade, shade, shade, 255]));
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
        let x_offset = placement.width.saturating_sub(glyph.width) / 2;
        let y_offset = placement.height.saturating_sub(glyph.height) / 2;
        for y in 0..glyph.height.min(placement.height) {
            for x in 0..glyph.width.min(placement.width) {
                let alpha = glyph.alpha_at(x, y);
                if alpha <= 0.01 {
                    continue;
                }
                let tx = placement.x + x_offset + x;
                let ty = placement.y + y_offset + y;
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

        let edges = extract_color_structure_edges(&DynamicImage::ImageRgba8(image), 0.2, 0.0);
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

    #[test]
    fn soft_grid_preset_matches_original_b2_grid_shape() {
        let config = soft_grid_preset(&[]).unwrap();
        assert_eq!(config.max_input_width, 384);
        assert_eq!(config.font_px, 16.0);
        assert_eq!(config.stripe_stride_px, 16);
        assert_eq!(config.input_mode, InputMode::TreatAsSoftLines);
        assert_eq!(config.placement_mode, PlacementMode::SoftGrid);
        assert_eq!(config.character_set, SOFT_GRID_CHARACTER_SET);
        assert!(config.character_set.contains('╲'));
        assert!(!config.character_set.contains('\\'));
        assert!(!config.character_set.contains('@'));
    }

    #[test]
    fn anime_sketch_preset_keeps_paper_style_downstream() {
        let Some(font_path) = find_paper_font() else {
            return;
        };

        let font_bytes = std::fs::read(font_path).unwrap();
        let config = anime_sketch_paper_preset(&font_bytes).unwrap();
        assert_eq!(config.input_mode, InputMode::TreatAsBinaryLines);
        assert_eq!(config.thinning_mode, ThinningMode::KmmK3mLookup);
        assert_eq!(config.placement_mode, PlacementMode::PaperGreedy);
        assert_eq!(config.character_set.chars().count(), PAPER_CHARACTER_TARGET);
    }
}
