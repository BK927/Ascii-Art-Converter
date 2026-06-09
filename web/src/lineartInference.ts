import * as ort from "onnxruntime-web/webgpu";

import { modelBytes, modelEntry } from "./modelStore";
import type { ModelCatalog, ModelId } from "./modelStore";

export interface RgbaSource {
  width: number;
  height: number;
  rgba: Uint8Array;
}

export interface AiLineart {
  width: number;
  height: number;
  rgba: Uint8Array;
}

interface RgbImage {
  width: number;
  height: number;
  data: Uint8Array;
}

interface GrayImage {
  width: number;
  height: number;
  data: Uint8Array;
}

interface TensorInput {
  data: Float32Array;
  dims: [number, number, number, number];
}

const sessionCache = new Map<ModelId, Promise<ort.InferenceSession>>();
let ortConfigured = false;

export async function extractAiLineart(
  baseUrl: string,
  catalog: ModelCatalog,
  model: ModelId,
  source: RgbaSource,
): Promise<AiLineart> {
  configureOrt(baseUrl);
  const session = await sessionForModel(catalog, model);
  const working = resizeRgbPreservingAspect(rgbaToRgb(source), 512);

  let gray: GrayImage;
  switch (model) {
    case "informative":
      gray = await runDynamicRgb(session, working, false);
      break;
    case "anime2sketch":
      gray = await runAnime2Sketch(session, working);
      break;
    case "anilines-basic":
      gray = await runDynamicRgb(session, working, true);
      break;
    case "anilines-detail":
      gray = await runAniLinesDetail(session, working);
      break;
  }

  return {
    width: gray.width,
    height: gray.height,
    rgba: grayToRgba(gray),
  };
}

function configureOrt(baseUrl: string): void {
  if (ortConfigured) {
    return;
  }
  ort.env.wasm.wasmPaths = `${baseUrl}ort/`;
  ort.env.wasm.numThreads = 1;
  ortConfigured = true;
}

function sessionForModel(catalog: ModelCatalog, model: ModelId): Promise<ort.InferenceSession> {
  const cached = sessionCache.get(model);
  if (cached) {
    return cached;
  }

  const entry = modelEntry(catalog, model);
  const created = modelBytes(entry).then(async (bytes) => {
    const preferredProviders: ort.InferenceSession.ExecutionProviderConfig[] =
      hasWebGpu() ? ["webgpu", "wasm"] : ["wasm"];
    try {
      return await ort.InferenceSession.create(bytes, {
        executionProviders: preferredProviders,
        graphOptimizationLevel: "all",
      });
    } catch (error) {
      if (preferredProviders[0] !== "webgpu") {
        throw error;
      }
      return ort.InferenceSession.create(bytes, {
        executionProviders: ["wasm"],
        graphOptimizationLevel: "all",
      });
    }
  });
  sessionCache.set(model, created);
  return created;
}

function hasWebGpu(): boolean {
  return typeof navigator !== "undefined" && "gpu" in navigator;
}

async function runDynamicRgb(
  session: ort.InferenceSession,
  image: RgbImage,
  sharpen: boolean,
): Promise<GrayImage> {
  const input = sharpen ? sharpenRgb(image, 5.0) : cloneRgb(image);
  const padded = padRgbToMultipleOfEight(input);
  const output = await runTensor(session, rgbToTensor01(padded.image));
  return tensorToGray(output, false, padded.width, padded.height);
}

async function runAnime2Sketch(session: ort.InferenceSession, image: RgbImage): Promise<GrayImage> {
  const square = letterboxRgb(image, 512);
  const output = await runTensor(session, rgbToTensorMinusOneToOne(square.image));
  const gray = tensorToGray(output, true, 512, 512);
  const cropped = cropGray(
    gray,
    square.offsetX,
    square.offsetY,
    square.contentWidth,
    square.contentHeight,
  );
  return resizeGray(cropped, image.width, image.height);
}

