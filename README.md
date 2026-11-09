# welcome-render

Нативный компоновщик сцены для welcome-картинок GOPlay. Не парсит SVG и не
загружает шрифты на каждый рендер: получает готовые координаты, фон, аватар и
текст, а возвращает готовый RGB PNG.

- Rust + N-API 10, только Node.js 26+
- шрифты загружаются один раз при старте
- декодированный фон и затемнение кэшируются в native
- рендер выполняется в libuv thread pool и не блокирует event loop
- без линковки с fontconfig, C-зависимостей и base64 в горячем пути
- один `.node` для `linux-x64-gnu`, без optional-пакетов

## Установка

Пакет распространяется через GitHub Releases:

```bash
pnpm add https://github.com/VolRencs/welcome-render/releases/download/v0.1.0/welcome-render-0.1.0.tgz
```

Требования: Linux x64 с glibc и Node.js >= 26.

## Быстрый старт

```js
import { WelcomeRenderer } from "welcome-render";

const renderer = await WelcomeRenderer.fromFiles({
  regular: "/path/Onest-Regular.ttf",
  bold: "/path/Onest-Bold.ttf",
  systemFonts: true,
  cacheBytes: 64 * 1024 * 1024,
});

const png = await renderer.renderAsync({
  width: 900,
  height: 480,
  background: backgroundBuffer,       // PNG, JPEG или WebP
  backgroundKey: `${mtime}:${size}`,  // ключ кэша фона
  overlay: { color: "#000000", opacity: 0.25 },
  avatar: avatarBuffer,
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
});
```

Без фона canvas будет `900x480` и зальётся градиентом `#111827 → #312e81`.
Если фон передан, размер canvas по умолчанию равен размеру фона.

### Координаты

Пакет не знает про дизайн 900x480 и не масштабирует конфиг. Координаты, размеры
и радиусы передаются уже в пикселях итогового canvas. В GOPlay для этого будет
общий `welcomeLayout(config, width, height)`, который также использует SVG-превью
в дашборде — чтобы превью и реальная картинка не разъезжались.

### Шрифты

- `regular` обязателен; `bold` используется для weight >= 600.
- `systemFonts: true` один раз сканирует системные шрифты и добавляет fallback
  для CJK/арабского/иврита и других письменностей.
- Дополнительные файлы можно передать в `fallback: [path, ...]`.
- Данные шрифтов можно передать напрямую в конструктор:

```js
const renderer = new WelcomeRenderer(
  { regular: regularBuffer, bold: boldBuffer, fallback: [] },
  { systemFonts: false, cacheBytes: 32 * 1024 * 1024 },
);
```

Цветные эмодзи не поддерживаются: как и resvg, пакет рисует только контурные
глифы. Монохромные emoji из системного шрифта работают.

## API

### `WelcomeRenderer.fromFiles(options)`

| Поле | Тип | По умолчанию | Описание |
|---|---|---|---|
| `regular` | `string \| URL` | — | обычный шрифт |
| `bold` | `string \| URL` | — | жирный шрифт |
| `fallback` | `(string \| URL)[]` | `[]` | дополнительные шрифты |
| `systemFonts` | `boolean` | `true` | сканировать системные шрифты один раз |
| `cacheBytes` | `number` | 64 MiB | лимит кэша декодированных фонов |
| `maxDimension` | `number` | 4096 | максимальная сторона изображения |
| `maxPixels` | `number` | 4096 × 4096 | максимальное число пикселей |

### `renderer.renderAsync(scene)`

Основной метод. Возвращает `Promise<Buffer>` с RGB PNG. Работает в libuv
thread pool, event loop не блокируется.

`renderer.render(scene)` — синхронный вариант для тестов и скриптов.

### Сцена

| Поле | Тип | Описание |
|---|---|---|
| `width`, `height` | `number?` | размер canvas; по умолчанию размер фона или 900x480 |
| `background` | `Buffer?` | PNG, JPEG или WebP; `null` — градиент |
| `backgroundKey` | `string?` | ключ кэша; без ключа фон не кэшируется |
| `overlay` | `{ color, opacity }?` | затемнение; по умолчанию чёрный 0.25 |
| `avatar` | `Buffer?` | PNG/JPEG/WebP |
| `avatarEllipse` | `{ cx, cy, rx, ry }?` | эллипс аватара |
| `texts` | `TextSpec[]` | до 16 текстовых слоёв |

