import { copyFileSync, cpSync, existsSync, mkdirSync, readdirSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const webDir = resolve(scriptDir, "..");
const repoDir = resolve(webDir, "..");
const publicDir = join(webDir, "public");

mkdirSync(publicDir, { recursive: true });

copyFileSync(join(repoDir, "assets", "model_catalog.json"), join(publicDir, "model_catalog.json"));
copyFileSync(join(repoDir, "THIRD_PARTY_NOTICES.md"), join(publicDir, "THIRD_PARTY_NOTICES.md"));

const ortDist = join(webDir, "node_modules", "onnxruntime-web", "dist");
const ortPublic = join(publicDir, "ort");

if (existsSync(ortDist)) {
  rmSync(ortPublic, { recursive: true, force: true });
  mkdirSync(ortPublic, { recursive: true });
  for (const fileName of readdirSync(ortDist)) {
    if (/^ort-.*\.(wasm|mjs)$/.test(fileName)) {
      copyFileSync(join(ortDist, fileName), join(ortPublic, fileName));
    }
  }
} else if (!existsSync(ortPublic)) {
  console.warn("onnxruntime-web is not installed; ORT runtime assets were not synced.");
}
