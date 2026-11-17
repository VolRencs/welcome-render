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

const ERROR_CODE = /^\[(?<code>[A-Z_]+)\]\s+(?<message>.*)$/s;
const MAX_U32 = 0xffff_ffff;
const INTEGER_OPTIONS = ["cacheBytes", "maxDimension", "maxPixels"];
const OPTIONAL_SCENE_KEYS = [
  "background",
  "backgroundKey",
  "overlay",
  "avatar",
  "avatarEllipse",
  "format",
  "quality",
  "strictBackground",
  "width",
  "height",
];

function invalid(message) {
  return new RenderError("INVALID_ARGUMENT", message);
}

function nativeUnavailable() {
  const detail =
    nativeLoadError instanceof Error
      ? nativeLoadError.message
      : String(nativeLoadError ?? "native addon is unavailable");
  return new RenderError("NATIVE_LOAD", `native addon is unavailable: ${detail}`, {
    cause: nativeLoadError,
  });
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
    return new RenderError("INTERNAL", message, { cause: error });
  }
  return new RenderError(match.groups.code, match.groups.message, {
    cause: error,
  });
}

function normalizeRendererOptions(options) {
  if (options == null) return undefined;
  if (typeof options !== "object" || Array.isArray(options)) {
    throw invalid("renderer options must be an object");
  }

  const normalized = {};
  if (options.systemFonts != null) {
    if (typeof options.systemFonts !== "boolean") {
      throw invalid("systemFonts must be a boolean");
    }
    normalized.systemFonts = options.systemFonts;
  }

  for (const key of INTEGER_OPTIONS) {
    const value = options[key];
    if (value == null) continue;
    if (!Number.isInteger(value) || value < 0 || value > MAX_U32) {
      throw invalid(`${key} must be an integer between 0 and ${MAX_U32}`);
    }
    if (key !== "cacheBytes" && value === 0) {
      throw invalid(`${key} must be at least 1`);
    }
    normalized[key] = value;
  }

  return Object.keys(normalized).length === 0 ? undefined : normalized;
}

function normalizeScene(scene) {
  if (!scene || typeof scene !== "object" || Array.isArray(scene)) {
    throw invalid("scene must be an object");
  }
  if (!Array.isArray(scene.texts)) {
    throw invalid("scene.texts must be an array");
  }
  for (const text of scene.texts) {
    if (!text || typeof text !== "object" || Array.isArray(text)) {
      throw invalid("scene.texts entries must be objects");
    }
  }

  let normalized = scene;
  const hasNullKey = OPTIONAL_SCENE_KEYS.some(
    (key) => Object.hasOwn(scene, key) && scene[key] == null,
  );
  if (hasNullKey) {
    normalized = { ...scene };
    for (const key of OPTIONAL_SCENE_KEYS) {
      if (Object.hasOwn(normalized, key) && normalized[key] == null) {
        delete normalized[key];
      }
    }
  }

  const textsNeedCopy = scene.texts.some(
    (text) => Object.hasOwn(text, "weight") && text.weight == null,
  );
  if (textsNeedCopy) {
    if (normalized === scene) normalized = { ...scene };
    normalized.texts = scene.texts.map((text) => {
      if (!Object.hasOwn(text, "weight") || text.weight != null) return text;
      const copy = { ...text };
      delete copy.weight;
      return copy;
    });
  }

  const hasWidth = normalized.width != null;
  const hasHeight = normalized.height != null;
  if (hasWidth !== hasHeight) {
    throw invalid("width and height must be provided together");
  }

  return normalized;
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
    if (!fonts || typeof fonts !== "object" || Array.isArray(fonts)) {
      throw invalid("fonts must be an object");
    }
    if (!(fonts.regular instanceof Uint8Array)) {
      throw invalid("regular font data must be a Buffer or Uint8Array");
    }
    if (fonts.bold != null && !(fonts.bold instanceof Uint8Array)) {
      throw invalid("bold font data must be a Buffer or Uint8Array");
    }
    if (
      fonts.fallback != null &&
      (!Array.isArray(fonts.fallback) ||
        fonts.fallback.some((font) => !(font instanceof Uint8Array)))
    ) {
      throw invalid("fallback must be an array of Buffers");
    }

    let nativeFonts = fonts;
    if (fonts.bold === null || fonts.fallback === null) {
      nativeFonts = { ...fonts };
      if (nativeFonts.bold === null) delete nativeFonts.bold;
      if (nativeFonts.fallback === null) delete nativeFonts.fallback;
    }

    try {
      this.#native = new NativeWelcomeRenderer(
        nativeFonts,
        normalizeRendererOptions(options),
      );
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
  } = {}) {
    if (!NativeWelcomeRenderer) throw nativeUnavailable();
    if (regular == null) {
      throw invalid("regular font path is required");
    }
    if (!Array.isArray(fallback)) {
      throw invalid("fallback must be an array of font paths");
    }

    const [regularData, boldData, fallbackData] = await Promise.all([
      readFont(regular, "regular font"),
      bold == null ? undefined : readFont(bold, "bold font"),
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
    try {
      return this.#native.clearCache();
    } catch (error) {
      throw toRenderError(error);
    }
  }

  get stats() {
    try {
      return this.#native.stats;
    } catch (error) {
      throw toRenderError(error);
    }
  }

  dispose() {
    try {
      this.#native.dispose();
    } catch (error) {
      throw toRenderError(error);
    }
  }

  [Symbol.dispose]() {
    this.dispose();
  }

  async [Symbol.asyncDispose]() {
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
