import "./styles.css";

type Preset = "clean" | "sensitive" | "color";

interface SourceImage {
  name: string;
  width: number;
  height: number;
  rgba: Uint8ClampedArray;
}

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

interface ConvertResult {
  text: string;
  width: number;
  height: number;
  ascii_rgba: Uint8Array | number[];
  line_rgba: Uint8Array | number[];
  orientation_rgba: Uint8Array | number[];
  stats: {
    working_width: number;
    working_height: number;
    glyphs: number;
    placed_glyphs: number;
    foreground_pixels: number;
  };
  timings: {
    total_ms: number;
  };
}

type WorkerMessage =
  | { type: "status"; id: number; message: string }
  | { type: "result"; id: number; result: ConvertResult }
  | { type: "error"; id: number; error: string };

const presetDefaults: Record<Preset, Record<string, string>> = {
  clean: {
    "max-width": "512",
    "font-px": "16",
    "stripe-px": "16",
    blur: "0.7",
    edge: "0.22",
    binary: "0.58",
    match: "1",
    mismatch: "0.65",
    cutoff: "0",
    "glyph-ink": "0.16",
  },
  sensitive: {
    "max-width": "512",
    "font-px": "16",
    "stripe-px": "16",
    blur: "0.65",
    edge: "0.2",
    binary: "0.56",
    match: "1.05",
    mismatch: "0.65",
    cutoff: "-4",
    "glyph-ink": "0.14",
  },
  color: {
    "max-width": "512",
    "font-px": "16",
    "stripe-px": "16",
    blur: "0.7",
    edge: "0.2",
    binary: "0.56",
    match: "1.05",
    mismatch: "0.65",
    cutoff: "-2",
    "glyph-ink": "0.16",
  },
};

const sliderSpecs = [
  { input: "max-width", output: "max-width-value", digits: 0 },
  { input: "font-px", output: "font-px-value", digits: 1 },
  { input: "stripe-px", output: "stripe-px-value", digits: 0 },
  { input: "blur", output: "blur-value", digits: 2 },
  { input: "edge", output: "edge-value", digits: 2 },
  { input: "binary", output: "binary-value", digits: 2 },
  { input: "match", output: "match-value", digits: 2 },
  { input: "mismatch", output: "mismatch-value", digits: 2 },
  { input: "cutoff", output: "cutoff-value", digits: 0 },
  { input: "glyph-ink", output: "glyph-ink-value", digits: 2 },
] as const;

const fileInput = element<HTMLInputElement>("file-input");
const dropzone = element<HTMLLabelElement>("dropzone");
const fileMeta = element<HTMLElement>("file-meta");
const statusEl = element<HTMLElement>("status");
const convertingOverlay = element<HTMLElement>("converting-overlay");
const convertingStage = element<HTMLElement>("converting-stage");
const convertButton = element<HTMLButtonElement>("convert");
const copyTextButton = element<HTMLButtonElement>("copy-text");
const downloadPngButton = element<HTMLButtonElement>("download-png");
const downloadTxtButton = element<HTMLButtonElement>("download-txt");
const charactersInput = element<HTMLTextAreaElement>("characters");
const originalCanvas = element<HTMLCanvasElement>("original-canvas");
const asciiCanvas = element<HTMLCanvasElement>("ascii-canvas");
const lineCanvas = element<HTMLCanvasElement>("line-canvas");
const orientationCanvas = element<HTMLCanvasElement>("orientation-canvas");
const originalSize = element<HTMLElement>("original-size");
const asciiSize = element<HTMLElement>("ascii-size");
const timingTotal = element<HTMLElement>("timing-total");
const statsList = element<HTMLElement>("stats-list");

let sourceImage: SourceImage | null = null;
let selectedPreset: Preset = "clean";
let worker: Worker | null = null;
let activeJobId = 0;
let busy = false;
let lastResult: ConvertResult | null = null;

syncSliderOutputs();
wireControls();
setCanvasEmpty(originalCanvas);
setCanvasEmpty(asciiCanvas);
setCanvasEmpty(lineCanvas);
setCanvasEmpty(orientationCanvas);

