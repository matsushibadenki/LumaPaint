# Local changes to resvg 0.49.0

The Apache-2.0 / MIT upstream sources are retained.

- `src/render.rs`: translate the inherited maximum layer/filter bounds into each offscreen layer coordinate system before rendering descendants and masks. Preserve the existing allocation extent limit and checked integer overflow behavior. Regression coverage is in lumapaint-renderer (nested masks, filters, clip paths, opacity and viewport pan/zoom).
