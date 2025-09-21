use egui::{Key, Modifiers, MouseWheelUnit, PointerButton, Pos2, RawInput, Rect};
use sdl2::event::WindowEvent;
use sdl2::keyboard::Keycode;
use sdl2::keyboard::Mod;
use sdl2::keyboard::Scancode;
use sdl2::mouse::{Cursor, MouseButton, SystemCursor};
use sdl2::video::Window;

pub struct FusedCursor {
    cursor: sdl2::mouse::Cursor,
    system_cursor: sdl2::mouse::SystemCursor,
}

impl Default for FusedCursor {
    fn default() -> Self {
        Self {
            cursor: sdl2::mouse::Cursor::from_system(sdl2::mouse::SystemCursor::Arrow).unwrap(),
            system_cursor: sdl2::mouse::SystemCursor::Arrow,
        }
    }
}

#[must_use]
#[derive(Clone, Copy, Debug, Default)]
pub struct EventResponse {
    /// If true, egui consumed this event, i.e. wants exclusive use of this event
    /// (e.g. a mouse click on an egui window, or entering text into a text field).
    ///
    /// For instance, if you use egui for a game, you should only
    /// pass on the events to your game when [`Self::consumed`] is `false`.
    ///
    /// Note that egui uses `tab` to move focus between elements, so this will always be `true` for tabs.
    pub consumed: bool,

    /// Do we need an egui refresh because of this event?
    pub repaint: bool,
}

impl EventResponse {
    pub fn new(consumed: bool, repaint: bool) -> Self {
        Self { consumed, repaint }
    }
}

/// Handles the integration between egui and a sdl2 Window.
///
/// Instantiate one of these per viewport/window.
pub struct State {
    /// Shared clone.
    egui_ctx: egui::Context,
    egui_input: RawInput,
    modifiers: Modifiers,
    start_time: std::time::Instant, // todo: use web_time?
    viewport_id: egui::ViewportId,
    mouse_pointer_position: egui::Pos2,
    fused_cursor: FusedCursor,
}

impl State {
    pub fn new(egui_ctx: egui::Context, viewport_id: egui::ViewportId) -> Self {
        State {
            egui_ctx,
            viewport_id,
            start_time: std::time::Instant::now(),
            egui_input: RawInput::default(),
            modifiers: Modifiers::default(),
            mouse_pointer_position: egui::Pos2::default(),
            fused_cursor: FusedCursor::default(),
        }
    }

    /// Call with the output given by `egui`.
    ///
    /// This will, if needed:
    /// * update the cursor
    /// * copy text to the clipboard
    /// * open any clicked urls
    /// * update the IME
    /// *
    pub fn handle_platform_output(
        &mut self,
        window: &Window,
        platform_output: egui::PlatformOutput,
    ) {
        for command in &platform_output.commands {
            match command {
                egui::OutputCommand::CopyText(text) => {
                    let result = window.subsystem().clipboard().set_clipboard_text(text);

                    if result.is_err() {
                        log::warn!("Failed to set copied text to clipboard");
                    }
                }
                egui::OutputCommand::CopyImage(_color_image) => {
                    log::warn!("CopyImage is not supported")
                }
                egui::OutputCommand::OpenUrl(_open_url) => log::warn!("OpenUrl is not supported"),
            }
        }

        set_cursor_icon(&mut self.fused_cursor, platform_output.cursor_icon);
    }

    /// Prepare for a new frame by extracting the accumulated input,
    ///
    /// as well as setting [the time](egui::RawInput::time) and [screen rectangle](egui::RawInput::screen_rect).
    ///
    /// You need to set [`egui::RawInput::viewports`] yourself though.
    /// Use [`update_viewport_info`] to update the info for each
    /// viewport.
    pub fn take_egui_input(&mut self, window: &Window) -> egui::RawInput {
        self.egui_input.time = Some(self.start_time.elapsed().as_secs_f64());
        self.update_screen_rect(window); // todo: do we need to it here?

        // Tell egui which viewport is now active:
        self.egui_input.viewport_id = self.viewport_id;

        self.egui_input
            .viewports
            .entry(self.viewport_id)
            .or_default()
            .native_pixels_per_point = Some(scale_factor(window) as f32);

        self.egui_input.take()
    }

