# Lucide icon source

These SVGs come from [Lucide](https://lucide.dev/), pinned to upstream
commit [`c29c276f16590cf017126671535ad8c792be64d8`](https://github.com/lucide-icons/lucide/tree/c29c276f16590cf017126671535ad8c792be64d8/icons).
The included `LICENSE` is the upstream ISC license with its inherited Feather MIT notice.

Only `stroke="currentColor"` is changed to `stroke="#ffffff"` so egui's SVG
rasterizer and image tint can apply the active theme's foreground color.
The 24 × 24 view box, 2-pixel strokes, and round caps/joins remain upstream defaults.
Icons are compiled into the desktop applications; no runtime download or icon font
is needed. Existing script and language icons remain Material Design Icons.
