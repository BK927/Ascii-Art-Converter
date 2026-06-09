import "./styles.css";

import {
  allModelStatuses,
  formatBytes,
  installModel,
  loadModelCatalog,
  modelEntry,
} from "./modelStore";
import type { InstallProgress, ModelCatalog, ModelId, ModelStatus } from "./modelStore";

type AppMode = "single" | "batch";
type Preset = "clean" | "sensitive" | "color" | "soft" | "ai";
type LineExtractorId = "builtin" | ModelId;
type BuiltInInputMode = "structure" | "binary" | "soft";
type CleanupPreset = "balanced" | "delicate" | "clean";
type BatchStatus = "queued" | "running" | "done" | "error";
type PreviewTab = "result" | "compare";
type CompareTileState = "pending" | "rendering" | "ready" | "skipped" | "error";

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
  | { type: "error"; id: number; error: string }
  | { type: "compare-started"; id: number; index: number }
  | { type: "compare-status"; id: number; index: number; message: string }
  | { type: "compare-result"; id: number; index: number; result: ConvertResult }
  | { type: "compare-error"; id: number; index: number; error: string }
  | { type: "compare-done"; id: number };

interface PendingJob {
  resolve: (result: ConvertResult) => void;
  reject: (error: Error) => void;
  onStatus: (message: string) => void;
}

interface PendingCompare {
  id: number;
  resolve: () => void;
  reject: (error: Error) => void;
}

interface CompareSelection {
  preset: Preset;
  lineExtractor: LineExtractorId;
  inputMode: BuiltInInputMode;
  cleanupPreset: CleanupPreset;
  sliderValues: Record<string, string>;
}

interface CompareTile {
  index: number;
  label: string;
  detail: string;
  state: CompareTileState;
  statusText: string;
  selection: CompareSelection;
  settings: ConversionSettings;
  result?: ConvertResult;
}

interface CompareJob {
  index: number;
  settings: ConversionSettings;
}