    /// Call this when there is a new event.
    ///
    /// The result can be found in [`Self::egui_input`] and be extracted with [`Self::take_egui_input`].
    pub fn on_event(
        &mut self,
        window: &sdl2::video::Window,
        event: &sdl2::event::Event,
    ) -> EventResponse {
        if event.get_window_id() != Some(window.id()) {
            return EventResponse::default();
        }

        use sdl2::event::Event::*;
        match event {
            Window { win_event, .. } => self.on_window_event(*win_event, window),
            MouseButtonDown { mouse_btn, .. } => self.on_mouse_event(*mouse_btn, false),
            MouseButtonUp { mouse_btn, .. } => self.on_mouse_event(*mouse_btn, true),
            MouseMotion { x, y, .. } => {
                let pixels_per_point = pixels_per_point(&self.egui_ctx, window);
                self.mouse_pointer_position =
                    egui::pos2(*x as f32 / pixels_per_point, *y as f32 / pixels_per_point);
                self.egui_input
                    .events
                    .push(egui::Event::PointerMoved(self.mouse_pointer_position));
                EventResponse {
                    repaint: true,
                    consumed: self.egui_ctx.is_using_pointer(),
                }
            }
            KeyUp {
                keycode: Some(kc),
                scancode: Some(sc),
                keymod,
                repeat,
                ..
            } => self.on_keyboard_event(*kc, *sc, *keymod, false, *repeat),
            KeyDown {
                keycode: Some(kc),
                scancode: Some(sc),
                keymod,
                repeat,
                ..
            } => {
                let resp = self.on_keyboard_event(*kc, *sc, *keymod, false, *repeat);

                if self.modifiers.command && *kc == Keycode::C {
                    self.egui_input.events.push(egui::Event::Copy);
                } else if self.modifiers.command && *kc == Keycode::X {
                    self.egui_input.events.push(egui::Event::Cut);
                } else if self.modifiers.command && *kc == Keycode::V {
                    if let Ok(contents) = window.subsystem().clipboard().clipboard_text() {
                        self.egui_input.events.push(egui::Event::Text(contents));
                    }
                }

                resp
            }
            TextInput { text, .. } => {
                if !text.is_empty() {
                    // On some platforms we get here when the user presses Cmd-C (copy), ctrl-W, etc.
                    // We need to ignore these characters that are side-effects of commands.
                    // Also make sure the key is pressed (not released). On Linux, text might
                    // contain some data even when the key is released.
                    let is_cmd = self.egui_input.modifiers.ctrl
                        || self.egui_input.modifiers.command
                        || self.egui_input.modifiers.mac_cmd;

                    if !is_cmd {
                        self.egui_input
                            .events
                            .push(egui::Event::Text(text.to_owned()));
                    }
                }

                EventResponse::new(false, true)
            }
            MouseWheel { x, y, .. } => {
                let pixels_per_point = pixels_per_point(&self.egui_ctx, window);
                let delta = egui::vec2(*x as f32, *y as f32) * pixels_per_point;

                if self.egui_input.modifiers.command {
                    // zoom
                    let delta = (delta.y / 125.0).exp();
                    self.egui_input.events.push(egui::Event::Zoom(delta));
                } else if self.egui_input.modifiers.shift {
                    // horizontal scroll
                    self.egui_input.events.push(egui::Event::MouseWheel {
                        unit: MouseWheelUnit::Point,
                        delta: egui::vec2(delta.x + delta.y, 0.0),
                        modifiers: Default::default(),
                    });
                } else {
                    // regular scroll
                    self.egui_input.events.push(egui::Event::MouseWheel {
                        unit: MouseWheelUnit::Point,
                        delta: egui::vec2(delta.x, delta.y),
                        modifiers: Default::default(),
                    });
                }
                EventResponse {
                    repaint: true,
                    consumed: self.egui_ctx.wants_pointer_input(),
                }
            }
            _ => EventResponse::default(),
        }
    }

