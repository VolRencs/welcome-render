# Changelog

## 0.1.0

- Native scene compositor for welcome images: background, overlay, ellipse
  avatar and text.
- PNG, JPEG and WebP decoders; RGB PNG output.
- Onest loading plus optional system-font fallback for CJK, Arabic, Hebrew and
  other scripts.
- LRU cache for decoded backgrounds; warm cache hits skip background decoding.
- Async rendering through the libuv thread pool.
- ESM-only public API with `fromFiles`, `RenderError`, `Symbol.dispose` and
  typed scene definitions.
- Linux x64 (glibc) prebuilt addon, GitHub Release packaging and CI.