const modelOrder: ModelId[] = ["informative", "anime2sketch", "anilines-basic", "anilines-detail"];

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
  ai: {
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

const cleanupDefaults: Record<
  CleanupPreset,
  { edge: string; binary: string; minComponentPixels: number; shortBranchPrunePx: number }
> = {
  balanced: {
    edge: "0.14",
    binary: "0.42",
    minComponentPixels: 4,
    shortBranchPrunePx: 4,
  },
  delicate: {
    edge: "0.08",
    binary: "0.30",
    minComponentPixels: 1,
    shortBranchPrunePx: 2,
  },
  clean: {
    edge: "0.20",
    binary: "0.58",
    minComponentPixels: 8,
    shortBranchPrunePx: 8,
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
const compareButton = element<HTMLButtonElement>("compare");
const convertBatchButton = element<HTMLButtonElement>("convert-batch");
const downloadBatchButton = element<HTMLButtonElement>("download-batch");
const copyTextButton = element<HTMLButtonElement>("copy-text");
const downloadPngButton = element<HTMLButtonElement>("download-png");
const downloadTxtButton = element<HTMLButtonElement>("download-txt");
const charactersInput = element<HTMLTextAreaElement>("characters");
const lineExtractorSelect = element<HTMLSelectElement>("line-extractor");
const inputModeSelect = element<HTMLSelectElement>("input-mode");
const cleanupPresetSelect = element<HTMLSelectElement>("cleanup-preset");
const inputModeRow = element<HTMLElement>("input-mode-row");
const cleanupRow = element<HTMLElement>("cleanup-row");
const modelManager = element<HTMLElement>("model-manager");
const modelStatusLabel = element<HTMLElement>("model-status-label");
const modelStatusDetail = element<HTMLElement>("model-status-detail");
const installModelButton = element<HTMLButtonElement>("install-model");
const resultView = element<HTMLElement>("result-view");
const compareView = element<HTMLElement>("compare-view");
const compareGrid = element<HTMLElement>("compare-grid");
const compareProgress = element<HTMLElement>("compare-progress");
const originalCanvas = element<HTMLCanvasElement>("original-canvas");
const asciiCanvas = element<HTMLCanvasElement>("ascii-canvas");
const lineCanvas = element<HTMLCanvasElement>("line-canvas");
const orientationCanvas = element<HTMLCanvasElement>("orientation-canvas");
const originalSize = element<HTMLElement>("original-size");
const asciiSize = element<HTMLElement>("ascii-size");
const timingTotal = element<HTMLElement>("timing-total");
const statsList = element<HTMLElement>("stats-list");

let appMode: AppMode = "single";
let previewTab: PreviewTab = "result";
let sourceImage: SourceImage | null = null;
let selectedPreset: Preset = "color";
let selectedLineExtractor: LineExtractorId = "builtin";
let selectedInputMode: BuiltInInputMode = "structure";
let selectedCleanupPreset: CleanupPreset = "balanced";
let modelCatalog: ModelCatalog | null = null;
let modelStatuses = new Map<ModelId, ModelStatus>();
let installingModel: ModelId | null = null;
let installProgress: InstallProgress | null = null;
let worker: Worker | null = null;
let activeJobId = 0;
let busy = false;
let lastResult: ConvertResult | null = null;
let batchItems: BatchItem[] = [];
let nextBatchId = 1;
let compareTiles: CompareTile[] = [];
let pendingCompare: PendingCompare | null = null;

const pendingJobs = new Map<number, PendingJob>();

syncSliderOutputs();
wireControls();
setCanvasEmpty(originalCanvas);
setCanvasEmpty(asciiCanvas);
setCanvasEmpty(lineCanvas);
setCanvasEmpty(orientationCanvas);
renderBatchList();
setMode("single");
void initializeModels();

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
    button.addEventListener("click", () => selectPreset(button.dataset.preset as Preset));
  }

  for (const button of document.querySelectorAll<HTMLButtonElement>("[data-preview-tab]")) {
    button.addEventListener("click", () => setPreviewTab(button.dataset.previewTab as PreviewTab));
  }

  lineExtractorSelect.addEventListener("change", () => {
    selectLineExtractor(lineExtractorSelect.value as LineExtractorId);
  });
  inputModeSelect.addEventListener("change", () => {
    selectedInputMode = inputModeSelect.value as BuiltInInputMode;
    syncControls();
  });
  cleanupPresetSelect.addEventListener("change", () => {
    selectedCleanupPreset = cleanupPresetSelect.value as CleanupPreset;
    applySliderValues(aiSliderDefaults(selectedCleanupPreset));
    syncControls();
  });

  for (const spec of sliderSpecs) {
    element<HTMLInputElement>(spec.input).addEventListener("input", () => {
      syncSliderOutputs();
    });
  }

  installModelButton.addEventListener("click", () => void installSelectedModel());
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

  compareGrid.addEventListener("click", (event) => {
    const target = event.target as HTMLElement;
    const button = target.closest<HTMLButtonElement>("[data-compare-index]");
    if (!button) {
      return;
    }
    const tile = compareTiles[Number(button.dataset.compareIndex)];
    if (tile?.state === "ready" && tile.result) {
      applyCompareTile(tile);
    }
  });

  convertButton.addEventListener("click", () => void runSingleConvert());
  compareButton.addEventListener("click", () => void runCompare());
  convertBatchButton.addEventListener("click", () => void runBatchConvert());
  downloadBatchButton.addEventListener("click", () => void downloadBatchZip());
  copyTextButton.addEventListener("click", () => void copyText());
  downloadPngButton.addEventListener("click", () => void downloadPng());
  downloadTxtButton.addEventListener("click", () => downloadTxt());
}

async function initializeModels(): Promise<void> {
  try {
    modelCatalog = await loadModelCatalog(import.meta.env.BASE_URL);
    renderLineExtractorOptions();
    modelStatuses = await allModelStatuses(modelCatalog);
    syncControls();
  } catch (error) {
    setStatus(error instanceof Error ? error.message : String(error), true);
  }
}

function renderLineExtractorOptions(): void {
  lineExtractorSelect.replaceChildren();
  lineExtractorSelect.append(new Option("Built-in extractor", "builtin"));
  if (!modelCatalog) {
    return;
  }
  for (const id of modelOrder) {
    const entry = modelEntry(modelCatalog, id);
    lineExtractorSelect.append(new Option(entry.name, id));
  }
}

function setMode(mode: AppMode): void {
  if (isBusy()) {
    return;
  }

  appMode = mode;
  for (const button of document.querySelectorAll<HTMLButtonElement>("[data-mode]")) {
    button.classList.toggle("selected", button.dataset.mode === mode);
  }
  if (mode === "batch" && previewTab === "compare") {
    setPreviewTab("result");
  }
  setHidden(".single-only, .single-action", mode !== "single");
  setHidden(".batch-only, .batch-action", mode !== "batch");
  setStatus(mode === "batch" ? "Batch ready" : "Ready");
  syncControls();
}

function setPreviewTab(tab: PreviewTab): void {
  if (tab === "compare" && appMode !== "single") {
    tab = "result";
  }
  previewTab = tab;
  for (const button of document.querySelectorAll<HTMLButtonElement>("[data-preview-tab]")) {
    button.classList.toggle("selected", button.dataset.previewTab === tab);
  }
  resultView.hidden = tab !== "result";
  compareView.hidden = tab !== "compare";
}

async function loadSingleFile(file: File): Promise<void> {
  try {
    setStatus("Reading image");
    sourceImage = await sourceFromFile(file);
    lastResult = null;
    compareTiles = [];

    renderRgba(originalCanvas, sourceImage.rgba, sourceImage.width, sourceImage.height);
    originalSize.textContent = `${sourceImage.width} x ${sourceImage.height}`;
    fileMeta.textContent = `${file.name} · ${sourceImage.width}x${sourceImage.height}`;
    clearResultPreview();
    compareProgress.textContent = "Run Compare to render candidates.";
    renderCompareGrid();
    setPreviewTab("result");
    setStatus("Ready");
    syncControls();
  } catch (error) {
    setStatus(error instanceof Error ? error.message : String(error), true);
  }
}

async function addBatchFiles(files: File[]): Promise<void> {
  busy = true;
  setStatus(`Reading ${files.length} image${files.length === 1 ? "" : "s"}`);
  syncControls();

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
  syncControls();
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

async function installSelectedModel(): Promise<void> {
  if (!modelCatalog || selectedLineExtractor === "builtin" || isBusy()) {
    return;
  }

  const entry = modelEntry(modelCatalog, selectedLineExtractor);
  installingModel = selectedLineExtractor;
  installProgress = null;
  setStatus(`Installing ${entry.name}`);
  syncControls();

  try {
    await installModel(entry, (progress) => {
      installProgress = progress;
      setStatus(
        `Installing ${entry.name}: ${progress.percent.toFixed(0)}% (${formatBytes(
          progress.downloaded,
        )}/${formatBytes(progress.total)})`,
      );
      syncControls();
    });
    modelStatuses = await allModelStatuses(modelCatalog);
    setStatus(`${entry.name} installed`);
  } catch (error) {
    setStatus(error instanceof Error ? error.message : String(error), true);
  } finally {
    installingModel = null;
    installProgress = null;
    syncControls();
  }
}

async function runSingleConvert(): Promise<void> {
  if (!sourceImage || isBusy() || !canUseCurrentSettings()) {
    return;
  }

  busy = true;
  lastResult = null;
  setStatus("Preparing image");
  setConvertingStage("Preparing image");
  setBusyVisual(true);
  syncControls();

  try {
    const result = await convertSource(sourceImage, currentSettings(), (message) => {
      setStatus(message);
      setConvertingStage(message);
    });
    showResult(sourceImage, result);
    setPreviewTab("result");
    setStatus("Converted");
  } catch (error) {
    setStatus(error instanceof Error ? error.message : String(error), true);
  } finally {
    busy = false;
    setBusyVisual(false);
    syncControls();
  }
}

async function runCompare(): Promise<void> {
  if (!sourceImage || isBusy() || appMode !== "single") {
    return;
  }

  compareTiles = buildCompareTiles();
  renderCompareGrid();
  setPreviewTab("compare");

  const jobs: CompareJob[] = compareTiles
    .filter((tile) => tile.state === "pending")
    .map((tile) => ({ index: tile.index, settings: tile.settings }));
  if (jobs.length === 0) {
    compareProgress.textContent = "No renderable candidates.";
    return;
  }

  busy = true;
  setStatus(`Comparing 0/${jobs.length}`);
  setBusyVisual(true);
  syncControls();

  try {
    await compareSource(sourceImage, jobs);
    const ready = compareTiles.filter((tile) => tile.state === "ready").length;
    compareProgress.textContent = `${ready}/${compareTiles.length} candidates ready`;
    setStatus("Compare complete");
  } catch (error) {
    setStatus(error instanceof Error ? error.message : String(error), true);
  } finally {
    busy = false;
    setBusyVisual(false);
    pendingCompare = null;
    syncControls();
  }
}

async function runBatchConvert(): Promise<void> {
  if (isBusy() || !canUseCurrentSettings()) {
    return;
  }

  const runnable = batchItems.filter((item) => item.source);
  if (runnable.length === 0) {
    setStatus("Add images first.", true);
    return;
  }

  busy = true;
  setBusyVisual(true);
  syncControls();

  for (const item of runnable) {
    item.status = "queued";
    item.result = undefined;
    item.error = undefined;
  }
  renderBatchList();

  const settings = currentSettings();
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
      const result = await convertSource(source, settings, (message) => {
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
  syncControls();
}

function convertSource(
  source: SourceImage,
  settings: ConversionSettings,
  onStatus: (message: string) => void,
): Promise<ConvertResult> {
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
        settings,
      },
      [imageRgba.buffer],
    );
  });
}