`TextSpec`: `{ text, x, y, size, color, weight? }`. `y` — baseline, `x` —
центр строки (`text-anchor: middle`). Цвета: `#rgb`, `#rgba`, `#rrggbb`,
`#rrggbbaa`.

### Кэш и статистика

```js
renderer.stats;
// { renders, cacheHits, cacheMisses, cacheBytes, cacheEntries }

renderer.clearCache(); // возвращает освобождённые байты
renderer.dispose();    // то же самое + совместимо с using
```

`using renderer = await WelcomeRenderer.fromFiles(...)` работает через
`Symbol.dispose`.

### Ошибки

Ошибки приходят как `RenderError` с полем `code`:

- `INVALID_ARGUMENT`
- `FONT_READ`
- `FONT_PARSE`
- `IMAGE_DECODE`
- `IMAGE_TOO_LARGE`
- `PNG_ENCODE`
- `INTERNAL`

Битый фон заменяется градиентом, битый аватар просто не рисуется — welcome-картинка
всё равно отправляется. Ошибки шрифтов и текста выбрасываются наружу.

## Ограничения v0.1.0

- только `linux-x64-gnu`, Node.js 26+;
- PNG/JPEG/WebP; GIF не поддерживается, у анимированных PNG/WebP берётся
  первый кадр;
- EXIF orientation JPEG пока не применяется;
- фон/аватар больше `maxDimension`/`maxPixels` не декодируются: фон
  заменяется градиентом, аватар пропускается;
- цветные эмодзи не рисуются;
- если явно задать `width`/`height`, отличающиеся от размера фона, фон
  масштабируется как `cover` (центральный кроп);
- SVG не парсится — координаты сцены должны быть уже в пикселях canvas.

## Производительность

Замер на Ryzen/Arch Linux, Node 26, 15 прогонов, медиана:

| Сценарий | resvg-js (system fonts) | resvg-js (no system fonts) | welcome-render |
|---|---:|---:|---:|
| 900×480 | ~140 мс | ~45 мс | **~11 мс** (warm) / ~15 мс (cold) |
| 1920×1080 PNG-фон | ~270 мс | ~175 мс | **~25 мс** (warm) / ~52 мс (cold) |
| 1920×1080 JPEG-фон | ~260 мс | ~160 мс | **~24 мс** (warm) / ~43 мс (cold) |

Event loop delay во время 20 параллельных рендеров: mean ~10 мс при resolution
10 мс, то есть заметной блокировки нет.

Размер release-бинарника: ~2.3 МБ. Воспроизвести:

```bash
pnpm bench
BENCH_RUNS=50 pnpm bench
```

## Разработка

```bash
rustup toolchain install 1.99.1
pnpm install
pnpm build        # release .node + native.js/native.d.ts
pnpm test         # node --test, TypeScript без сборки
pnpm typecheck    # tsc --noEmit
pnpm check        # fmt + clippy + typecheck
pnpm bench
```

Структура:

```
src/
  lib.rs          N-API класс и типы
  render.rs       canvas, фон, overlay, аватар
  text.rs         bidi, shaping, glyph outlines
  font.rs         fontdb, fallback-цепочки
  image.rs        PNG/JPEG/WebP декодеры
  png_encode.rs   RGB PNG-энкодер
  cache.rs        LRU кэш полотен
  scene.rs        внутренние структуры сцены
index.js          публичный ESM-фасад
index.d.ts        публичные типы
native.js         генерируется napi-rs
```

Перед публикацией: `pnpm pack` собирает release, прогоняет тесты и typecheck,
затем кладёт в tarball `index.js`, `index.d.ts`, `native.js`, `native.d.ts`,
`.node`, README и лицензии.

## Лицензия

GPL-3.0-only. Исходники пакета доступны в репозитории и в теге релиза.
Сторонние компоненты и лицензия шрифта Onest перечислены в
[`THIRD_PARTY_NOTICES.md`](./THIRD_PARTY_NOTICES.md).