async function runAniLinesDetail(
  session: ort.InferenceSession,
  image: RgbImage,
): Promise<GrayImage> {
  const gray = rgbToGray(image);
  const sobel = invertedSobel(gray);
  const padded = padTwoGrayToMultipleOfEight(gray, sobel);
  const output = await runTensor(session, twoGrayToTensor01(padded.first, padded.second));
  return tensorToGray(output, false, padded.width, padded.height);
}

async function runTensor(
  session: ort.InferenceSession,
  input: TensorInput,
): Promise<ort.Tensor> {
  const tensor = new ort.Tensor("float32", input.data, input.dims);
  const outputs = await session.run({ [session.inputNames[0]]: tensor });
  const output = outputs[session.outputNames[0]];
  if (!output) {
    throw new Error("model did not return an output tensor");
  }
  return output;
}

function tensorToGray(
  output: ort.Tensor,
  anime2sketch: boolean,
  width: number,
  height: number,
): GrayImage {
  const dims = output.dims;
  if (dims.length !== 4 || dims[0] !== 1 || dims[1] !== 1) {
    throw new Error(`unexpected model output shape: ${dims.join("x")}`);
  }
  const outHeight = Number(dims[2]);
  const outWidth = Number(dims[3]);
  const grayWidth = Math.min(width, outWidth);
  const grayHeight = Math.min(height, outHeight);
  const data = tensorFloatData(output);
  const gray = new Uint8Array(grayWidth * grayHeight);

  for (let y = 0; y < grayHeight; y += 1) {
    for (let x = 0; x < grayWidth; x += 1) {
      let value = data[y * outWidth + x] ?? 0;
      if (anime2sketch) {
        value = (value + 1.0) / 2.0;
      }
      gray[y * grayWidth + x] = Math.round(clamp(value, 0, 1) * 255);
    }
  }

  return { width: grayWidth, height: grayHeight, data: gray };
}

function tensorFloatData(output: ort.Tensor): Float32Array {
  if (output.data instanceof Float32Array) {
    return output.data;
  }
  return Float32Array.from(output.data as Iterable<number>);
}

function rgbaToRgb(source: RgbaSource): RgbImage {
  const data = new Uint8Array(source.width * source.height * 3);
  for (let i = 0, j = 0; i < source.rgba.length; i += 4, j += 3) {
    data[j] = source.rgba[i] ?? 0;
    data[j + 1] = source.rgba[i + 1] ?? 0;
    data[j + 2] = source.rgba[i + 2] ?? 0;
  }
  return { width: source.width, height: source.height, data };
}

function cloneRgb(image: RgbImage): RgbImage {
  return { width: image.width, height: image.height, data: image.data.slice() };
}

function resizeRgbPreservingAspect(image: RgbImage, maxSide: number): RgbImage {
  const longest = Math.max(image.width, image.height, 1);
  if (longest <= maxSide) {
    return cloneRgb(image);
  }
  const scale = maxSide / longest;
  const width = Math.max(1, Math.round(image.width * scale));
  const height = Math.max(1, Math.round(image.height * scale));
  return resizeRgb(image, width, height);
}

function resizeRgb(image: RgbImage, width: number, height: number): RgbImage {
  const data = new Uint8Array(width * height * 3);
  for (let y = 0; y < height; y += 1) {
    const sourceY = (y + 0.5) * (image.height / height) - 0.5;
    const y0 = Math.floor(clamp(sourceY, 0, image.height - 1));
    const y1 = Math.min(image.height - 1, y0 + 1);
    const wy = sourceY - y0;
    for (let x = 0; x < width; x += 1) {
      const sourceX = (x + 0.5) * (image.width / width) - 0.5;
      const x0 = Math.floor(clamp(sourceX, 0, image.width - 1));
      const x1 = Math.min(image.width - 1, x0 + 1);
      const wx = sourceX - x0;
      const out = (y * width + x) * 3;
      for (let channel = 0; channel < 3; channel += 1) {
        const top =
          sampleRgb(image, x0, y0, channel) * (1 - wx) + sampleRgb(image, x1, y0, channel) * wx;
        const bottom =
          sampleRgb(image, x0, y1, channel) * (1 - wx) + sampleRgb(image, x1, y1, channel) * wx;
        data[out + channel] = Math.round(top * (1 - wy) + bottom * wy);
      }
    }
  }
  return { width, height, data };
}

