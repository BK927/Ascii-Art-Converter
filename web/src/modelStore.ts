export type ModelId = "informative" | "anime2sketch" | "anilines-basic" | "anilines-detail";

export type ModelStatusKind = "installed" | "missing" | "corrupt";

export interface ModelInstall {
  method: "direct-mirror";
  url: string;
}

export interface ModelEntry {
  id: ModelId;
  name: string;
  version: string;
  filename: string;
  sha256: string;
  size: number;
  install: ModelInstall;
  preprocess: ModelId;
  license_name: string;
  license_url: string;
  source_url: string;
  upstream_model_url: string;
  redistribution_basis: string;
}

export interface ModelCatalog {
  models: ModelEntry[];
}

export interface StoredModel {
  id: ModelId;
  filename: string;
  version: string;
  size: number;
  sha256: string;
  installedAt: string;
  blob: Blob;
}

export interface ModelStatus {
  kind: ModelStatusKind;
  label: string;
  detail: string;
}

export interface InstallProgress {
  downloaded: number;
  total: number;
  percent: number;
}

const DB_NAME = "aa-converter-models-v1";
const DB_VERSION = 1;
const STORE_NAME = "models";

let catalogPromise: Promise<ModelCatalog> | null = null;
let dbPromise: Promise<IDBDatabase> | null = null;

export async function loadModelCatalog(baseUrl: string): Promise<ModelCatalog> {
  if (!catalogPromise) {
    catalogPromise = fetch(`${baseUrl}model_catalog.json`).then(async (response) => {
      if (!response.ok) {
        throw new Error(`model catalog download failed: ${response.status}`);
      }
      const catalog = (await response.json()) as ModelCatalog;
      validateCatalog(catalog);
      return catalog;
    });
  }
  return catalogPromise;
}

export function modelEntry(catalog: ModelCatalog, id: ModelId): ModelEntry {
  const entry = catalog.models.find((candidate) => candidate.id === id);
  if (!entry) {
    throw new Error(`missing model catalog entry: ${id}`);
  }
  return entry;
}

export async function statusForModel(entry: ModelEntry): Promise<ModelStatus> {
  const stored = await readStoredModel(entry.id);
  if (!stored) {
    return {
      kind: "missing",
      label: "Not installed",
      detail: "Install model first",
    };
  }
  if (
    stored.filename !== entry.filename ||
    stored.version !== entry.version ||
    stored.size !== entry.size ||
    stored.sha256 !== entry.sha256 ||
    stored.blob.size !== entry.size
  ) {
    return {
      kind: "corrupt",
      label: "Needs repair",
      detail: "Stored model metadata does not match the catalog",
    };
  }
  return {
    kind: "installed",
    label: "Installed",
    detail: `${formatBytes(entry.size)} stored in this browser`,
  };
}

export async function allModelStatuses(catalog: ModelCatalog): Promise<Map<ModelId, ModelStatus>> {
  const entries = await Promise.all(
    catalog.models.map(async (entry) => [entry.id, await statusForModel(entry)] as const),
  );
  return new Map(entries);
}

export async function installModel(
  entry: ModelEntry,
  onProgress: (progress: InstallProgress) => void,
): Promise<void> {
  const response = await fetch(entry.install.url);
  if (!response.ok || !response.body) {
    throw new Error(`model download failed: ${response.status}`);
  }

  const total = Number(response.headers.get("content-length")) || entry.size;
  const reader = response.body.getReader();
  const chunks: Uint8Array[] = [];
  let downloaded = 0;

  while (true) {
    const read = await reader.read();
    if (read.done) {
      break;
    }
    chunks.push(read.value);
    downloaded += read.value.byteLength;
    onProgress(progress(downloaded, total));
  }

  const bytes = joinChunks(chunks, downloaded);
  const actual = await sha256(bytes);
  if (actual !== entry.sha256) {
    throw new Error(`checksum mismatch: expected ${entry.sha256}, got ${actual}`);
  }

  await writeStoredModel({
    id: entry.id,
    filename: entry.filename,
    version: entry.version,
    size: entry.size,
    sha256: entry.sha256,
    installedAt: new Date().toISOString(),
    blob: new Blob([bytes], { type: "application/octet-stream" }),
  });
  onProgress(progress(entry.size, entry.size));
}

export async function modelBytes(entry: ModelEntry): Promise<ArrayBuffer> {
  const stored = await readStoredModel(entry.id);
  if (!stored) {
    throw new Error(`${entry.name} is not installed`);
  }
  if (stored.sha256 !== entry.sha256 || stored.blob.size !== entry.size) {
    throw new Error(`${entry.name} needs repair`);
  }
  return stored.blob.arrayBuffer();
}

export function formatBytes(bytes: number): string {
  if (bytes >= 1024 * 1024) {
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  }
  if (bytes >= 1024) {
    return `${(bytes / 1024).toFixed(1)} KB`;
  }
  return `${bytes} B`;
}

function validateCatalog(catalog: ModelCatalog): void {
  if (!Array.isArray(catalog.models) || catalog.models.length === 0) {
    throw new Error("model catalog is empty");
  }
  for (const entry of catalog.models) {
    if (!entry.id || !entry.name || !entry.filename || !entry.sha256 || !entry.install?.url) {
      throw new Error("model catalog entry is incomplete");
    }
  }
}

function progress(downloaded: number, total: number): InstallProgress {
  return {
    downloaded,
    total,
    percent: total > 0 ? Math.min(100, (downloaded / total) * 100) : 0,
  };
}

function joinChunks(chunks: Uint8Array[], length: number): Uint8Array<ArrayBuffer> {
  const output = new Uint8Array(new ArrayBuffer(length));
  let offset = 0;
  for (const chunk of chunks) {
    output.set(chunk, offset);
    offset += chunk.byteLength;
  }
  return output;
}

async function sha256(bytes: Uint8Array<ArrayBuffer>): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", bytes);
  return Array.from(new Uint8Array(digest))
    .map((byte) => byte.toString(16).padStart(2, "0"))
    .join("");
}

function openDatabase(): Promise<IDBDatabase> {
  if (!dbPromise) {
    dbPromise = new Promise((resolve, reject) => {
      const request = indexedDB.open(DB_NAME, DB_VERSION);
      request.onerror = () => reject(request.error ?? new Error("IndexedDB open failed"));
      request.onupgradeneeded = () => {
        const db = request.result;
        if (!db.objectStoreNames.contains(STORE_NAME)) {
          db.createObjectStore(STORE_NAME, { keyPath: "id" });
        }
      };
      request.onsuccess = () => resolve(request.result);
    });
  }
  return dbPromise;
}

async function readStoredModel(id: ModelId): Promise<StoredModel | undefined> {
  const db = await openDatabase();
  return new Promise((resolve, reject) => {
    const transaction = db.transaction(STORE_NAME, "readonly");
    const request = transaction.objectStore(STORE_NAME).get(id);
    request.onerror = () => reject(request.error ?? new Error("IndexedDB read failed"));
    request.onsuccess = () => resolve(request.result as StoredModel | undefined);
  });
}

async function writeStoredModel(model: StoredModel): Promise<void> {
  const db = await openDatabase();
  await new Promise<void>((resolve, reject) => {
    const transaction = db.transaction(STORE_NAME, "readwrite");
    transaction.onerror = () => reject(transaction.error ?? new Error("IndexedDB write failed"));
    transaction.oncomplete = () => resolve();
    transaction.objectStore(STORE_NAME).put(model);
  });
}