function wireControls(): void {
  fileInput.addEventListener("change", () => {
    const file = fileInput.files?.[0];
    if (file) {
      void loadFile(file);
    }
  });

  dropzone.addEventListener("dragover", (event) => {
    event.preventDefault();
    dropzone.classList.add("dragging");
  });
  dropzone.addEventListener("dragleave", () => dropzone.classList.remove("dragging"));
  dropzone.addEventListener("drop", (event) => {
    event.preventDefault();
    dropzone.classList.remove("dragging");
    const file = event.dataTransfer?.files[0];
    if (file) {
      void loadFile(file);
    }
  });

  for (const button of document.querySelectorAll<HTMLButtonElement>("[data-preset]")) {
    button.addEventListener("click", () => {
      const preset = button.dataset.preset as Preset;
      selectPreset(preset);
    });
  }

  for (const spec of sliderSpecs) {
    element<HTMLInputElement>(spec.input).addEventListener("input", syncSliderOutputs);
  }

  convertButton.addEventListener("click", () => void runConvert());
  copyTextButton.addEventListener("click", () => void copyText());
  downloadPngButton.addEventListener("click", () => downloadPng());
  downloadTxtButton.addEventListener("click", () => downloadTxt());
}

async function loadFile(file: File): Promise<void> {
  try {
    setStatus("Reading image");
    const bitmap = await createImageBitmap(file);
    const canvas = document.createElement("canvas");
    canvas.width = bitmap.width;
    canvas.height = bitmap.height;
    const context = requireContext(canvas);
    context.drawImage(bitmap, 0, 0);
    const data = context.getImageData(0, 0, bitmap.width, bitmap.height);
    bitmap.close();

    sourceImage = {
      name: file.name,
      width: data.width,
      height: data.height,
      rgba: data.data.slice(),
    };
    lastResult = null;

    renderRgba(originalCanvas, sourceImage.rgba, sourceImage.width, sourceImage.height);
    originalSize.textContent = `${sourceImage.width} x ${sourceImage.height}`;
    fileMeta.textContent = `${file.name} · ${sourceImage.width}x${sourceImage.height}`;
    setCanvasEmpty(asciiCanvas);
    setCanvasEmpty(lineCanvas);
    setCanvasEmpty(orientationCanvas);
    asciiSize.textContent = "-";
    timingTotal.textContent = "-";
    renderStats(null);
    setStatus("Ready");
    updateButtons();
  } catch (error) {
    setStatus(error instanceof Error ? error.message : String(error), true);
  }
}

async function runConvert(): Promise<void> {
  if (!sourceImage || busy) {
    return;
  }

  busy = true;
  lastResult = null;
  setStatus("Preparing image");
  setConvertingStage("Preparing image");
  setBusyVisual(true);
  updateButtons();
  const id = ++activeJobId;
  const imageRgba = new Uint8Array(sourceImage.rgba);

  ensureWorker().postMessage(
    {
      type: "convert",
      id,
      baseUrl: import.meta.env.BASE_URL,
      imageRgba,
      imageWidth: sourceImage.width,
      imageHeight: sourceImage.height,
      preset: selectedPreset,
      options: readOptions(),
    },
    [imageRgba.buffer],
  );
}

function handleWorkerMessage(event: MessageEvent<WorkerMessage>): void {
  if (event.data.id !== activeJobId) {
    return;
  }

  if (event.data.type === "status") {
    setStatus(event.data.message);
    setConvertingStage(event.data.message);
    return;
  }

  busy = false;
  if (event.data.type === "error") {
    setBusyVisual(false);
    setStatus(event.data.error, true);
    updateButtons();
    return;
  }

  lastResult = event.data.result;
  renderRgba(asciiCanvas, lastResult.ascii_rgba, lastResult.width, lastResult.height);
  renderRgba(lineCanvas, lastResult.line_rgba, lastResult.width, lastResult.stats.working_height);
  renderRgba(
    orientationCanvas,
    lastResult.orientation_rgba,
    lastResult.width,
    lastResult.stats.working_height,
  );
  asciiSize.textContent = `${lastResult.width} x ${lastResult.height}`;
  timingTotal.textContent = `${lastResult.timings.total_ms.toFixed(0)} ms`;
  renderStats(lastResult);
  setBusyVisual(false);
  setStatus("Converted");
  updateButtons();
}

function ensureWorker(): Worker {
  if (!worker) {
    worker = new Worker(new URL("./worker.ts", import.meta.url), { type: "module" });
    worker.addEventListener("message", handleWorkerMessage);
  }
  return worker;
}

function selectPreset(preset: Preset): void {
  selectedPreset = preset;
  for (const button of document.querySelectorAll<HTMLButtonElement>("[data-preset]")) {
    button.classList.toggle("selected", button.dataset.preset === preset);
  }
  for (const [id, value] of Object.entries(presetDefaults[preset])) {
    element<HTMLInputElement>(id).value = value;
  }
  syncSliderOutputs();
}

