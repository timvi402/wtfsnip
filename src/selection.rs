//! Rectangle selection state + keyboard navigation.
//!
//! Ported from the illogical-impulse `RegionSelection.qml` model: an anchor
//! corner (`drag_start`) and a free corner (`dragging`); the normalized region
//! is the axis-aligned box between them. All coordinates are in the surface's
//! logical pixel space (top-left origin).

/// Held-key speed tiers, matching the QML nudge timer.
const SPEED_NORMAL: f32 = 6.0;
const SPEED_FAST: f32 = 30.0; // Alt
const SPEED_FINE: f32 = 1.0; // Ctrl

/// Default box spawned on the first keyboard nudge when nothing is selected yet.
const DEFAULT_W: f32 = 400.0;
const DEFAULT_H: f32 = 300.0;

#[derive(Clone, Copy, Debug, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// The set of currently-held navigation keys. Applied on each animation tick so
/// two axes held together produce diagonal motion.
#[derive(Clone, Copy, Debug, Default)]
pub struct Held {
    pub left: bool,
    pub right: bool,
    pub up: bool,
    pub down: bool,
    pub resize: bool, // Shift -> resize instead of move
    pub fine: bool,   // Ctrl -> 1px steps
    pub fast: bool,   // Alt -> 5x faster
}

impl Held {
    pub fn any_arrow(&self) -> bool {
        self.left || self.right || self.up || self.down
    }
}

#[derive(Clone, Debug)]
pub struct Selection {
    pub screen_w: f32,
    pub screen_h: f32,
    // Anchor corner.
    pub start_x: f32,
    pub start_y: f32,
    // Free corner.
    pub drag_x: f32,
    pub drag_y: f32,
    // Latest pointer position (for the crosshair; independent of the selection).
    pub mouse_x: f32,
    pub mouse_y: f32,
    // True once a drag/keyboard selection has begun.
    pub active: bool,
    pub held: Held,
}

impl Selection {
    pub fn new(screen_w: f32, screen_h: f32) -> Self {
        Self {
            screen_w,
            screen_h,
            start_x: 0.0,
            start_y: 0.0,
            drag_x: 0.0,
            drag_y: 0.0,
            mouse_x: 0.0,
            mouse_y: 0.0,
            active: false,
            held: Held::default(),
        }
    }

    fn clamp_x(&self, x: f32) -> f32 {
        x.clamp(0.0, self.screen_w)
    }
    fn clamp_y(&self, y: f32) -> f32 {
        y.clamp(0.0, self.screen_h)
    }

    /// Update the crosshair pointer position (clamped, no effect on selection).
    pub fn set_mouse(&mut self, x: f32, y: f32) {
        self.mouse_x = self.clamp_x(x);
        self.mouse_y = self.clamp_y(y);
    }

    /// Spawn the default centered box if nothing is selected yet (for keyboard
    /// taps). Returns true if a box was created.
    pub fn ensure_box(&mut self) -> bool {
        self.ensure()
    }

    /// Normalized region (top-left + positive size).
    pub fn region(&self) -> Rect {
        Rect {
            x: self.start_x.min(self.drag_x),
            y: self.start_y.min(self.drag_y),
            w: (self.drag_x - self.start_x).abs(),
            h: (self.drag_y - self.start_y).abs(),
        }
    }

    pub fn has_area(&self) -> bool {
        let r = self.region();
        r.w >= 1.0 && r.h >= 1.0
    }

    // --- Pointer-driven ---------------------------------------------------

    pub fn press(&mut self, x: f32, y: f32) {
        let (x, y) = (self.clamp_x(x), self.clamp_y(y));
        self.start_x = x;
        self.start_y = y;
        self.drag_x = x;
        self.drag_y = y;
        self.mouse_x = x;
        self.mouse_y = y;
        self.active = true;
    }