function resizeGray(image: GrayImage, width: number, height: number): GrayImage {
  const data = new Uint8Array(width * height);
  for (let y = 0; y < height; y += 1) {
    const sourceY = (y + 0.5) * (image.height / height) - 0.5;
    const y0 = Math.floor(clamp(sourceY, 0, image.height - 1));
    const y1 = Math.min(image.height - 1, y0 + 1);
    const wy = sourceY - y0;
    for (let x = 0; x < width; x += 1) {
      const sourceX = (x + 0.5) * (image.width / width) - 0.5;
      const x0 = Math.floor(clamp(sourceX, 0, image.width - 1));
      const x1 = Math.min(image.width - 1, x0 + 1);
      const wx = sourceX - x0;
      const top = sampleGray(image, x0, y0) * (1 - wx) + sampleGray(image, x1, y0) * wx;
      const bottom = sampleGray(image, x0, y1) * (1 - wx) + sampleGray(image, x1, y1) * wx;
      data[y * width + x] = Math.round(top * (1 - wy) + bottom * wy);
    }
  }
  return { width, height, data };
}

function padRgbToMultipleOfEight(image: RgbImage): { image: RgbImage; width: number; height: number } {
  const width = image.width;
  const height = image.height;
  const paddedWidth = Math.ceil(width / 8) * 8;
  const paddedHeight = Math.ceil(height / 8) * 8;
  const data = new Uint8Array(paddedWidth * paddedHeight * 3);
  for (let y = 0; y < paddedHeight; y += 1) {
    for (let x = 0; x < paddedWidth; x += 1) {
      const sourceX = Math.min(x, width - 1);
      const sourceY = Math.min(y, height - 1);
      const out = (y * paddedWidth + x) * 3;
      const input = (sourceY * width + sourceX) * 3;
      data[out] = image.data[input] ?? 0;
      data[out + 1] = image.data[input + 1] ?? 0;
      data[out + 2] = image.data[input + 2] ?? 0;
    }
  }
  return { image: { width: paddedWidth, height: paddedHeight, data }, width, height };
}

function padTwoGrayToMultipleOfEight(
  first: GrayImage,
  second: GrayImage,
): { first: GrayImage; second: GrayImage; width: number; height: number } {
  const width = first.width;
  const height = first.height;
  const paddedWidth = Math.ceil(width / 8) * 8;
  const paddedHeight = Math.ceil(height / 8) * 8;
  const firstData = new Uint8Array(paddedWidth * paddedHeight);
  const secondData = new Uint8Array(paddedWidth * paddedHeight);
  for (let y = 0; y < paddedHeight; y += 1) {
    for (let x = 0; x < paddedWidth; x += 1) {
      const sourceX = Math.min(x, width - 1);
      const sourceY = Math.min(y, height - 1);
      const out = y * paddedWidth + x;
      firstData[out] = sampleGray(first, sourceX, sourceY);
      secondData[out] = sampleGray(second, sourceX, sourceY);
    }
  }
  return {
    first: { width: paddedWidth, height: paddedHeight, data: firstData },
    second: { width: paddedWidth, height: paddedHeight, data: secondData },
    width,
    height,
  };
}

function letterboxRgb(
  image: RgbImage,
  size: number,
): {
  image: RgbImage;
  offsetX: number;
  offsetY: number;
  contentWidth: number;
  contentHeight: number;
} {
  const scale = size / Math.max(image.width, image.height, 1);
  const contentWidth = Math.max(1, Math.min(size, Math.round(image.width * scale)));
  const contentHeight = Math.max(1, Math.min(size, Math.round(image.height * scale)));
  const resized = resizeRgb(image, contentWidth, contentHeight);
  const offsetX = Math.floor((size - contentWidth) / 2);
  const offsetY = Math.floor((size - contentHeight) / 2);
  const data = new Uint8Array(size * size * 3);
  data.fill(255);

  for (let y = 0; y < contentHeight; y += 1) {
    for (let x = 0; x < contentWidth; x += 1) {
      const input = (y * contentWidth + x) * 3;
      const out = ((y + offsetY) * size + x + offsetX) * 3;
      data[out] = resized.data[input] ?? 255;
      data[out + 1] = resized.data[input + 1] ?? 255;
      data[out + 2] = resized.data[input + 2] ?? 255;
    }
  }

  return {
    image: { width: size, height: size, data },
    offsetX,
    offsetY,
    contentWidth,
    contentHeight,
  };
}

