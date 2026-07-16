//! Overlay rendering with tiny-skia, styled after the ii region selector:
//! bright selection over a dimmed frozen frame, dashed border, `W x H` label.

use crate::selection::Rect as SelRect;
use crate::theme::Theme;
use crate::windows::WinRegion;
use fontdue::Font;
use tiny_skia::{
    Color, Paint, PathBuilder, Pixmap, PixmapPaint, Rect, Stroke, StrokeDash, Transform,
};

const FONT_CANDIDATES: &[&str] = &[
    "/usr/share/fonts/TTF/Rubik%5Bwght%5D.ttf",
    "/usr/share/fonts/TTF/Rubik-Regular.ttf",
    "/usr/share/fonts/liberation/LiberationSans-Regular.ttf",
    "/usr/share/fonts/TTF/DejaVuSans.ttf",
    "/usr/share/fonts/dejavu/DejaVuSans.ttf",
];

const LABEL_PX: f32 = 15.0;
const BORDER_ALPHA: f32 = 0.9;

pub struct Renderer {
    font: Option<Font>,
}

impl Renderer {
    pub fn new() -> Self {
        let font = FONT_CANDIDATES.iter().find_map(|p| {
            let bytes = std::fs::read(p).ok()?;
            Font::from_bytes(bytes, fontdue::FontSettings::default()).ok()
        });
        Self { font }
    }

    /// Produce a darkened copy of `bright` (the dim base drawn everywhere).
    pub fn make_dim(bright: &Pixmap, overlay: Color) -> Pixmap {
        let mut dim = bright.clone();
        let mut paint = Paint::default();
        paint.set_color(overlay);
        paint.anti_alias = false;
        if let Some(rect) = Rect::from_xywh(0.0, 0.0, dim.width() as f32, dim.height() as f32) {
            dim.fill_rect(rect, &paint, Transform::identity(), None);
        }
        dim
    }

    /// Render one frame into `target` (logical-size RGBA pixmap).
    ///
    /// `display` is the bright frozen frame, `dim` its darkened version, both at
    /// the same logical size as `target`. `region` is in logical coordinates.
    pub fn render(
        &self,
        target: &mut Pixmap,
        display: &Pixmap,
        dim: &Pixmap,
        region: SelRect,
        theme: &Theme,
        active: bool,
        crosshair: Option<(f32, f32)>,
        windows: &[WinRegion],
        targeted: Option<usize>,
    ) {
        let w = target.width() as f32;
        let h = target.height() as f32;
        let has_area = active && region.w >= 1.0 && region.h >= 1.0;

        if has_area {
            // Bright everywhere, then dim the four bands around the region.
            target.draw_pixmap(
                0,
                0,
                display.as_ref(),
                &PixmapPaint::default(),
                Transform::identity(),
                None,
            );
            let mut dimpaint = Paint::default();
            dimpaint.set_color(theme.overlay);
            dimpaint.anti_alias = false;
            let bands = [
                (0.0, 0.0, w, region.y),                                     // top
                (0.0, region.y + region.h, w, h - (region.y + region.h)),   // bottom
                (0.0, region.y, region.x, region.h),                        // left
                (region.x + region.w, region.y, w - (region.x + region.w), region.h), // right
            ];
            for (x, y, bw, bh) in bands {
                if bw > 0.0 && bh > 0.0 {
                    if let Some(r) = Rect::from_xywh(x, y, bw, bh) {
                        target.fill_rect(r, &dimpaint, Transform::identity(), None);
                    }
                }
            }
        } else {
            // Nothing selected yet: whole screen dimmed.
            target.draw_pixmap(
                0,
                0,
                dim.as_ref(),
                &PixmapPaint::default(),
                Transform::identity(),
                None,
            );
        }

        // Crosshair aim lines (subtle) while hovering before a selection.
        if let Some((mx, my)) = crosshair {
            let mut line = Paint::default();
            let mut c = theme.border;
            c.set_alpha(0.2);
            line.set_color(c);
            line.anti_alias = false;
            if let Some(r) = Rect::from_xywh(mx.round(), 0.0, 1.0, h) {
                target.fill_rect(r, &line, Transform::identity(), None);
            }
            if let Some(r) = Rect::from_xywh(0.0, my.round(), w, 1.0) {
                target.fill_rect(r, &line, Transform::identity(), None);
            }
        }

        // Window targeting outlines (only shown while hovering, before a drag).
        if !has_area && !windows.is_empty() {
            self.draw_windows(target, windows, targeted, theme);
        }

        if !has_area {
            return;
        }

        // Dashed selection border, aligned to pixel centers for a crisp 1px line.
        let mut bpaint = Paint::default();
        let mut bc = theme.border;
        bc.set_alpha(BORDER_ALPHA);
        bpaint.set_color(bc);
        bpaint.anti_alias = true;
        let stroke = Stroke {
            width: 1.0,
            dash: StrokeDash::new(vec![8.0, 4.0], 0.0),
            ..Default::default()
        };
        let rx = region.x.round() + 0.5;
        let ry = region.y.round() + 0.5;
        let rw = region.w.round() - 1.0;
        let rh = region.h.round() - 1.0;
        if let Some(rect) = Rect::from_xywh(rx, ry, rw.max(0.0), rh.max(0.0)) {
            let mut pb = PathBuilder::new();
            pb.push_rect(rect);
            if let Some(path) = pb.finish() {
                target.stroke_path(&path, &bpaint, &stroke, Transform::identity(), None);
            }
        }

        // "W x H" label below the bottom-right corner (right-aligned).
        let label = format!("{} x {}", region.w.round() as i32, region.h.round() as i32);
        let tw = self.text_width(&label, LABEL_PX);
        let mut lx = region.x + region.w - tw;
        let mut ly = region.y + region.h + 8.0;
        lx = lx.clamp(2.0, (w - tw).max(2.0));
        if ly + LABEL_PX + 4.0 > h {
            ly = region.y - LABEL_PX - 10.0; // flip above if it would clip
        }
        ly = ly.max(2.0);
        self.draw_text(target, &label, LABEL_PX, lx, ly, bc);
    }

