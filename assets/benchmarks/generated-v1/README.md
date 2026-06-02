# Generated Character Benchmark v1

This directory is the intended home for the commercial-safe character benchmark.
The benchmark runner expects a manifest at `manifest.json`; `manifest.example.json` shows the schema.

## Asset Policy

- Generate original, non-famous character images with `imagegen`.
- Keep a mix of color illustrations and line-art controls, because the converter must handle color-to-structure preprocessing.
- Use clean light backgrounds, no text, no watermark, and no known character likeness.
- Save generated images under `images/` as PNG files.
- Copy `manifest.example.json` to `manifest.json`, then fill in the real image paths, prompts, provenance, review notes, and ROI boxes.
- Keep prompt/provenance/license fields non-empty. The CLI rejects incomplete benchmark cases.

## v1 Starter Mix

- 2 line-art controls
- 4 color illustrations
- Coverage: eyes, face, dense hair, bust, full-body silhouette, hand/hair occlusion stress

## Run

```powershell
cargo run -p aa-cli -- bench run --manifest assets/benchmarks/generated-v1/manifest.json --out target/benchmarks/generated-v1
```

Open `target/benchmarks/generated-v1/index.html` to run blind A/B votes locally.
The default starter run compares `left-to-right` and `paper-greedy`. Use `--algorithms density-grid,fixed-grid,left-to-right,paper-greedy,ours-current` only when you want the slower full suite.
