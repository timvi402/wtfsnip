# waysnip

A snappy Wayland region-screenshot tool, styled after the
[illogical-impulse](https://github.com/end-4/dots-hyprland) (ii) region selector
— minus the shape toggle. Rectangle selection only.

Freezes every output with `wlr-screencopy`, shows a fullscreen layer-shell
overlay (dimmed frozen frame + dashed selection border + `W x H` readout), lets
you pick a rectangle with the mouse or keyboard, then crops the region and copies
it to the clipboard as PNG.

Built directly on `wayland-client` / `smithay-client-toolkit` with `tiny-skia`
for drawing — no GTK/Qt, single self-contained binary, fast cold start.

## Features

- **Frozen backdrop** — captures each output up front so the selection is stable.
- **Mouse**: click-drag a rectangle; release to copy. Right-click cancels.
- **Window targeting** (Hyprland): hover a window to highlight it (border + label),
  then click without dragging to grab that window's bounds. Uses `hyprctl -j`;
  gracefully disabled on other compositors.
- **Keyboard**:
  - Arrows **move** the box (hold two for diagonal). First press spawns a
    centered box.
  - **Shift** + arrows **resize** (moves the bottom-right corner).
  - **Ctrl** = fine 1px steps, **Alt** = 5× faster.
  - **Enter** confirms & copies, **Esc** cancels.
- **Themed** — reads `~/.local/state/quickshell/user/generated/colors.json`, so
  the dim/border colors follow your current omarchy/material theme (falls back to
  a sensible dark palette).
- **Fractional-scale aware** — selection is in logical pixels; the crop is taken
  from the full-resolution captured frame.
- Multi-monitor: every output gets an overlay; the selection lives on the output
  you interact with.

## Build

```sh
cargo build --release
# binary at target/release/waysnip
```

## Usage

```sh
waysnip
```

The result lands on the clipboard as `image/png` (via `wl-copy`). Paste it
anywhere, or pipe it to a file:

```sh
wl-paste --type image/png > shot.png
```

### Bind it

Hyprland, to put it on PrtSc:

```
bindd = , PRINT, Region screenshot, exec, waysnip
```

## Requirements

- A `wlr-layer-shell` + `wlr-screencopy` compositor (Hyprland, sway, river, …).
- `wl-copy` (wl-clipboard) on `PATH`.
- A font for the dimension label (tries Rubik, then Liberation Sans / DejaVu).

## Dependencies

`smithay-client-toolkit`, `wayland-client`, `wayland-protocols-wlr`, `tiny-skia`,
`fontdue`, `memmap2`, `serde`/`serde_json`.

## Not (yet) implemented

- Save-to-file / annotate / OCR hand-off (clipboard only for now).
- Cross-monitor drag (each selection is confined to one output).
