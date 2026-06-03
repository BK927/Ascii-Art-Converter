# Implementation Notes

AA Converter is not a bit-exact reproduction of the IEEE Access paper. It is an
independent implementation inspired by the paper's main ideas and by related
line-drawing and skeletonization work.

## Fidelity Notes

The original paper does not publish every implementation detail needed for
exact figure matching, including:

- the exact 752-character list and ordering
- all preprocessing parameters used for the published figures
- some blank-filling behavior
- tie-breaking behavior in placement
- exact source crops for the paper figures

The implementation is intentionally organized so that these pieces can be
swapped independently:

- structure-line extraction (`ETF/FDoG`-style and Scharr alternatives)
- thinning mode
- glyph set and font profile
- glyph scoring weights
- placement algorithm

## Current Practical Default

`paper-greedy` remains the practical default. Several experimental variants are
kept in the benchmark runner, but visual review so far suggests that many
metric improvements are not strong enough to replace the stable baseline.

## Color Images

The `Illustration` preset is experimental. It adds color-boundary detection to
the structure-line stage, but it still renders monochrome ASCII art. It should
not be treated as a mature color illustration converter yet.