function cropGray(image: GrayImage, x: number, y: number, width: number, height: number): GrayImage {
  const data = new Uint8Array(width * height);
  for (let row = 0; row < height; row += 1) {
    for (let col = 0; col < width; col += 1) {
      data[row * width + col] = sampleGray(image, x + col, y + row);
    }
  }
  return { width, height, data };
}

function rgbToTensor01(image: RgbImage): TensorInput {
  const data = new Float32Array(3 * image.width * image.height);
  const plane = image.width * image.height;
  for (let y = 0; y < image.height; y += 1) {
    for (let x = 0; x < image.width; x += 1) {
      const input = (y * image.width + x) * 3;
      const pixel = y * image.width + x;
      data[pixel] = (image.data[input] ?? 0) / 255;
      data[plane + pixel] = (image.data[input + 1] ?? 0) / 255;
      data[plane * 2 + pixel] = (image.data[input + 2] ?? 0) / 255;
    }
  }
  return { data, dims: [1, 3, image.height, image.width] };
}

function rgbToTensorMinusOneToOne(image: RgbImage): TensorInput {
  const tensor = rgbToTensor01(image);
  for (let index = 0; index < tensor.data.length; index += 1) {
    tensor.data[index] = (tensor.data[index] - 0.5) / 0.5;
  }
  return tensor;
}

function twoGrayToTensor01(first: GrayImage, second: GrayImage): TensorInput {
  const plane = first.width * first.height;
  const data = new Float32Array(2 * plane);
  for (let y = 0; y < first.height; y += 1) {
    for (let x = 0; x < first.width; x += 1) {
      const pixel = y * first.width + x;
      data[pixel] = sampleGray(first, x, y) / 255;
      data[plane + pixel] = sampleGray(second, x, y) / 255;
    }
  }
  return { data, dims: [1, 2, first.height, first.width] };
}

function rgbToGray(image: RgbImage): GrayImage {
  const data = new Uint8Array(image.width * image.height);
  for (let y = 0; y < image.height; y += 1) {
    for (let x = 0; x < image.width; x += 1) {
      const input = (y * image.width + x) * 3;
      const value =
        0.299 * (image.data[input] ?? 0) +
        0.587 * (image.data[input + 1] ?? 0) +
        0.114 * (image.data[input + 2] ?? 0);
      data[y * image.width + x] = Math.round(clamp(value, 0, 255));
    }
  }
  return { width: image.width, height: image.height, data };
}

function invertedSobel(gray: GrayImage): GrayImage {
  const magnitudes = new Float32Array(gray.width * gray.height);
  let maxMagnitude = 0;
  for (let y = 0; y < gray.height; y += 1) {
    for (let x = 0; x < gray.width; x += 1) {
      const gx =
        -sampleGray(gray, x - 1, y - 1) +
        sampleGray(gray, x + 1, y - 1) -
        2 * sampleGray(gray, x - 1, y) +
        2 * sampleGray(gray, x + 1, y) -
        sampleGray(gray, x - 1, y + 1) +
        sampleGray(gray, x + 1, y + 1);
      const gy =
        -sampleGray(gray, x - 1, y - 1) -
        2 * sampleGray(gray, x, y - 1) -
        sampleGray(gray, x + 1, y - 1) +
        sampleGray(gray, x - 1, y + 1) +
        2 * sampleGray(gray, x, y + 1) +
        sampleGray(gray, x + 1, y + 1);
      const magnitude = Math.sqrt(gx * gx + gy * gy);
      magnitudes[y * gray.width + x] = magnitude;
      maxMagnitude = Math.max(maxMagnitude, magnitude);
    }
  }

  const data = new Uint8Array(gray.width * gray.height);
  for (let index = 0; index < data.length; index += 1) {
    const normalized = maxMagnitude > 0 ? (magnitudes[index] ?? 0) / maxMagnitude : 0;
    data[index] = Math.round((1 - clamp(normalized, 0, 1)) * 255);
  }
  return { width: gray.width, height: gray.height, data };
}

