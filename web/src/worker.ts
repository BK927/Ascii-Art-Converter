import initWasm, { convert_rgba } from "../pkg/aa_wasm.js";

type Preset = "clean" | "sensitive" | "color";

interface ConvertOptions {
  max_width: number;
  font_px: number;
  stripe_px: number;
  blur: number;
  edge: number;
  binary: number;
  mismatch: number;
  match: number;
  cutoff: number;
  glyph_ink: number;
  character_set?: string;
}

interface ConvertRequest {
  type: "convert";
  id: number;
  baseUrl: string;
  imageRgba: Uint8Array;
  imageWidth: number;
  imageHeight: number;
  preset: Preset;
  options: ConvertOptions;
}

interface ConvertResult {
  text: string;
  width: number;
  height: number;
  ascii_rgba: Uint8Array | number[];
  line_rgba: Uint8Array | number[];
  orientation_rgba: Uint8Array | number[];
  stats: {
    input_width: number;
    input_height: number;
    working_width: number;
    working_height: number;
    stripes: number;
    glyphs: number;
    placed_glyphs: number;
    foreground_pixels: number;
  };
  timings: {
    preprocess_ms: number;
    feature_ms: number;
    glyph_analysis_ms: number;
    scoring_ms: number;
    placement_ms: number;
    rendering_ms: number;
    total_ms: number;
  };
}

let wasmReady: Promise<void> | null = null;
let fontBytes: Uint8Array | null = null;

self.addEventListener("message", (event: MessageEvent<ConvertRequest>) => {
  if (event.data.type !== "convert") {
    return;
  }
  void convert(event.data);
});

async function convert(message: ConvertRequest): Promise<void> {
  try {
    postStatus(message.id, "Loading engine");
    await ensureEngine(message.baseUrl);
    if (!fontBytes) {
      throw new Error("font did not load");
    }

    postStatus(message.id, "Converting");
    const rawResult = convert_rgba(
      message.imageRgba,
      message.imageWidth,
      message.imageHeight,
      fontBytes,
      message.preset,
      message.options,
    ) as ConvertResult;

    const result = {
      ...rawResult,
      ascii_rgba: asUint8Array(rawResult.ascii_rgba),
      line_rgba: asUint8Array(rawResult.line_rgba),
      orientation_rgba: asUint8Array(rawResult.orientation_rgba),
    };

    self.postMessage(
      {
        type: "result",
        id: message.id,
        result,
      },
      [
        result.ascii_rgba.buffer,
        result.line_rgba.buffer,
        result.orientation_rgba.buffer,
      ],
    );
  } catch (error) {
    self.postMessage({
      type: "error",
      id: message.id,
      error: error instanceof Error ? error.message : String(error),
    });
  }
}

async function ensureEngine(baseUrl: string): Promise<void> {
  if (!wasmReady) {
    wasmReady = initWasm().then(() => undefined);
  }
  await wasmReady;

  if (!fontBytes) {
    const response = await fetch(`${baseUrl}fonts/Saitamaar-Regular.ttf`);
    if (!response.ok) {
      throw new Error(`font download failed: ${response.status}`);
    }
    fontBytes = new Uint8Array(await response.arrayBuffer());
  }
}

function asUint8Array(value: Uint8Array | number[]): Uint8Array {
  return value instanceof Uint8Array ? value : new Uint8Array(value);
}

function postStatus(id: number, message: string): void {
  self.postMessage({
    type: "status",
    id,
    message,
  });
}