    /// Set the selection to an explicit rectangle (e.g. a targeted window).
    pub fn set_rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        self.start_x = x;
        self.start_y = y;
        self.drag_x = x + w;
        self.drag_y = y + h;
        self.active = true;
    }

    pub fn drag_to(&mut self, x: f32, y: f32) {
        self.drag_x = self.clamp_x(x);
        self.drag_y = self.clamp_y(y);
        self.mouse_x = self.drag_x;
        self.mouse_y = self.drag_y;
    }

    // --- Keyboard-driven --------------------------------------------------

    /// Spawn a default centered box on the first key press when nothing is
    /// selected yet. Returns true if it created one (caller stops there so the
    /// first press just reveals the box).
    fn ensure(&mut self) -> bool {
        if self.has_area() {
            return false;
        }
        let w = DEFAULT_W.min(self.screen_w);
        let h = DEFAULT_H.min(self.screen_h);
        self.start_x = ((self.screen_w - w) / 2.0).round();
        self.start_y = ((self.screen_h - h) / 2.0).round();
        self.drag_x = self.start_x + w;
        self.drag_y = self.start_y + h;
        self.active = true;
        true
    }

    /// Make the anchor the top-left and the free corner the bottom-right.
    fn normalize(&mut self) {
        let (x0, x1) = (self.start_x.min(self.drag_x), self.start_x.max(self.drag_x));
        let (y0, y1) = (self.start_y.min(self.drag_y), self.start_y.max(self.drag_y));
        self.start_x = x0;
        self.drag_x = x1;
        self.start_y = y0;
        self.drag_y = y1;
    }

    fn mv(&mut self, dx: f32, dy: f32) {
        if self.ensure() {
            return;
        }
        self.normalize();
        let w = self.drag_x - self.start_x;
        let h = self.drag_y - self.start_y;
        let nx = (self.start_x + dx).clamp(0.0, self.screen_w - w);
        let ny = (self.start_y + dy).clamp(0.0, self.screen_h - h);
        self.start_x = nx;
        self.drag_x = nx + w;
        self.start_y = ny;
        self.drag_y = ny + h;
    }

    fn resize(&mut self, dx: f32, dy: f32) {
        if self.ensure() {
            return;
        }
        self.normalize();
        self.drag_x = (self.drag_x + dx).clamp(self.start_x + 1.0, self.screen_w);
        self.drag_y = (self.drag_y + dy).clamp(self.start_y + 1.0, self.screen_h);
    }

    /// Apply one animation tick of held-key motion. Returns true if the region
    /// changed (caller should redraw). Mirrors the QML 16ms nudge timer.
    pub fn tick(&mut self) -> bool {
        if !self.held.any_arrow() {
            return false;
        }
        let speed = if self.held.fine {
            SPEED_FINE
        } else if self.held.fast {
            SPEED_FAST
        } else {
            SPEED_NORMAL
        };
        let dx = ((self.held.right as i32 - self.held.left as i32) as f32) * speed;
        let dy = ((self.held.down as i32 - self.held.up as i32) as f32) * speed;
        if dx == 0.0 && dy == 0.0 {
            return false;
        }
        if self.held.resize {
            self.resize(dx, dy);
        } else {
            self.mv(dx, dy);
        }
        true
    }

    pub fn clear_held(&mut self) {
        self.held = Held::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_nudge_spawns_centered_box() {
        let mut s = Selection::new(1920.0, 1080.0);
        s.held.right = true;
        // First tick spawns the box (no motion yet).
        assert!(s.tick());
        let r = s.region();
        assert_eq!(r.w, 400.0);
        assert_eq!(r.h, 300.0);
        assert_eq!(r.x, (1920.0 - 400.0) / 2.0);
    }

    #[test]
    fn diagonal_move_both_axes() {
        let mut s = Selection::new(1920.0, 1080.0);
        s.press(100.0, 100.0);
        s.drag_to(300.0, 250.0);
        let before = s.region();
        s.held.right = true;
        s.held.down = true;
        assert!(s.tick());
        let after = s.region();
        assert_eq!(after.x, before.x + SPEED_NORMAL);
        assert_eq!(after.y, before.y + SPEED_NORMAL);
    }

    #[test]
    fn move_clamps_to_screen() {
        let mut s = Selection::new(1000.0, 1000.0);
        s.press(0.0, 0.0);
        s.drag_to(100.0, 100.0);
        s.held.left = true;
        // Many ticks pushing left should never go negative.
        for _ in 0..100 {
            s.tick();
        }
        assert_eq!(s.region().x, 0.0);
    }

    #[test]
    fn shift_alt_fast_diagonal_resize() {
        // Shift (resize) + Alt (fast) + two arrows grows the bottom-right corner
        // on both axes by the fast step in a single tick.
        let mut s = Selection::new(1920.0, 1080.0);
        s.press(100.0, 100.0);
        s.drag_to(300.0, 250.0);
        let before = s.region();
        s.held.resize = true; // Shift
        s.held.fast = true; // Alt
        s.held.right = true;
        s.held.down = true;
        assert!(s.tick());
        let after = s.region();
        assert_eq!(after.w, before.w + SPEED_FAST);
        assert_eq!(after.h, before.h + SPEED_FAST);
        // The anchor (top-left) stays put; only the free corner moves.
        assert_eq!(after.x, before.x);
        assert_eq!(after.y, before.y);
    }

    #[test]
    fn resize_keeps_min_size() {
        let mut s = Selection::new(1000.0, 1000.0);
        s.press(100.0, 100.0);
        s.drag_to(150.0, 150.0);
        s.held.resize = true;
        s.held.left = true;
        s.held.up = true;
        for _ in 0..100 {
            s.tick();
        }
        let r = s.region();
        assert!(r.w >= 1.0 && r.h >= 1.0);
    }
}
