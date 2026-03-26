use std::time::Instant;

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use egui::{
    Event as EguiEvent, Key, Modifiers, MouseWheelUnit, PlatformOutput, Pos2, RawInput, Rect, Vec2,
    ViewportId,
};

#[derive(Clone, Copy, Debug, Default)]
pub struct EventResponse {
    pub consumed: bool,
    pub repaint: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct StateOptions {
    pub pixels_per_point: f32,
    pub cell_width_px: f32,
    pub cell_height_px: f32,
}

impl Default for StateOptions {
    fn default() -> Self {
        Self {
            pixels_per_point: 1.0,
            cell_width_px: 8.0,
            cell_height_px: 16.0,
        }
    }
}

pub struct State {
    egui_ctx: egui::Context,
    raw_input: RawInput,
    start_time: Instant,
    cols: u16,
    rows: u16,
    options: StateOptions,
    pointer_pos: Option<Pos2>,
    clipboard: String,
}

impl State {
    pub fn new(egui_ctx: egui::Context, cols: u16, rows: u16, options: StateOptions) -> Self {
        let mut slf = Self {
            egui_ctx,
            raw_input: RawInput::default(),
            start_time: Instant::now(),
            cols: cols.max(1),
            rows: rows.max(1),
            options,
            pointer_pos: None,
            clipboard: String::new(),
        };
        slf.egui_ctx.set_pixels_per_point(options.pixels_per_point);
        slf.sync_screen_rect();
        slf
    }

    #[inline]
    pub fn egui_ctx(&self) -> &egui::Context {
        &self.egui_ctx
    }

    #[inline]
    pub fn egui_input(&self) -> &RawInput {
        &self.raw_input
    }

    #[inline]
    pub fn egui_input_mut(&mut self) -> &mut RawInput {
        &mut self.raw_input
    }

    #[inline]
    pub fn screen_size_cells(&self) -> (u16, u16) {
        (self.cols, self.rows)
    }

    #[inline]
    pub fn screen_size_pixels(&self) -> (u32, u32) {
        (
            (self.cols as f32 * self.options.cell_width_px).round() as u32,
            (self.rows as f32 * self.options.cell_height_px).round() as u32,
        )
    }

    #[inline]
    pub fn pixels_per_point(&self) -> f32 {
        self.options.pixels_per_point
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        self.cols = cols.max(1);
        self.rows = rows.max(1);
        self.sync_screen_rect();
    }

    pub fn take_egui_input(&mut self) -> RawInput {
        self.raw_input.time = Some(self.start_time.elapsed().as_secs_f64());
        self.raw_input.viewport_id = ViewportId::ROOT;
        self.raw_input.take()
    }

    pub fn on_event(&mut self, event: &Event) -> EventResponse {
        match event {
            Event::Resize(cols, rows) => {
                self.resize(*cols, *rows);
                EventResponse {
                    consumed: false,
                    repaint: true,
                }
            }
            Event::FocusGained => {
                self.raw_input.focused = true;
                self.raw_input.events.push(EguiEvent::WindowFocused(true));
                EventResponse {
                    consumed: false,
                    repaint: true,
                }
            }
            Event::FocusLost => {
                self.raw_input.focused = false;
                self.raw_input.events.push(EguiEvent::WindowFocused(false));
                EventResponse {
                    consumed: false,
                    repaint: true,
                }
            }
            Event::Mouse(mouse) => self.on_mouse(*mouse),
            Event::Key(key) => self.on_key(*key),
            Event::Paste(text) => {
                self.raw_input.events.push(EguiEvent::Paste(text.clone()));
                EventResponse {
                    consumed: self.egui_ctx.wants_keyboard_input(),
                    repaint: true,
                }
            }
        }
    }

    pub fn handle_platform_output(&mut self, output: PlatformOutput) {
        for command in output.commands {
            match command {
                egui::OutputCommand::CopyText(text) => {
                    self.clipboard = text;
                }
                egui::OutputCommand::CopyImage(_) => {}
                egui::OutputCommand::OpenUrl(_) => {}
            }
        }
    }