function sharpenRgb(image: RgbImage, amount: number): RgbImage {
  const blurred = gaussianBlurRgb(image, 1.0);
  const data = new Uint8Array(image.data.length);
  for (let index = 0; index < image.data.length; index += 1) {
    const value = (image.data[index] ?? 0) + ((image.data[index] ?? 0) - (blurred.data[index] ?? 0)) * amount;
    data[index] = Math.round(clamp(value, 0, 255));
  }
  return { width: image.width, height: image.height, data };
}

function gaussianBlurRgb(image: RgbImage, sigma: number): RgbImage {
  const kernel = gaussianKernel(sigma);
  const radius = Math.floor(kernel.length / 2);
  const temp = new Float32Array(image.data.length);
  const data = new Uint8Array(image.data.length);

  for (let y = 0; y < image.height; y += 1) {
    for (let x = 0; x < image.width; x += 1) {
      for (let channel = 0; channel < 3; channel += 1) {
        let sum = 0;
        for (let k = -radius; k <= radius; k += 1) {
          const sampleX = Math.round(clamp(x + k, 0, image.width - 1));
          sum += sampleRgb(image, sampleX, y, channel) * (kernel[k + radius] ?? 0);
        }
        temp[(y * image.width + x) * 3 + channel] = sum;
      }
    }
  }

  for (let y = 0; y < image.height; y += 1) {
    for (let x = 0; x < image.width; x += 1) {
      for (let channel = 0; channel < 3; channel += 1) {
        let sum = 0;
        for (let k = -radius; k <= radius; k += 1) {
          const sampleY = Math.round(clamp(y + k, 0, image.height - 1));
          sum += temp[(sampleY * image.width + x) * 3 + channel] * (kernel[k + radius] ?? 0);
        }
        data[(y * image.width + x) * 3 + channel] = Math.round(clamp(sum, 0, 255));
      }
    }
  }

  return { width: image.width, height: image.height, data };
}

function gaussianKernel(sigma: number): Float32Array {
  const radius = Math.ceil(sigma * 3);
  const kernel = new Float32Array(radius * 2 + 1);
  let sum = 0;
  for (let index = 0; index < kernel.length; index += 1) {
    const x = index - radius;
    const value = Math.exp(-(x * x) / (2 * sigma * sigma));
    kernel[index] = value;
    sum += value;
  }
  for (let index = 0; index < kernel.length; index += 1) {
    kernel[index] /= sum;
  }
  return kernel;
}

function grayToRgba(gray: GrayImage): Uint8Array {
  const rgba = new Uint8Array(gray.width * gray.height * 4);
  for (let i = 0, j = 0; i < gray.data.length; i += 1, j += 4) {
    const value = gray.data[i] ?? 0;
    rgba[j] = value;
    rgba[j + 1] = value;
    rgba[j + 2] = value;
    rgba[j + 3] = 255;
  }
  return rgba;
}

function sampleRgb(image: RgbImage, x: number, y: number, channel: number): number {
  const clampedX = Math.round(clamp(x, 0, image.width - 1));
  const clampedY = Math.round(clamp(y, 0, image.height - 1));
  return image.data[(clampedY * image.width + clampedX) * 3 + channel] ?? 0;
}

function sampleGray(image: GrayImage, x: number, y: number): number {
  const clampedX = Math.round(clamp(x, 0, image.width - 1));
  const clampedY = Math.round(clamp(y, 0, image.height - 1));
  return image.data[clampedY * image.width + clampedX] ?? 0;
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}
