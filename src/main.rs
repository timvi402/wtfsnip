//! waysnip — a snappy Wayland region screenshot tool.
//!
//! Freezes each output (wlr-screencopy), shows a fullscreen layer-shell overlay
//! styled after the illogical-impulse region selector, lets you pick a rectangle
//! with the mouse or keyboard, then crops and copies it to the clipboard.

mod render;
mod selection;
mod theme;
mod windows;

use std::io::Write;
use std::process::{Command, Stdio};

use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_keyboard, delegate_layer, delegate_output, delegate_pointer,
    delegate_registry, delegate_seat, delegate_shm,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers},
        pointer::{PointerEvent, PointerEventKind, PointerHandler, BTN_LEFT, BTN_RIGHT},
        Capability, SeatHandler, SeatState,
    },
    shell::{
        wlr_layer::{Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface, LayerSurfaceConfigure},
        WaylandSurface,
    },
    shm::{slot::SlotPool, Shm, ShmHandler},
};
use tiny_skia::{Pixmap, PixmapPaint, Transform};
use wayland_client::{
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
    Connection, Dispatch, Proxy, QueueHandle,
};
use wayland_protocols_wlr::screencopy::v1::client::{
    zwlr_screencopy_frame_v1::{self, ZwlrScreencopyFrameV1},
    zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1,
};

use render::Renderer;
use selection::Selection;
use theme::Theme;