    fn on_window_event(&mut self, event: WindowEvent, window: &Window) -> EventResponse {
        match event {
            WindowEvent::Minimized
            | WindowEvent::Maximized
            | WindowEvent::Resized(_, _)
            | WindowEvent::SizeChanged(_, _) => {
                self.update_screen_rect(window);

                EventResponse {
                    repaint: true,
                    consumed: false,
                }
            }
            WindowEvent::Shown
            | WindowEvent::Hidden
            | WindowEvent::Exposed
            | WindowEvent::Moved(_, _)
            | WindowEvent::Restored
            | WindowEvent::Enter
            | WindowEvent::Leave
            | WindowEvent::Close => EventResponse::new(false, true),
            WindowEvent::TakeFocus | WindowEvent::FocusGained => {
                self.egui_input.focused = true;
                self.egui_input
                    .events
                    .push(egui::Event::WindowFocused(true));
                EventResponse {
                    repaint: true,
                    consumed: false,
                }
            }
            WindowEvent::FocusLost => {
                self.egui_input.focused = false;
                self.egui_input
                    .events
                    .push(egui::Event::WindowFocused(false));
                EventResponse {
                    repaint: true,
                    consumed: false,
                }
            }
            WindowEvent::HitTest
            | WindowEvent::ICCProfChanged
            | WindowEvent::DisplayChanged(_)
            | WindowEvent::None => EventResponse::default(),
        }
    }

    fn on_mouse_event(&mut self, button: MouseButton, pressed: bool) -> EventResponse {
        let Some(button) = into_egui_button(button) else {
            return EventResponse::default();
        };

        self.egui_input.events.push(egui::Event::PointerButton {
            pos: self.mouse_pointer_position,
            button,
            pressed,
            modifiers: self.modifiers,
        });
        EventResponse {
            repaint: true,
            consumed: self.egui_ctx.wants_pointer_input(),
        }
    }

    fn on_keyboard_event(
        &mut self,
        keycode: Keycode,
        scancode: Scancode,
        keymod: Mod,
        pressed: bool,
        repeat: bool,
    ) -> EventResponse {
        let Some(key) = into_egui_key(keycode) else {
            return EventResponse::default();
        };

        self.modifiers = into_egui_modifiers(keymod);
        self.egui_input.events.push(egui::Event::Key {
            key,
            physical_key: into_egui_physical_key(scancode),
            pressed,
            repeat,
            modifiers: self.modifiers,
        });

        // When pressing the Tab key, egui focuses the first focusable element, hence Tab always consumes.
        let consumed = self.egui_ctx.wants_keyboard_input() || key == Key::Tab;
        EventResponse {
            repaint: true,
            consumed,
        }
    }

    fn update_screen_rect(&mut self, window: &Window) {
        let screen_size_in_pixels = screen_size_in_pixels(window);
        let screen_size_in_points =
            screen_size_in_pixels / pixels_per_point(&self.egui_ctx, window);

        self.egui_input.screen_rect = (screen_size_in_points.x > 0.0
            && screen_size_in_points.y > 0.0)
            .then(|| Rect::from_min_size(Pos2::ZERO, screen_size_in_points));
    }
}

