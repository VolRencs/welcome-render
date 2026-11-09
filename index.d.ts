/// <reference types="node" />

export interface Fonts {
  regular: Buffer;
  bold?: Buffer | undefined;
  fallback?: Buffer[] | undefined;
}

export interface RendererOptions {
  /** Load system fonts once during construction. Default: true. */
  systemFonts?: boolean | undefined;
  /** Maximum bytes kept in the decoded-background cache. Default: 64 MiB. */
  cacheBytes?: number | undefined;
  /** Maximum image dimension. Default: 4096. */
  maxDimension?: number | undefined;
  /** Maximum decoded pixel count. Default: 4096 * 4096. */
  maxPixels?: number | undefined;
}

export interface FromFilesOptions extends RendererOptions {
  regular: string | URL;
  bold?: string | URL | undefined;
  fallback?: (string | URL)[] | undefined;
}

export type ImageFormat = "png" | "jpeg" | "webp";

export interface ImageInfo {
  width: number;
  height: number;
  format: ImageFormat;
  orientation: number;
}

export interface NormalizeImageOptions {
  maxDimension?: number | undefined;
  maxPixels?: number | undefined;
  format?: "png" | "jpeg" | "jpg" | undefined;
  quality?: number | undefined;
  background?: string | undefined;
}

export interface NormalizedImage {
  data: Buffer;
  width: number;
  height: number;
  format: "png" | "jpeg";
  originalWidth: number;
  originalHeight: number;
  orientation: number;
}

export interface Overlay {
  color: string;
  opacity: number;
}

export interface Ellipse {
  cx: number;
  cy: number;
  rx: number;
  ry: number;
}

export interface TextSpec {
  text: string;
  x: number;
  y: number;
  size: number;
  color: string;
  /** CSS-like weight. Values >= 600 select the bold face. Default: 400. */
  weight?: number | undefined;
}

export interface Scene {
  /** Defaults to the background size, or 900x480 without a background. */
  width?: number | undefined;
  height?: number | undefined;
  /** Encoded PNG, JPEG or WebP. */
  background?: Buffer | null | undefined;
  /** Cache key for the decoded background, e.g. `${mtime}:${size}`. */
  backgroundKey?: string | null | undefined;
  /** Default: black at 0.25 opacity. */
  overlay?: Overlay | null | undefined;
  /** Encoded PNG, JPEG or WebP. */
  avatar?: Buffer | null | undefined;
  avatarEllipse?: Ellipse | null | undefined;
  texts: TextSpec[];
  format?: "png" | "jpeg" | "jpg" | undefined;
  quality?: number | undefined;
  strictBackground?: boolean | undefined;
}

export interface RenderStats {
  renders: number;
  cacheHits: number;
  cacheMisses: number;
  cacheBytes: number;
  cacheEntries: number;
  backgroundFailures: number;
  avatarFailures: number;
}

export function probeImage(data: Buffer): ImageInfo;

export function normalizeImage(
  data: Buffer,
  options?: NormalizeImageOptions,
): Promise<NormalizedImage>;

export class RenderError extends Error {
  readonly code: string;
  constructor(code: string, message: string, options?: ErrorOptions);
}

export class WelcomeRenderer {
  constructor(fonts: Fonts, options?: RendererOptions);

  static fromFiles(options: FromFilesOptions): Promise<WelcomeRenderer>;

  render(scene: Scene): Buffer;
  renderAsync(scene: Scene): Promise<Buffer>;

  /** Clears the decoded-background cache. Returns the number of bytes freed. */
  clearCache(): number;

  readonly stats: RenderStats;

  dispose(): void;
  [Symbol.dispose](): void;
}

export default WelcomeRenderer;
