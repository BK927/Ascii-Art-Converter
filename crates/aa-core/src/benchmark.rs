use std::collections::HashSet;
use std::f32::consts::PI;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Instant;

use fontdue::{Font, FontSettings};
use image::imageops::FilterType;
use image::{DynamicImage, RgbaImage};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use ttf_parser::Face;

use super::*;

const MATCH_TOLERANCE_PX: f32 = 2.0;

#[derive(Debug, Clone)]
pub struct BenchmarkRunOptions {
    pub font_profile: String,
    pub custom_font: Option<PathBuf>,
    pub custom_font_license: Option<PathBuf>,
    pub algorithms: Vec<BenchmarkAlgorithm>,
}

impl Default for BenchmarkRunOptions {
    fn default() -> Self {
        Self {
            font_profile: "saitamaar-16".to_owned(),
            custom_font: None,
            custom_font_license: None,
            algorithms: BenchmarkAlgorithm::default_suite(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BenchmarkAlgorithm {
    DensityGrid,
    FixedGrid,
    LeftToRight,
    PaperGreedy,
    PaperGreedyKang,
    PaperGreedyKmm,
    PaperGreedyKangKmm,
    PaperGreedyClean,
    PaperGreedyBalanced,
    PaperGreedyPretty,
    PaperGreedyInterval,
    PaperGreedyIntervalClean,
    PaperGreedyIntervalBalanced,
    PaperGreedyPostPrune,
    PaperGreedyLocalPrune,
    OursCurrent,
}

impl BenchmarkAlgorithm {
    pub fn default_suite() -> Vec<Self> {
        vec![Self::LeftToRight, Self::PaperGreedy]
    }

    pub fn full_suite() -> Vec<Self> {
        vec![
            Self::DensityGrid,
            Self::FixedGrid,
            Self::LeftToRight,
            Self::PaperGreedy,
            Self::PaperGreedyKang,
            Self::PaperGreedyKmm,
            Self::PaperGreedyKangKmm,
            Self::PaperGreedyClean,
            Self::PaperGreedyBalanced,
            Self::PaperGreedyPretty,
            Self::PaperGreedyInterval,
            Self::PaperGreedyIntervalClean,
            Self::PaperGreedyIntervalBalanced,
            Self::PaperGreedyPostPrune,
            Self::PaperGreedyLocalPrune,
            Self::OursCurrent,
        ]
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::DensityGrid => "density-grid",
            Self::FixedGrid => "fixed-grid",
            Self::LeftToRight => "left-to-right",
            Self::PaperGreedy => "paper-greedy",
            Self::PaperGreedyKang => "paper-greedy-kang",
            Self::PaperGreedyKmm => "paper-greedy-kmm",
            Self::PaperGreedyKangKmm => "paper-greedy-kang-kmm",
            Self::PaperGreedyClean => "paper-greedy-clean",
            Self::PaperGreedyBalanced => "paper-greedy-balanced",
            Self::PaperGreedyPretty => "paper-greedy-pretty",
            Self::PaperGreedyInterval => "paper-greedy-interval",
            Self::PaperGreedyIntervalClean => "paper-greedy-interval-clean",
            Self::PaperGreedyIntervalBalanced => "paper-greedy-interval-balanced",
            Self::PaperGreedyPostPrune => "paper-greedy-postprune",
            Self::PaperGreedyLocalPrune => "paper-greedy-local-prune",
            Self::OursCurrent => "ours-current",
        }
    }
}

impl FromStr for BenchmarkAlgorithm {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "density-grid" => Ok(Self::DensityGrid),
            "fixed-grid" => Ok(Self::FixedGrid),
            "left-to-right" => Ok(Self::LeftToRight),
            "paper-greedy" => Ok(Self::PaperGreedy),
            "paper-greedy-kang" => Ok(Self::PaperGreedyKang),
            "paper-greedy-kmm" => Ok(Self::PaperGreedyKmm),
            "paper-greedy-kang-kmm" => Ok(Self::PaperGreedyKangKmm),
            "paper-greedy-clean" => Ok(Self::PaperGreedyClean),
            "paper-greedy-balanced" => Ok(Self::PaperGreedyBalanced),
            "paper-greedy-pretty" => Ok(Self::PaperGreedyPretty),
            "paper-greedy-interval" => Ok(Self::PaperGreedyInterval),
            "paper-greedy-interval-clean" => Ok(Self::PaperGreedyIntervalClean),
            "paper-greedy-interval-balanced" => Ok(Self::PaperGreedyIntervalBalanced),
            "paper-greedy-postprune" => Ok(Self::PaperGreedyPostPrune),
            "paper-greedy-local-prune" => Ok(Self::PaperGreedyLocalPrune),
            "ours-current" => Ok(Self::OursCurrent),
            other => Err(format!("unknown benchmark algorithm: {other}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkManifest {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub cases: Vec<BenchmarkCase>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkCase {
    pub id: String,
    pub image: PathBuf,
    pub prompt: String,
    pub provenance: String,
    pub license_status: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub roi: Vec<RoiRect>,
    #[serde(default)]
    pub notes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoiRect {
    pub label: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkReport {
    pub name: String,
    pub description: String,
    pub font_profile: FontProfileReport,
    pub algorithms: Vec<String>,
    pub cases: Vec<BenchmarkCaseReport>,
    pub gallery_pairs: Vec<GalleryPair>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FontProfileReport {
    pub id: String,
    pub font_path: String,
    pub license_path: String,
    pub missing_glyph_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkCaseReport {
    pub id: String,
    pub source_image: String,
    pub tags: Vec<String>,
    pub results: Vec<BenchmarkResultReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkResultReport {
    pub algorithm: String,
    pub output_dir: String,
    pub render_png: String,
    pub text_file: String,
    pub metrics: BenchmarkMetrics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkMetrics {
    pub skeleton_f1: f32,
    pub precision: f32,
    pub recall: f32,
    pub chamfer_distance: f32,
    pub orientation_agreement: f32,
    pub overdraw_ratio: f32,
    pub underdraw_ratio: f32,
    pub roi_weighted_score: f32,
    pub missing_glyph_count: usize,
    pub runtime_ms: f64,
    pub roi: Vec<RoiMetric>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoiMetric {
    pub label: String,
    pub weight: f32,
    pub skeleton_f1: f32,
    pub overdraw_ratio: f32,
    pub underdraw_ratio: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GalleryPair {
    pub id: String,
    pub case_id: String,
    pub original: String,
    pub a_render: String,
    pub b_render: String,
    pub a_algorithm: String,
    pub b_algorithm: String,
}

struct ResolvedFontProfile {
    id: String,
    font_path: PathBuf,
    license_path: PathBuf,
    font_bytes: Vec<u8>,
    config: AsciiConfig,
    missing_glyph_count: usize,
}

pub fn run_benchmark(
    manifest_path: impl AsRef<Path>,
    output_dir: impl AsRef<Path>,
    options: &BenchmarkRunOptions,
) -> Result<BenchmarkReport, AaError> {
    let manifest_path = manifest_path.as_ref();
    let output_dir = output_dir.as_ref();
    let manifest_dir = manifest_path
        .parent()
        .ok_or_else(|| AaError::Benchmark("manifest path has no parent directory".to_owned()))?;
    let manifest_text = std::fs::read_to_string(manifest_path)?;
    let manifest: BenchmarkManifest = serde_json::from_str(&manifest_text)?;
    validate_manifest(&manifest)?;

    let profile = resolve_font_profile(options)?;
    std::fs::create_dir_all(output_dir)?;

    let algorithms = if options.algorithms.is_empty() {
        BenchmarkAlgorithm::default_suite()
    } else {
        options.algorithms.clone()
    };

    let mut case_reports = Vec::new();
    for case in &manifest.cases {
        let image_path = manifest_dir.join(&case.image);
        let image = image::open(&image_path)?;
        let case_dir = output_dir.join("cases").join(safe_id(&case.id));
        std::fs::create_dir_all(&case_dir)?;
        let source_path = case_dir.join("00-original.png");
        image.save(&source_path)?;

        let mut results = Vec::new();
        for algorithm in &algorithms {
            let algorithm_dir = case_dir.join(algorithm.id());
            let result =
                convert_for_algorithm(&image, &profile.font_bytes, &profile.config, *algorithm)?;
            save_stage_bundle(&result, &algorithm_dir)?;
            let mut metrics = evaluate_result(&result, &case.roi);
            metrics.missing_glyph_count = profile.missing_glyph_count;
            metrics.runtime_ms = result.timings.total.as_secs_f64() * 1000.0;

            results.push(BenchmarkResultReport {
                algorithm: algorithm.id().to_owned(),
                output_dir: relative_slash(output_dir, &algorithm_dir),
                render_png: relative_slash(output_dir, &algorithm_dir.join("03-ascii-render.png")),
                text_file: relative_slash(output_dir, &algorithm_dir.join("04-ascii.txt")),
                metrics,
            });
        }

        case_reports.push(BenchmarkCaseReport {
            id: case.id.clone(),
            source_image: relative_slash(output_dir, &source_path),
            tags: case.tags.clone(),
            results,
        });
    }

    let gallery_pairs = build_gallery_pairs(&case_reports);
    let report = BenchmarkReport {
        name: manifest.name,
        description: manifest.description,
        font_profile: FontProfileReport {
            id: profile.id,
            font_path: profile.font_path.display().to_string(),
            license_path: profile.license_path.display().to_string(),
            missing_glyph_count: profile.missing_glyph_count,
        },
        algorithms: algorithms
            .iter()
            .map(|algorithm| algorithm.id().to_owned())
            .collect(),
        cases: case_reports,
        gallery_pairs,
    };

    std::fs::write(
        output_dir.join("report.json"),
        serde_json::to_string_pretty(&report)?,
    )?;
    std::fs::write(output_dir.join("index.html"), render_gallery_html(&report)?)?;
    std::fs::write(
        output_dir.join("overview.html"),
        render_overview_html(&report)?,
    )?;
    Ok(report)
}

fn validate_manifest(manifest: &BenchmarkManifest) -> Result<(), AaError> {
    if manifest.cases.is_empty() {
        return Err(AaError::Benchmark(
            "benchmark manifest must contain at least one case".to_owned(),
        ));
    }

    let mut ids = HashSet::new();
    for case in &manifest.cases {
        if case.id.trim().is_empty() {
            return Err(AaError::Benchmark(
                "benchmark case id is required".to_owned(),
            ));
        }
        if !ids.insert(case.id.clone()) {
            return Err(AaError::Benchmark(format!(
                "duplicate benchmark case id: {}",
                case.id
            )));
        }
        if case.image.as_os_str().is_empty() {
            return Err(AaError::Benchmark(format!(
                "benchmark case {} is missing image",
                case.id
            )));
        }
        if case.prompt.trim().is_empty() {
            return Err(AaError::Benchmark(format!(
                "benchmark case {} is missing prompt",
                case.id
            )));
        }
        if case.provenance.trim().is_empty() {
            return Err(AaError::Benchmark(format!(
                "benchmark case {} is missing provenance",
                case.id
            )));
        }
        if case.license_status.trim().is_empty() {
            return Err(AaError::Benchmark(format!(
                "benchmark case {} is missing license_status",
                case.id
            )));
        }
    }

    Ok(())
}

fn resolve_font_profile(options: &BenchmarkRunOptions) -> Result<ResolvedFontProfile, AaError> {
    match options.font_profile.as_str() {
        "saitamaar-16" => {
            let font_path = find_paper_font().ok_or_else(|| {
                AaError::Benchmark(
                    "saitamaar-16 requires assets/fonts/Saitamaar-Regular.ttf".to_owned(),
                )
            })?;
            let license_path = find_saitamaar_license(&font_path).ok_or_else(|| {
                AaError::Benchmark(
                    "saitamaar-16 requires assets/fonts/Saitamaar-OFL.txt".to_owned(),
                )
            })?;
            let font_bytes = std::fs::read(&font_path)?;
            let mut config = paper_preset(&font_bytes)?;
            config.max_input_width = 512;
            let missing_glyph_count = missing_glyph_count(&font_bytes, &config.character_set)?;
            Ok(ResolvedFontProfile {
                id: "saitamaar-16".to_owned(),
                font_path,
                license_path,
                font_bytes,
                config,
                missing_glyph_count,
            })
        }
        "noto-commercial-16" => {
            let (font_path, license_path) = resolve_commercial_font(
                options.custom_font.as_deref(),
                options.custom_font_license.as_deref(),
                "Noto",
            )?;
            let font_bytes = std::fs::read(&font_path)?;
            let mut config = AsciiConfig {
                max_input_width: 512,
                font_px: 16.0,
                stripe_stride_px: 18,
                ..AsciiConfig::default()
            };
            config.character_set = DEFAULT_CHARACTER_SET.to_owned();
            let missing_glyph_count = missing_glyph_count(&font_bytes, &config.character_set)?;
            Ok(ResolvedFontProfile {
                id: "noto-commercial-16".to_owned(),
                font_path,
                license_path,
                font_bytes,
                config,
                missing_glyph_count,
            })
        }
        "custom" => {
            let font_path = options
                .custom_font
                .clone()
                .ok_or_else(|| AaError::Benchmark("custom profile requires --font".to_owned()))?;
            let license_path = options.custom_font_license.clone().ok_or_else(|| {
                AaError::Benchmark("custom profile requires --font-license".to_owned())
            })?;
            if !font_path.exists() {
                return Err(AaError::Benchmark(format!(
                    "custom font does not exist: {}",
                    font_path.display()
                )));
            }
            if !license_path.exists() {
                return Err(AaError::Benchmark(format!(
                    "custom font license does not exist: {}",
                    license_path.display()
                )));
            }
            let font_bytes = std::fs::read(&font_path)?;
            let config = AsciiConfig {
                max_input_width: 512,
                font_px: 16.0,
                ..AsciiConfig::default()
            };
            let missing_glyph_count = missing_glyph_count(&font_bytes, &config.character_set)?;
            Ok(ResolvedFontProfile {
                id: "custom".to_owned(),
                font_path,
                license_path,
                font_bytes,
                config,
                missing_glyph_count,
            })
        }
        other => Err(AaError::Benchmark(format!(
            "unknown font profile: {other}; expected saitamaar-16, noto-commercial-16, or custom"
        ))),
    }
}

fn resolve_commercial_font(
    custom_font: Option<&Path>,
    custom_license: Option<&Path>,
    name: &str,
) -> Result<(PathBuf, PathBuf), AaError> {
    if let Some(font_path) = custom_font {
        let license_path = custom_license.ok_or_else(|| {
            AaError::Benchmark(format!(
                "{name} commercial profile requires --font-license for official scoring"
            ))
        })?;
        if !font_path.exists() {
            return Err(AaError::Benchmark(format!(
                "font does not exist: {}",
                font_path.display()
            )));
        }
        if !license_path.exists() {
            return Err(AaError::Benchmark(format!(
                "font license does not exist: {}",
                license_path.display()
            )));
        }
        return Ok((font_path.to_path_buf(), license_path.to_path_buf()));
    }

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let candidates = [
        (
            manifest_dir.join("../../assets/fonts/NotoSansMono-Regular.ttf"),
            manifest_dir.join("../../assets/fonts/Noto-OFL.txt"),
        ),
        (
            PathBuf::from("assets/fonts/NotoSansMono-Regular.ttf"),
            PathBuf::from("assets/fonts/Noto-OFL.txt"),
        ),
    ];
    candidates
        .into_iter()
        .find(|(font, license)| font.exists() && license.exists())
        .ok_or_else(|| {
            AaError::Benchmark(
                "noto-commercial-16 requires assets/fonts/NotoSansMono-Regular.ttf and assets/fonts/Noto-OFL.txt, or pass --font and --font-license".to_owned(),
            )
        })
}

fn find_saitamaar_license(font_path: &Path) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(parent) = font_path.parent() {
        candidates.push(parent.join("Saitamaar-OFL.txt"));
        candidates.push(parent.join("OFL.txt"));
    }
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    candidates.push(manifest_dir.join("../../assets/fonts/Saitamaar-OFL.txt"));
    candidates.push(PathBuf::from("assets/fonts/Saitamaar-OFL.txt"));
    candidates
        .into_iter()
        .find(|path| path.exists())
        .and_then(|path| path.canonicalize().ok().or(Some(path)))
}

fn convert_for_algorithm(
    image: &DynamicImage,
    font_bytes: &[u8],
    base_config: &AsciiConfig,
    algorithm: BenchmarkAlgorithm,
) -> Result<AsciiResult, AaError> {
    match algorithm {
        BenchmarkAlgorithm::DensityGrid => {
            convert_grid_baseline(image, font_bytes, base_config, GridBaseline::Density)
        }
        BenchmarkAlgorithm::FixedGrid => {
            convert_grid_baseline(image, font_bytes, base_config, GridBaseline::FixedScore)
        }
        BenchmarkAlgorithm::LeftToRight => {
            let mut config = base_config.clone();
            config.placement_mode = PlacementMode::LeftToRight;
            convert_image(image, font_bytes, &config)
        }
        BenchmarkAlgorithm::PaperGreedy => {
            let mut config = base_config.clone();
            config.placement_mode = PlacementMode::PaperGreedy;
            convert_image(image, font_bytes, &config)
        }
        BenchmarkAlgorithm::PaperGreedyKang => {
            let mut config = base_config.clone();
            config.placement_mode = PlacementMode::PaperGreedy;
            apply_kang_fdog_profile(&mut config);
            convert_image(image, font_bytes, &config)
        }
        BenchmarkAlgorithm::PaperGreedyKmm => {
            let mut config = base_config.clone();
            config.placement_mode = PlacementMode::PaperGreedy;
            apply_paper_kmm_profile(&mut config);
            convert_image(image, font_bytes, &config)
        }
        BenchmarkAlgorithm::PaperGreedyKangKmm => {
            let mut config = base_config.clone();
            config.placement_mode = PlacementMode::PaperGreedy;
            apply_kang_fdog_profile(&mut config);
            apply_paper_kmm_profile(&mut config);
            convert_image(image, font_bytes, &config)
        }
        BenchmarkAlgorithm::PaperGreedyClean => {
            let mut config = base_config.clone();
            config.placement_mode = PlacementMode::PaperGreedy;
            config.character_set = pruned_character_set(font_bytes, &config, GlyphPrune::Clean)?;
            convert_image(image, font_bytes, &config)
        }
        BenchmarkAlgorithm::PaperGreedyBalanced => {
            let mut config = base_config.clone();
            config.placement_mode = PlacementMode::PaperGreedy;
            config.character_set = pruned_character_set(font_bytes, &config, GlyphPrune::Clean)?;
            config.mismatch_weight = (config.mismatch_weight * 1.08).max(0.7);
            config.score_cutoff = config.score_cutoff.min(-0.12);
            convert_image(image, font_bytes, &config)
        }
        BenchmarkAlgorithm::PaperGreedyPretty => {
            let mut config = base_config.clone();
            config.placement_mode = PlacementMode::PaperGreedy;
            config.character_set = pruned_character_set(font_bytes, &config, GlyphPrune::Pretty)?;
            config.mismatch_weight = (config.mismatch_weight * 1.2).max(0.75);
            config.match_weight *= 0.95;
            config.score_cutoff = config.score_cutoff.min(-0.35);
            convert_image(image, font_bytes, &config)
        }
        BenchmarkAlgorithm::PaperGreedyInterval => {
            let mut config = base_config.clone();
            config.placement_mode = PlacementMode::PaperGreedy;
            convert_interval_search(image, font_bytes, &config)
        }
        BenchmarkAlgorithm::PaperGreedyIntervalClean => {
            let mut config = base_config.clone();
            config.placement_mode = PlacementMode::PaperGreedy;
            config.character_set = pruned_character_set(font_bytes, &config, GlyphPrune::Clean)?;
            convert_interval_search(image, font_bytes, &config)
        }
        BenchmarkAlgorithm::PaperGreedyIntervalBalanced => {
            let mut config = base_config.clone();
            config.placement_mode = PlacementMode::PaperGreedy;
            config.character_set = pruned_character_set(font_bytes, &config, GlyphPrune::Clean)?;
            config.mismatch_weight = (config.mismatch_weight * 1.08).max(0.7);
            config.score_cutoff = config.score_cutoff.min(-0.12);
            convert_interval_search(image, font_bytes, &config)
        }
        BenchmarkAlgorithm::PaperGreedyPostPrune => {
            let mut config = base_config.clone();
            config.placement_mode = PlacementMode::PaperGreedy;
            let result = convert_image(image, font_bytes, &config)?;
            post_prune_result(result, font_bytes, &config, PruneMode::Support)
        }
        BenchmarkAlgorithm::PaperGreedyLocalPrune => {
            let mut config = base_config.clone();
            config.placement_mode = PlacementMode::PaperGreedy;
            let result = convert_image(image, font_bytes, &config)?;
            post_prune_result(result, font_bytes, &config, PruneMode::LocalSearch)
        }
        BenchmarkAlgorithm::OursCurrent => {
            let mut config = base_config.clone();
            config.placement_mode = PlacementMode::PaperGreedy;
            convert_image(image, font_bytes, &config)
        }
    }
}

fn apply_kang_fdog_profile(config: &mut AsciiConfig) {
    config.input_mode = InputMode::ExtractStructureLines;
    config.structure_line_mode = StructureLineMode::FlowDog;
    config.flowdog_etf_radius = 5;
    config.flowdog_etf_iterations = 3;
    config.flowdog_sigma_c = 1.0;
    config.flowdog_sigma_s = 1.6;
    config.flowdog_sigma_m = 3.0;
    config.flowdog_rho = 0.99;
    config.edge_threshold = 0.32;
}

fn apply_paper_kmm_profile(config: &mut AsciiConfig) {
    config.font_px = 16.0;
    config.stripe_stride_px = 18;
    config.thinning_mode = ThinningMode::KmmK3mLookup;
    config.gaussian_sigma = 0.7;
}

#[derive(Debug, Clone, Copy)]
enum GlyphPrune {
    Clean,
    Pretty,
}

fn pruned_character_set(
    font_bytes: &[u8],
    config: &AsciiConfig,
    profile: GlyphPrune,
) -> Result<String, AaError> {
    let font = Font::from_bytes(font_bytes, FontSettings::default())
        .map_err(|err| AaError::Font(err.to_owned()))?;
    let glyphs = build_glyphs(&font, config)?;
    let mut chars = String::new();
    let mut seen = HashSet::new();

    for glyph in glyphs {
        if !seen.insert(glyph.ch) {
            continue;
        }

        if glyph.is_blank || glyph_is_useful_for_profile(&glyph, profile) {
            chars.push(glyph.ch);
        }
    }

    if chars.chars().all(char::is_whitespace) {
        return Ok(config.character_set.clone());
    }

    Ok(chars)
}

fn glyph_is_useful_for_profile(glyph: &GlyphImage, profile: GlyphPrune) -> bool {
    let area = (glyph.width * glyph.height).max(1) as f32;
    let hard_density = glyph.foreground.len() as f32 / area;
    let alpha_density = glyph.alpha.iter().sum::<f32>() / area;
    let max_hard_density = match profile {
        GlyphPrune::Clean => 0.34,
        GlyphPrune::Pretty => 0.28,
    };
    let max_alpha_density = match profile {
        GlyphPrune::Clean => 0.26,
        GlyphPrune::Pretty => 0.22,
    };

    hard_density <= max_hard_density && alpha_density <= max_alpha_density
}

fn convert_interval_search(
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
        .map(|(stripe, scores)| place_interval_search(scores, &glyphs, stripe as u32, config))
        .collect();
    let mut placements: Vec<PlacedGlyph> = stripe_results.into_iter().flatten().collect();
    placements.sort_by_key(|placement| (placement.y, placement.x));
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

#[derive(Debug, Clone, Copy)]
struct IntervalPrev {
    previous_x: usize,
    candidate_index: Option<usize>,
}

fn place_interval_search(
    scores: &StripeScore,
    glyphs: &[GlyphImage],
    stripe: u32,
    config: &AsciiConfig,
) -> Vec<PlacedGlyph> {
    const TOP_CANDIDATES_PER_X: usize = 24;

    let width = scores.width as usize;
    let y = stripe * config.stripe_stride_px;
    let mut candidates_by_x = vec![Vec::<usize>::new(); width + 1];
    for (index, candidate) in scores.candidates.iter().enumerate() {
        let glyph = &glyphs[candidate.glyph_index];
        if glyph.is_blank
            || candidate.score >= config.score_cutoff
            || candidate.x as usize >= width
            || candidate.x + glyph.advance > scores.width
        {
            continue;
        }
        candidates_by_x[candidate.x as usize].push(index);
    }

    for candidates in &mut candidates_by_x {
        candidates.sort_by(|a, b| {
            scores.candidates[*a]
                .score
                .total_cmp(&scores.candidates[*b].score)
        });
        candidates.truncate(TOP_CANDIDATES_PER_X);
    }

    let mut costs = vec![f32::INFINITY; width + 1];
    let mut previous = vec![None::<IntervalPrev>; width + 1];
    costs[0] = 0.0;

    for x in 0..width {
        if !costs[x].is_finite() {
            continue;
        }

        if costs[x] < costs[x + 1] {
            costs[x + 1] = costs[x];
            previous[x + 1] = Some(IntervalPrev {
                previous_x: x,
                candidate_index: None,
            });
        }

        for &candidate_index in &candidates_by_x[x] {
            let candidate = &scores.candidates[candidate_index];
            let glyph = &glyphs[candidate.glyph_index];
            let end = (candidate.x + glyph.advance) as usize;
            let candidate_cost = costs[x] + candidate.score;
            if candidate_cost < costs[end] {
                costs[end] = candidate_cost;
                previous[end] = Some(IntervalPrev {
                    previous_x: x,
                    candidate_index: Some(candidate_index),
                });
            }
        }
    }

    let mut placements = Vec::new();
    let mut cursor = width;
    while cursor > 0 {
        let Some(prev) = previous[cursor] else {
            break;
        };
        if let Some(candidate_index) = prev.candidate_index {
            let candidate = &scores.candidates[candidate_index];
            let glyph = &glyphs[candidate.glyph_index];
            placements.push(PlacedGlyph {
                ch: glyph.ch,
                x: candidate.x,
                y,
                width: glyph.advance,
                height: glyph.height,
            });
        }
        cursor = prev.previous_x;
    }

    placements.reverse();
    placements
}

#[derive(Debug, Clone, Copy)]
enum PruneMode {
    Support,
    LocalSearch,
}

#[derive(Debug, Clone)]
struct PlacementWeakness {
    ch: char,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    unsupported_ratio: f32,
    support_ratio: f32,
    support_pixels: usize,
}

fn post_prune_result(
    mut result: AsciiResult,
    font_bytes: &[u8],
    config: &AsciiConfig,
    mode: PruneMode,
) -> Result<AsciiResult, AaError> {
    let started = Instant::now();
    let font = Font::from_bytes(font_bytes, FontSettings::default())
        .map_err(|err| AaError::Font(err.to_owned()))?;
    let glyphs = build_glyphs(&font, config)?;
    let reference = rgba_to_binary_ink(&result.line_preview, 245);
    let ref_dist = distance_map(&reference);

    let mut placements = match mode {
        PruneMode::Support => support_pruned_placements(
            &result.placements,
            &glyphs,
            &ref_dist,
            reference.width,
            reference.height,
        ),
        PruneMode::LocalSearch => {
            local_search_pruned_placements(&result, &glyphs, &reference, &ref_dist)
        }
    };
    placements.sort_by_key(|placement| (placement.y, placement.x));

    result.placements = placements;
    result.text = build_text(
        &result.placements,
        &glyphs,
        result.width,
        config.stripe_stride_px,
    );
    result.ascii_preview =
        render_ascii_preview(result.width, result.height, &result.placements, &glyphs);
    result.timings.placement += started.elapsed();
    result.timings.total += started.elapsed();
    result.stats.placed_glyphs = result
        .placements
        .iter()
        .filter(|placement| !placement.ch.is_whitespace())
        .count();
    Ok(result)
}

fn support_pruned_placements(
    placements: &[PlacedGlyph],
    glyphs: &[GlyphImage],
    ref_dist: &[f32],
    ref_width: u32,
    ref_height: u32,
) -> Vec<PlacedGlyph> {
    placements
        .iter()
        .filter(|placement| {
            let Some(weakness) =
                placement_weakness(placement, glyphs, ref_dist, ref_width, ref_height)
            else {
                return false;
            };
            weakness.support_pixels >= 2
                && !(weakness.unsupported_ratio > 0.92 && weakness.support_ratio < 0.08)
        })
        .cloned()
        .collect()
}

fn local_search_pruned_placements(
    result: &AsciiResult,
    glyphs: &[GlyphImage],
    reference: &InkImage,
    ref_dist: &[f32],
) -> Vec<PlacedGlyph> {
    const MAX_TRIALS: usize = 96;
    const MIN_IMPROVEMENT: f32 = 0.0008;

    let mut placements = result.placements.clone();
    let mut current_score = prune_objective(reference, &result.ascii_preview);
    let mut candidates: Vec<PlacementWeakness> = placements
        .iter()
        .filter_map(|placement| {
            placement_weakness(
                placement,
                glyphs,
                ref_dist,
                reference.width,
                reference.height,
            )
        })
        .collect();

    candidates.sort_by(|a, b| {
        b.unsupported_ratio
            .total_cmp(&a.unsupported_ratio)
            .then_with(|| a.support_ratio.total_cmp(&b.support_ratio))
    });
    candidates.truncate(MAX_TRIALS);

    for candidate in candidates {
        let Some(index) = placements.iter().position(|placement| {
            placement.ch == candidate.ch
                && placement.x == candidate.x
                && placement.y == candidate.y
                && placement.width == candidate.width
                && placement.height == candidate.height
        }) else {
            continue;
        };

        let mut trial = placements.clone();
        trial.remove(index);
        let trial_preview = render_ascii_preview(result.width, result.height, &trial, glyphs);
        let trial_score = prune_objective(reference, &trial_preview);
        if trial_score > current_score + MIN_IMPROVEMENT {
            placements = trial;
            current_score = trial_score;
        }
    }

    placements
}

fn placement_weakness(
    placement: &PlacedGlyph,
    glyphs: &[GlyphImage],
    ref_dist: &[f32],
    ref_width: u32,
    ref_height: u32,
) -> Option<PlacementWeakness> {
    const SUPPORT_DISTANCE: f32 = 1.75;

    let glyph = glyphs.iter().find(|glyph| glyph.ch == placement.ch)?;
    if glyph.is_blank || glyph.foreground.is_empty() {
        return None;
    }

    let mut supported = 0usize;
    let mut unsupported = 0usize;
    for &(gx, gy) in &glyph.foreground {
        let tx = placement.x + gx;
        let ty = placement.y + gy;
        if tx >= ref_width || ty >= ref_height {
            continue;
        }
        let idx = (ty * ref_width + tx) as usize;
        let distance = ref_dist.get(idx).copied().unwrap_or(1.0e6);
        if distance <= SUPPORT_DISTANCE {
            supported += 1;
        } else {
            unsupported += 1;
        }
    }

    let total = supported + unsupported;
    if total == 0 {
        return None;
    }

    Some(PlacementWeakness {
        ch: placement.ch,
        x: placement.x,
        y: placement.y,
        width: placement.width,
        height: placement.height,
        unsupported_ratio: unsupported as f32 / total as f32,
        support_ratio: supported as f32 / total as f32,
        support_pixels: supported,
    })
}

fn prune_objective(reference: &InkImage, ascii_preview: &RgbaImage) -> f32 {
    let candidate_raw = resize_rgba_to(ascii_preview, reference.width, reference.height);
    let candidate = rgba_to_binary_ink(&candidate_raw, 245);
    let shape = shape_metrics(reference, &candidate);
    shape.f1 - 0.42 * shape.overdraw_ratio - 0.34 * shape.underdraw_ratio
}

#[derive(Debug, Clone, Copy)]
enum GridBaseline {
    Density,
    FixedScore,
}

fn convert_grid_baseline(
    image: &DynamicImage,
    font_bytes: &[u8],
    config: &AsciiConfig,
    baseline: GridBaseline,
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
    let grid_step = median_advance(&glyphs).max(1);

    let started = Instant::now();
    let stripe_results: Vec<Vec<PlacedGlyph>> = (0..stripe_count)
        .into_par_iter()
        .map(|stripe| {
            place_grid_baseline(
                &features,
                &glyphs,
                stripe as u32,
                config,
                baseline,
                grid_step,
            )
        })
        .collect();
    let mut placements: Vec<PlacedGlyph> = stripe_results.into_iter().flatten().collect();
    placements.sort_by_key(|placement| (placement.y, placement.x));
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

fn place_grid_baseline(
    features: &FeatureImage,
    glyphs: &[GlyphImage],
    stripe: u32,
    config: &AsciiConfig,
    baseline: GridBaseline,
    grid_step: u32,
) -> Vec<PlacedGlyph> {
    let mut placements = Vec::new();
    let y = stripe * config.stripe_stride_px;
    let mut x = 0;
    while x < features.width {
        let best = match baseline {
            GridBaseline::Density => best_density_glyph(features, glyphs, x, y, config),
            GridBaseline::FixedScore => best_scored_glyph(features, glyphs, x, y, config),
        };

        if let Some((glyph_index, score)) = best {
            let glyph = &glyphs[glyph_index];
            let should_place = match baseline {
                GridBaseline::Density => {
                    source_density(features, x, y, glyph.advance, config.stripe_stride_px) > 0.005
                }
                GridBaseline::FixedScore => score < config.score_cutoff,
            };
            if !glyph.is_blank && should_place {
                placements.push(PlacedGlyph {
                    ch: glyph.ch,
                    x,
                    y,
                    width: glyph.advance,
                    height: glyph.height,
                });
            }
        }

        x += grid_step;
    }
    placements
}

fn best_density_glyph(
    features: &FeatureImage,
    glyphs: &[GlyphImage],
    x: u32,
    y: u32,
    config: &AsciiConfig,
) -> Option<(usize, f32)> {
    glyphs
        .iter()
        .enumerate()
        .filter(|(_, glyph)| !glyph.is_blank && x + glyph.advance <= features.width)
        .map(|(index, glyph)| {
            let source_density =
                source_density(features, x, y, glyph.advance, config.stripe_stride_px);
            let glyph_density = glyph.foreground.len() as f32 / (glyph.width * glyph.height) as f32;
            (index, (source_density - glyph_density).abs())
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
}

fn best_scored_glyph(
    features: &FeatureImage,
    glyphs: &[GlyphImage],
    x: u32,
    y: u32,
    config: &AsciiConfig,
) -> Option<(usize, f32)> {
    glyphs
        .iter()
        .enumerate()
        .filter(|(_, glyph)| !glyph.is_blank && x + glyph.advance <= features.width)
        .map(|(index, glyph)| (index, score_glyph_at(features, glyph, x, y, config)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
}

fn source_density(features: &FeatureImage, x: u32, y: u32, width: u32, height: u32) -> f32 {
    let mut foreground = 0usize;
    let mut total = 0usize;
    let max_y = (y + height).min(features.height);
    let max_x = (x + width).min(features.width);
    for sy in y..max_y {
        for sx in x..max_x {
            total += 1;
            if features.source_at(sx as i32, sy as i32) > 0.0 {
                foreground += 1;
            }
        }
    }
    if total == 0 {
        0.0
    } else {
        foreground as f32 / total as f32
    }
}

fn median_advance(glyphs: &[GlyphImage]) -> u32 {
    let mut advances: Vec<u32> = glyphs
        .iter()
        .filter(|glyph| !glyph.is_blank)
        .map(|glyph| glyph.advance.max(1))
        .collect();
    if advances.is_empty() {
        return 1;
    }
    advances.sort_unstable();
    advances[advances.len() / 2]
}

fn evaluate_result(result: &AsciiResult, rois: &[RoiRect]) -> BenchmarkMetrics {
    let reference = rgba_to_binary_ink(&result.line_preview, 245);
    let candidate_raw = resize_rgba_to(&result.ascii_preview, reference.width, reference.height);
    let candidate = rgba_to_binary_ink(&candidate_raw, 245);
    let reference_skeleton = thin_image(&reference, ThinningMode::KmmK3mLookup);
    let candidate_skeleton = thin_image(&candidate, ThinningMode::KmmK3mLookup);

    let shape = shape_metrics(&reference_skeleton, &candidate_skeleton);
    let orientation_agreement = orientation_agreement(&reference_skeleton, &candidate_skeleton);
    let roi_metrics = roi_metrics(&reference_skeleton, &candidate_skeleton, rois);
    let roi_weighted_score = if roi_metrics.is_empty() {
        shape.f1
    } else {
        let total_weight: f32 = roi_metrics.iter().map(|roi| roi.weight).sum();
        if total_weight <= 0.0 {
            shape.f1
        } else {
            roi_metrics
                .iter()
                .map(|roi| roi.skeleton_f1 * roi.weight)
                .sum::<f32>()
                / total_weight
        }
    };

    BenchmarkMetrics {
        skeleton_f1: shape.f1,
        precision: shape.precision,
        recall: shape.recall,
        chamfer_distance: shape.chamfer_distance,
        orientation_agreement,
        overdraw_ratio: shape.overdraw_ratio,
        underdraw_ratio: shape.underdraw_ratio,
        roi_weighted_score,
        missing_glyph_count: 0,
        runtime_ms: 0.0,
        roi: roi_metrics,
    }
}

fn rgba_to_binary_ink(image: &RgbaImage, threshold: u8) -> InkImage {
    let mut ink = InkImage::new(image.width(), image.height());
    for y in 0..image.height() {
        for x in 0..image.width() {
            let pixel = image.get_pixel(x, y);
            let luminance =
                (0.2126 * pixel[0] as f32 + 0.7152 * pixel[1] as f32 + 0.0722 * pixel[2] as f32)
                    .round() as u8;
            if luminance < threshold {
                ink.set(x, y, 1.0);
            }
        }
    }
    ink
}

fn resize_rgba_to(image: &RgbaImage, width: u32, height: u32) -> RgbaImage {
    if image.width() == width && image.height() == height {
        return image.clone();
    }
    image::imageops::resize(image, width, height, FilterType::Nearest)
}

#[derive(Debug, Clone, Copy)]
struct ShapeMetrics {
    precision: f32,
    recall: f32,
    f1: f32,
    chamfer_distance: f32,
    overdraw_ratio: f32,
    underdraw_ratio: f32,
}

fn shape_metrics(reference: &InkImage, candidate: &InkImage) -> ShapeMetrics {
    let ref_dist = distance_map(reference);
    let cand_dist = distance_map(candidate);
    let ref_points = foreground_points(reference);
    let cand_points = foreground_points(candidate);

    if ref_points.is_empty() && cand_points.is_empty() {
        return ShapeMetrics {
            precision: 1.0,
            recall: 1.0,
            f1: 1.0,
            chamfer_distance: 0.0,
            overdraw_ratio: 0.0,
            underdraw_ratio: 0.0,
        };
    }

    let precision_hits = cand_points
        .iter()
        .filter(|(x, y)| ref_dist[(y * reference.width + x) as usize] <= MATCH_TOLERANCE_PX)
        .count();
    let recall_hits = ref_points
        .iter()
        .filter(|(x, y)| cand_dist[(y * candidate.width + x) as usize] <= MATCH_TOLERANCE_PX)
        .count();

    let precision = ratio(precision_hits, cand_points.len());
    let recall = ratio(recall_hits, ref_points.len());
    let f1 = if precision + recall <= f32::EPSILON {
        0.0
    } else {
        2.0 * precision * recall / (precision + recall)
    };

    let cand_to_ref = average_distance(&cand_points, reference.width, &ref_dist);
    let ref_to_cand = average_distance(&ref_points, candidate.width, &cand_dist);
    let chamfer_distance = match (cand_to_ref, ref_to_cand) {
        (Some(a), Some(b)) => (a + b) / 2.0,
        (Some(a), None) | (None, Some(a)) => a,
        (None, None) => 0.0,
    };

    ShapeMetrics {
        precision,
        recall,
        f1,
        chamfer_distance,
        overdraw_ratio: 1.0 - precision,
        underdraw_ratio: 1.0 - recall,
    }
}

fn distance_map(image: &InkImage) -> Vec<f32> {
    let len = (image.width * image.height) as usize;
    let mut dist = vec![1.0e6f32; len];
    for y in 0..image.height {
        for x in 0..image.width {
            if image.get(x as i32, y as i32) > 0.0 {
                dist[(y * image.width + x) as usize] = 0.0;
            }
        }
    }

    for y in 0..image.height {
        for x in 0..image.width {
            relax(&mut dist, image.width, image.height, x, y, -1, 0, 1.0);
            relax(&mut dist, image.width, image.height, x, y, 0, -1, 1.0);
            relax(&mut dist, image.width, image.height, x, y, -1, -1, 1.4142);
            relax(&mut dist, image.width, image.height, x, y, 1, -1, 1.4142);
        }
    }

    for y in (0..image.height).rev() {
        for x in (0..image.width).rev() {
            relax(&mut dist, image.width, image.height, x, y, 1, 0, 1.0);
            relax(&mut dist, image.width, image.height, x, y, 0, 1, 1.0);
            relax(&mut dist, image.width, image.height, x, y, 1, 1, 1.4142);
            relax(&mut dist, image.width, image.height, x, y, -1, 1, 1.4142);
        }
    }
    dist
}

fn relax(dist: &mut [f32], width: u32, height: u32, x: u32, y: u32, dx: i32, dy: i32, cost: f32) {
    let nx = x as i32 + dx;
    let ny = y as i32 + dy;
    if nx < 0 || ny < 0 || nx >= width as i32 || ny >= height as i32 {
        return;
    }
    let idx = (y * width + x) as usize;
    let nidx = (ny as u32 * width + nx as u32) as usize;
    let candidate = dist[nidx] + cost;
    if candidate < dist[idx] {
        dist[idx] = candidate;
    }
}

fn foreground_points(image: &InkImage) -> Vec<(u32, u32)> {
    let mut points = Vec::new();
    for y in 0..image.height {
        for x in 0..image.width {
            if image.get(x as i32, y as i32) > 0.0 {
                points.push((x, y));
            }
        }
    }
    points
}

fn average_distance(points: &[(u32, u32)], width: u32, dist: &[f32]) -> Option<f32> {
    if points.is_empty() {
        return None;
    }
    let sum: f32 = points
        .iter()
        .map(|(x, y)| dist[(y * width + x) as usize].min(64.0))
        .sum();
    Some(sum / points.len() as f32)
}

fn orientation_agreement(reference: &InkImage, candidate: &InkImage) -> f32 {
    let reference_features = extract_features(reference, 0.7);
    let candidate_features = extract_features(candidate, 0.7);
    let mut total = 0.0f32;
    let mut count = 0usize;

    for y in 0..candidate.height {
        for x in 0..candidate.width {
            if candidate.get(x as i32, y as i32) <= 0.0 {
                continue;
            }
            let Some(candidate_theta) = candidate_features.orientation_at(x as i32, y as i32)
            else {
                continue;
            };
            let Some(reference_theta) =
                nearest_orientation(&reference_features, reference, x, y, 2)
            else {
                continue;
            };
            let delta = angle_delta(candidate_theta, reference_theta);
            total += (1.0 - delta / (PI / 2.0)).clamp(0.0, 1.0);
            count += 1;
        }
    }

    if count == 0 {
        if reference.foreground_count() == 0 && candidate.foreground_count() == 0 {
            1.0
        } else {
            0.0
        }
    } else {
        total / count as f32
    }
}

fn nearest_orientation(
    features: &FeatureImage,
    ink: &InkImage,
    x: u32,
    y: u32,
    radius: i32,
) -> Option<f32> {
    let mut best = None;
    let mut best_dist = i32::MAX;
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            let sx = x as i32 + dx;
            let sy = y as i32 + dy;
            if ink.get(sx, sy) <= 0.0 {
                continue;
            }
            let Some(theta) = features.orientation_at(sx, sy) else {
                continue;
            };
            let distance = dx * dx + dy * dy;
            if distance < best_dist {
                best = Some(theta);
                best_dist = distance;
            }
        }
    }
    best
}

fn roi_metrics(reference: &InkImage, candidate: &InkImage, rois: &[RoiRect]) -> Vec<RoiMetric> {
    rois.iter()
        .filter_map(|roi| {
            let (x, y, width, height) = roi.to_pixel_box(reference.width, reference.height)?;
            let reference_crop = crop_ink(reference, x, y, width, height);
            let candidate_crop = crop_ink(candidate, x, y, width, height);
            let shape = shape_metrics(&reference_crop, &candidate_crop);
            Some(RoiMetric {
                label: roi.label.clone(),
                weight: roi_weight(&roi.label),
                skeleton_f1: shape.f1,
                overdraw_ratio: shape.overdraw_ratio,
                underdraw_ratio: shape.underdraw_ratio,
            })
        })
        .collect()
}

impl RoiRect {
    fn to_pixel_box(&self, image_width: u32, image_height: u32) -> Option<(u32, u32, u32, u32)> {
        if self.width <= 0.0 || self.height <= 0.0 {
            return None;
        }
        let normalized = self.x <= 1.0 && self.y <= 1.0 && self.width <= 1.0 && self.height <= 1.0;
        let (x, y, width, height) = if normalized {
            (
                (self.x * image_width as f32).round(),
                (self.y * image_height as f32).round(),
                (self.width * image_width as f32).round(),
                (self.height * image_height as f32).round(),
            )
        } else {
            (
                self.x.round(),
                self.y.round(),
                self.width.round(),
                self.height.round(),
            )
        };
        let x = x.clamp(0.0, image_width as f32) as u32;
        let y = y.clamp(0.0, image_height as f32) as u32;
        let right = (x + width.max(1.0) as u32).min(image_width);
        let bottom = (y + height.max(1.0) as u32).min(image_height);
        (right > x && bottom > y).then_some((x, y, right - x, bottom - y))
    }
}

fn crop_ink(image: &InkImage, x: u32, y: u32, width: u32, height: u32) -> InkImage {
    let mut crop = InkImage::new(width, height);
    for cy in 0..height {
        for cx in 0..width {
            crop.set(cx, cy, image.get((x + cx) as i32, (y + cy) as i32));
        }
    }
    crop
}

fn roi_weight(label: &str) -> f32 {
    match label {
        "eyes" => 4.0,
        "mouth" => 3.0,
        "face" => 3.0,
        "hair" => 2.0,
        "body" => 1.0,
        _ => 1.0,
    }
}

fn ratio(numerator: usize, denominator: usize) -> f32 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f32 / denominator as f32
    }
}

fn missing_glyph_count(font_bytes: &[u8], character_set: &str) -> Result<usize, AaError> {
    let face = Face::parse(font_bytes, 0).map_err(|err| AaError::Font(err.to_string()))?;
    let mut seen = HashSet::new();
    Ok(character_set
        .chars()
        .filter(|ch| seen.insert(*ch))
        .filter(|ch| face.glyph_index(*ch).is_none())
        .count())
}

fn build_gallery_pairs(cases: &[BenchmarkCaseReport]) -> Vec<GalleryPair> {
    let mut pairs = Vec::new();
    for case in cases {
        for left in 0..case.results.len() {
            for right in (left + 1)..case.results.len() {
                let a = &case.results[left];
                let b = &case.results[right];
                pairs.push(GalleryPair {
                    id: format!("{}__{}__{}", case.id, a.algorithm, b.algorithm),
                    case_id: case.id.clone(),
                    original: case.source_image.clone(),
                    a_render: a.render_png.clone(),
                    b_render: b.render_png.clone(),
                    a_algorithm: a.algorithm.clone(),
                    b_algorithm: b.algorithm.clone(),
                });
            }
        }
    }
    pairs
}

fn render_gallery_html(report: &BenchmarkReport) -> Result<String, AaError> {
    let data = serde_json::to_string(&report.gallery_pairs)?.replace("</", "<\\/");
    let title = html_escape(&format!("{} benchmark", report.name));
    Ok(format!(
        r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>{title}</title>
  <style>
    :root {{ color-scheme: light dark; font-family: Inter, system-ui, sans-serif; }}
    body {{ margin: 0; background: #f6f8f7; color: #1b1f1e; }}
    header {{ padding: 24px 32px 12px; border-bottom: 1px solid #cbd4d0; }}
    main {{ padding: 24px 32px 48px; max-width: 1280px; margin: 0 auto; }}
    h1 {{ margin: 0 0 8px; font-size: 24px; }}
    button {{ border: 1px solid #1b1f1e; background: #ffffff; border-radius: 6px; padding: 8px 12px; cursor: pointer; }}
    .toolbar {{ display: flex; gap: 8px; flex-wrap: wrap; margin-top: 12px; }}
    .pair {{ display: grid; grid-template-columns: 1fr 1fr 1fr; gap: 16px; margin: 28px 0; align-items: start; }}
    .panel {{ background: #ffffff; border: 1px solid #cbd4d0; border-radius: 8px; padding: 12px; }}
    .panel h2 {{ margin: 0 0 10px; font-size: 14px; font-weight: 700; }}
    img {{ width: 100%; image-rendering: auto; background: #fafafa; border: 1px solid #cbd4d0; }}
    .vote {{ display: grid; grid-template-columns: 1fr 1fr; gap: 8px; margin-top: 10px; }}
    .vote button.selected {{ background: #153d35; color: #ffffff; }}
    .meta {{ font-size: 13px; color: #58615e; }}
    @media (max-width: 900px) {{ .pair {{ grid-template-columns: 1fr; }} }}
  </style>
</head>
<body>
  <header>
    <h1>{title}</h1>
    <div class="meta">Blind A/B gallery. Algorithm names are hidden in the UI and included only in exported JSON.</div>
    <div class="toolbar">
      <a href="overview.html"><button type="button">Open overview</button></a>
      <button id="export">Export votes JSON</button>
      <button id="clear">Clear votes</button>
    </div>
  </header>
  <main id="app"></main>
  <script>
    const pairs = {data};
    const storeKey = "aa-benchmark-votes:{title}";
    const votes = JSON.parse(localStorage.getItem(storeKey) || "{{}}");
    const app = document.getElementById("app");

    function save() {{
      localStorage.setItem(storeKey, JSON.stringify(votes));
    }}

    function vote(pairId, question, choice) {{
      votes[pairId] = votes[pairId] || {{}};
      votes[pairId][question] = choice;
      save();
      render();
    }}

    function selected(pairId, question, choice) {{
      return votes[pairId] && votes[pairId][question] === choice ? "selected" : "";
    }}

    function render() {{
      app.innerHTML = pairs.map((pair, index) => `
        <section class="pair">
          <div class="panel">
            <h2>Original · ${{pair.case_id}}</h2>
            <img src="${{pair.original}}" alt="Original input">
          </div>
          <div class="panel">
            <h2>Candidate A</h2>
            <img src="${{pair.a_render}}" alt="Candidate A">
            <div class="vote">
              <button class="${{selected(pair.id, "similar", "A")}}" onclick="vote('${{pair.id}}', 'similar', 'A')">More similar</button>
              <button class="${{selected(pair.id, "aesthetic", "A")}}" onclick="vote('${{pair.id}}', 'aesthetic', 'A')">More aesthetic</button>
            </div>
          </div>
          <div class="panel">
            <h2>Candidate B</h2>
            <img src="${{pair.b_render}}" alt="Candidate B">
            <div class="vote">
              <button class="${{selected(pair.id, "similar", "B")}}" onclick="vote('${{pair.id}}', 'similar', 'B')">More similar</button>
              <button class="${{selected(pair.id, "aesthetic", "B")}}" onclick="vote('${{pair.id}}', 'aesthetic', 'B')">More aesthetic</button>
            </div>
          </div>
        </section>
      `).join("");
    }}

    document.getElementById("export").onclick = () => {{
      const payload = pairs.map(pair => ({{
        pair_id: pair.id,
        case_id: pair.case_id,
        a_algorithm: pair.a_algorithm,
        b_algorithm: pair.b_algorithm,
        vote: votes[pair.id] || {{}}
      }}));
      const blob = new Blob([JSON.stringify(payload, null, 2)], {{ type: "application/json" }});
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = "aa-benchmark-votes.json";
      a.click();
      URL.revokeObjectURL(url);
    }};

    document.getElementById("clear").onclick = () => {{
      if (confirm("Clear local votes?")) {{
        localStorage.removeItem(storeKey);
        for (const key of Object.keys(votes)) delete votes[key];
        render();
      }}
    }};

    render();
  </script>
</body>
</html>
"#
    ))
}

fn render_overview_html(report: &BenchmarkReport) -> Result<String, AaError> {
    let data = serde_json::to_string(&report.cases)?.replace("</", "<\\/");
    let title = html_escape(&format!("{} overview", report.name));
    Ok(format!(
        r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>{title}</title>
  <style>
    :root {{
      color-scheme: light;
      --tile: 260px;
      font-family: "Segoe UI", ui-sans-serif, system-ui, sans-serif;
      background: #eef1ee;
      color: #151918;
    }}
    body {{ margin: 0; background: #eef1ee; }}
    header {{
      position: sticky;
      top: 0;
      z-index: 5;
      padding: 18px 24px;
      border-bottom: 1px solid #c4ccc7;
      background: rgba(238, 241, 238, 0.96);
      backdrop-filter: blur(12px);
    }}
    main {{ padding: 20px 24px 48px; }}
    h1 {{ margin: 0 0 8px; font-size: 22px; }}
    button, .link-button {{
      border: 1px solid #202826;
      background: #ffffff;
      color: #151918;
      border-radius: 6px;
      padding: 7px 10px;
      cursor: pointer;
      text-decoration: none;
      font: inherit;
      font-size: 13px;
    }}
    .toolbar {{ display: flex; gap: 10px; align-items: center; flex-wrap: wrap; }}
    .toolbar label {{ display: flex; gap: 8px; align-items: center; font-size: 13px; color: #4f5a56; }}
    .case {{
      margin: 0 0 28px;
      padding: 16px;
      border: 1px solid #c4ccc7;
      border-radius: 8px;
      background: #f9faf8;
    }}
    .case-head {{ display: flex; align-items: baseline; justify-content: space-between; gap: 16px; margin-bottom: 12px; }}
    .case h2 {{ margin: 0; font-size: 16px; }}
    .tags {{ font-size: 12px; color: #65706c; }}
    .grid {{
      display: grid;
      grid-template-columns: repeat(var(--cols), minmax(var(--tile), 1fr));
      gap: 12px;
      overflow-x: auto;
      padding-bottom: 6px;
    }}
    .tile {{
      min-width: var(--tile);
      border: 1px solid #d2d9d5;
      border-radius: 8px;
      background: #ffffff;
      padding: 10px;
    }}
    .tile h3 {{
      display: flex;
      justify-content: space-between;
      gap: 10px;
      margin: 0 0 8px;
      font-size: 13px;
      line-height: 1.25;
    }}
    .role {{ color: #65706c; font-weight: 500; }}
    img {{
      width: 100%;
      aspect-ratio: 1 / 1;
      object-fit: contain;
      background: #fbfbf8;
      border: 1px solid #d2d9d5;
    }}
    .metrics {{
      display: grid;
      grid-template-columns: repeat(2, minmax(0, 1fr));
      gap: 4px 8px;
      margin-top: 8px;
      color: #4f5a56;
      font-size: 11px;
      font-variant-numeric: tabular-nums;
    }}
    .vote {{ display: grid; grid-template-columns: 1fr 1fr; gap: 6px; margin-top: 8px; }}
    .vote button {{ font-size: 12px; padding: 6px 8px; }}
    .vote button.selected {{ background: #153d35; color: #fff; }}
    .meta {{ font-size: 13px; color: #4f5a56; margin-bottom: 12px; }}
    @media (max-width: 760px) {{
      :root {{ --tile: 220px; }}
      main {{ padding: 16px 12px 36px; }}
      header {{ padding: 14px 12px; }}
    }}
  </style>
</head>
<body>
  <header>
    <h1>{title}</h1>
    <div class="meta">Non-blind overview. Compare every algorithm for each case at once, then mark quick picks locally.</div>
    <div class="toolbar">
      <a class="link-button" href="index.html">Blind A/B</a>
      <button id="export">Export picks JSON</button>
      <button id="clear">Clear picks</button>
      <label>Tile size <input id="tile-size" type="range" min="180" max="420" value="260"></label>
    </div>
  </header>
  <main id="app"></main>
  <script>
    const cases = {data};
    const storeKey = "aa-benchmark-overview-picks:{title}";
    const picks = JSON.parse(localStorage.getItem(storeKey) || "{{}}");
    const app = document.getElementById("app");

    function metric(value) {{
      return Number(value || 0).toFixed(3);
    }}

    function save() {{
      localStorage.setItem(storeKey, JSON.stringify(picks));
    }}

    function pick(caseId, kind, algorithm) {{
      picks[caseId] = picks[caseId] || {{}};
      picks[caseId][kind] = picks[caseId][kind] === algorithm ? null : algorithm;
      save();
      render();
    }}

    function selected(caseId, kind, algorithm) {{
      return picks[caseId] && picks[caseId][kind] === algorithm ? "selected" : "";
    }}

    function resultTile(caseId, result) {{
      const m = result.metrics;
      return `
        <article class="tile">
          <h3><span>${{result.algorithm}}</span><span class="role">result</span></h3>
          <img src="${{result.render_png}}" alt="${{caseId}} ${{result.algorithm}}">
          <div class="metrics">
            <span>F1 ${{metric(m.skeleton_f1)}}</span>
            <span>ROI ${{metric(m.roi_weighted_score)}}</span>
            <span>over ${{metric(m.overdraw_ratio)}}</span>
            <span>under ${{metric(m.underdraw_ratio)}}</span>
          </div>
          <div class="vote">
            <button class="${{selected(caseId, "similar", result.algorithm)}}" onclick="pick('${{caseId}}', 'similar', '${{result.algorithm}}')">Similar</button>
            <button class="${{selected(caseId, "aesthetic", result.algorithm)}}" onclick="pick('${{caseId}}', 'aesthetic', '${{result.algorithm}}')">Pretty</button>
          </div>
        </article>
      `;
    }}

    function render() {{
      app.innerHTML = cases.map((item) => `
        <section class="case">
          <div class="case-head">
            <h2>${{item.id}}</h2>
            <div class="tags">${{(item.tags || []).join(" · ")}}</div>
          </div>
          <div class="grid" style="--cols: ${{item.results.length + 1}}">
            <article class="tile">
              <h3><span>Original</span><span class="role">input</span></h3>
              <img src="${{item.source_image}}" alt="${{item.id}} original">
            </article>
            ${{item.results.map((result) => resultTile(item.id, result)).join("")}}
          </div>
        </section>
      `).join("");
    }}

    document.getElementById("tile-size").addEventListener("input", (event) => {{
      document.documentElement.style.setProperty("--tile", `${{event.target.value}}px`);
    }});

    document.getElementById("export").onclick = () => {{
      const blob = new Blob([JSON.stringify(picks, null, 2)], {{ type: "application/json" }});
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = "aa-benchmark-overview-picks.json";
      a.click();
      URL.revokeObjectURL(url);
    }};

    document.getElementById("clear").onclick = () => {{
      if (confirm("Clear local picks?")) {{
        localStorage.removeItem(storeKey);
        for (const key of Object.keys(picks)) delete picks[key];
        render();
      }}
    }};

    render();
  </script>
</body>
</html>
"#
    ))
}

fn relative_slash(root: &Path, path: &Path) -> String {
    let relative = path.strip_prefix(root).unwrap_or(path);
    relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn safe_id(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn identical_previews_score_high() {
        let image = cross_preview(32, 32);
        let mut result = fake_result(image.clone(), image);
        let metrics = evaluate_result(&result, &[]);
        assert!(metrics.skeleton_f1 > 0.95);
        assert!(metrics.chamfer_distance < 0.1);
        assert!(!metrics.skeleton_f1.is_nan());
        result.ascii_preview = RgbaImage::from_pixel(32, 32, Rgba([250, 250, 246, 255]));
        let blank_metrics = evaluate_result(&result, &[]);
        assert!(blank_metrics.skeleton_f1 < metrics.skeleton_f1);
    }

    #[test]
    fn blank_previews_do_not_produce_nan() {
        let blank = RgbaImage::from_pixel(24, 24, Rgba([250, 250, 246, 255]));
        let metrics = evaluate_result(&fake_result(blank.clone(), blank), &[]);
        assert_eq!(metrics.skeleton_f1, 1.0);
        assert!(!metrics.chamfer_distance.is_nan());
        assert!(!metrics.orientation_agreement.is_nan());
    }

    #[test]
    fn roi_weighting_changes_score_when_important_region_differs() {
        let reference = two_region_preview();
        let mut candidate = RgbaImage::from_pixel(40, 20, Rgba([250, 250, 246, 255]));
        for x in 2..18 {
            candidate.put_pixel(x, 10, Rgba([10, 10, 10, 255]));
        }
        let rois = vec![
            RoiRect {
                label: "eyes".to_owned(),
                x: 0.0,
                y: 0.0,
                width: 0.5,
                height: 1.0,
            },
            RoiRect {
                label: "body".to_owned(),
                x: 0.5,
                y: 0.0,
                width: 0.5,
                height: 1.0,
            },
        ];
        let metrics = evaluate_result(&fake_result(reference, candidate), &rois);
        assert!(metrics.roi_weighted_score > metrics.skeleton_f1);
    }

    #[test]
    fn missing_glyph_count_detects_unsupported_character() {
        let Some(font_path) = find_default_font() else {
            return;
        };
        let font_bytes = std::fs::read(font_path).unwrap();
        let count = missing_glyph_count(&font_bytes, "A\u{10ffff}").unwrap();
        assert!(count >= 1);
    }

    #[test]
    fn run_benchmark_writes_report_and_gallery() {
        let Some(_) = find_paper_font() else {
            return;
        };
        let root =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-work/bench-run");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("inputs")).unwrap();
        let image_path = root.join("inputs/cross.png");
        cross_preview(64, 64).save(&image_path).unwrap();
        let manifest_path = root.join("manifest.json");
        let manifest = serde_json::json!({
            "name": "unit-bench",
            "description": "unit test",
            "cases": [{
                "id": "cross",
                "image": "inputs/cross.png",
                "prompt": "synthetic cross fixture",
                "provenance": "unit-test",
                "license_status": "synthetic fixture",
                "tags": ["synthetic"],
                "roi": [{ "label": "eyes", "x": 0.0, "y": 0.0, "width": 1.0, "height": 0.5 }]
            }]
        });
        std::fs::write(
            &manifest_path,
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();
        let out_dir = root.join("out");
        let options = BenchmarkRunOptions {
            algorithms: vec![
                BenchmarkAlgorithm::LeftToRight,
                BenchmarkAlgorithm::PaperGreedy,
            ],
            ..BenchmarkRunOptions::default()
        };
        let report = run_benchmark(&manifest_path, &out_dir, &options).unwrap();
        assert_eq!(report.cases.len(), 1);
        assert!(out_dir.join("report.json").exists());
        assert!(out_dir.join("index.html").exists());
        assert!(out_dir.join("overview.html").exists());
        assert!(
            out_dir
                .join("cases/cross/left-to-right/03-ascii-render.png")
                .exists()
        );
    }

    fn fake_result(line_preview: RgbaImage, ascii_preview: RgbaImage) -> AsciiResult {
        AsciiResult {
            text: String::new(),
            width: line_preview.width(),
            height: line_preview.height(),
            line_preview,
            orientation_preview: RgbaImage::new(1, 1),
            ascii_preview,
            placements: Vec::new(),
            timings: PipelineTimings::default(),
            stats: PipelineStats::default(),
        }
    }

    fn cross_preview(width: u32, height: u32) -> RgbaImage {
        let mut image = RgbaImage::from_pixel(width, height, Rgba([250, 250, 246, 255]));
        for i in 4..width.min(height).saturating_sub(4) {
            image.put_pixel(i, i, Rgba([10, 10, 10, 255]));
            image.put_pixel(width - 1 - i, i, Rgba([10, 10, 10, 255]));
        }
        image
    }

    fn two_region_preview() -> RgbaImage {
        let mut image = RgbaImage::from_pixel(40, 20, Rgba([250, 250, 246, 255]));
        for x in 2..18 {
            image.put_pixel(x, 10, Rgba([10, 10, 10, 255]));
        }
        for x in 22..38 {
            image.put_pixel(x, 10, Rgba([10, 10, 10, 255]));
        }
        image
    }
}
