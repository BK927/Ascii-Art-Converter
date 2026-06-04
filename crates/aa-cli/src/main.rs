use std::path::PathBuf;

use aa_core::benchmark::{BenchmarkAlgorithm, BenchmarkRunOptions, run_benchmark};
use aa_core::{
    AsciiConfig, InputMode, anime_sketch_paper_preset, color_illustration_preset,
    find_default_font, find_paper_font, paper_preset, save_stage_bundle, soft_grid_preset,
};

fn main() {
    if let Err(err) = run() {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|arg| arg == "bench") {
        return run_bench(&args[1..]);
    }
    run_convert(args.into_iter())
}

fn run_convert(mut args: impl Iterator<Item = String>) -> Result<(), String> {
    let mut input = None;
    let mut output = None;
    let mut font = None;
    let mut preset = "paper".to_owned();
    let mut input_mode = None;
    let mut binary_threshold = None;
    let mut edge_threshold = None;
    let mut score_cutoff = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--input" => input = args.next().map(PathBuf::from),
            "--out" => output = args.next().map(PathBuf::from),
            "--font" => font = args.next().map(PathBuf::from),
            "--preset" => preset = args.next().unwrap_or_else(|| "paper".to_owned()),
            "--input-mode" => input_mode = args.next(),
            "--binary-threshold" => {
                binary_threshold = parse_next_f32(&mut args, "--binary-threshold")?
            }
            "--edge-threshold" => edge_threshold = parse_next_f32(&mut args, "--edge-threshold")?,
            "--score-cutoff" => score_cutoff = parse_next_f32(&mut args, "--score-cutoff")?,
            "--help" | "-h" => {
                print_help();
                return Ok(());
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    let input = input.ok_or("missing --input")?;
    let output = output.ok_or("missing --out")?;
    let font = font
        .or_else(find_paper_font)
        .or_else(find_default_font)
        .ok_or("no font found; pass --font")?;
    let font_bytes = std::fs::read(&font).map_err(|err| err.to_string())?;

    let mut config = match preset.as_str() {
        "paper" => paper_preset(&font_bytes).map_err(|err| err.to_string())?,
        "color" => color_illustration_preset(&font_bytes).map_err(|err| err.to_string())?,
        "ai-sketch" | "anime-sketch" => {
            anime_sketch_paper_preset(&font_bytes).map_err(|err| err.to_string())?
        }
        "soft-grid" | "b2" => soft_grid_preset(&font_bytes).map_err(|err| err.to_string())?,
        "default" => AsciiConfig::default(),
        other => return Err(format!("unknown preset: {other}")),
    };
    if let Some(input_mode) = input_mode {
        config.input_mode = match input_mode.as_str() {
            "binary" => InputMode::TreatAsBinaryLines,
            "soft" => InputMode::TreatAsSoftLines,
            "structure" => InputMode::ExtractStructureLines,
            other => return Err(format!("unknown input mode: {other}")),
        };
    }
    if let Some(binary_threshold) = binary_threshold {
        config.binary_threshold = binary_threshold;
    }
    if let Some(edge_threshold) = edge_threshold {
        config.edge_threshold = edge_threshold;
    }
    if let Some(score_cutoff) = score_cutoff {
        config.score_cutoff = score_cutoff;
    }

    let result = aa_core::convert_path(&input, &font, &config).map_err(|err| err.to_string())?;
    save_stage_bundle(&result, &output).map_err(|err| err.to_string())?;

    println!(
        "converted {} -> {} using {}",
        input.display(),
        output.display(),
        font.display()
    );
    println!("{}", aa_core::result_metrics(&result));
    Ok(())
}

fn run_bench(args: &[String]) -> Result<(), String> {
    if args.first().is_none_or(|arg| arg != "run") {
        print_bench_help();
        return Ok(());
    }

    let mut manifest = None;
    let mut output = None;
    let mut font_profile = "saitamaar-16".to_owned();
    let mut font = None;
    let mut font_license = None;
    let mut algorithms = None;

    let mut iter = args.iter().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--manifest" => manifest = iter.next().map(PathBuf::from),
            "--out" => output = iter.next().map(PathBuf::from),
            "--font-profile" => {
                font_profile = iter
                    .next()
                    .cloned()
                    .ok_or("missing value for --font-profile")?
            }
            "--font" => font = iter.next().map(PathBuf::from),
            "--font-license" => font_license = iter.next().map(PathBuf::from),
            "--algorithms" => {
                let value = iter.next().ok_or("missing value for --algorithms")?;
                algorithms = Some(parse_algorithms(value)?);
            }
            "--help" | "-h" => {
                print_bench_help();
                return Ok(());
            }
            other => return Err(format!("unknown bench argument: {other}")),
        }
    }

    let manifest = manifest.ok_or("missing --manifest")?;
    let output = output.ok_or("missing --out")?;
    let options = BenchmarkRunOptions {
        font_profile,
        custom_font: font,
        custom_font_license: font_license,
        algorithms: algorithms.unwrap_or_else(BenchmarkAlgorithm::default_suite),
    };
    let report = run_benchmark(&manifest, &output, &options).map_err(|err| err.to_string())?;
    println!(
        "benchmarked {} case(s) with {} algorithm(s)",
        report.cases.len(),
        report.algorithms.len()
    );
    println!("report: {}", output.join("report.json").display());
    println!("gallery: {}", output.join("index.html").display());
    println!("overview: {}", output.join("overview.html").display());
    Ok(())
}

fn print_help() {
    println!(
        "aa-cli --input <image> --out <directory> [--font <ttf/otf>] [--preset paper|color|ai-sketch|soft-grid|default] [--input-mode structure|binary|soft] [--binary-threshold N] [--edge-threshold N] [--score-cutoff N]\n\
         aa-cli bench run --manifest <manifest.json> --out <directory> [--font-profile saitamaar-16|noto-commercial-16|custom] [--font <ttf/otf>] [--font-license <txt>] [--algorithms density-grid,fixed-grid,left-to-right,paper-greedy,paper-greedy-kang,paper-greedy-kmm,paper-greedy-kang-kmm,paper-greedy-clean,paper-greedy-balanced,paper-greedy-pretty,paper-greedy-interval,paper-greedy-interval-clean,paper-greedy-interval-balanced,paper-greedy-postprune,paper-greedy-local-prune,illustration-current,illustration-density,illustration-density-prune,ours-current]"
    );
}

fn print_bench_help() {
    println!(
        "aa-cli bench run --manifest <manifest.json> --out <directory> [--font-profile saitamaar-16|noto-commercial-16|custom] [--font <ttf/otf>] [--font-license <txt>] [--algorithms density-grid,fixed-grid,left-to-right,paper-greedy,paper-greedy-kang,paper-greedy-kmm,paper-greedy-kang-kmm,paper-greedy-clean,paper-greedy-balanced,paper-greedy-pretty,paper-greedy-interval,paper-greedy-interval-clean,paper-greedy-interval-balanced,paper-greedy-postprune,paper-greedy-local-prune,illustration-current,illustration-density,illustration-density-prune,ours-current]\n\
         default algorithms: left-to-right,paper-greedy"
    );
}

fn parse_algorithms(value: &str) -> Result<Vec<BenchmarkAlgorithm>, String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::parse)
        .collect()
}

fn parse_next_f32(
    args: &mut impl Iterator<Item = String>,
    name: &str,
) -> Result<Option<f32>, String> {
    let value = args
        .next()
        .ok_or_else(|| format!("missing value for {name}"))?;
    value
        .parse::<f32>()
        .map(Some)
        .map_err(|err| format!("invalid value for {name}: {err}"))
}