    fn on_mouse(&mut self, mouse: MouseEvent) -> EventResponse {
        self.raw_input.modifiers = key_modifiers(mouse.modifiers);
        let pos = self.pointer_pos_from_cell(mouse.column, mouse.row);
        self.pointer_pos = Some(pos);

        self.raw_input.events.push(EguiEvent::PointerMoved(pos));

        match mouse.kind {
            MouseEventKind::Down(button) => {
                if let Some(button) = translate_mouse_button(button) {
                    self.raw_input.events.push(EguiEvent::PointerButton {
                        pos,
                        button,
                        pressed: true,
                        modifiers: self.raw_input.modifiers,
                    });
                }
            }
            MouseEventKind::Up(button) => {
                if let Some(button) = translate_mouse_button(button) {
                    self.raw_input.events.push(EguiEvent::PointerButton {
                        pos,
                        button,
                        pressed: false,
                        modifiers: self.raw_input.modifiers,
                    });
                }
            }
            MouseEventKind::Drag(_) | MouseEventKind::Moved => {}
            MouseEventKind::ScrollUp => {
                self.push_scroll(Vec2::new(0.0, -1.0));
            }
            MouseEventKind::ScrollDown => {
                self.push_scroll(Vec2::new(0.0, 1.0));
            }
            MouseEventKind::ScrollLeft => {
                self.push_scroll(Vec2::new(-1.0, 0.0));
            }
            MouseEventKind::ScrollRight => {
                self.push_scroll(Vec2::new(1.0, 0.0));
            }
        }

        EventResponse {
            consumed: self.egui_ctx.wants_pointer_input() || self.egui_ctx.is_using_pointer(),
            repaint: true,
        }
    }

    fn push_scroll(&mut self, delta: Vec2) {
        self.raw_input.events.push(EguiEvent::MouseWheel {
            unit: MouseWheelUnit::Line,
            delta,
            modifiers: self.raw_input.modifiers,
        });
    }

    fn on_key(&mut self, key_event: KeyEvent) -> EventResponse {
        let modifiers = key_modifiers(key_event.modifiers);
        self.raw_input.modifiers = modifiers;

        let (pressed, repeat) = match key_event.kind {
            KeyEventKind::Press => (true, false),
            KeyEventKind::Repeat => (true, true),
            KeyEventKind::Release => (false, false),
        };

        if pressed && modifiers.command {
            if let KeyCode::Char(ch) = key_event.code {
                match ch.to_ascii_lowercase() {
                    'c' => self.raw_input.events.push(EguiEvent::Copy),
                    'x' => self.raw_input.events.push(EguiEvent::Cut),
                    'v' => {
                        if !self.clipboard.is_empty() {
                            self.raw_input
                                .events
                                .push(EguiEvent::Paste(self.clipboard.clone()));
                        }
                    }
                    _ => {}
                }
            }
        }

        if let Some(key) = translate_key_code(key_event.code) {
            self.raw_input.events.push(EguiEvent::Key {
                key,
                physical_key: None,
                pressed,
                repeat,
                modifiers,
            });
        }

        if pressed && !modifiers.ctrl && !modifiers.alt {
            if let Some(text) = translate_text(key_event.code) {
                if !text.is_empty() {
                    self.raw_input.events.push(EguiEvent::Text(text));
                }
            }
        }

        EventResponse {
            consumed: self.egui_ctx.wants_keyboard_input(),
            repaint: true,
        }
    }

    fn sync_screen_rect(&mut self) {
        let size = self.screen_size_points();
        self.raw_input.screen_rect = Some(Rect::from_min_size(Pos2::ZERO, size));
        self.raw_input
            .viewports
            .entry(ViewportId::ROOT)
            .or_default()
            .native_pixels_per_point = Some(self.options.pixels_per_point);
    }

    fn screen_size_points(&self) -> Vec2 {
        let width = self.cols as f32 * self.options.cell_width_px / self.options.pixels_per_point;
        let height = self.rows as f32 * self.options.cell_height_px / self.options.pixels_per_point;
        Vec2::new(width.max(1.0), height.max(1.0))
    }