fn main() {
    if let Err(e) = run() {
        eprintln!("waysnip: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let conn = Connection::connect_to_env().map_err(|e| format!("connect: {e}"))?;
    let (globals, mut event_queue) =
        registry_queue_init(&conn).map_err(|e| format!("registry: {e}"))?;
    let qh = event_queue.handle();

    let compositor =
        CompositorState::bind(&globals, &qh).map_err(|_| "wl_compositor missing".to_string())?;
    let layer_shell =
        LayerShell::bind(&globals, &qh).map_err(|_| "wlr-layer-shell missing".to_string())?;
    let shm = Shm::bind(&globals, &qh).map_err(|_| "wl_shm missing".to_string())?;
    let screencopy: ZwlrScreencopyManagerV1 = globals
        .bind(&qh, 1..=1, ())
        .map_err(|_| "wlr-screencopy missing".to_string())?;

    let capture_pool =
        SlotPool::new(1920 * 1080 * 4, &shm).map_err(|e| format!("pool: {e}"))?;
    let render_pool =
        SlotPool::new(1920 * 1080 * 4, &shm).map_err(|e| format!("pool: {e}"))?;

    let mut app = App {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        shm,
        compositor,
        layer_shell,
        screencopy,
        capture_pool,
        render_pool,
        renderer: Renderer::new(),
        theme: Theme::load(),
        keyboard: None,
        pointer: None,
        monitors: Vec::new(),
        active: None,
        exit: false,
        layers_created: false,
        held_keys: std::collections::HashSet::new(),
        pending_exit: false,
    };

    // Enumerate outputs.
    event_queue
        .roundtrip(&mut app)
        .map_err(|e| format!("roundtrip: {e}"))?;

    // Start a capture for every output.
    let outputs: Vec<wl_output::WlOutput> = app.output_state.outputs().collect();
    if outputs.is_empty() {
        return Err("no outputs".into());
    }
    // Best-effort window geometry for click-to-grab targeting (Hyprland).
    let mut regions_by_output = windows::regions_by_output();
    for output in outputs {
        let idx = app.monitors.len();
        let name = app
            .output_state
            .info(&output)
            .and_then(|i| i.name)
            .unwrap_or_default();
        app.screencopy.capture_output(0, &output, &qh, idx);
        let mut mon = Monitor::new(output);
        mon.window_regions = regions_by_output.remove(&name).unwrap_or_default();
        app.monitors.push(mon);
    }

    // Pump events until all captures are done, then bring up the overlays.
    loop {
        event_queue
            .blocking_dispatch(&mut app)
            .map_err(|e| format!("dispatch: {e}"))?;
        if app.exit {
            return Ok(());
        }
        if !app.layers_created && app.monitors.iter().all(|m| m.bright.is_some()) {
            app.create_layers(&qh);
            app.layers_created = true;
        }
        if app.layers_created {
            app.draw_dirty(&qh);
        }
    }
}

/// Per-output state: the frozen capture, its overlay surface, and the selection.
struct Monitor {
    output: wl_output::WlOutput,
    // Window targeting (Hyprland), in this output's logical coordinates.
    window_regions: Vec<windows::WinRegion>,
    targeted: Option<usize>,
    // Capture (physical pixels).
    capture_buffer: Option<smithay_client_toolkit::shm::slot::Buffer>,
    fmt: wl_shm::Format,
    phys_w: u32,
    phys_h: u32,
    stride: u32,
    y_invert: bool,
    copied: bool,
    bright: Option<Pixmap>, // physical-res frozen frame (for cropping)
    // Overlay (logical pixels).
    layer: Option<LayerSurface>,
    log_w: u32,
    log_h: u32,
    display: Option<Pixmap>, // frozen frame scaled to logical size
    dim: Option<Pixmap>,     // display darkened
    scratch: Option<Pixmap>, // reusable render target
    selection: Selection,
    pointer_in: bool,
    dirty: bool,
    configured: bool,
}

impl Monitor {
    fn new(output: wl_output::WlOutput) -> Self {
        Self {
            output,
            window_regions: Vec::new(),
            targeted: None,
            capture_buffer: None,
            fmt: wl_shm::Format::Argb8888,
            phys_w: 0,
            phys_h: 0,
            stride: 0,
            y_invert: false,
            copied: false,
            bright: None,
            layer: None,
            log_w: 0,
            log_h: 0,
            display: None,
            dim: None,
            scratch: None,
            selection: Selection::new(0.0, 0.0),
            pointer_in: false,
            dirty: false,
            configured: false,
        }
    }
}

struct App {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    shm: Shm,
    compositor: CompositorState,
    layer_shell: LayerShell,
    screencopy: ZwlrScreencopyManagerV1,
    capture_pool: SlotPool,
    render_pool: SlotPool,
    renderer: Renderer,
    theme: Theme,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    pointer: Option<wl_pointer::WlPointer>,
    monitors: Vec<Monitor>,
    active: Option<usize>,
    exit: bool,
    layers_created: bool,
    // Raw keycodes currently held down. We defer quitting until this is empty so
    // key-release events don't leak to the terminal that regains focus (which,
    // with the kitty keyboard protocol, would insert `\e[..u` junk).
    held_keys: std::collections::HashSet<u32>,
    pending_exit: bool,
}

impl App {
    fn create_layers(&mut self, qh: &QueueHandle<Self>) {
        for m in &mut self.monitors {
            let surface = self.compositor.create_surface(qh);
            let layer = self.layer_shell.create_layer_surface(
                qh,
                surface,
                Layer::Overlay,
                Some("waysnip"),
                Some(&m.output),
            );
            layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
            layer.set_exclusive_zone(-1);
            // Exclusive: a modal screenshot selector should take keys immediately
            // (Esc / arrows / Enter) without needing a click first.
            layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
            layer.set_size(0, 0);
            layer.commit();
            m.layer = Some(layer);
        }
    }

    fn monitor_by_surface(&self, surface: &wl_surface::WlSurface) -> Option<usize> {
        self.monitors
            .iter()
            .position(|m| m.layer.as_ref().is_some_and(|l| l.wl_surface() == surface))
    }

    fn draw_dirty(&mut self, qh: &QueueHandle<Self>) {
        for i in 0..self.monitors.len() {
            if self.monitors[i].configured && self.monitors[i].dirty {
                self.draw(i, qh);
            }
        }
    }

    fn ensure_scaled(&mut self, i: usize) {
        let overlay = self.theme.overlay;
        let m = &mut self.monitors[i];
        if m.display.is_some() {
            return;
        }
        let Some(bright) = &m.bright else { return };
        let (lw, lh) = (m.log_w, m.log_h);
        if lw == 0 || lh == 0 {
            return;
        }
        let display = if bright.width() == lw && bright.height() == lh {
            bright.clone()
        } else {
            let mut d = Pixmap::new(lw, lh).unwrap();
            let sx = lw as f32 / bright.width() as f32;
            let sy = lh as f32 / bright.height() as f32;
            let mut paint = PixmapPaint::default();
            paint.quality = tiny_skia::FilterQuality::Bilinear;
            d.draw_pixmap(0, 0, bright.as_ref(), &paint, Transform::from_scale(sx, sy), None);
            d
        };
        m.dim = Some(Renderer::make_dim(&display, overlay));
        m.display = Some(display);
        m.scratch = Some(Pixmap::new(lw, lh).unwrap());
    }

    fn draw(&mut self, i: usize, qh: &QueueHandle<Self>) {
        self.ensure_scaled(i);
        let (lw, lh, animating) = {
            let m = &self.monitors[i];
            (m.log_w, m.log_h, m.selection.held.any_arrow())
        };
        if lw == 0 || lh == 0 {
            return;
        }

        // Render the overlay into the scratch pixmap.
        {
            let Self { monitors, renderer, theme, .. } = self;
            let m = &mut monitors[i];
            let (Some(display), Some(dim), Some(scratch)) =
                (m.display.as_ref(), m.dim.as_ref(), m.scratch.as_mut())
            else {
                return;
            };
            let region = m.selection.region();
            let show_windows = m.pointer_in && !m.selection.active;
            let crosshair = if show_windows {
                Some((m.selection.mouse_x, m.selection.mouse_y))
            } else {
                None
            };
            let win_slice: &[windows::WinRegion] = if show_windows { &m.window_regions } else { &[] };
            renderer.render(
                scratch,
                display,
                dim,
                region,
                theme,
                m.selection.active,
                crosshair,
                win_slice,
                m.targeted,
            );
        }

        // Copy into a fresh shm buffer (RGBA -> BGRX) and present.
        let stride = lw as i32 * 4;
        let (buffer, canvas) = self
            .render_pool
            .create_buffer(lw as i32, lh as i32, stride, wl_shm::Format::Xrgb8888)
            .expect("render buffer");
        {
            let src = self.monitors[i].scratch.as_ref().unwrap().data();
            for (dst, s) in canvas.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
                dst[0] = s[2]; // B
                dst[1] = s[1]; // G
                dst[2] = s[0]; // R
                dst[3] = 0xff; // X
            }
        }

        let m = &mut self.monitors[i];
        let surface = m.layer.as_ref().unwrap().wl_surface();
        surface.damage_buffer(0, 0, lw as i32, lh as i32);
        if animating {
            surface.frame(qh, surface.clone());
        }
        buffer.attach_to(surface).expect("attach");
        m.layer.as_ref().unwrap().commit();
        m.dirty = false;
    }

    fn build_bright(&mut self, i: usize) {
        let Self { monitors, capture_pool, .. } = self;
        let m = &mut monitors[i];
        let Some(buffer) = m.capture_buffer.as_ref() else { return };
        let Some(canvas) = buffer.canvas(capture_pool) else { return };
        let (w, h, stride) = (m.phys_w, m.phys_h, m.stride);
        let mut pix = Pixmap::new(w, h).unwrap();
        let dst = pix.data_mut();
        for y in 0..h as usize {
            let src_y = if m.y_invert { h as usize - 1 - y } else { y };
            let row = &canvas[src_y * stride as usize..src_y * stride as usize + w as usize * 4];
            let out = &mut dst[y * w as usize * 4..(y + 1) * w as usize * 4];
            for (o, s) in out.chunks_exact_mut(4).zip(row.chunks_exact(4)) {
                o[0] = s[2]; // R <- B
                o[1] = s[1]; // G
                o[2] = s[0]; // B <- R
                o[3] = 0xff; // opaque
            }
        }
        m.bright = Some(pix);
        m.capture_buffer = None; // free the capture slot
    }

    /// Request exit once every held key is released, so release events don't
    /// leak to the window that regains keyboard focus.
    fn request_exit(&mut self) {
        self.pending_exit = true;
        if self.held_keys.is_empty() {
            self.exit = true;
        }
    }

    /// Crop the active selection out of the frozen frame and copy it as PNG.
    fn confirm(&mut self) {
        if self.pending_exit {
            return;
        }
        let Some(i) = self.active else {
            self.request_exit();
            return;
        };
        let m = &self.monitors[i];
        let region = m.selection.region();
        let (Some(bright), true) = (m.bright.as_ref(), m.selection.active && m.selection.has_area())
        else {
            self.request_exit();
            return;
        };
        let sx = bright.width() as f32 / m.log_w.max(1) as f32;
        let sy = bright.height() as f32 / m.log_h.max(1) as f32;
        let px = (region.x * sx).round().max(0.0) as u32;
        let py = (region.y * sy).round().max(0.0) as u32;
        let pw = ((region.w * sx).round() as u32).min(bright.width().saturating_sub(px));
        let ph = ((region.h * sy).round() as u32).min(bright.height().saturating_sub(py));
        if pw == 0 || ph == 0 {
            self.exit = true;
            return;
        }

        let mut crop = Pixmap::new(pw, ph).unwrap();
        crop.draw_pixmap(
            0,
            0,
            bright.as_ref(),
            &PixmapPaint::default(),
            Transform::from_translate(-(px as f32), -(py as f32)),
            None,
        );

        match crop.encode_png() {
            Ok(png) => copy_png(&png),
            Err(e) => eprintln!("waysnip: encode: {e}"),
        }
        self.request_exit();
    }

    fn cancel(&mut self) {
        self.request_exit();
    }
}

fn copy_png(png: &[u8]) {
    match Command::new("wl-copy")
        .arg("--type")
        .arg("image/png")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(mut child) => {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(png);
            }
            // wl-copy daemonizes once stdin closes; don't block on it.
        }
        Err(e) => eprintln!("waysnip: wl-copy: {e}"),
    }
}

