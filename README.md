# wtfsnip

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
  then click without dragging to grab that window's bounds. Or press **Tab** to
  pick a window from the keyboard — it highlights each window in turn (Shift+Tab
  cycles backwards); **Enter** grabs the highlighted one. Uses `hyprctl -j`;
  gracefully disabled on other compositors.
- **Keyboard**:
  - Arrows **move** the box (hold two for diagonal). First press spawns a
    centered box.
  - **Shift** + arrows **resize** (moves the bottom-right corner; hold two
    for diagonal resize).
  - **Ctrl** = fine 1px steps, **Alt** = 5× faster. Combine e.g.
    **Shift** + **Alt** + Right + Down to resize diagonally in big steps.
  - **Tab** / **Shift+Tab** cycle through targetable windows; **Enter** grabs the
    highlighted one.
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
# binary at target/release/wtfsnip
```

## Usage

```sh
wtfsnip
```

The result lands on the clipboard as `image/png` (via `wl-copy`). Paste it
anywhere, or pipe it to a file:

```sh
wl-paste --type image/png > shot.png
```

By default it **also** saves each shot to `~/Pictures/Screenshots` as
`wtfsnip_<timestamp>.png` (the directory is created if missing).

### Configuration

Optional, at `$XDG_CONFIG_HOME/wtfsnip/config.json` (i.e.
`~/.config/wtfsnip/config.json`). Every field is optional:

```json
{
  "save": true,
  "save_dir": "~/Pictures/Screenshots"
}
```

- **`save_dir`** — where auto-saved screenshots go. `~` and `$HOME` are expanded.
- **`save`** — set to `false` to disable auto-save and keep clipboard-only.

### Bind it

Hyprland, to put it on PrtSc:

```
bindd = , PRINT, Region screenshot, exec, wtfsnip
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
