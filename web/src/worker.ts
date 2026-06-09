import initWasm, { convert_rgba } from "../pkg/aa_wasm.js";

import { extractAiLineart } from "./lineartInference";
import type { AiLineart, RgbaSource } from "./lineartInference";
import { loadModelCatalog } from "./modelStore";
import type { ModelCatalog, ModelId } from "./modelStore";

type Preset = "clean" | "sensitive" | "color" | "soft" | "ai";
type LineExtractorId = "builtin" | ModelId;
type BuiltInInputMode = "structure" | "binary" | "soft";
type CleanupPreset = "balanced" | "delicate" | "clean";

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
  input_mode: "structure" | "binary" | "soft" | "ai";
  structure_line_mode: "flowdog";
  thinning_mode: "kmm" | "guo-hall";
  placement_mode: "paper-greedy" | "soft-grid";
  stroke_tolerance: boolean;
  min_component_pixels: number;
  short_branch_prune_px: number;
  character_set?: string;
}

interface ConversionSettings {
  preset: Preset;
  lineExtractor: LineExtractorId;
  inputMode: BuiltInInputMode;
  cleanupPreset: CleanupPreset;
  options: ConvertOptions;
}

interface ConvertRequest {
  type: "convert";
  id: number;
  baseUrl: string;
  imageRgba: Uint8Array;
  imageWidth: number;
  imageHeight: number;
  settings: ConversionSettings;
}

interface CompareJob {
  index: number;
  settings: ConversionSettings;
}

interface CompareRequest {
  type: "compare";
  id: number;
  baseUrl: string;
  imageRgba: Uint8Array;
  imageWidth: number;
  imageHeight: number;
  jobs: CompareJob[];
}

interface ConvertResult {
  text: string;
  width: number;
  height: number;
  ascii_rgba: Uint8Array | number[];
  line_rgba: Uint8Array | number[];
  orientation_rgba: Uint8Array | number[];
  ai_line_rgba?: Uint8Array | number[];
  ai_line_width?: number;
  ai_line_height?: number;
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

type WorkerRequest = ConvertRequest | CompareRequest;

let wasmReady: Promise<void> | null = null;
let fontBytes: Uint8Array | null = null;

self.addEventListener("message", (event: MessageEvent<WorkerRequest>) => {
  if (event.data.type === "convert") {
    void convert(event.data);
  } else {
    void compare(event.data);
  }
});

async function convert(message: ConvertRequest): Promise<void> {
  try {
    postStatus(message.id, "Loading engine");
    await ensureEngine(message.baseUrl);
    const catalog = await loadModelCatalog(message.baseUrl);

    postStatus(message.id, "Converting");
    const result = await convertSource(
      message.baseUrl,
      catalog,
      {
        width: message.imageWidth,
        height: message.imageHeight,
        rgba: message.imageRgba,
      },
      message.settings,
      undefined,
      (status) => postStatus(message.id, status),
    );

    postResult(message.id, "result", result);
  } catch (error) {
    postError(message.id, error);
  }
}

async function compare(message: CompareRequest): Promise<void> {
  const lineartCache = new Map<ModelId, AiLineart>();
  try {
    postStatus(message.id, "Loading engine");
    await ensureEngine(message.baseUrl);
    const catalog = await loadModelCatalog(message.baseUrl);
    const source = {
      width: message.imageWidth,
      height: message.imageHeight,
      rgba: message.imageRgba,
    };

    for (const job of message.jobs) {
      self.postMessage({ type: "compare-started", id: message.id, index: job.index });
      try {
        const result = await convertSource(
          message.baseUrl,
          catalog,
          source,
          job.settings,
          lineartCache,
          (status) =>
            self.postMessage({
              type: "compare-status",
              id: message.id,
              index: job.index,
              message: status,
            }),
        );
        postResult(message.id, "compare-result", result, job.index);
      } catch (error) {
        self.postMessage({
          type: "compare-error",
          id: message.id,
          index: job.index,
          error: error instanceof Error ? error.message : String(error),
        });
      }
    }
    self.postMessage({ type: "compare-done", id: message.id });
  } catch (error) {
    postError(message.id, error);
    self.postMessage({ type: "compare-done", id: message.id });
  }
}

async function convertSource(
  baseUrl: string,
  catalog: ModelCatalog,
  source: RgbaSource,
  settings: ConversionSettings,
  lineartCache: Map<ModelId, AiLineart> | undefined,
  onStatus: (message: string) => void,
): Promise<ConvertResult> {
  if (!fontBytes) {
    throw new Error("font did not load");
  }

  let input = source;
  let aiLineart: AiLineart | undefined;
  if (settings.lineExtractor !== "builtin") {
    onStatus(`Running ${settings.lineExtractor}`);
    aiLineart = await cachedAiLineart(baseUrl, catalog, settings.lineExtractor, source, lineartCache);
    input = aiLineart;
  }

  const rawResult = convert_rgba(
    input.rgba,
    input.width,
    input.height,
    fontBytes,
    settings.preset,
    settings.options,
  ) as ConvertResult;

  const result = normalizeResult(rawResult);
  if (aiLineart) {
    result.ai_line_rgba = aiLineart.rgba.slice();
    result.ai_line_width = aiLineart.width;
    result.ai_line_height = aiLineart.height;
  }
  return result;
}

async function cachedAiLineart(
  baseUrl: string,
  catalog: ModelCatalog,
  model: ModelId,
  source: RgbaSource,
  cache: Map<ModelId, AiLineart> | undefined,
): Promise<AiLineart> {
  const cached = cache?.get(model);
  if (cached) {
    return cached;
  }
  const lineart = await extractAiLineart(baseUrl, catalog, model, source);
  cache?.set(model, lineart);
  return lineart;
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

function normalizeResult(result: ConvertResult): ConvertResult {
  return {
    ...result,
    ascii_rgba: asUint8Array(result.ascii_rgba),
    line_rgba: asUint8Array(result.line_rgba),
    orientation_rgba: asUint8Array(result.orientation_rgba),
    ...(result.ai_line_rgba ? { ai_line_rgba: asUint8Array(result.ai_line_rgba) } : {}),
  };
}

function postResult(
  id: number,
  type: "result" | "compare-result",
  result: ConvertResult,
  index?: number,
): void {
  const transfer = [
    asUint8Array(result.ascii_rgba).buffer,
    asUint8Array(result.line_rgba).buffer,
    asUint8Array(result.orientation_rgba).buffer,
  ];
  if (result.ai_line_rgba) {
    transfer.push(asUint8Array(result.ai_line_rgba).buffer);
  }
  self.postMessage(
    {
      type,
      id,
      ...(index === undefined ? {} : { index }),
      result,
    },
    transfer,
  );
}

function postStatus(id: number, message: string): void {
  self.postMessage({
    type: "status",
    id,
    message,
  });
}

function postError(id: number, error: unknown): void {
  self.postMessage({
    type: "error",
    id,
    error: error instanceof Error ? error.message : String(error),
  });
}

function asUint8Array(value: Uint8Array | number[]): Uint8Array {
  return value instanceof Uint8Array ? value : new Uint8Array(value);
}