function compareSource(source: SourceImage, jobs: CompareJob[]): Promise<void> {
  return new Promise((resolve, reject) => {
    const id = ++activeJobId;
    pendingCompare = { id, resolve, reject };
    const imageRgba = new Uint8Array(source.rgba);
    ensureWorker().postMessage(
      {
        type: "compare",
        id,
        baseUrl: import.meta.env.BASE_URL,
        imageRgba,
        imageWidth: source.width,
        imageHeight: source.height,
        jobs,
      },
      [imageRgba.buffer],
    );
  });
}

function handleWorkerMessage(event: MessageEvent<WorkerMessage>): void {
  const message = event.data;
  if (message.type === "status") {
    const job = pendingJobs.get(message.id);
    job?.onStatus(message.message);
    return;
  }

  if (message.type === "result" || message.type === "error") {
    const job = pendingJobs.get(message.id);
    if (job) {
      pendingJobs.delete(message.id);
      if (message.type === "error") {
        job.reject(new Error(message.error));
      } else {
        job.resolve(message.result);
      }
      return;
    }
    if (pendingCompare?.id === message.id && message.type === "error") {
      pendingCompare.reject(new Error(message.error));
    }
    return;
  }

  if (!pendingCompare || pendingCompare.id !== message.id) {
    return;
  }

  switch (message.type) {
    case "compare-started":
      updateCompareTile(message.index, { state: "rendering", statusText: "Rendering" });
      break;
    case "compare-status":
      updateCompareTile(message.index, { statusText: message.message });
      break;
    case "compare-result":
      updateCompareTile(message.index, {
        state: "ready",
        statusText: `${message.result.timings.total_ms.toFixed(0)} ms`,
        result: message.result,
      });
      break;
    case "compare-error":
      updateCompareTile(message.index, { state: "error", statusText: message.error });
      break;
    case "compare-done":
      pendingCompare.resolve();
      break;
  }
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
  if (preset === "ai") {
    if (selectedLineExtractor === "builtin") {
      selectedLineExtractor = "informative";
    }
    selectedCleanupPreset = "balanced";
    applySliderValues(aiSliderDefaults(selectedCleanupPreset));
  } else {
    selectedLineExtractor = "builtin";
    selectedInputMode = preset === "soft" ? "soft" : "structure";
    applySliderValues(presetDefaults[preset]);
  }
  syncControls();
}