function readOptions(): ConvertOptions {
  const characters = charactersInput.value.trim();
  return {
    max_width: readNumber("max-width"),
    font_px: readNumber("font-px"),
    stripe_px: readNumber("stripe-px"),
    blur: readNumber("blur"),
    edge: readNumber("edge"),
    binary: readNumber("binary"),
    match: readNumber("match"),
    mismatch: readNumber("mismatch"),
    cutoff: readNumber("cutoff"),
    glyph_ink: readNumber("glyph-ink"),
    ...(characters ? { character_set: charactersInput.value } : {}),
  };
}

function syncSliderOutputs(): void {
  for (const spec of sliderSpecs) {
    const value = Number(element<HTMLInputElement>(spec.input).value);
    element<HTMLOutputElement>(spec.output).value = value.toFixed(spec.digits);
  }
}

async function copyText(): Promise<void> {
  if (!lastResult) {
    return;
  }
  try {
    await navigator.clipboard.writeText(lastResult.text);
    setStatus("Text copied");
  } catch (error) {
    setStatus(error instanceof Error ? error.message : String(error), true);
  }
}

function downloadPng(): void {
  if (!lastResult) {
    return;
  }
  asciiCanvas.toBlob((blob) => {
    if (blob) {
      downloadBlob(blob, `${outputBaseName()}-ascii.png`);
    }
  }, "image/png");
}

function downloadTxt(): void {
  if (!lastResult) {
    return;
  }
  downloadBlob(new Blob([lastResult.text], { type: "text/plain;charset=utf-8" }), `${outputBaseName()}-ascii.txt`);
}

function downloadBlob(blob: Blob, fileName: string): void {
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = fileName;
  anchor.click();
  URL.revokeObjectURL(url);
}

function renderRgba(
  canvas: HTMLCanvasElement,
  bytes: Uint8Array | Uint8ClampedArray | number[],
  width: number,
  height: number,
): void {
  canvas.width = width;
  canvas.height = height;
  requireContext(canvas).putImageData(new ImageData(toClamped(bytes), width, height), 0, 0);
  canvas.classList.add("filled");
}

function renderStats(result: ConvertResult | null): void {
  const values = result
    ? [
        result.stats.glyphs.toLocaleString(),
        result.stats.placed_glyphs.toLocaleString(),
        result.stats.foreground_pixels.toLocaleString(),
        `${result.stats.working_width} x ${result.stats.working_height}`,
      ]
    : ["-", "-", "-", "-"];

  for (const [index, value] of values.entries()) {
    const dd = statsList.querySelectorAll("dd")[index];
    if (dd) {
      dd.textContent = value;
    }
  }
}

function setCanvasEmpty(canvas: HTMLCanvasElement): void {
  canvas.width = 1;
  canvas.height = 1;
  const context = requireContext(canvas);
  context.clearRect(0, 0, 1, 1);
  canvas.classList.remove("filled");
}

function updateButtons(): void {
  convertButton.disabled = !sourceImage || busy;
  convertButton.textContent = busy ? "Converting..." : "Convert";
  copyTextButton.disabled = !lastResult || busy;
  downloadPngButton.disabled = !lastResult || busy;
  downloadTxtButton.disabled = !lastResult || busy;
}

function setStatus(message: string, error = false): void {
  statusEl.textContent = message;
  statusEl.classList.toggle("error", error);
}

function setBusyVisual(active: boolean): void {
  document.body.classList.toggle("is-converting", active);
  statusEl.classList.toggle("working", active);
  convertingOverlay.hidden = !active;
}

function setConvertingStage(message: string): void {
  convertingStage.textContent = message;
}

function readNumber(id: string): number {
  return Number(element<HTMLInputElement>(id).value);
}

function outputBaseName(): string {
  return sourceImage?.name.replace(/\.[^.]+$/, "") || "aa-converter";
}

function toClamped(bytes: Uint8Array | Uint8ClampedArray | number[]): Uint8ClampedArray<ArrayBuffer> {
  if (bytes instanceof Uint8Array) {
    return new Uint8ClampedArray(bytes);
  }
  return new Uint8ClampedArray(bytes);
}

function requireContext(canvas: HTMLCanvasElement): CanvasRenderingContext2D {
  const context = canvas.getContext("2d");
  if (!context) {
    throw new Error("canvas is unavailable");
  }
  return context;
}

function element<T extends HTMLElement>(id: string): T {
  const target = document.getElementById(id);
  if (!target) {
    throw new Error(`missing element: ${id}`);
  }
  return target as T;
}
