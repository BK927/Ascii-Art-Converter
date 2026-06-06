use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use image::{DynamicImage, GrayImage, Luma, Rgb, RgbImage, imageops::FilterType};
use ndarray::Array4;
use ort::session::Session;
use ort::value::TensorRef;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

const CATALOG_JSON: &str = include_str!("../../../assets/model_catalog.json");
const USER_AGENT: &str = "AA-Converter model manager";

#[derive(Debug, Error)]
pub enum LineartError {
    #[error("model catalog is invalid: {0}")]
    Catalog(#[from] serde_json::Error),
    #[error("I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error(
        "model install folder is not writable: {path}. Move AA Converter to a writable folder and try again. ({source})"
    )]
    DownloadDirectory {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error(
        "model install folder is unavailable because the executable path could not be determined"
    )]
    DownloadDirectoryUnavailable,
    #[error("model install failed: {0}")]
    Download(#[from] reqwest::Error),
    #[error("model {0} was not found")]
    UnknownModel(String),
    #[error("model is not installed: {0}")]
    MissingModel(String),
    #[error("model verification failed for {path}: expected {expected}, got {actual}")]
    BadChecksum {
        path: PathBuf,
        expected: String,
        actual: String,
    },
    #[error("ONNX inference failed: {0}")]
    Ort(String),
    #[error("unexpected model output shape: {0:?}")]
    OutputShape(Vec<usize>),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModelCatalog {
    pub models: Vec<ModelEntry>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModelEntry {
    pub id: String,
    pub name: String,
    pub version: String,
    pub filename: String,
    pub sha256: String,
    pub size: u64,
    pub install: ModelInstall,
    pub preprocess: PreprocessKind,
    pub license_name: String,
    pub license_url: String,
    pub source_url: String,
    pub upstream_model_url: String,
    pub redistribution_basis: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModelInstall {
    pub method: ModelInstallMethod,
    pub url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ModelInstallMethod {
    DirectMirror,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PreprocessKind {
    Informative,
    #[serde(rename = "anime2sketch")]
    Anime2Sketch,
    AnilinesBasic,
    AnilinesDetail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineartModel {
    Informative,
    Anime2Sketch,
    AnilinesBasic,
    AnilinesDetail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineCleanupPreset {
    Balanced,
    Delicate,
    Clean,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelLocation {
    AppFolder,
    Installed,
    LocalFile,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelStatus {
    Available {
        path: PathBuf,
        location: ModelLocation,
    },
    Missing,
    Corrupt {
        path: PathBuf,
        expected: String,
        actual: String,
    },
}

#[derive(Debug, Clone)]
pub struct ModelAvailability {
    pub entry: ModelEntry,
    pub status: ModelStatus,
}

#[derive(Debug, Clone)]
pub struct ModelManager {
    catalog: ModelCatalog,
    roots: Vec<ModelRoot>,
    download_dir: Option<PathBuf>,
}

#[derive(Debug, Clone)]
struct ModelRoot {
    location: ModelLocation,
    path: PathBuf,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DownloadProgress {
    pub downloaded: u64,
    pub total: u64,
}

pub fn bundled_catalog() -> Result<ModelCatalog, LineartError> {
    Ok(serde_json::from_str(CATALOG_JSON)?)
}

impl LineartModel {
    pub const ALL: [Self; 4] = [
        Self::Informative,
        Self::Anime2Sketch,
        Self::AnilinesBasic,
        Self::AnilinesDetail,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::Informative => "informative",
            Self::Anime2Sketch => "anime2sketch",
            Self::AnilinesBasic => "anilines-basic",
            Self::AnilinesDetail => "anilines-detail",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Informative => "Informative",
            Self::Anime2Sketch => "Anime2Sketch",
            Self::AnilinesBasic => "AniLines Basic",
            Self::AnilinesDetail => "AniLines Detail",
        }
    }
}

impl LineCleanupPreset {
    pub const ALL: [Self; 3] = [Self::Balanced, Self::Delicate, Self::Clean];

    pub fn label(self) -> &'static str {
        match self {
            Self::Balanced => "Balanced",
            Self::Delicate => "Delicate",
            Self::Clean => "Clean",
        }
    }

    pub fn note(self) -> &'static str {
        match self {
            Self::Balanced => "Default 1px cleanup for most AI line art.",
            Self::Delicate => "Keeps faint lines; can keep more noise.",
            Self::Clean => "Stronger denoise; can drop weak details.",
        }
    }
}

impl ModelStatus {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Available {
                location: ModelLocation::AppFolder,
                ..
            } => "App folder",
            Self::Available {
                location: ModelLocation::Installed,
                ..
            } => "Installed",
            Self::Available {
                location: ModelLocation::LocalFile,
                ..
            } => "Local file",
            Self::Missing => "Not installed",
            Self::Corrupt { .. } => "Needs repair",
        }
    }

    pub fn is_available(&self) -> bool {
        matches!(self, Self::Available { .. })
    }
}

impl ModelCatalog {
    pub fn entry(&self, id: &str) -> Option<&ModelEntry> {
        self.models.iter().find(|model| model.id == id)
    }
}

impl ModelManager {
    pub fn new() -> Result<Self, LineartError> {
        let catalog = bundled_catalog()?;
        let app_model_dir = app_model_dir();
        let mut roots = Vec::new();
        if let Some(path) = app_model_dir.clone() {
            roots.push(ModelRoot {
                location: ModelLocation::AppFolder,
                path,
            });
        }
        add_local_model_roots(&mut roots);
        Ok(Self {
            catalog,
            roots,
            download_dir: app_model_dir,
        })
    }

    pub fn catalog(&self) -> &ModelCatalog {
        &self.catalog
    }

    pub fn preferred_download_dir(&self) -> Option<&Path> {
        self.download_dir.as_deref()
    }

    pub fn availability(&self) -> Vec<ModelAvailability> {
        self.catalog
            .models
            .iter()
            .cloned()
            .map(|entry| {
                let status = self.status_for(&entry);
                ModelAvailability { entry, status }
            })
            .collect()
    }

    pub fn entry_for_model(&self, model: LineartModel) -> Result<&ModelEntry, LineartError> {
        self.catalog
            .entry(model.id())
            .ok_or_else(|| LineartError::UnknownModel(model.id().to_owned()))
    }

    pub fn status_for_model(&self, model: LineartModel) -> Result<ModelStatus, LineartError> {
        Ok(self.status_for(self.entry_for_model(model)?))
    }

    pub fn path_for_model(&self, model: LineartModel) -> Result<PathBuf, LineartError> {
        let entry = self.entry_for_model(model)?;
        match self.status_for(entry) {
            ModelStatus::Available { path, .. } => Ok(path),
            ModelStatus::Corrupt {
                path,
                expected,
                actual,
            } => Err(LineartError::BadChecksum {
                path,
                expected,
                actual,
            }),
            ModelStatus::Missing => Err(LineartError::MissingModel(entry.name.clone())),
        }
    }

    pub fn download_model<F>(
        &self,
        model: LineartModel,
        mut progress: F,
    ) -> Result<PathBuf, LineartError>
    where
        F: FnMut(DownloadProgress),
    {
        let entry = self.entry_for_model(model)?.clone();
        let download_dir = self.prepare_download_dir()?;
        let final_path = download_dir.join(&entry.filename);
        let part_path = download_dir.join(format!("{}.part", entry.filename));
        let _ = fs::remove_file(&part_path);

        let client = reqwest::blocking::Client::builder()
            .user_agent(USER_AGENT)
            .build()?;
        let mut response = client.get(&entry.install.url).send()?.error_for_status()?;
        let total = response.content_length().unwrap_or(entry.size);
        progress(DownloadProgress {
            downloaded: 0,
            total,
        });

        let mut file = fs::File::create(&part_path)?;
        let mut downloaded = 0u64;
        let mut buffer = [0u8; 1024 * 128];
        loop {
            let read = response.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            file.write_all(&buffer[..read])?;
            downloaded += read as u64;
            progress(DownloadProgress { downloaded, total });
        }
        file.flush()?;
        drop(file);

        let actual = sha256_file(&part_path)?;
        if actual != entry.sha256 {
            let _ = fs::remove_file(&part_path);
            return Err(LineartError::BadChecksum {
                path: part_path,
                expected: entry.sha256,
                actual,
            });
        }

        if final_path.exists() {
            fs::remove_file(&final_path)?;
        }
        fs::rename(&part_path, &final_path)?;
        Ok(final_path)
    }

    fn prepare_download_dir(&self) -> Result<PathBuf, LineartError> {
        let dir = self
            .download_dir
            .as_ref()
            .ok_or(LineartError::DownloadDirectoryUnavailable)?;
        prepare_writable_dir(dir).map_err(|source| LineartError::DownloadDirectory {
            path: dir.clone(),
            source,
        })?;
        Ok(dir.clone())
    }

    fn status_for(&self, entry: &ModelEntry) -> ModelStatus {
        let mut corrupt = None;
        for root in &self.roots {
            let path = root.path.join(&entry.filename);
            if !path.exists() {
                continue;
            }
            match sha256_file(&path) {
                Ok(actual) if actual == entry.sha256 => {
                    return ModelStatus::Available {
                        path,
                        location: root.location,
                    };
                }
                Ok(actual) => {
                    corrupt = Some(ModelStatus::Corrupt {
                        path,
                        expected: entry.sha256.clone(),
                        actual,
                    });
                }
                Err(_) => {
                    corrupt = Some(ModelStatus::Corrupt {
                        path,
                        expected: entry.sha256.clone(),
                        actual: "unreadable".to_owned(),
                    });
                }
            }
        }
        corrupt.unwrap_or(ModelStatus::Missing)
    }
}

pub struct LineartSession {
    model: LineartModel,
    session: Session,
}

impl LineartSession {
    pub fn new(model: LineartModel, path: impl AsRef<Path>) -> Result<Self, LineartError> {
        let session = Session::builder()
            .map_err(|error| LineartError::Ort(error.to_string()))?
            .with_optimization_level(ort::session::builder::GraphOptimizationLevel::Level3)
            .map_err(|error| LineartError::Ort(error.to_string()))?
            .commit_from_file(path)
            .map_err(|error| LineartError::Ort(error.to_string()))?;
        Ok(Self { model, session })
    }

    pub fn extract(&mut self, image: &DynamicImage) -> Result<GrayImage, LineartError> {
        let working = resize_rgb_preserving_aspect(image, 512);
        match self.model {
            LineartModel::Informative => self.run_dynamic_rgb(&working, false),
            LineartModel::Anime2Sketch => self.run_anime2sketch(&working),
            LineartModel::AnilinesBasic => self.run_dynamic_rgb(&working, true),
            LineartModel::AnilinesDetail => self.run_anilines_detail(&working),
        }
    }

    fn run_dynamic_rgb(
        &mut self,
        image: &RgbImage,
        sharpen: bool,
    ) -> Result<GrayImage, LineartError> {
        let input = if sharpen {
            sharpen_rgb(image, 5.0)
        } else {
            image.clone()
        };
        let (padded, width, height) = pad_rgb_to_multiple_of_eight(&input);
        let tensor = rgb_to_tensor_01(&padded);
        let output = self.run_tensor(&tensor)?;
        tensor_to_gray(&output, false, width, height)
    }

    fn run_anime2sketch(&mut self, image: &RgbImage) -> Result<GrayImage, LineartError> {
        let (square, offset_x, offset_y, content_width, content_height) = letterbox_rgb(image, 512);
        let tensor = rgb_to_tensor_minus_one_to_one(&square);
        let output = self.run_tensor(&tensor)?;
        let gray = tensor_to_gray(&output, true, 512, 512)?;
        let cropped =
            image::imageops::crop_imm(&gray, offset_x, offset_y, content_width, content_height)
                .to_image();
        Ok(image::imageops::resize(
            &cropped,
            image.width(),
            image.height(),
            FilterType::CatmullRom,
        ))
    }

    fn run_anilines_detail(&mut self, image: &RgbImage) -> Result<GrayImage, LineartError> {
        let gray = rgb_to_gray(image);
        let sobel = inverted_sobel(&gray);
        let (gray, sobel, width, height) = pad_two_gray_to_multiple_of_eight(&gray, &sobel);
        let tensor = two_gray_to_tensor_01(&gray, &sobel);
        let output = self.run_tensor(&tensor)?;
        tensor_to_gray(&output, false, width, height)
    }

    fn run_tensor(&mut self, input: &Array4<f32>) -> Result<ndarray::ArrayD<f32>, LineartError> {
        let tensor = TensorRef::from_array_view(input)
            .map_err(|error| LineartError::Ort(error.to_string()))?;
        let outputs = self
            .session
            .run(ort::inputs![tensor])
            .map_err(|error| LineartError::Ort(error.to_string()))?;
        Ok(outputs[0]
            .try_extract_array::<f32>()
            .map_err(|error| LineartError::Ort(error.to_string()))?
            .to_owned())
    }
}

pub fn sha256_file(path: impl AsRef<Path>) -> Result<String, std::io::Error> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 1024 * 128];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn app_model_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(exe.parent()?.join("models"))
}

fn prepare_writable_dir(dir: &Path) -> Result<(), std::io::Error> {
    fs::create_dir_all(dir)?;
    let probe = dir.join(".aa-converter-write-test");
    {
        let mut file = fs::File::create(&probe)?;
        file.write_all(b"ok")?;
        file.flush()?;
    }
    let _ = fs::remove_file(probe);
    Ok(())
}

#[cfg(debug_assertions)]
fn add_local_model_roots(roots: &mut Vec<ModelRoot>) {
    roots.push(ModelRoot {
        location: ModelLocation::LocalFile,
        path: PathBuf::from("target/research/ai-lineart/models"),
    });
    roots.push(ModelRoot {
        location: ModelLocation::LocalFile,
        path: PathBuf::from("assets/models"),
    });
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    roots.push(ModelRoot {
        location: ModelLocation::LocalFile,
        path: manifest_dir.join("../../target/research/ai-lineart/models"),
    });
    roots.push(ModelRoot {
        location: ModelLocation::LocalFile,
        path: manifest_dir.join("../../assets/models"),
    });
}

#[cfg(not(debug_assertions))]
fn add_local_model_roots(_roots: &mut Vec<ModelRoot>) {}

fn resize_rgb_preserving_aspect(image: &DynamicImage, max_side: u32) -> RgbImage {
    let rgb = image.to_rgb8();
    let width = rgb.width();
    let height = rgb.height();
    let longest = width.max(height).max(1);
    if longest <= max_side {
        return rgb;
    }
    let scale = max_side as f32 / longest as f32;
    let next_width = (width as f32 * scale).round().max(1.0) as u32;
    let next_height = (height as f32 * scale).round().max(1.0) as u32;
    image::imageops::resize(&rgb, next_width, next_height, FilterType::CatmullRom)
}

fn pad_rgb_to_multiple_of_eight(input: &RgbImage) -> (RgbImage, u32, u32) {
    let width = input.width();
    let height = input.height();
    let padded_width = width.div_ceil(8) * 8;
    let padded_height = height.div_ceil(8) * 8;
    let mut output = RgbImage::new(padded_width, padded_height);
    for y in 0..padded_height {
        for x in 0..padded_width {
            let sx = x.min(width - 1);
            let sy = y.min(height - 1);
            output.put_pixel(x, y, *input.get_pixel(sx, sy));
        }
    }
    (output, width, height)
}

fn pad_two_gray_to_multiple_of_eight(
    first: &GrayImage,
    second: &GrayImage,
) -> (GrayImage, GrayImage, u32, u32) {
    let width = first.width();
    let height = first.height();
    let padded_width = width.div_ceil(8) * 8;
    let padded_height = height.div_ceil(8) * 8;
    let mut first_out = GrayImage::new(padded_width, padded_height);
    let mut second_out = GrayImage::new(padded_width, padded_height);
    for y in 0..padded_height {
        for x in 0..padded_width {
            let sx = x.min(width - 1);
            let sy = y.min(height - 1);
            first_out.put_pixel(x, y, *first.get_pixel(sx, sy));
            second_out.put_pixel(x, y, *second.get_pixel(sx, sy));
        }
    }
    (first_out, second_out, width, height)
}

fn letterbox_rgb(input: &RgbImage, size: u32) -> (RgbImage, u32, u32, u32, u32) {
    let width = input.width();
    let height = input.height();
    let scale = size as f32 / width.max(height).max(1) as f32;
    let content_width = (width as f32 * scale).round().clamp(1.0, size as f32) as u32;
    let content_height = (height as f32 * scale).round().clamp(1.0, size as f32) as u32;
    let resized =
        image::imageops::resize(input, content_width, content_height, FilterType::CatmullRom);
    let offset_x = (size - content_width) / 2;
    let offset_y = (size - content_height) / 2;
    let mut output = RgbImage::from_pixel(size, size, Rgb([255, 255, 255]));
    image::imageops::overlay(&mut output, &resized, offset_x.into(), offset_y.into());
    (output, offset_x, offset_y, content_width, content_height)
}

fn rgb_to_tensor_01(image: &RgbImage) -> Array4<f32> {
    let mut input = Array4::<f32>::zeros((1, 3, image.height() as usize, image.width() as usize));
    for y in 0..image.height() {
        for x in 0..image.width() {
            let pixel = image.get_pixel(x, y);
            for channel in 0..3 {
                input[[0, channel, y as usize, x as usize]] = pixel[channel] as f32 / 255.0;
            }
        }
    }
    input
}

fn rgb_to_tensor_minus_one_to_one(image: &RgbImage) -> Array4<f32> {
    rgb_to_tensor_01(image).mapv(|value| (value - 0.5) / 0.5)
}

fn two_gray_to_tensor_01(first: &GrayImage, second: &GrayImage) -> Array4<f32> {
    let mut input = Array4::<f32>::zeros((1, 2, first.height() as usize, first.width() as usize));
    for y in 0..first.height() {
        for x in 0..first.width() {
            input[[0, 0, y as usize, x as usize]] = first.get_pixel(x, y)[0] as f32 / 255.0;
            input[[0, 1, y as usize, x as usize]] = second.get_pixel(x, y)[0] as f32 / 255.0;
        }
    }
    input
}

fn tensor_to_gray(
    output: &ndarray::ArrayD<f32>,
    anime2sketch: bool,
    width: u32,
    height: u32,
) -> Result<GrayImage, LineartError> {
    let shape = output.shape();
    if shape.len() != 4 || shape[0] != 1 || shape[1] != 1 {
        return Err(LineartError::OutputShape(shape.to_vec()));
    }
    let out_height = shape[2] as u32;
    let out_width = shape[3] as u32;
    let mut image = GrayImage::new(width.min(out_width), height.min(out_height));
    for y in 0..image.height() {
        for x in 0..image.width() {
            let mut value = output[[0, 0, y as usize, x as usize]];
            if anime2sketch {
                value = (value + 1.0) / 2.0;
            }
            image.put_pixel(x, y, Luma([(value.clamp(0.0, 1.0) * 255.0).round() as u8]));
        }
    }
    Ok(image)
}

fn rgb_to_gray(image: &RgbImage) -> GrayImage {
    let mut gray = GrayImage::new(image.width(), image.height());
    for y in 0..image.height() {
        for x in 0..image.width() {
            let pixel = image.get_pixel(x, y);
            let value =
                (0.299 * pixel[0] as f32 + 0.587 * pixel[1] as f32 + 0.114 * pixel[2] as f32)
                    .round()
                    .clamp(0.0, 255.0) as u8;
            gray.put_pixel(x, y, Luma([value]));
        }
    }
    gray
}

fn inverted_sobel(gray: &GrayImage) -> GrayImage {
    let mut output = GrayImage::new(gray.width(), gray.height());
    let mut magnitudes = vec![0.0f32; (gray.width() * gray.height()) as usize];
    let mut max_magnitude = 0.0f32;
    for y in 0..gray.height() {
        for x in 0..gray.width() {
            let gx = -sample_gray(gray, x as i32 - 1, y as i32 - 1)
                + sample_gray(gray, x as i32 + 1, y as i32 - 1)
                - 2.0 * sample_gray(gray, x as i32 - 1, y as i32)
                + 2.0 * sample_gray(gray, x as i32 + 1, y as i32)
                - sample_gray(gray, x as i32 - 1, y as i32 + 1)
                + sample_gray(gray, x as i32 + 1, y as i32 + 1);
            let gy = -sample_gray(gray, x as i32 - 1, y as i32 - 1)
                - 2.0 * sample_gray(gray, x as i32, y as i32 - 1)
                - sample_gray(gray, x as i32 + 1, y as i32 - 1)
                + sample_gray(gray, x as i32 - 1, y as i32 + 1)
                + 2.0 * sample_gray(gray, x as i32, y as i32 + 1)
                + sample_gray(gray, x as i32 + 1, y as i32 + 1);
            let magnitude = (gx * gx + gy * gy).sqrt();
            let idx = (y * gray.width() + x) as usize;
            magnitudes[idx] = magnitude;
            max_magnitude = max_magnitude.max(magnitude);
        }
    }

    for y in 0..gray.height() {
        for x in 0..gray.width() {
            let idx = (y * gray.width() + x) as usize;
            let normalized = if max_magnitude > 0.0 {
                magnitudes[idx] / max_magnitude
            } else {
                0.0
            };
            let value = ((1.0 - normalized).clamp(0.0, 1.0) * 255.0).round() as u8;
            output.put_pixel(x, y, Luma([value]));
        }
    }
    output
}

fn sample_gray(gray: &GrayImage, x: i32, y: i32) -> f32 {
    let x = x.clamp(0, gray.width() as i32 - 1) as u32;
    let y = y.clamp(0, gray.height() as i32 - 1) as u32;
    gray.get_pixel(x, y)[0] as f32
}

fn sharpen_rgb(image: &RgbImage, amount: f32) -> RgbImage {
    let blurred = image::imageops::blur(image, 1.0);
    let mut output = image.clone();
    for y in 0..image.height() {
        for x in 0..image.width() {
            let source = image.get_pixel(x, y);
            let blur = blurred.get_pixel(x, y);
            let mut next = [0u8; 3];
            for channel in 0..3 {
                let value = source[channel] as f32
                    + (source[channel] as f32 - blur[channel] as f32) * amount;
                next[channel] = value.round().clamp(0.0, 255.0) as u8;
            }
            output.put_pixel(x, y, Rgb(next));
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_catalog_contains_expected_models() {
        let catalog = bundled_catalog().unwrap();
        assert!(catalog.entry("informative").is_some());
        assert!(catalog.entry("anime2sketch").is_some());
        assert!(catalog.entry("anilines-basic").is_some());
        assert!(catalog.entry("anilines-detail").is_some());
    }

    #[test]
    fn bundled_catalog_contains_install_metadata() {
        let catalog = bundled_catalog().unwrap();
        for entry in &catalog.models {
            assert_eq!(entry.install.method, ModelInstallMethod::DirectMirror);
            assert!(entry.install.url.starts_with(
                "https://github.com/BK927/Ascii-Art-Converter/releases/download/third-party-models-v1/"
            ));
            assert!(entry.install.url.ends_with(&entry.filename));
            assert!(!entry.license_url.is_empty());
            assert!(!entry.source_url.is_empty());
            assert!(!entry.upstream_model_url.is_empty());
            assert!(!entry.redistribution_basis.is_empty());
        }
    }

    #[test]
    fn status_labels_are_product_facing() {
        assert_eq!(
            ModelStatus::Available {
                path: PathBuf::from("model.onnx"),
                location: ModelLocation::AppFolder
            }
            .label(),
            "App folder"
        );
        assert_eq!(
            ModelStatus::Available {
                path: PathBuf::from("model.onnx"),
                location: ModelLocation::Installed
            }
            .label(),
            "Installed"
        );
        assert_eq!(
            ModelStatus::Available {
                path: PathBuf::from("model.onnx"),
                location: ModelLocation::LocalFile
            }
            .label(),
            "Local file"
        );
        assert_eq!(ModelStatus::Missing.label(), "Not installed");
        assert_eq!(
            ModelStatus::Corrupt {
                path: PathBuf::from("model.onnx"),
                expected: "expected".to_owned(),
                actual: "actual".to_owned(),
            }
            .label(),
            "Needs repair"
        );
    }

    #[cfg(debug_assertions)]
    #[test]
    fn debug_build_detects_local_models_when_present() {
        let manager = ModelManager::new().unwrap();
        let availability = manager.availability();
        assert_eq!(availability.len(), 4);
        for item in availability {
            if Path::new("target/research/ai-lineart/models")
                .join(&item.entry.filename)
                .exists()
            {
                assert!(item.status.is_available());
            }
        }
    }

    #[test]
    fn app_folder_is_the_only_download_location() {
        let manager = ModelManager::new().unwrap();
        assert_eq!(manager.preferred_download_dir(), app_model_dir().as_deref());
        assert!(
            !manager
                .roots
                .iter()
                .any(|root| root.location == ModelLocation::Installed)
        );
    }

    #[cfg(not(debug_assertions))]
    #[test]
    fn release_build_omits_local_model_roots() {
        let manager = ModelManager::new().unwrap();
        assert!(
            !manager
                .roots
                .iter()
                .any(|root| root.location == ModelLocation::LocalFile)
        );
    }

    #[test]
    fn available_models_smoke_test() {
        let manager = ModelManager::new().unwrap();
        let image = DynamicImage::ImageRgb8(image::ImageBuffer::from_fn(96, 128, |x, y| {
            let value = ((x + y) % 255) as u8;
            Rgb([value, 255 - value, value / 2])
        }));
        for model in LineartModel::ALL {
            let Ok(path) = manager.path_for_model(model) else {
                continue;
            };
            let mut session = LineartSession::new(model, path).unwrap();
            let lineart = session.extract(&image).unwrap();
            assert!(lineart.width() > 0);
            assert!(lineart.height() > 0);
        }
    }

    #[test]
    fn temporary_hash_is_stable() {
        let mut path = std::env::temp_dir();
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        path.push(format!("aa-lineart-{stamp}.bin"));
        fs::write(&path, b"abc").unwrap();
        assert_eq!(
            sha256_file(&path).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let _ = fs::remove_file(path);
    }
}