function selectLineExtractor(extractor: LineExtractorId): void {
  selectedLineExtractor = extractor;
  if (extractor === "builtin") {
    if (selectedPreset === "ai") {
      selectedPreset = "color";
      selectedInputMode = "structure";
      applySliderValues(presetDefaults.color);
    }
  } else {
    selectedPreset = "ai";
    applySliderValues(aiSliderDefaults(selectedCleanupPreset));
  }
  syncControls();
}

function currentSettings(): ConversionSettings {
  return settingsFromSelection({
    preset: selectedLineExtractor === "builtin" ? selectedPreset : "ai",
    lineExtractor: selectedLineExtractor,
    inputMode: selectedInputMode,
    cleanupPreset: selectedCleanupPreset,
    sliderValues: currentSliderValues(),
  });
}

function settingsFromSelection(selection: CompareSelection): ConversionSettings {
  return {
    preset: selection.preset,
    lineExtractor: selection.lineExtractor,
    inputMode: selection.inputMode,
    cleanupPreset: selection.cleanupPreset,
    options: optionsFromValues(selection),
  };
}

function optionsFromValues(selection: CompareSelection): ConvertOptions {
  const isAi = selection.lineExtractor !== "builtin";
  const cleanup = cleanupDefaults[selection.cleanupPreset];
  const values = selection.sliderValues;
  const characters = charactersInput.value.trim();
  return {
    max_width: numberFromValues(values, "max-width"),
    font_px: numberFromValues(values, "font-px"),
    stripe_px: numberFromValues(values, "stripe-px"),
    blur: numberFromValues(values, "blur"),
    edge: numberFromValues(values, "edge"),
    binary: numberFromValues(values, "binary"),
    match: numberFromValues(values, "match"),
    mismatch: numberFromValues(values, "mismatch"),
    cutoff: numberFromValues(values, "cutoff"),
    glyph_ink: numberFromValues(values, "glyph-ink"),
    input_mode: isAi ? "ai" : selection.inputMode,
    structure_line_mode: "flowdog",
    thinning_mode: isAi ? "guo-hall" : "kmm",
    placement_mode: !isAi && selection.preset === "soft" ? "soft-grid" : "paper-greedy",
    stroke_tolerance: !isAi && selection.preset === "color",
    min_component_pixels: isAi ? cleanup.minComponentPixels : 0,
    short_branch_prune_px: isAi ? cleanup.shortBranchPrunePx : 0,
    ...(characters ? { character_set: charactersInput.value } : {}),
  };
}

