import "./styles.css";

type AppMode = "single" | "batch";
type Preset = "clean" | "sensitive" | "color" | "soft" | "ai-sketch";
type BatchStatus = "queued" | "running" | "done" | "error";

interface SourceImage {
  name: string;
  width: number;
  height: number;
  rgba: Uint8ClampedArray;
}

interface BatchItem {
  id: number;
  name: string;
  source?: SourceImage;
  status: BatchStatus;
  result?: ConvertResult;
  error?: string;
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

interface PendingJob {
  resolve: (result: ConvertResult) => void;
  reject: (error: Error) => void;
  onStatus: (message: string) => void;
}

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
  soft: {
    "max-width": "384",
    "font-px": "16",
    "stripe-px": "16",
    blur: "0.65",
    edge: "0.2",
    binary: "0.58",
    match: "1",
    mismatch: "0.65",
    cutoff: "0",
    "glyph-ink": "0.14",
  },
  "ai-sketch": {
    "max-width": "512",
    "font-px": "16",
    "stripe-px": "16",
    blur: "0.55",
    edge: "0.14",
    binary: "0.42",
    match: "1",
    mismatch: "0.65",
    cutoff: "0",
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
const batchInput = element<HTMLInputElement>("batch-input");
const dropzone = element<HTMLLabelElement>("dropzone");
const fileMeta = element<HTMLElement>("file-meta");
const batchMeta = element<HTMLElement>("batch-meta");
const batchList = element<HTMLElement>("batch-list");
const addBatchButton = element<HTMLButtonElement>("add-batch");
const clearBatchButton = element<HTMLButtonElement>("clear-batch");
const statusEl = element<HTMLElement>("status");
const convertingOverlay = element<HTMLElement>("converting-overlay");
const convertingStage = element<HTMLElement>("converting-stage");
const convertButton = element<HTMLButtonElement>("convert");
const convertBatchButton = element<HTMLButtonElement>("convert-batch");
const downloadBatchButton = element<HTMLButtonElement>("download-batch");
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

let appMode: AppMode = "single";
let sourceImage: SourceImage | null = null;
let selectedPreset: Preset = "color";
let worker: Worker | null = null;
let activeJobId = 0;
let busy = false;
let lastResult: ConvertResult | null = null;
let batchItems: BatchItem[] = [];
let nextBatchId = 1;

const pendingJobs = new Map<number, PendingJob>();

syncSliderOutputs();
wireControls();
setCanvasEmpty(originalCanvas);
setCanvasEmpty(asciiCanvas);
setCanvasEmpty(lineCanvas);
setCanvasEmpty(orientationCanvas);
renderBatchList();
setMode("single");

function wireControls(): void {
  fileInput.addEventListener("change", () => {
    const file = fileInput.files?.[0];
    if (file) {
      void loadSingleFile(file);
    }
  });

  batchInput.addEventListener("change", () => {
    const files = Array.from(batchInput.files ?? []);
    if (files.length > 0) {
      void addBatchFiles(files);
    }
    batchInput.value = "";
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
      void loadSingleFile(file);
    }
  });

  for (const button of document.querySelectorAll<HTMLButtonElement>("[data-mode]")) {
    button.addEventListener("click", () => setMode(button.dataset.mode as AppMode));
  }

  for (const button of document.querySelectorAll<HTMLButtonElement>("[data-preset]")) {
    button.addEventListener("click", () => {
      const preset = button.dataset.preset as Preset;
      selectPreset(preset);
    });
  }

  for (const spec of sliderSpecs) {
    element<HTMLInputElement>(spec.input).addEventListener("input", syncSliderOutputs);
  }

  addBatchButton.addEventListener("click", () => batchInput.click());
  clearBatchButton.addEventListener("click", clearBatch);
  batchList.addEventListener("click", (event) => {
    const target = event.target as HTMLElement;
    const button = target.closest<HTMLButtonElement>("[data-batch-id]");
    if (!button) {
      return;
    }
    const id = Number(button.dataset.batchId);
    const item = batchItems.find((candidate) => candidate.id === id);
    if (item) {
      previewBatchItem(item);
    }
  });

  convertButton.addEventListener("click", () => void runSingleConvert());
  convertBatchButton.addEventListener("click", () => void runBatchConvert());
  downloadBatchButton.addEventListener("click", () => void downloadBatchZip());
  copyTextButton.addEventListener("click", () => void copyText());
  downloadPngButton.addEventListener("click", () => void downloadPng());
  downloadTxtButton.addEventListener("click", () => downloadTxt());
}

function setMode(mode: AppMode): void {
  if (busy) {
    return;
  }

  appMode = mode;
  for (const button of document.querySelectorAll<HTMLButtonElement>("[data-mode]")) {
    button.classList.toggle("selected", button.dataset.mode === mode);
  }
  setHidden(".single-only, .single-action", mode !== "single");
  setHidden(".batch-only, .batch-action", mode !== "batch");
  setStatus(mode === "batch" ? "Batch ready" : "Ready");
  updateButtons();
}

async function loadSingleFile(file: File): Promise<void> {
  try {
    setStatus("Reading image");
    sourceImage = await sourceFromFile(file);
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

async function addBatchFiles(files: File[]): Promise<void> {
  busy = true;
  setStatus(`Reading ${files.length} image${files.length === 1 ? "" : "s"}`);
  updateButtons();

  for (const file of files) {
    try {
      const source = await sourceFromFile(file);
      batchItems.push({
        id: nextBatchId++,
        name: file.name,
        source,
        status: "queued",
      });
    } catch (error) {
      batchItems.push({
        id: nextBatchId++,
        name: file.name,
        status: "error",
        error: error instanceof Error ? error.message : String(error),
      });
    }
  }

  busy = false;
  renderBatchList();
  setStatus(`${batchItems.filter((item) => item.source).length} image(s) queued`);
  updateButtons();
}

async function sourceFromFile(file: File): Promise<SourceImage> {
  const bitmap = await createImageBitmap(file);
  const canvas = document.createElement("canvas");
  canvas.width = bitmap.width;
  canvas.height = bitmap.height;
  const context = requireContext(canvas);
  context.drawImage(bitmap, 0, 0);
  const data = context.getImageData(0, 0, bitmap.width, bitmap.height);
  bitmap.close();

  return {
    name: file.name,
    width: data.width,
    height: data.height,
    rgba: data.data.slice(),
  };
}

async function runSingleConvert(): Promise<void> {
  if (!sourceImage || busy) {
    return;
  }

  busy = true;
  lastResult = null;
  setStatus("Preparing image");
  setConvertingStage("Preparing image");
  setBusyVisual(true);
  updateButtons();

  try {
    const result = await convertSource(sourceImage, (message) => {
      setStatus(message);
      setConvertingStage(message);
    });
    showResult(sourceImage, result);
    setStatus("Converted");
  } catch (error) {
    setStatus(error instanceof Error ? error.message : String(error), true);
  } finally {
    busy = false;
    setBusyVisual(false);
    updateButtons();
  }
}

async function runBatchConvert(): Promise<void> {
  if (busy) {
    return;
  }

  const runnable = batchItems.filter((item) => item.source);
  if (runnable.length === 0) {
    setStatus("Add images first.", true);
    return;
  }

  busy = true;
  setBusyVisual(true);
  updateButtons();

  for (const item of runnable) {
    item.status = "queued";
    item.result = undefined;
    item.error = undefined;
  }
  renderBatchList();

  let converted = 0;
  let failed = 0;
  for (const [index, item] of runnable.entries()) {
    const source = item.source;
    if (!source) {
      continue;
    }

    item.status = "running";
    renderBatchList();
    setStatus(`Converting ${index + 1}/${runnable.length}: ${item.name}`);
    setConvertingStage(`${index + 1}/${runnable.length}: ${item.name}`);

    try {
      const result = await convertSource(source, (message) => {
        setStatus(`${message} · ${index + 1}/${runnable.length}`);
        setConvertingStage(`${index + 1}/${runnable.length}: ${message}`);
      });
      item.status = "done";
      item.result = result;
      converted += 1;
      showResult(source, result);
    } catch (error) {
      item.status = "error";
      item.error = error instanceof Error ? error.message : String(error);
      failed += 1;
    }
    renderBatchList();
  }

  busy = false;
  setBusyVisual(false);
  setStatus(`Batch complete: ${converted} converted${failed ? `, ${failed} failed` : ""}`);
  updateButtons();
}

function convertSource(source: SourceImage, onStatus: (message: string) => void): Promise<ConvertResult> {
  return new Promise((resolve, reject) => {
    const id = ++activeJobId;
    pendingJobs.set(id, { resolve, reject, onStatus });
    const imageRgba = new Uint8Array(source.rgba);

    ensureWorker().postMessage(
      {
        type: "convert",
        id,
        baseUrl: import.meta.env.BASE_URL,
        imageRgba,
        imageWidth: source.width,
        imageHeight: source.height,
        preset: selectedPreset,
        options: readOptions(),
      },
      [imageRgba.buffer],
    );
  });
}

function handleWorkerMessage(event: MessageEvent<WorkerMessage>): void {
  const job = pendingJobs.get(event.data.id);
  if (!job) {
    return;
  }

  if (event.data.type === "status") {
    job.onStatus(event.data.message);
    return;
  }

  pendingJobs.delete(event.data.id);
  if (event.data.type === "error") {
    job.reject(new Error(event.data.error));
    return;
  }

  job.resolve(event.data.result);
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

function previewBatchItem(item: BatchItem): void {
  if (!item.source) {
    setStatus(item.error ?? "Image could not be loaded.", true);
    return;
  }

  sourceImage = item.source;
  renderRgba(originalCanvas, item.source.rgba, item.source.width, item.source.height);
  originalSize.textContent = `${item.source.width} x ${item.source.height}`;
  fileMeta.textContent = `${item.name} · ${item.source.width}x${item.source.height}`;

  if (item.result) {
    showResult(item.source, item.result);
    setStatus(`${item.name} ready`);
  } else {
    lastResult = null;
    setCanvasEmpty(asciiCanvas);
    setCanvasEmpty(lineCanvas);
    setCanvasEmpty(orientationCanvas);
    asciiSize.textContent = "-";
    timingTotal.textContent = "-";
    renderStats(null);
    setStatus(`${item.name} queued`);
  }
  updateButtons();
}

function showResult(source: SourceImage, result: ConvertResult): void {
  sourceImage = source;
  lastResult = result;
  renderRgba(originalCanvas, source.rgba, source.width, source.height);
  renderRgba(asciiCanvas, result.ascii_rgba, result.width, result.height);
  renderRgba(lineCanvas, result.line_rgba, result.width, result.stats.working_height);
  renderRgba(
    orientationCanvas,
    result.orientation_rgba,
    result.width,
    result.stats.working_height,
  );
  originalSize.textContent = `${source.width} x ${source.height}`;
  asciiSize.textContent = `${result.width} x ${result.height}`;
  timingTotal.textContent = `${result.timings.total_ms.toFixed(0)} ms`;
  renderStats(result);
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
  void resultPngBlob(lastResult).then((blob) => downloadBlob(blob, `${outputBaseName()}-ascii.png`));
}

function downloadTxt(): void {
  if (!lastResult) {
    return;
  }
  downloadBlob(
    new Blob([lastResult.text], { type: "text/plain;charset=utf-8" }),
    `${outputBaseName()}-ascii.txt`,
  );
}

async function downloadBatchZip(): Promise<void> {
  const done = batchItems.filter((item) => item.result);
  if (done.length === 0 || busy) {
    return;
  }

  busy = true;
  setStatus("Building ZIP");
  updateButtons();

  try {
    const { default: JSZip } = await import("jszip");
    const zip = new JSZip();
    for (const [index, item] of done.entries()) {
      const result = item.result;
      if (!result) {
        continue;
      }
      const prefix = `${String(index + 1).padStart(3, "0")}-${safeBaseName(item.name)}`;
      zip.file(`${prefix}-ascii.txt`, result.text);
      zip.file(`${prefix}-ascii.png`, await resultPngBlob(result));
    }
    const blob = await zip.generateAsync({ type: "blob" });
    downloadBlob(blob, "aa-converter-batch.zip");
    setStatus(`ZIP ready: ${done.length} result(s)`);
  } catch (error) {
    setStatus(error instanceof Error ? error.message : String(error), true);
  } finally {
    busy = false;
    updateButtons();
  }
}

function resultPngBlob(result: ConvertResult): Promise<Blob> {
  const canvas = document.createElement("canvas");
  canvas.width = result.width;
  canvas.height = result.height;
  requireContext(canvas).putImageData(
    new ImageData(toClamped(result.ascii_rgba), result.width, result.height),
    0,
    0,
  );

  return new Promise((resolve, reject) => {
    canvas.toBlob((blob) => {
      if (blob) {
        resolve(blob);
      } else {
        reject(new Error("PNG export failed"));
      }
    }, "image/png");
  });
}

function downloadBlob(blob: Blob, fileName: string): void {
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = fileName;
  anchor.click();
  URL.revokeObjectURL(url);
}

function renderBatchList(): void {
  const total = batchItems.length;
  const converted = batchItems.filter((item) => item.status === "done").length;
  const failed = batchItems.filter((item) => item.status === "error").length;
  batchMeta.textContent =
    total === 0
      ? "No images queued"
      : `${total} queued · ${converted} converted${failed ? ` · ${failed} failed` : ""}`;

  batchList.replaceChildren(
    ...batchItems.map((item) => {
      const row = document.createElement("div");
      row.className = `batch-item ${item.status}`;

      const button = document.createElement("button");
      button.type = "button";
      button.dataset.batchId = String(item.id);
      button.textContent = item.name;
      button.title = item.error ?? item.name;

      const status = document.createElement("span");
      status.className = "batch-state";
      status.textContent = item.status;

      row.append(button, status);
      return row;
    }),
  );
}

function clearBatch(): void {
  if (busy) {
    return;
  }
  batchItems = [];
  renderBatchList();
  setStatus("Batch cleared");
  updateButtons();
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
  const hasBatchInput = batchItems.some((item) => item.source);
  const hasBatchResults = batchItems.some((item) => item.result);

  convertButton.disabled = !sourceImage || busy;
  convertButton.textContent = busy && appMode === "single" ? "Converting..." : "Convert";
  copyTextButton.disabled = !lastResult || busy;
  downloadPngButton.disabled = !lastResult || busy;
  downloadTxtButton.disabled = !lastResult || busy;

  convertBatchButton.disabled = !hasBatchInput || busy;
  convertBatchButton.textContent = busy && appMode === "batch" ? "Converting..." : "Convert All";
  downloadBatchButton.disabled = !hasBatchResults || busy;
  addBatchButton.disabled = busy;
  clearBatchButton.disabled = busy || batchItems.length === 0;
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

function setHidden(selector: string, hidden: boolean): void {
  for (const element of document.querySelectorAll<HTMLElement>(selector)) {
    element.hidden = hidden;
  }
}

function readNumber(id: string): number {
  return Number(element<HTMLInputElement>(id).value);
}

function outputBaseName(): string {
  return safeBaseName(sourceImage?.name ?? "aa-converter");
}

function safeBaseName(name: string): string {
  return name
    .replace(/\.[^.]+$/, "")
    .replace(/[<>:"/\\|?*\x00-\x1f]/g, "_")
    .trim() || "image";
}

function toClamped(bytes: Uint8Array | Uint8ClampedArray | number[]): Uint8ClampedArray<ArrayBuffer> {
  const clamped = new Uint8ClampedArray(bytes.length);
  clamped.set(bytes);
  return clamped;
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