    fn pointer_pos_from_cell(&self, column: u16, row: u16) -> Pos2 {
        // Terminal mouse events are cell-addressed. Use the center of the cell
        // as the pointer position to avoid systematic misses on widget edges.
        let x =
            ((column as f32 + 0.5) * self.options.cell_width_px) / self.options.pixels_per_point;
        let y = ((row as f32 + 0.5) * self.options.cell_height_px) / self.options.pixels_per_point;
        Pos2::new(x, y)
    }
}

fn translate_mouse_button(button: MouseButton) -> Option<egui::PointerButton> {
    match button {
        MouseButton::Left => Some(egui::PointerButton::Primary),
        MouseButton::Right => Some(egui::PointerButton::Secondary),
        MouseButton::Middle => Some(egui::PointerButton::Middle),
    }
}

fn key_modifiers(modifiers: KeyModifiers) -> Modifiers {
    Modifiers {
        alt: modifiers.contains(KeyModifiers::ALT),
        ctrl: modifiers.contains(KeyModifiers::CONTROL),
        shift: modifiers.contains(KeyModifiers::SHIFT),
        mac_cmd: false,
        command: modifiers.contains(KeyModifiers::CONTROL),
    }
}

fn translate_text(code: KeyCode) -> Option<String> {
    match code {
        KeyCode::Char(ch) if !ch.is_control() => Some(ch.to_string()),
        _ => None,
    }
}

fn translate_key_code(code: KeyCode) -> Option<Key> {
    match code {
        KeyCode::Backspace => Some(Key::Backspace),
        KeyCode::Enter => Some(Key::Enter),
        KeyCode::Left => Some(Key::ArrowLeft),
        KeyCode::Right => Some(Key::ArrowRight),
        KeyCode::Up => Some(Key::ArrowUp),
        KeyCode::Down => Some(Key::ArrowDown),
        KeyCode::Home => Some(Key::Home),
        KeyCode::End => Some(Key::End),
        KeyCode::PageUp => Some(Key::PageUp),
        KeyCode::PageDown => Some(Key::PageDown),
        KeyCode::Tab | KeyCode::BackTab => Some(Key::Tab),
        KeyCode::Delete => Some(Key::Delete),
        KeyCode::Insert => Some(Key::Insert),
        KeyCode::Esc => Some(Key::Escape),
        KeyCode::F(n) => function_key(n),
        KeyCode::Char(ch) => key_from_char(ch),
        _ => None,
    }
}

fn function_key(n: u8) -> Option<Key> {
    match n {
        1 => Some(Key::F1),
        2 => Some(Key::F2),
        3 => Some(Key::F3),
        4 => Some(Key::F4),
        5 => Some(Key::F5),
        6 => Some(Key::F6),
        7 => Some(Key::F7),
        8 => Some(Key::F8),
        9 => Some(Key::F9),
        10 => Some(Key::F10),
        11 => Some(Key::F11),
        12 => Some(Key::F12),
        _ => None,
    }
}

fn key_from_char(ch: char) -> Option<Key> {
    use Key::*;

    match ch.to_ascii_lowercase() {
        'a' => Some(A),
        'b' => Some(B),
        'c' => Some(C),
        'd' => Some(D),
        'e' => Some(E),
        'f' => Some(F),
        'g' => Some(G),
        'h' => Some(H),
        'i' => Some(I),
        'j' => Some(J),
        'k' => Some(K),
        'l' => Some(L),
        'm' => Some(M),
        'n' => Some(N),
        'o' => Some(O),
        'p' => Some(P),
        'q' => Some(Q),
        'r' => Some(R),
        's' => Some(S),
        't' => Some(T),
        'u' => Some(U),
        'v' => Some(V),
        'w' => Some(W),
        'x' => Some(X),
        'y' => Some(Y),
        'z' => Some(Z),
        '0' => Some(Num0),
        '1' => Some(Num1),
        '2' => Some(Num2),
        '3' => Some(Num3),
        '4' => Some(Num4),
        '5' => Some(Num5),
        '6' => Some(Num6),
        '7' => Some(Num7),
        '8' => Some(Num8),
        '9' => Some(Num9),
        ' ' => Some(Space),
        ':' => Some(Colon),
        ',' => Some(Comma),
        '\\' => Some(Backslash),
        '/' => Some(Slash),
        '|' => Some(Pipe),
        '?' => Some(Questionmark),
        '!' => Some(Exclamationmark),
        '[' => Some(OpenBracket),
        ']' => Some(CloseBracket),
        '{' => Some(OpenCurlyBracket),
        '}' => Some(CloseCurlyBracket),
        '`' => Some(Backtick),
        '-' => Some(Minus),
        '.' => Some(Period),
        '+' => Some(Plus),
        '=' => Some(Equals),
        ';' => Some(Semicolon),
        '\'' => Some(Quote),
        _ => None,
    }
}
