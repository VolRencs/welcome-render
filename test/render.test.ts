import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { after, before, describe, test } from "node:test";

import { RenderError, WelcomeRenderer, type Scene } from "../index.js";

const fixtures = join(import.meta.dirname, "fixtures");
const fontRegular = join(fixtures, "fonts/Onest-Regular.ttf");
const fontBold = join(fixtures, "fonts/Onest-Bold.ttf");

const [backgroundPng, backgroundJpg, backgroundWebp, avatar] = await Promise.all([
  readFile(join(fixtures, "images/background.png")),
  readFile(join(fixtures, "images/background.jpg")),
  readFile(join(fixtures, "images/background.webp")),
  readFile(join(fixtures, "images/avatar.png")),
]);

type LooseScene = { [K in keyof Scene]?: Scene[K] | undefined };

function scene(overrides: LooseScene = {}): Scene {
  return {
    width: 900,
    height: 480,
    background: backgroundPng,
    backgroundKey: "fixture",
    overlay: { color: "#000000", opacity: 0.25 },
    avatar,
    avatarEllipse: { cx: 450, cy: 175, rx: 82, ry: 82 },
    texts: [
      { text: "Новый участник", x: 450, y: 315, size: 38, color: "#f8fafc", weight: 700 },
      {
        text: "Добро пожаловать на сервер! · Участник #123",
        x: 450,
        y: 360,
        size: 22,
        color: "#e2e8f0",
      },
    ],
    ...overrides,
  } as Scene;
}

function pngInfo(png: Buffer) {
  assert.equal(png.subarray(0, 8).toString("hex"), "89504e470d0a1a0a");
  return {
    width: png.readUInt32BE(16),
    height: png.readUInt32BE(20),
    colorType: png[25],
  };
}