pub fn into_egui_modifiers(m: Mod) -> Modifiers {
    let mut mods = Modifiers::NONE;

    if m.intersects(Mod::LCTRLMOD | Mod::RCTRLMOD) {
        mods.ctrl = true;
        mods.command = true;
    }

    if m.intersects(Mod::LSHIFTMOD | Mod::RSHIFTMOD) {
        mods.shift = true;
    }

    if m.intersects(Mod::LALTMOD | Mod::RALTMOD) {
        mods.alt = true;
    }

    if m.intersects(Mod::LGUIMOD | Mod::RGUIMOD) {
        mods.mac_cmd = true;
        mods.command = true;
    }

    mods
}
pub fn into_egui_physical_key(scancode: Scancode) -> Option<Key> {
    match scancode {
        // Letters
        Scancode::A => Some(Key::A),
        Scancode::B => Some(Key::B),
        Scancode::C => Some(Key::C),
        Scancode::D => Some(Key::D),
        Scancode::E => Some(Key::E),
        Scancode::F => Some(Key::F),
        Scancode::G => Some(Key::G),
        Scancode::H => Some(Key::H),
        Scancode::I => Some(Key::I),
        Scancode::J => Some(Key::J),
        Scancode::K => Some(Key::K),
        Scancode::L => Some(Key::L),
        Scancode::M => Some(Key::M),
        Scancode::N => Some(Key::N),
        Scancode::O => Some(Key::O),
        Scancode::P => Some(Key::P),
        Scancode::Q => Some(Key::Q),
        Scancode::R => Some(Key::R),
        Scancode::S => Some(Key::S),
        Scancode::T => Some(Key::T),
        Scancode::U => Some(Key::U),
        Scancode::V => Some(Key::V),
        Scancode::W => Some(Key::W),
        Scancode::X => Some(Key::X),
        Scancode::Y => Some(Key::Y),
        Scancode::Z => Some(Key::Z),

        // Numbers
        Scancode::Num0 => Some(Key::Num0),
        Scancode::Num1 => Some(Key::Num1),
        Scancode::Num2 => Some(Key::Num2),
        Scancode::Num3 => Some(Key::Num3),
        Scancode::Num4 => Some(Key::Num4),
        Scancode::Num5 => Some(Key::Num5),
        Scancode::Num6 => Some(Key::Num6),
        Scancode::Num7 => Some(Key::Num7),
        Scancode::Num8 => Some(Key::Num8),
        Scancode::Num9 => Some(Key::Num9),

        // Function keys
        Scancode::F1 => Some(Key::F1),
        Scancode::F2 => Some(Key::F2),
        Scancode::F3 => Some(Key::F3),
        Scancode::F4 => Some(Key::F4),
        Scancode::F5 => Some(Key::F5),
        Scancode::F6 => Some(Key::F6),
        Scancode::F7 => Some(Key::F7),
        Scancode::F8 => Some(Key::F8),
        Scancode::F9 => Some(Key::F9),
        Scancode::F10 => Some(Key::F10),
        Scancode::F11 => Some(Key::F11),
        Scancode::F12 => Some(Key::F12),

        // Navigation
        Scancode::Up => Some(Key::ArrowUp),
        Scancode::Down => Some(Key::ArrowDown),
        Scancode::Left => Some(Key::ArrowLeft),
        Scancode::Right => Some(Key::ArrowRight),

        // Special
        Scancode::Return => Some(Key::Enter),
        Scancode::Escape => Some(Key::Escape),
        Scancode::Backspace => Some(Key::Backspace),
        Scancode::Tab => Some(Key::Tab),
        Scancode::Space => Some(Key::Space),

        _ => None,
    }
}

fn set_cursor_icon(fused: &mut FusedCursor, cursor_icon: egui::CursorIcon) {
    let system_cursor = match cursor_icon {
        egui::CursorIcon::Crosshair => SystemCursor::Crosshair,
        egui::CursorIcon::Default => SystemCursor::Arrow,
        egui::CursorIcon::Grab => SystemCursor::Hand,
        egui::CursorIcon::Grabbing => SystemCursor::SizeAll,
        egui::CursorIcon::Move => SystemCursor::SizeAll,
        egui::CursorIcon::PointingHand => SystemCursor::Hand,
        egui::CursorIcon::ResizeHorizontal => SystemCursor::SizeWE,
        egui::CursorIcon::ResizeNeSw => SystemCursor::SizeNESW,
        egui::CursorIcon::ResizeNwSe => SystemCursor::SizeNWSE,
        egui::CursorIcon::ResizeVertical => SystemCursor::SizeNS,
        egui::CursorIcon::Text => SystemCursor::IBeam,
        egui::CursorIcon::NotAllowed | egui::CursorIcon::NoDrop => SystemCursor::No,
        egui::CursorIcon::Wait => SystemCursor::Wait,
        //There doesn't seem to be a suitable SDL equivalent...
        _ => SystemCursor::Arrow,
    };

    if system_cursor != fused.system_cursor {
        fused.cursor = Cursor::from_system(system_cursor).unwrap();
        fused.system_cursor = system_cursor;
        fused.cursor.set();
    }
}

pub fn screen_size_in_pixels(window: &Window) -> egui::Vec2 {
    let (width, height) = window.drawable_size();
    egui::vec2(width as f32, height as f32)
}

pub fn pixels_per_point(egui_ctx: &egui::Context, window: &Window) -> f32 {
    let (drawable_w, _drawable_h) = window.drawable_size();
    let (win_w, win_h) = window.size();

    // Avoid divide by zero
    let native_pixels_per_point = if win_w > 0 && win_h > 0 {
        drawable_w as f32 / win_w as f32
    } else {
        1.0
    };

    let egui_zoom_factor = egui_ctx.zoom_factor();
    egui_zoom_factor * native_pixels_per_point
}