function numberFromValues(values: Record<string, string>, key: string): number {
  return Number(values[key] ?? "0");
}

function buildCompareTiles(): CompareTile[] {
  const tiles: CompareTile[] = [];
  const builtIn: Array<[string, Preset, BuiltInInputMode]> = [
    ["Illustration", "color", "structure"],
    ["Line Art", "clean", "structure"],
    ["Fine Lines", "sensitive", "structure"],
    ["Soft Sketch", "soft", "soft"],
  ];

  for (const [label, preset, inputMode] of builtIn) {
    const selection = {
      preset,
      lineExtractor: "builtin" as const,
      inputMode,
      cleanupPreset: "balanced" as const,
      sliderValues: { ...presetDefaults[preset] },
    };
    tiles.push(compareTile(label, "Built-in", selection, true));
  }

  if (modelCatalog) {
    for (const model of modelOrder) {
      const entry = modelEntry(modelCatalog, model);
      const installed = modelStatuses.get(model)?.kind === "installed";
      for (const cleanup of ["delicate", "balanced", "clean"] as const) {
        const selection = {
          preset: "ai" as const,
          lineExtractor: model,
          inputMode: "structure" as const,
          cleanupPreset: cleanup,
          sliderValues: aiSliderDefaults(cleanup),
        };
        tiles.push(compareTile(`${entry.name} · ${cleanupLabel(cleanup)}`, "AI", selection, installed));
      }
    }
  }

  return tiles.map((tile, index) => ({ ...tile, index }));
}

function compareTile(
  label: string,
  detail: string,
  selection: CompareSelection,
  renderable: boolean,
): CompareTile {
  return {
    index: 0,
    label,
    detail,
    selection,
    settings: settingsFromSelection(selection),
    state: renderable ? "pending" : "skipped",
    statusText: renderable ? "Pending" : "Install model first",
  };
}

function updateCompareTile(index: number, patch: Partial<CompareTile>): void {
  const current = compareTiles[index];
  if (!current) {
    return;
  }
  compareTiles[index] = { ...current, ...patch };
  const completed = compareTiles.filter((tile) => tile.state === "ready" || tile.state === "error").length;
  const renderable = compareTiles.filter((tile) => tile.state !== "skipped").length;
  compareProgress.textContent = `Rendering ${completed}/${renderable}`;
  setStatus(`Comparing ${completed}/${renderable}`);
  renderCompareGrid();
}

