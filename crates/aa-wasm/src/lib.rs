use aa_core::{
    AsciiConfig, AsciiResult, InputMode, PlacementMode, StructureLineMode, ThinningMode,
    color_illustration_preset, convert_image, paper_preset,
};
use image::{DynamicImage, RgbaImage};
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ConvertOptions {
    max_width: Option<u32>,
    font_px: Option<f32>,
    stripe_px: Option<u32>,
    blur: Option<f32>,
    edge: Option<f32>,
    binary: Option<f32>,
    mismatch: Option<f32>,
    #[serde(rename = "match")]
    match_weight: Option<f32>,
    cutoff: Option<f32>,
    glyph_ink: Option<f32>,
    character_set: Option<String>,
}

#[derive(Debug, Serialize)]
struct ConvertResult {
    text: String,
    width: u32,
    height: u32,
    #[serde(with = "serde_bytes")]
    ascii_rgba: Vec<u8>,
    #[serde(with = "serde_bytes")]
    line_rgba: Vec<u8>,
    #[serde(with = "serde_bytes")]
    orientation_rgba: Vec<u8>,
    stats: StatsResult,
    timings: TimingsResult,
}

#[derive(Debug, Serialize)]
struct StatsResult {
    input_width: u32,
    input_height: u32,
    working_width: u32,
    working_height: u32,
    stripes: usize,
    glyphs: usize,
    placed_glyphs: usize,
    foreground_pixels: usize,
}

#[derive(Debug, Serialize)]
struct TimingsResult {
    preprocess_ms: f64,
    feature_ms: f64,
    glyph_analysis_ms: f64,
    scoring_ms: f64,
    placement_ms: f64,
    rendering_ms: f64,
    total_ms: f64,
}

#[wasm_bindgen]
pub fn convert_rgba(
    image_rgba: &[u8],
    image_width: u32,
    image_height: u32,
    font_bytes: &[u8],
    preset: &str,
    options: JsValue,
) -> Result<JsValue, JsValue> {
    console_error_panic_hook::set_once();

    let expected_len = image_width
        .checked_mul(image_height)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| JsValue::from_str("image dimensions are too large"))?
        as usize;
    if image_rgba.len() != expected_len {
        return Err(JsValue::from_str(
            "RGBA buffer length does not match image size",
        ));
    }

    let options = parse_options(options)?;
    let mut config = config_for_preset(preset, font_bytes).map_err(to_js_error)?;
    apply_web_defaults(&mut config);
    apply_options(&mut config, options);

    let rgba = RgbaImage::from_raw(image_width, image_height, image_rgba.to_vec())
        .ok_or_else(|| JsValue::from_str("failed to create RGBA image"))?;
    let image = DynamicImage::ImageRgba8(rgba);
    let result = convert_image(&image, font_bytes, &config).map_err(to_js_error)?;

    serde_wasm_bindgen::to_value(&convert_result(result)).map_err(to_js_error)
}

fn parse_options(options: JsValue) -> Result<ConvertOptions, JsValue> {
    if options.is_null() || options.is_undefined() {
        return Ok(ConvertOptions::default());
    }
    serde_wasm_bindgen::from_value(options).map_err(to_js_error)
}

fn config_for_preset(preset: &str, font_bytes: &[u8]) -> Result<AsciiConfig, aa_core::AaError> {
    match preset {
        "clean" | "paper" => paper_preset(font_bytes),
        "sensitive" => {
            let mut config = paper_preset(font_bytes)?;
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
            Ok(config)
        }
        "color" => color_illustration_preset(font_bytes),
        _ => paper_preset(font_bytes),
    }
}

fn apply_web_defaults(config: &mut AsciiConfig) {
    config.max_input_width = 512;
}

fn apply_options(config: &mut AsciiConfig, options: ConvertOptions) {
    if let Some(value) = options.max_width {
        config.max_input_width = value.clamp(128, 1280);
    }
    if let Some(value) = options.font_px {
        config.font_px = value.clamp(8.0, 32.0);
    }
    if let Some(value) = options.stripe_px {
        config.stripe_stride_px = value.clamp(10, 44);
    }
    if let Some(value) = options.blur {
        config.gaussian_sigma = value.clamp(0.3, 2.2);
    }
    if let Some(value) = options.edge {
        config.edge_threshold = value.clamp(0.04, 0.72);
    }
    if let Some(value) = options.binary {
        config.binary_threshold = value.clamp(0.05, 0.95);
    }
    if let Some(value) = options.mismatch {
        config.mismatch_weight = value.clamp(0.0, 2.0);
    }
    if let Some(value) = options.match_weight {
        config.match_weight = value.clamp(0.1, 2.5);
    }
    if let Some(value) = options.cutoff {
        config.score_cutoff = value.clamp(-240.0, 60.0);
    }
    if let Some(value) = options.glyph_ink {
        config.glyph_alpha_threshold = value.clamp(0.02, 0.6);
    }
    if let Some(value) = options.character_set {
        if !value.trim().is_empty() {
            config.character_set = value;
        }
    }
}

fn convert_result(result: AsciiResult) -> ConvertResult {
    let AsciiResult {
        text,
        width,
        height,
        line_preview,
        orientation_preview,
        ascii_preview,
        timings,
        stats,
        ..
    } = result;

    ConvertResult {
        text,
        width,
        height,
        ascii_rgba: ascii_preview.into_raw(),
        line_rgba: line_preview.into_raw(),
        orientation_rgba: orientation_preview.into_raw(),
        stats: StatsResult {
            input_width: stats.input_size.0,
            input_height: stats.input_size.1,
            working_width: stats.working_size.0,
            working_height: stats.working_size.1,
            stripes: stats.stripes,
            glyphs: stats.glyphs,
            placed_glyphs: stats.placed_glyphs,
            foreground_pixels: stats.foreground_pixels,
        },
        timings: TimingsResult {
            preprocess_ms: timings.preprocess.as_secs_f64() * 1000.0,
            feature_ms: timings.feature_extraction.as_secs_f64() * 1000.0,
            glyph_analysis_ms: timings.glyph_analysis.as_secs_f64() * 1000.0,
            scoring_ms: timings.scoring.as_secs_f64() * 1000.0,
            placement_ms: timings.placement.as_secs_f64() * 1000.0,
            rendering_ms: timings.rendering.as_secs_f64() * 1000.0,
            total_ms: timings.total.as_secs_f64() * 1000.0,
        },
    }
}

fn to_js_error(error: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&error.to_string())
}