pub fn scale_factor(window: &Window) -> f32 {
    let (win_w, win_h) = window.size();
    let (draw_w, _draw_h) = window.drawable_size();

    if win_w > 0 && win_h > 0 {
        draw_w as f32 / win_w as f32
    } else {
        1.0
    }
}

pub fn into_egui_button(btn: MouseButton) -> Option<PointerButton> {
    match btn {
        MouseButton::Left => Some(egui::PointerButton::Primary),
        MouseButton::Middle => Some(egui::PointerButton::Middle),
        MouseButton::Right => Some(egui::PointerButton::Secondary),
        MouseButton::Unknown => None,
        MouseButton::X1 => Some(egui::PointerButton::Extra1),
        MouseButton::X2 => Some(egui::PointerButton::Extra2),
    }
}

pub fn into_egui_key(key: Keycode) -> Option<Key> {
    Some(match key {
        // Arrows
        Keycode::Left => Key::ArrowLeft,
        Keycode::Up => Key::ArrowUp,
        Keycode::Right => Key::ArrowRight,
        Keycode::Down => Key::ArrowDown,

        // Control keys
        Keycode::Escape => Key::Escape,
        Keycode::Tab => Key::Tab,
        Keycode::Backspace => Key::Backspace,
        Keycode::Space => Key::Space,
        Keycode::Return => Key::Enter,

        // Navigation
        Keycode::Insert => Key::Insert,
        Keycode::Home => Key::Home,
        Keycode::Delete => Key::Delete,
        Keycode::End => Key::End,
        Keycode::PageDown => Key::PageDown,
        Keycode::PageUp => Key::PageUp,

        // Numbers (top row + numpad)
        Keycode::Kp0 | Keycode::Num0 => Key::Num0,
        Keycode::Kp1 | Keycode::Num1 => Key::Num1,
        Keycode::Kp2 | Keycode::Num2 => Key::Num2,
        Keycode::Kp3 | Keycode::Num3 => Key::Num3,
        Keycode::Kp4 | Keycode::Num4 => Key::Num4,
        Keycode::Kp5 | Keycode::Num5 => Key::Num5,
        Keycode::Kp6 | Keycode::Num6 => Key::Num6,
        Keycode::Kp7 | Keycode::Num7 => Key::Num7,
        Keycode::Kp8 | Keycode::Num8 => Key::Num8,
        Keycode::Kp9 | Keycode::Num9 => Key::Num9,

        // Letters
        Keycode::A => Key::A,
        Keycode::B => Key::B,
        Keycode::C => Key::C,
        Keycode::D => Key::D,
        Keycode::E => Key::E,
        Keycode::F => Key::F,
        Keycode::G => Key::G,
        Keycode::H => Key::H,
        Keycode::I => Key::I,
        Keycode::J => Key::J,
        Keycode::K => Key::K,
        Keycode::L => Key::L,
        Keycode::M => Key::M,
        Keycode::N => Key::N,
        Keycode::O => Key::O,
        Keycode::P => Key::P,
        Keycode::Q => Key::Q,
        Keycode::R => Key::R,
        Keycode::S => Key::S,
        Keycode::T => Key::T,
        Keycode::U => Key::U,
        Keycode::V => Key::V,
        Keycode::W => Key::W,
        Keycode::X => Key::X,
        Keycode::Y => Key::Y,
        Keycode::Z => Key::Z,

        // Function keys
        Keycode::F1 => Key::F1,
        Keycode::F2 => Key::F2,
        Keycode::F3 => Key::F3,
        Keycode::F4 => Key::F4,
        Keycode::F5 => Key::F5,
        Keycode::F6 => Key::F6,
        Keycode::F7 => Key::F7,
        Keycode::F8 => Key::F8,
        Keycode::F9 => Key::F9,
        Keycode::F10 => Key::F10,
        Keycode::F11 => Key::F11,
        Keycode::F12 => Key::F12,

        // Symbols & punctuation (only those egui supports)
        Keycode::Minus => Key::Minus,
        Keycode::Equals => Key::Equals,
        Keycode::Semicolon => Key::Semicolon,
        Keycode::Comma => Key::Comma,
        Keycode::Period => Key::Period,
        Keycode::Slash => Key::Slash,
        Keycode::Backslash => Key::Backslash,

        _ => {
            return None;
        }
    })
}
