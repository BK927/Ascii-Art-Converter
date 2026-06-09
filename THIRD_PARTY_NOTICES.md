# Third-Party Notices

AA Converter can use third-party AI line extraction models when they are
installed through the model manager.

The app does not bundle these model files in the desktop package or web bundle.
Instead, it installs optional models from an AA Converter third-party model
mirror:

https://github.com/BK927/Ascii-Art-Converter/releases/tag/third-party-models-v1

This mirror is a convenience cache for verified installation UX. The models are
not authored by AA Converter. Before publishing any new mirror release or
changing the exact files listed below, verify that the exact model weights and
converted ONNX files may be redistributed for the intended use.

## Informative Drawings Line Art

- Source: https://github.com/josephrocca/image-to-line-art-js
- Upstream model reference: https://huggingface.co/rocca/informative-drawings-line-art-onnx
- License: MIT
- License reference: https://github.com/josephrocca/image-to-line-art-js/blob/main/LICENSE
- Redistribution basis: MIT-licensed integration repository mirrors an ONNX
  conversion that is publicly hosted on Hugging Face; attribution is preserved
  here and in the model mirror release notes.
- Model filename: `informative-drawings-line-art.onnx`
- SHA256: `1fef40b8f7126d827e30fbebccf95ae9b0b391795df926bf9366a821bad4f498`

## Anime2Sketch

- Source: https://github.com/Mukosame/Anime2Sketch
- Upstream model reference: https://drive.google.com/drive/folders/1Srf-WYUixK0wiUddc9y3pNKHHno5PN6R?usp=sharing
- License: MIT
- License reference: https://github.com/Mukosame/Anime2Sketch/blob/master/LICENSE
- Redistribution basis: upstream README describes the repository as containing
  testing code and pretrained weights, provides model weight links, and releases
  the project under MIT.
- Model filename: `anime2sketch-default-512.onnx`
- SHA256: `26453ebc688c9b2fae4128b3d4f92d74a685af1d0d460e30dc069d053ea5d9c2`

## AniLines

- Source: https://github.com/zhenglinpan/AniLines-Anime-Lineart-Extractor
- License: MIT
- License reference: https://github.com/zhenglinpan/AniLines-Anime-Lineart-Extractor/blob/master/LICENSE
- Redistribution basis: upstream README provides pretrained Basic and Detail
  model links and states the project is licensed under MIT. The mirror hosts
  converted ONNX artifacts with attribution.
- Model filenames:
  - `anilines-basic-dynamic.onnx`
    - Upstream model reference: https://drive.google.com/file/d/14Bp8mbQAbiR1rQrEsFp-uNdOou8hoCFr/view?usp=sharing
    - SHA256: `1b674ae9093e71e49289aca730e7fa3e2edd7b7bad5c23786c3a3b061d9a6f09`
  - `anilines-detail-dynamic.onnx`
    - Upstream model reference: https://drive.google.com/file/d/12U1Mwlonoipk2Yvr12mNaFB30foy420o/view?usp=sharing
    - SHA256: `2fee16b1756a7dd22f367035a7e29736da7149a90c0101aa5ce201fc2d53b5f4`

## Saitamaar

- Source: bundled under `assets/fonts/`
- License: SIL Open Font License, see `assets/fonts/Saitamaar-OFL.txt`
