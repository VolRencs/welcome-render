import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { monitorEventLoopDelay, performance } from "node:perf_hooks";

import { Resvg } from "@resvg/resvg-js";

import { WelcomeRenderer, type Scene } from "../index.js";

const root = join(import.meta.dirname, "..");
const fixtures = join(root, "test", "fixtures");
const regular = join(fixtures, "fonts", "Onest-Regular.ttf");
const bold = join(fixtures, "fonts", "Onest-Bold.ttf");

const [background900, background1920, jpeg1920, avatar] = await Promise.all([
  readFile(join(fixtures, "images", "background.png")),
  readFile(join(fixtures, "images", "background-1920.png")),
  readFile(join(fixtures, "images", "background-1920.jpg")),
  readFile(join(fixtures, "images", "avatar.png")),
]);

const title = "Новый участник";
const subtitle = "Добро пожаловать на сервер! · Участник #123";

function dataUri(data: Buffer, mime: string) {
  return `data:${mime};base64,${data.toString("base64")}`;
}

function resvgSvg(background: Buffer | null, backgroundMime: string, width: number, height: number) {
  const scale = width / 900;
  const avatarWidth = 164 * scale;
  const avatarHeight = 164 * scale;
  const cx = 450 * scale;
  const cy = 175 * scale;
  const backgroundMarkup = background
    ? `<image href="${dataUri(background, backgroundMime)}" width="${width}" height="${height}"/>`
    : `<defs><linearGradient id="g"><stop stop-color="#111827"/><stop offset="1" stop-color="#312e81"/></linearGradient></defs><rect width="100%" height="100%" fill="url(#g)"/>`;
  return `<svg viewBox="0 0 ${width} ${height}" width="${width}" height="${height}" xmlns="http://www.w3.org/2000/svg">
    <rect width="100%" height="100%" fill="#111827"/>
    ${backgroundMarkup}
    <rect width="100%" height="100%" fill="#000" opacity=".25"/>
    <clipPath id="c"><ellipse cx="${cx}" cy="${cy}" rx="${avatarWidth / 2}" ry="${avatarHeight / 2}"/></clipPath>
    <image href="${dataUri(avatar, "image/png")}" x="${cx - avatarWidth / 2}" y="${cy - avatarHeight / 2}" width="${avatarWidth}" height="${avatarHeight}" preserveAspectRatio="xMidYMid slice" clip-path="url(#c)"/>
    <text x="${cx}" y="${315 * scale}" text-anchor="middle" fill="#f8fafc" font-family="Onest, sans-serif" font-size="${38 * scale}" font-weight="700">${title}</text>
    <text x="${cx}" y="${360 * scale}" text-anchor="middle" fill="#e2e8f0" font-family="Onest, sans-serif" font-size="${22 * scale}">${subtitle}</text>
  </svg>`;
}

function scene(background: Buffer | null, backgroundKey: string, width: number, height: number): Scene {
  const scale = width / 900;
  return {
    width,
    height,
    ...(background ? { background, backgroundKey } : {}),
    overlay: { color: "#000000", opacity: 0.25 },
    avatar,
    avatarEllipse: { cx: 450 * scale, cy: 175 * scale, rx: 82 * scale, ry: 82 * scale },
    texts: [
      { text: title, x: 450 * scale, y: 315 * scale, size: 38 * scale, color: "#f8fafc", weight: 700 },
      { text: subtitle, x: 450 * scale, y: 360 * scale, size: 22 * scale, color: "#e2e8f0" },
    ],
  };
}

function summary(label: string, times: number[]) {
  const sorted = [...times].sort((a, b) => a - b);
  const median = sorted[Math.floor(sorted.length / 2)] ?? 0;
  const p95 = sorted[Math.floor(sorted.length * 0.95)] ?? 0;
  return {
    scenario: label,
    "median ms": +median.toFixed(2),
    "p95 ms": +p95.toFixed(2),
  };
}

const runs = Number(process.env.BENCH_RUNS ?? 30);

const renderer = await WelcomeRenderer.fromFiles({
  regular,
  bold,
  systemFonts: false,
  cacheBytes: 64 * 1024 * 1024,
});

async function bench(name: string, width: number, height: number, background: Buffer | null, mime: string) {
  const svg = resvgSvg(background, mime, width, height);
  const sceneInput = scene(background, `${name}-${width}x${height}`, width, height);

  const resvgSystem: number[] = [];
  const resvgNoSystem: number[] = [];
  for (let index = 0; index < runs; index += 1) {
    const started = performance.now();
    new Resvg(svg, { font: { fontFiles: [regular, bold] } }).render().asPng();
    resvgSystem.push(performance.now() - started);

    const optimizedStart = performance.now();
    new Resvg(svg, {
      font: { fontFiles: [regular, bold], loadSystemFonts: false },
    }).render().asPng();
    resvgNoSystem.push(performance.now() - optimizedStart);
  }

  await renderer.renderAsync(sceneInput);
  const warm: number[] = [];
  const cold: number[] = [];
  for (let index = 0; index < runs; index += 1) {
    const started = performance.now();
    await renderer.renderAsync(sceneInput);
    warm.push(performance.now() - started);

    renderer.clearCache();
    const coldStart = performance.now();
    await renderer.renderAsync(sceneInput);
    cold.push(performance.now() - coldStart);
  }

  return [
    summary(`resvg  ${name} (system fonts)`, resvgSystem),
    summary(`resvg  ${name} (no system fonts)`, resvgNoSystem),
    summary(`ours   ${name} (warm cache)`, warm),
    summary(`ours   ${name} (cold cache)`, cold),
  ];
}

const rows = [
  ...(await bench("900x480", 900, 480, background900, "image/png")),
  ...(await bench("1920x1080 png", 1920, 1080, background1920, "image/png")),
  ...(await bench("1920x1080 jpg", 1920, 1080, jpeg1920, "image/jpeg")),
];

console.table(rows);

const histogram = monitorEventLoopDelay({ resolution: 10 });
histogram.enable();
await Promise.all(
  Array.from({ length: 20 }, (_, index) =>
    renderer.renderAsync(scene(background1920, `event-loop-${index % 2}`, 1920, 1080)),
  ),
);
histogram.disable();

console.log("event loop delay (ms)", {
  mean: +(histogram.mean / 1e6).toFixed(2),
  p95: +(histogram.percentile(95) / 1e6).toFixed(2),
  max: +(histogram.max / 1e6).toFixed(2),
});

renderer.dispose();
