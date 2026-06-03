# Algorithms

AA Converter keeps several algorithm variants in the benchmark runner. Most of
them are experiments or baselines; the practical default is still
`paper-greedy`.

## Main Algorithms

- `density-grid`: simple density baseline. It chooses glyphs mainly by matching
  local ink density.
- `fixed-grid`: fixed grid baseline with glyph score matching.
- `left-to-right`: sequential baseline using the same glyph score model as the
  paper-inspired variants.
- `paper-greedy`: current default. This follows the Chung/Kwon-style
  stripe-local greedy divide-and-conquer placement approach.
- `ours-current`: alias for the current recommended baseline candidate.

## Paper-Greedy Variants

- `paper-greedy-clean`: `paper-greedy` with dense/noisy glyphs pruned.
- `paper-greedy-balanced`: cleaner `paper-greedy` with a slightly more
  conservative score threshold.
- `paper-greedy-pretty`: sparse aesthetic variant.
- `paper-greedy-kang`: Kang ETF/FDoG-leaning preprocessing variant.
- `paper-greedy-kmm`: alternate paper/KMM stride profile.
- `paper-greedy-kang-kmm`: combined Kang-style preprocessing and KMM profile.

## Interval And Pruning Experiments

- `paper-greedy-interval`: interval-search placement over each stripe.
- `paper-greedy-interval-clean`: interval search with dense/noisy glyph
  pruning. It can score higher on automatic metrics, but visual differences
  from `paper-greedy` are often small.
- `paper-greedy-interval-balanced`: interval search with more conservative
  density.
- `paper-greedy-postprune`: post-placement support pruning experiment.
- `paper-greedy-local-prune`: post-placement local removal experiment.

## Notes

The benchmark keeps these variants so scoring changes and placement ideas can
be compared against the stable `paper-greedy` baseline. Automatic metrics can
reward denser line coverage even when the visual result does not feel better,
so human visual review remains important.