// --- Screencopy dispatch -------------------------------------------------

impl Dispatch<ZwlrScreencopyManagerV1, ()> for App {
    fn event(
        _: &mut Self,
        _: &ZwlrScreencopyManagerV1,
        _: <ZwlrScreencopyManagerV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrScreencopyFrameV1, usize> for App {
    fn event(
        app: &mut Self,
        frame: &ZwlrScreencopyFrameV1,
        event: <ZwlrScreencopyFrameV1 as Proxy>::Event,
        &idx: &usize,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use zwlr_screencopy_frame_v1::Event;
        match event {
            Event::Buffer { format, width, height, stride } => {
                if app.monitors[idx].copied {
                    return;
                }
                let fmt = format.into_result().unwrap_or(wl_shm::Format::Argb8888);
                {
                    let m = &mut app.monitors[idx];
                    m.fmt = fmt;
                    m.phys_w = width;
                    m.phys_h = height;
                    m.stride = stride;
                }
                match app.capture_pool.create_buffer(
                    width as i32,
                    height as i32,
                    stride as i32,
                    fmt,
                ) {
                    Ok((buffer, _)) => {
                        frame.copy(buffer.wl_buffer());
                        let m = &mut app.monitors[idx];
                        m.capture_buffer = Some(buffer);
                        m.copied = true;
                    }
                    Err(e) => {
                        eprintln!("waysnip: capture buffer: {e}");
                        app.exit = true;
                    }
                }
            }
            Event::Flags { flags } => {
                if let Ok(f) = flags.into_result() {
                    app.monitors[idx].y_invert =
                        f.contains(zwlr_screencopy_frame_v1::Flags::YInvert);
                }
            }
            Event::Ready { .. } => {
                app.build_bright(idx);
            }
            Event::Failed => {
                eprintln!("waysnip: screencopy failed on output {idx}");
                app.exit = true;
            }
            _ => {}
        }
    }
}

// --- SCTK handlers -------------------------------------------------------

impl CompositorHandler for App {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: i32) {}
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wl_output::Transform) {}
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}

    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, surface: &wl_surface::WlSurface, _: u32) {
        if let Some(i) = self.monitor_by_surface(surface) {
            // Animation tick for held-key motion.
            if self.monitors[i].selection.tick() {
                self.monitors[i].dirty = true;
            } else if self.monitors[i].selection.held.any_arrow() {
                self.monitors[i].dirty = true; // keep the loop alive while held
            }
        }
    }
}

impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl LayerShellHandler for App {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        self.exit = true;
    }
    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _: u32,
    ) {
        if let Some(i) = self.monitors.iter().position(|m| m.layer.as_ref() == Some(layer)) {
            let (mut w, mut h) = configure.new_size;
            if w == 0 || h == 0 {
                // Fall back to the physical capture size if the compositor left it to us.
                w = self.monitors[i].phys_w;
                h = self.monitors[i].phys_h;
            }
            let m = &mut self.monitors[i];
            m.log_w = w;
            m.log_h = h;
            m.selection = Selection::new(w as f32, h as f32);
            m.configured = true;
            m.dirty = true;
        }
    }
}

impl SeatHandler for App {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn new_capability(&mut self, _: &Connection, qh: &QueueHandle<Self>, seat: wl_seat::WlSeat, cap: Capability) {
        if cap == Capability::Keyboard && self.keyboard.is_none() {
            if let Ok(kb) = self.seat_state.get_keyboard(qh, &seat, None) {
                self.keyboard = Some(kb);
            }
        }
        if cap == Capability::Pointer && self.pointer.is_none() {
            if let Ok(ptr) = self.seat_state.get_pointer(qh, &seat) {
                self.pointer = Some(ptr);
            }
        }
    }
    fn remove_capability(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat, _: Capability) {}
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl KeyboardHandler for App {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        surface: &wl_surface::WlSurface,
        _: u32,
        _: &[u32],
        _: &[Keysym],
    ) {
        if let Some(i) = self.monitor_by_surface(surface) {
            self.active = Some(i);
        }
    }
    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, surface: &wl_surface::WlSurface, _: u32) {
        // Clear held arrows so motion doesn't get stuck if focus leaves mid-press.
        if let Some(i) = self.monitor_by_surface(surface) {
            self.monitors[i].selection.clear_held();
            self.monitors[i].dirty = true;
        }
    }

    // Held-key state is tracked via press/release, so synthetic repeats are ignored.
    fn repeat_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, _: KeyEvent) {}

    fn press_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, event: KeyEvent) {
        self.held_keys.insert(event.raw_code);
        if self.pending_exit {
            return; // already quitting; just track releases
        }
        match event.keysym {
            Keysym::Escape => self.cancel(),
            Keysym::Return | Keysym::KP_Enter => self.confirm(),
            k => {
                if let Some(i) = self.active {
                    let sel = &mut self.monitors[i].selection;
                    match k {
                        Keysym::Left => sel.held.left = true,
                        Keysym::Right => sel.held.right = true,
                        Keysym::Up => sel.held.up = true,
                        Keysym::Down => sel.held.down = true,
                        _ => return,
                    }
                    // Spawn the centered box on the first arrow so a tap works too;
                    // holding then nudges it via the animation tick.
                    sel.ensure_box();
                    self.monitors[i].dirty = true;
                }
            }
        }
    }

    fn release_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, event: KeyEvent) {
        self.held_keys.remove(&event.raw_code);
        if self.pending_exit {
            // Quit only once every physically-held key is up.
            if self.held_keys.is_empty() {
                self.exit = true;
            }
            return;
        }
        if let Some(i) = self.active {
            let held = &mut self.monitors[i].selection.held;
            match event.keysym {
                Keysym::Left => held.left = false,
                Keysym::Right => held.right = false,
                Keysym::Up => held.up = false,
                Keysym::Down => held.down = false,
                _ => return,
            }
            self.monitors[i].dirty = true;
        }
    }

    fn update_modifiers(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        modifiers: Modifiers,
        _: RawModifiers,
        _: u32,
    ) {
        for m in &mut self.monitors {
            m.selection.held.resize = modifiers.shift;
            m.selection.held.fine = modifiers.ctrl;
            m.selection.held.fast = modifiers.alt;
        }
    }
}