describe("WelcomeRenderer", () => {
  let renderer: WelcomeRenderer;

  before(async () => {
    renderer = await WelcomeRenderer.fromFiles({
      regular: fontRegular,
      bold: fontBold,
      systemFonts: false,
      cacheBytes: 32 * 1024 * 1024,
    });
  });

  after(() => {
    renderer.dispose();
  });

  test("renders an opaque RGB PNG", async () => {
    const png = await renderer.renderAsync(scene());
    const info = pngInfo(png);
    assert.deepEqual(info, { width: 900, height: 480, colorType: 2 });
  });

  test("accepts PNG, JPEG and WebP backgrounds", async () => {
    const cases = [
      { data: backgroundPng, width: 900, height: 480 },
      { data: backgroundJpg, width: 640, height: 360 },
      { data: backgroundWebp, width: 320, height: 200 },
    ];
    for (const item of cases) {
      const png = await renderer.renderAsync(
        scene({
          width: item.width,
          height: item.height,
          background: item.data,
          backgroundKey: `${item.width}x${item.height}`,
          avatarEllipse: { cx: item.width / 2, cy: item.height / 3, rx: 50, ry: 50 },
          texts: [{ text: "Тест", x: item.width / 2, y: item.height * 0.8, size: 24, color: "#ffffff" }],
        }),
      );
      const info = pngInfo(png);
      assert.equal(info.width, item.width);
      assert.equal(info.height, item.height);
    }
  });

  test("uses design size without a background", async () => {
    const png = await renderer.renderAsync(
      scene({ width: undefined, height: undefined, background: undefined, backgroundKey: undefined }),
    );
    assert.deepEqual(pngInfo(png), { width: 900, height: 480, colorType: 2 });
  });

  test("normalizes null optional fields", async () => {
    const png = await renderer.renderAsync(
      scene({
        background: null,
        backgroundKey: null,
        overlay: null,
        avatar: null,
        avatarEllipse: null,
      }),
    );
    assert.equal(pngInfo(png).width, 900);
  });

  test("text and avatar change the output", async () => {
    const plain = await renderer.renderAsync(
      scene({ avatar: null, avatarEllipse: null, texts: [] }),
    );
    const withText = await renderer.renderAsync(scene({ avatar: null, avatarEllipse: null }));
    const withAvatar = await renderer.renderAsync(scene({ texts: [] }));
    assert.equal(plain.equals(withText), false);
    assert.equal(plain.equals(withAvatar), false);
  });

  test("caches decoded backgrounds", async () => {
    const local = await WelcomeRenderer.fromFiles({
      regular: fontRegular,
      bold: fontBold,
      systemFonts: false,
      cacheBytes: 16 * 1024 * 1024,
    });
    try {
      await local.renderAsync(scene({ backgroundKey: "cache-test" }));
      assert.equal(local.stats.cacheMisses, 1);
      await local.renderAsync(scene({ backgroundKey: "cache-test" }));
      assert.equal(local.stats.cacheHits, 1);
      assert.ok(local.stats.cacheBytes > 0);
      const freed = local.clearCache();
      assert.ok(freed > 0);
      assert.equal(local.stats.cacheBytes, 0);
    } finally {
      local.dispose();
    }
  });

  test("rejects invalid colors with a code", async () => {
    await assert.rejects(
      () =>
        renderer.renderAsync(
          scene({ texts: [{ text: "x", x: 10, y: 10, size: 12, color: "red" }] }),
        ),
      (error: unknown) => {
        assert.ok(error instanceof RenderError);
        assert.equal(error.code, "INVALID_ARGUMENT");
        return true;
      },
    );
  });

  test("rejects one-sided dimensions", async () => {
    await assert.rejects(
      () => renderer.renderAsync(scene({ width: 900, height: undefined })),
      (error: unknown) => {
        assert.ok(error instanceof RenderError);
        assert.equal(error.code, "INVALID_ARGUMENT");
        return true;
      },
    );
  });

  test("rejects too many text layers", async () => {
    const texts = Array.from({ length: 17 }, (_, index) => ({
      text: `line ${index}`,
      x: 10,
      y: 10 + index,
      size: 10,
      color: "#ffffff",
    }));
    await assert.rejects(
      () => renderer.renderAsync(scene({ texts })),
      (error: unknown) => {
        assert.ok(error instanceof RenderError);
        assert.equal(error.code, "INVALID_ARGUMENT");
        return true;
      },
    );
  });

  test("ignores a broken avatar", async () => {
    const png = await renderer.renderAsync(scene({ avatar: Buffer.from("not an image") }));
    assert.equal(pngInfo(png).width, 900);
  });

  test("falls back to a gradient for a broken background", async () => {
    const png = await renderer.renderAsync(
      scene({ background: Buffer.from("not an image"), width: undefined, height: undefined }),
    );
    assert.deepEqual(pngInfo(png), { width: 900, height: 480, colorType: 2 });
  });

  test("clamps oversized canvases proportionally", async () => {
    const local = await WelcomeRenderer.fromFiles({
      regular: fontRegular,
      bold: fontBold,
      systemFonts: false,
      maxDimension: 320,
      cacheBytes: 0,
    });
    try {
      const png = await local.renderAsync(scene({ width: 1920, height: 1080 }));
      assert.deepEqual(pngInfo(png), { width: 320, height: 180, colorType: 2 });
    } finally {
      local.dispose();
    }
  });

  test("supports synchronous render", () => {
    const png = renderer.render(scene());
    assert.equal(pngInfo(png).width, 900);
  });

  test("wraps font read and parse errors", async () => {
    await assert.rejects(
      () =>
        WelcomeRenderer.fromFiles({
          regular: join(fixtures, "fonts/does-not-exist.ttf"),
          systemFonts: false,
        }),
      (error: unknown) => {
        assert.ok(error instanceof RenderError);
        assert.equal(error.code, "FONT_READ");
        return true;
      },
    );

    assert.throws(
      () => new WelcomeRenderer({ regular: Buffer.from("not a font") }),
      (error: unknown) => {
        assert.ok(error instanceof RenderError);
        assert.equal(error.code, "FONT_PARSE");
        return true;
      },
    );
  });

  test("supports Symbol.dispose via fromFiles", async () => {
    using local = await WelcomeRenderer.fromFiles({
      regular: new URL("./fixtures/fonts/Onest-Regular.ttf", import.meta.url),
      systemFonts: false,
      cacheBytes: 0,
    });
    const png = await local.renderAsync(scene());
    assert.equal(pngInfo(png).height, 480);
  });

  test("renders CJK and RTL text with system fallback", async () => {
    using local = await WelcomeRenderer.fromFiles({
      regular: fontRegular,
      bold: fontBold,
      systemFonts: true,
      cacheBytes: 0,
    });
    const png = await local.renderAsync(
      scene({
        avatar: null,
        avatarEllipse: null,
        texts: [
          { text: "新しいメンバー", x: 450, y: 200, size: 36, color: "#ffffff" },
          { text: "مرحبا بك في السيرفر", x: 450, y: 280, size: 36, color: "#ffffff" },
          { text: "🎉 Добро пожаловать", x: 450, y: 360, size: 36, color: "#ffffff" },
        ],
      }),
    );
    assert.equal(pngInfo(png).width, 900);
  });
});