    /// Faint outlines for every targetable window, with the hovered one filled
    /// and labelled (matches the ii window-region look).
    fn draw_windows(
        &self,
        target: &mut Pixmap,
        windows: &[WinRegion],
        targeted: Option<usize>,
        theme: &Theme,
    ) {
        let solid = Stroke { width: 1.0, ..Default::default() };
        // The picked window gets a bolder dashed border so it stands out.
        let dashed = Stroke {
            width: 1.5,
            dash: StrokeDash::new(vec![8.0, 4.0], 0.0),
            ..Default::default()
        };
        for (i, win) in windows.iter().enumerate() {
            let is_target = targeted == Some(i);
            let Some(rect) = Rect::from_xywh(win.x + 0.5, win.y + 0.5, (win.w - 1.0).max(0.0), (win.h - 1.0).max(0.0))
            else {
                continue;
            };

            if is_target {
                let mut fill = Paint::default();
                let mut fc = theme.window;
                fc.set_alpha(0.15);
                fill.set_color(fc);
                if let Some(r) = Rect::from_xywh(win.x, win.y, win.w, win.h) {
                    target.fill_rect(r, &fill, Transform::identity(), None);
                }
            }

            let mut paint = Paint::default();
            let mut c = theme.window;
            c.set_alpha(if is_target { 0.95 } else { 0.35 });
            paint.set_color(c);
            paint.anti_alias = true;
            let mut pb = PathBuilder::new();
            pb.push_rect(rect);
            if let Some(path) = pb.finish() {
                let stroke = if is_target { &dashed } else { &solid };
                target.stroke_path(&path, &paint, stroke, Transform::identity(), None);
            }

            // Diagonal cross corner-to-corner, so the picked window is obvious.
            if is_target {
                let mut xc = theme.window;
                xc.set_alpha(0.5);
                let mut xpaint = Paint::default();
                xpaint.set_color(xc);
                xpaint.anti_alias = true;
                let mut db = PathBuilder::new();
                db.move_to(win.x, win.y);
                db.line_to(win.x + win.w, win.y + win.h);
                db.move_to(win.x + win.w, win.y);
                db.line_to(win.x, win.y + win.h);
                if let Some(path) = db.finish() {
                    target.stroke_path(&path, &xpaint, &dashed, Transform::identity(), None);
                }
            }
        }

        // Draw the label last so it sits above the fills.
        if let Some(i) = targeted {
            if let Some(win) = windows.get(i) {
                if !win.label.is_empty() {
                    let mut lc = theme.window;
                    lc.set_alpha(0.95);
                    self.draw_text(target, &win.label, 14.0, win.x + 8.0, win.y + 6.0, lc);
                }
            }
        }
    }