impl PointerHandler for App {
    fn pointer_frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_pointer::WlPointer, events: &[PointerEvent]) {
        for event in events {
            let Some(i) = self.monitor_by_surface(&event.surface) else { continue };
            let (x, y) = (event.position.0 as f32, event.position.1 as f32);
            match event.kind {
                PointerEventKind::Enter { .. } => {
                    self.monitors[i].pointer_in = true;
                    self.monitors[i].selection.set_mouse(x, y);
                    self.active = Some(i);
                    self.monitors[i].dirty = true;
                }
                PointerEventKind::Leave { .. } => {
                    self.monitors[i].pointer_in = false;
                    self.monitors[i].dirty = true;
                }
                PointerEventKind::Motion { .. } => {
                    let m = &mut self.monitors[i];
                    if m.selection.active {
                        m.selection.drag_to(x, y);
                        m.targeted = None;
                    } else {
                        m.selection.set_mouse(x, y);
                        m.targeted = windows::target_at(&m.window_regions, x, y);
                    }
                    m.dirty = true;
                }
                PointerEventKind::Press { button, .. } => {
                    self.active = Some(i);
                    if button == BTN_RIGHT {
                        self.cancel();
                    } else if button == BTN_LEFT {
                        self.monitors[i].selection.press(x, y);
                        self.monitors[i].dirty = true;
                    }
                }
                PointerEventKind::Release { button, .. } => {
                    if button == BTN_LEFT {
                        self.active = Some(i);
                        if self.monitors[i].selection.has_area() {
                            // Dragged a rectangle.
                            self.confirm();
                        } else if let Some(t) = self.monitors[i].targeted {
                            // Click with no drag on a window -> grab that window.
                            if let Some(rect) = self
                                .monitors[i]
                                .window_regions
                                .get(t)
                                .map(|w| (w.x, w.y, w.w, w.h))
                            {
                                self.monitors[i].selection.set_rect(rect.0, rect.1, rect.2, rect.3);
                                self.confirm();
                            }
                        }
                    }
                }
                PointerEventKind::Axis { .. } => {}
            }
        }
    }
}

impl ShmHandler for App {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

delegate_compositor!(App);
delegate_output!(App);
delegate_shm!(App);
delegate_seat!(App);
delegate_keyboard!(App);
delegate_pointer!(App);
delegate_layer!(App);
delegate_registry!(App);
