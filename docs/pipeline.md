# Pipeline

This document describes the current AA Converter runtime pipeline in more
detail than the README.

## Conversion Steps

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

## Presets

### Clean Lines

The default GUI preset. It uses the bundled Saitamaar font, the paper-inspired
character set, structure-line extraction, KMM/K3M-family thinning, and
`paper-greedy` placement.

Best for clean black-and-white character line art.

### Sensitive

A more aggressive line-art preset. It lowers edge and glyph thresholds so faint
or thin lines are less likely to disappear. It can also pick up more noise.

Best for faint sketches, thin line art, and detail-heavy monochrome images.

### Color Edges

An experimental preset built on top of the paper-inspired pipeline. It keeps the
monochrome ASCII render output, but adds color-channel edge extraction before
placement.

Best for trying color illustrations where important contours are separated by
color rather than luminance. It is not a full color-ASCII renderer.

## Output Files

The CLI and stage export can write:

- `01-structure-lines.png`
- `02-orientation-map.png`
- `03-ascii-render.png`
- `04-ascii.txt`
- `metrics.txt`

The GUI also exposes direct copy/save actions for the rendered image and text.
