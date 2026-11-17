# Changelog

## 0.3.0

- Rust toolchain and MSRV raised to 1.100; updated `napi`, `napi-derive` and
  transitive crates.
- Typed internal error codes; the public error-code strings are unchanged.
- Scene and renderer-option validation now consistently throws `RenderError`.
- Added `Symbol.asyncDispose` for `await using`.
- Reworked the background cache into a lazy LRU without O(n) list scans.
- Deduplicated dimension fitting, output-format/quality parsing and RGB
  conversion.
- Switched pixel loops to `slice::as_chunks` and removed the obsolete Clippy
  allowance.
- Reduced allocations in text shaping and removed per-character strings.
- Updated CI actions, added locked dependency checks, `--locked` cargo runs and
  release packaging without a duplicate prepack.
- Refreshed README, changelog and third-party notices; added `jpeg-encoder`
  licensing information.

## 0.2.0

- Added `probeImage` and `normalizeImage`.
- Added JPEG output through `format`/`quality`.
- Applied EXIF orientation to JPEG backgrounds and normalized images.
- Added `strictBackground`.
- Added `backgroundFailures` and `avatarFailures` statistics.
- Cached composited backgrounds through `Arc`.
- Scaled mismatched backgrounds with cover cropping.

## 0.1.0

- Native scene compositor for welcome images: background, overlay, ellipse
  avatar and text.
- PNG, JPEG and WebP decoders; RGB PNG output.
- Onest loading plus optional system-font fallback for CJK, Arabic, Hebrew and
  other scripts.
- LRU cache for decoded backgrounds; warm cache hits skip background decoding.
- Cache entries are shared through `Arc`, so cold misses do not clone the
  composited canvas.
- Failed background decodes fall back to the gradient without poisoning the
  cache.
- Panics from decoders/render code are contained and returned as JS errors.
- Async rendering through the libuv thread pool.
- ESM-only public API with `fromFiles`, `RenderError`, `Symbol.dispose` and
  typed scene definitions.
- Linux x64 (glibc) prebuilt addon, GitHub Release packaging and CI.