function applyCompareTile(tile: CompareTile): void {
  applySelection(tile.selection);
  if (sourceImage && tile.result) {
    showResult(sourceImage, tile.result);
  }
  setPreviewTab("result");
  setStatus(`${tile.label} applied`);
}

function applySelection(selection: CompareSelection): void {
  selectedPreset = selection.preset;
  selectedLineExtractor = selection.lineExtractor;
  selectedInputMode = selection.inputMode;
  selectedCleanupPreset = selection.cleanupPreset;
  applySliderValues(selection.sliderValues);
  syncControls();
}

function currentSliderValues(): Record<string, string> {
  return Object.fromEntries(
    sliderSpecs.map((spec) => [spec.input, element<HTMLInputElement>(spec.input).value]),
  );
}

function aiSliderDefaults(cleanup: CleanupPreset): Record<string, string> {
  const cleanupValues = cleanupDefaults[cleanup];
  return {
    ...presetDefaults.ai,
    edge: cleanupValues.edge,
    binary: cleanupValues.binary,
  };
}

function applySliderValues(values: Record<string, string>): void {
  for (const [id, value] of Object.entries(values)) {
    element<HTMLInputElement>(id).value = value;
  }
  syncSliderOutputs();
}

function syncControls(): void {
  for (const button of document.querySelectorAll<HTMLButtonElement>("[data-preset]")) {
    button.classList.toggle("selected", button.dataset.preset === selectedPreset);
  }
  lineExtractorSelect.value = selectedLineExtractor;
  inputModeSelect.value = selectedInputMode;
  cleanupPresetSelect.value = selectedCleanupPreset;

  const isAi = selectedLineExtractor !== "builtin";
  inputModeRow.hidden = isAi;
  cleanupRow.hidden = !isAi;
  modelManager.hidden = !isAi;
  renderModelStatus();
  updateButtons();
}

function renderModelStatus(): void {
  if (selectedLineExtractor === "builtin" || !modelCatalog) {
    return;
  }

  const entry = modelEntry(modelCatalog, selectedLineExtractor);
  const status = modelStatuses.get(selectedLineExtractor) ?? {
    kind: "missing",
    label: "Not installed",
    detail: "Install model first",
  };

  modelStatusLabel.textContent = status.label;
  modelStatusDetail.textContent =
    installingModel === selectedLineExtractor && installProgress
      ? `${formatBytes(installProgress.downloaded)}/${formatBytes(installProgress.total)}`
      : `${entry.name} · ${status.detail}`;

  installModelButton.disabled = installingModel !== null || busy || status.kind === "installed";
  installModelButton.textContent =
    installingModel === selectedLineExtractor
      ? `Installing ${installProgress ? installProgress.percent.toFixed(0) : 0}%`
      : status.kind === "corrupt"
        ? "Repair model"
        : status.kind === "installed"
          ? "Installed"
          : "Install model";
  installModelButton.title =
    "Downloads from the verified AA Converter third-party model mirror. See THIRD_PARTY_NOTICES.md.";
}

function canUseCurrentSettings(): boolean {
  if (selectedLineExtractor === "builtin") {
    return true;
  }
  return modelStatuses.get(selectedLineExtractor)?.kind === "installed";
}

function isBusy(): boolean {
  return busy || installingModel !== null;
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
    clearResultPreview();
    setStatus(`${item.name} queued`);
  }
  setPreviewTab("result");
  syncControls();
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
  syncControls();
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
  void rgbaPngBlob(lastResult.ascii_rgba, lastResult.width, lastResult.height).then((blob) =>
    downloadBlob(blob, `${outputBaseName()}-ascii.png`),
  );
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
  if (done.length === 0 || isBusy()) {
    return;
  }

  busy = true;
  setStatus("Building ZIP");
  syncControls();

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
      zip.file(`${prefix}-ascii.png`, await rgbaPngBlob(result.ascii_rgba, result.width, result.height));
      if (result.ai_line_rgba && result.ai_line_width && result.ai_line_height) {
        zip.file(
          `${prefix}-ai-lineart.png`,
          await rgbaPngBlob(result.ai_line_rgba, result.ai_line_width, result.ai_line_height),
        );
      }
    }
    const blob = await zip.generateAsync({ type: "blob" });
    downloadBlob(blob, "aa-converter-batch.zip");
    setStatus(`ZIP ready: ${done.length} result(s)`);
  } catch (error) {
    setStatus(error instanceof Error ? error.message : String(error), true);
  } finally {
    busy = false;
    syncControls();
  }
}