    fn text_width(&self, text: &str, px: f32) -> f32 {
        let Some(font) = &self.font else {
            return text.len() as f32 * px * 0.5;
        };
        text.chars()
            .map(|c| font.metrics(c, px).advance_width)
            .sum()
    }

    /// Rasterize `text` and alpha-blend it into `target` at baseline-top `y`.
    fn draw_text(&self, target: &mut Pixmap, text: &str, px: f32, x: f32, y: f32, color: Color) {
        let Some(font) = &self.font else {
            return;
        };
        let tw = target.width() as i32;
        let th = target.height() as i32;
        let ascent = font.horizontal_line_metrics(px).map(|m| m.ascent).unwrap_or(px);
        let cr = (color.red() * 255.0) as u16;
        let cg = (color.green() * 255.0) as u16;
        let cb = (color.blue() * 255.0) as u16;
        let ca = color.alpha();

        let mut pen_x = x;
        let data = target.data_mut();
        for ch in text.chars() {
            let (metrics, bitmap) = font.rasterize(ch, px);
            let gx0 = (pen_x + metrics.xmin as f32).round() as i32;
            let gy0 = (y + ascent - metrics.height as f32 - metrics.ymin as f32).round() as i32;
            for gy in 0..metrics.height as i32 {
                let py = gy0 + gy;
                if py < 0 || py >= th {
                    continue;
                }
                for gx in 0..metrics.width as i32 {
                    let px_ = gx0 + gx;
                    if px_ < 0 || px_ >= tw {
                        continue;
                    }
                    let cov = bitmap[(gy * metrics.width as i32 + gx) as usize] as f32 / 255.0;
                    let a = cov * ca;
                    if a <= 0.0 {
                        continue;
                    }
                    let idx = ((py * tw + px_) * 4) as usize;
                    // Source-over, premultiplied (glyph color is opaque-ish).
                    let inv = 1.0 - a;
                    data[idx] = (cr as f32 * a + data[idx] as f32 * inv) as u8;
                    data[idx + 1] = (cg as f32 * a + data[idx + 1] as f32 * inv) as u8;
                    data[idx + 2] = (cb as f32 * a + data[idx + 2] as f32 * inv) as u8;
                    data[idx + 3] = (255.0 * a + data[idx + 3] as f32 * inv) as u8;
                }
            }
            pen_x += metrics.advance_width;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tiny_skia::Color;

    /// Headless render preview. Run with an output path to eyeball the look:
    /// `WAYSNIP_PREVIEW=/tmp/p.png cargo test render::tests::preview -- --nocapture`
    #[test]
    fn preview() {
        let Ok(out) = std::env::var("WAYSNIP_PREVIEW") else {
            return;
        };
        let (w, h) = (800u32, 500u32);
        let mut display = Pixmap::new(w, h).unwrap();
        display.fill(Color::from_rgba8(38, 42, 58, 255));
        let theme = Theme::default();
        let dim = Renderer::make_dim(&display, theme.overlay);
        let mut target = Pixmap::new(w, h).unwrap();
        let r = Renderer::new();
        let wins = vec![
            WinRegion { x: 40.0, y: 60.0, w: 300.0, h: 360.0, floating: false, label: "kitty".into() },
            WinRegion { x: 380.0, y: 120.0, w: 360.0, h: 300.0, floating: true, label: "org.gnome.Nautilus".into() },
        ];
        r.render(
            &mut target,
            &display,
            &dim,
            SelRect::default(),
            &theme,
            false,
            Some((560.0, 270.0)),
            &wins,
            Some(1),
        );
        std::fs::write(&out, target.encode_png().unwrap()).unwrap();
    }
}
