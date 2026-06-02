# AA Converter

AA Converter is a Rust workspace for converting images into rendered ASCII art.
It focuses on character illustration and line-art inputs, commercial-safe font
profiles, and benchmarkable output rather than bit-for-bit reproduction of any
single paper figure.

The core implementation is inspired by the text-placement pipeline in Moonjun
Chung and Taesoo Kwon's 2022 IEEE Access paper, "Fast Text Placement Scheme for
ASCII Art Synthesis". The project also includes experimental variants that try
to improve the baseline placement quality for practical image-to-ASCII use.

Repository: <https://github.com/BK927/Ascii-Art-Converter>

## Workspace

- `crates/aa-core`: image preprocessing, glyph scoring, placement, rendering,
  and benchmark logic.
- `crates/aa-cli`: command-line conversion and benchmark runner.
- `crates/aa-egui`: desktop GUI prototype built with `egui`/`eframe`.
- `assets/fonts`: bundled font assets and license notices.
- `assets/benchmarks`: generated benchmark manifests and input images.

## Quick Start

Run the desktop GUI:

```powershell
cargo run -p aa-egui
```

Convert one image from the command line:

```powershell
cargo run -p aa-cli -- --input path\to\input.png --out target\aa-output
```

Use the paper-inspired preset explicitly:

```powershell
cargo run -p aa-cli -- --input path\to\input.png --out target\aa-output --preset paper
```

The output directory contains:

- `01-structure-lines.png`: extracted line image.
- `02-orientation-map.png`: orientation preview.
- `03-ascii-render.png`: rendered ASCII art image.
- `04-ascii.txt`: approximate text output.
- `metrics.txt`: pipeline timings and basic stats.

## Benchmark Runner

Run the starter benchmark:

```powershell
cargo run -p aa-cli -- bench run --manifest assets\benchmarks\generated-v1\manifest.json --out target\benchmarks\generated-v1
```

The benchmark writes:

- `report.json`: metrics and paths for all cases.
- `index.html`: blind A/B comparison gallery.
- `overview.html`: side-by-side gallery with algorithm names and metrics.
- Per-case stage bundles under `cases/`.

Example research comparison:

```powershell
cargo run -p aa-cli -- bench run --manifest assets\benchmarks\generated-v1\manifest.json --out target\benchmarks\generated-v1-interval --algorithms paper-greedy,paper-greedy-interval,paper-greedy-interval-clean,paper-greedy-interval-balanced
```

## Algorithms

The benchmark runner currently supports these algorithm IDs:

- `density-grid`: simple density baseline.
- `fixed-grid`: fixed grid with glyph score matching.
- `left-to-right`: sequential baseline using the same score model.
- `paper-greedy`: the main Chung/Kwon-style greedy divide-and-conquer
  placement baseline.
- `paper-greedy-clean`: `paper-greedy` with dense/noisy glyphs pruned.
- `paper-greedy-balanced`: cleaner `paper-greedy` with a slightly more
  conservative score threshold.
- `paper-greedy-pretty`: a sparse aesthetic variant.
- `paper-greedy-kang`: a Kang ETF/FDoG-leaning preprocessing variant.
- `paper-greedy-kmm`: an alternate paper/KMM stride profile.
- `paper-greedy-kang-kmm`: combined Kang-style preprocessing and KMM profile.
- `paper-greedy-interval`: interval-search placement over each stripe.
- `paper-greedy-interval-clean`: interval search with dense/noisy glyph
  pruning. This currently scores higher on several automatic metrics in the
  bundled generated-v1 benchmark, but it is not promoted as the default because
  the visual difference from `paper-greedy` is often small.
- `paper-greedy-interval-balanced`: interval search with more conservative
  density.
- `paper-greedy-postprune`: post-placement support pruning experiment.
- `paper-greedy-local-prune`: post-placement local removal experiment.
- `ours-current`: alias for the current recommended baseline candidate.

The default benchmark suite stays small (`left-to-right,paper-greedy`) so that
the starter benchmark finishes in a practical amount of time.

## Current Pipeline

1. Resize the input to a working width.
2. Extract a thin binary structure-line image from grayscale or binary input.
3. Optionally boost color-boundary edges for color illustrations.
4. Apply pre-thinning denoising.
5. Skeletonize with the current KMM/K3M-family lookup thinning pass.
6. Extract per-pixel orientation using Gaussian blur, Scharr gradients, and a
   local orientation window.
7. Rasterize the configured character set from the selected font.
8. Score each glyph with orientation-aware match and mismatch terms.
9. Place glyphs by the selected placement algorithm.
10. Render PNG/TXT output and benchmark stage bundles.

## Font And License Notes

The bundled `saitamaar-16` profile uses:

- `assets/fonts/Saitamaar-Regular.ttf`
- `assets/fonts/Saitamaar-OFL.txt`

Saitamaar is bundled with its OFL notice. For commercial or product use, keep
the font license file with any distributed font asset. The generated ASCII
documents/images are not automatically subject to the font license, but the
font file itself and modified font files are.

The benchmark runner also supports `noto-commercial-16` and `custom` profiles,
but official scoring requires an explicit font license file:

```powershell
cargo run -p aa-cli -- bench run --manifest assets\benchmarks\generated-v1\manifest.json --out target\benchmarks\custom --font-profile custom --font path\to\font.ttf --font-license path\to\LICENSE.txt
```

## Fidelity Notes

This project is not a bit-exact reproduction of the IEEE Access paper. The
paper does not publish every implementation detail needed for exact figure
matching, including the exact 752-character list/order, all preprocessing
parameters, and some blank-filling/tie-breaking behavior.

The implementation is intentionally organized so that these pieces can be
swapped independently:

- Structure-line extraction (`ETF/FDoG`-style and Scharr alternatives).
- Thinning mode.
- Glyph set and font profile.
- Glyph scoring weights.
- Placement algorithm.

## Implementation References

The ASCII-art logic in this repository was implemented with reference to these
papers:

- Moonjun Chung and Taesoo Kwon. "Fast Text Placement Scheme for ASCII Art
  Synthesis." IEEE Access, 10:40677-40686, 2022. DOI:
  `10.1109/ACCESS.2022.3167567`. This is the primary reference for the
  structure-line, feature extraction, glyph scoring, and greedy text-placement
  pipeline.
- Henry Kang, Seungyong Lee, and Charles K. Chui. "Coherent Line Drawing."
  NPAR 2007. DOI: `10.1145/1274871.1274878`. This is the reference for the
  Edge Tangent Flow / Flow-based Difference-of-Gaussians idea used by the
  structure-line extraction path.
- Khalid Saeed, Marek Tabedzki, Mariusz Rybnik, and Marcin Adamski. "K3M: A
  Universal Algorithm for Image Skeletonization and a Review of Thinning
  Techniques." International Journal of Applied Mathematics and Computer
  Science, 20(2):317-335, 2010. DOI: `10.2478/v10006-010-0024-4`. This is a
  reference for the K3M-family skeletonization/thinning approach.
- Khalid Saeed, Mariusz Rybnik, and Marek Tabedzki. "Implementation and
  Advanced Results on the Non-Interrupted Skeletonization Algorithm." This is
  a reference for the KMM skeletonization family discussed by Chung and Kwon.

## Development

Format and check:

```powershell
cargo fmt --check
cargo check -p aa-core
cargo check -p aa-cli
cargo check -p aa-egui
```

Run tests:

```powershell
cargo test
```

## License

The Rust workspace is licensed as `MIT OR Apache-2.0` in `Cargo.toml`.
Bundled third-party font assets keep their own license notices under
`assets/fonts/`.