function rgbaPngBlob(
  bytes: Uint8Array | Uint8ClampedArray | number[],
  width: number,
  height: number,
): Promise<Blob> {
  const canvas = document.createElement("canvas");
  canvas.width = width;
  canvas.height = height;
  requireContext(canvas).putImageData(new ImageData(toClamped(bytes), width, height), 0, 0);

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

function renderCompareGrid(): void {
  if (compareTiles.length === 0) {
    compareGrid.replaceChildren(emptyComparePlaceholder());
    return;
  }

  compareGrid.replaceChildren(
    ...compareTiles.map((tile) => {
      const button = document.createElement("button");
      button.type = "button";
      button.className = `compare-tile ${tile.state}`;
      button.dataset.compareIndex = String(tile.index);
      button.disabled = tile.state !== "ready";

      const head = document.createElement("div");
      head.className = "compare-tile-head";
      const title = document.createElement("strong");
      title.textContent = tile.label;
      const detail = document.createElement("span");
      detail.textContent = tile.detail;
      head.append(title, detail);

      if (tile.result) {
        const canvas = document.createElement("canvas");
        renderRgba(canvas, tile.result.ascii_rgba, tile.result.width, tile.result.height);
        button.append(head, canvas, compareFoot(tile));
      } else {
        const placeholder = document.createElement("div");
        placeholder.className = "compare-placeholder";
        placeholder.textContent = tile.statusText;
        button.append(head, placeholder, compareFoot(tile));
      }

      return button;
    }),
  );
}

function compareFoot(tile: CompareTile): HTMLElement {
  const foot = document.createElement("div");
  foot.className = "compare-tile-foot";
  foot.textContent = tile.statusText;
  foot.title = tile.statusText;
  return foot;
}

function emptyComparePlaceholder(): HTMLElement {
  const placeholder = document.createElement("div");
  placeholder.className = "compare-placeholder";
  placeholder.textContent = "Run Compare to render candidates.";
  return placeholder;
}

function clearBatch(): void {
  if (isBusy()) {
    return;
  }
  batchItems = [];
  renderBatchList();
  setStatus("Batch cleared");
  syncControls();
}

function clearResultPreview(): void {
  setCanvasEmpty(asciiCanvas);
  setCanvasEmpty(lineCanvas);
  setCanvasEmpty(orientationCanvas);
  asciiSize.textContent = "-";
  timingTotal.textContent = "-";
  renderStats(null);
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
  const busyNow = isBusy();
  const canConvert = canUseCurrentSettings();

  convertButton.disabled = !sourceImage || busyNow || !canConvert;
  convertButton.textContent = busy && appMode === "single" ? "Converting..." : "Convert";
  compareButton.disabled = !sourceImage || busyNow || appMode !== "single";
  copyTextButton.disabled = !lastResult || busyNow;
  downloadPngButton.disabled = !lastResult || busyNow;
  downloadTxtButton.disabled = !lastResult || busyNow;

  convertBatchButton.disabled = !hasBatchInput || busyNow || !canConvert;
  convertBatchButton.textContent = busy && appMode === "batch" ? "Converting..." : "Convert All";
  downloadBatchButton.disabled = !hasBatchResults || busyNow;
  addBatchButton.disabled = busyNow;
  clearBatchButton.disabled = busyNow || batchItems.length === 0;
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
  for (const target of document.querySelectorAll<HTMLElement>(selector)) {
    target.hidden = hidden;
  }
}

function cleanupLabel(cleanup: CleanupPreset): string {
  switch (cleanup) {
    case "balanced":
      return "Balanced";
    case "delicate":
      return "Delicate";
    case "clean":
      return "Clean";
  }
}

function outputBaseName(): string {
  return safeBaseName(sourceImage?.name ?? "aa-converter");
}

function safeBaseName(name: string): string {
  return (
    name
      .replace(/\.[^.]+$/, "")
      .replace(/[<>:"/\\|?*\x00-\x1f]/g, "_")
      .trim() || "image"
  );
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
