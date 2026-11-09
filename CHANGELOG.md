# Changelog

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
