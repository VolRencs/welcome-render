import { readFile } from "node:fs/promises";

let NativeWelcomeRenderer;
let nativeNormalizeImage;
let nativeProbeImage;
let nativeLoadError;

try {
  const native = await import("./native.js");
  NativeWelcomeRenderer = native.NativeWelcomeRenderer;
  nativeNormalizeImage = native.normalizeImage;
  nativeProbeImage = native.probeImage;
} catch (error) {
  nativeLoadError = error;
}

const ERROR_CODE = /^\[([A-Z_]+)\]\s+(.*)$/s;

function nativeUnavailable() {
  const detail = nativeLoadError instanceof Error ? nativeLoadError.message : String(nativeLoadError ?? "native addon is unavailable");
  return new RenderError("NATIVE_LOAD", `native addon is unavailable: ${detail}`, { cause: nativeLoadError });
}

function normalizeScene(scene) {
  if (!scene || typeof scene !== "object") return scene;
  const normalized = { ...scene };
  for (const key of ["background", "avatar", "overlay", "avatarEllipse", "backgroundKey", "format", "quality", "strictBackground"]) {
    if (normalized[key] == null) delete normalized[key];
  }
  if (Array.isArray(normalized.texts)) {
    normalized.texts = normalized.texts.map((text) => {
      if (!text || text.weight != null) return text;
      const copy = { ...text };
      delete copy.weight;
      return copy;
    });
  }
  return normalized;
}

export class RenderError extends Error {
  constructor(code, message, options) {
    super(message, options);
    this.name = "RenderError";
    this.code = code;
  }
}

function toRenderError(error) {
  if (error instanceof RenderError) return error;
  const message = error instanceof Error ? error.message : String(error);
  const match = ERROR_CODE.exec(message);
  if (!match) {
    return error instanceof Error
      ? error
      : new RenderError("INTERNAL", message);
  }
  return new RenderError(match[1], match[2], { cause: error });
}

async function readFont(pathOrUrl, label) {
  try {
    return await readFile(pathOrUrl);
  } catch (error) {
    throw new RenderError(
      "FONT_READ",
      `failed to read ${label}: ${String(pathOrUrl)}`,
      { cause: error },
    );
  }
}

export class WelcomeRenderer {
  #native;

  constructor(fonts, options) {
    if (!NativeWelcomeRenderer) throw nativeUnavailable();
    if (!fonts || !fonts.regular) {
      throw new RenderError("INVALID_ARGUMENT", "regular font data is required");
    }
    try {
      this.#native = new NativeWelcomeRenderer(fonts, options);
    } catch (error) {
      throw toRenderError(error);
    }
  }

  static async fromFiles({
    regular,
    bold,
    fallback = [],
    systemFonts,
    cacheBytes,
    maxDimension,
    maxPixels,
  }) {
    if (!NativeWelcomeRenderer) throw nativeUnavailable();
    if (!regular) {
      throw new RenderError("INVALID_ARGUMENT", "regular font path is required");
    }
    if (!Array.isArray(fallback)) {
      throw new RenderError("INVALID_ARGUMENT", "fallback must be an array of font paths");
    }

    const [regularData, boldData, fallbackData] = await Promise.all([
      readFont(regular, "regular font"),
      bold ? readFont(bold, "bold font") : undefined,
      Promise.all(
        fallback.map((path, index) => readFont(path, `fallback font ${index}`)),
      ),
    ]);

    const options = {};
    if (systemFonts !== undefined) options.systemFonts = systemFonts;
    if (cacheBytes !== undefined) options.cacheBytes = cacheBytes;
    if (maxDimension !== undefined) options.maxDimension = maxDimension;
    if (maxPixels !== undefined) options.maxPixels = maxPixels;

    return new WelcomeRenderer(
      { regular: regularData, bold: boldData, fallback: fallbackData },
      options,
    );
  }

  render(scene) {
    try {
      return this.#native.render(normalizeScene(scene));
    } catch (error) {
      throw toRenderError(error);
    }
  }

  async renderAsync(scene) {
    try {
      return await this.#native.renderAsync(normalizeScene(scene));
    } catch (error) {
      throw toRenderError(error);
    }
  }

  clearCache() {
    return this.#native.clearCache();
  }

  get stats() {
    return this.#native.stats;
  }

  dispose() {
    this.#native.dispose();
  }

  [Symbol.dispose]() {
    this.dispose();
  }
}

export function probeImage(data) {
  if (!nativeProbeImage) throw nativeUnavailable();
  try {
    return nativeProbeImage(data);
  } catch (error) {
    throw toRenderError(error);
  }
}

export async function normalizeImage(data, options) {
  if (!nativeNormalizeImage) throw nativeUnavailable();
  try {
    return await nativeNormalizeImage(data, options);
  } catch (error) {
    throw toRenderError(error);
  }
}

export default WelcomeRenderer;
