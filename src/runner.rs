use std::{
    io::{self, Write},
    time::{Duration, Instant},
};

use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
    },
    execute,
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};

use crate::{kitty, OffscreenRenderer, Result, State, StateOptions};

#[derive(Clone, Copy, Debug)]
pub struct RunOptions {
    pub state: StateOptions,
    pub exit_on_escape: bool,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            state: StateOptions::default(),
            exit_on_escape: true,
        }
    }
}

#[derive(Default)]
pub struct Frame {
    should_close: bool,
}

impl Frame {
    pub fn close(&mut self) {
        self.should_close = true;
    }
}

pub fn run(app: impl FnMut(&egui::Context, &mut Frame)) -> Result<()> {
    run_with(RunOptions::default(), app)
}

pub fn run_with(
    options: RunOptions,
    mut app: impl FnMut(&egui::Context, &mut Frame),
) -> Result<()> {
    let mut stdout = io::stdout();
    let _guard = TerminalGuard::enter(&mut stdout)?;

    let (mut cols, mut rows) = terminal::size()?;
    if cols == 0 || rows == 0 {
        cols = 80;
        rows = 24;
    }

    let ctx = egui::Context::default();
    let mut state = State::new(ctx.clone(), cols, rows, options.state);
    let mut renderer = OffscreenRenderer::new()?;

    let mut needs_repaint = true;
    let mut next_repaint_at = Instant::now();

    'main: loop {
        let timeout = if needs_repaint {
            Duration::ZERO
        } else {
            next_repaint_at.saturating_duration_since(Instant::now())
        };

        if event::poll(timeout)? {
            let evt = event::read()?;
            if should_exit_event(&evt, options.exit_on_escape) {
                break;
            }
            needs_repaint |= state.on_event(&evt).repaint;

            while event::poll(Duration::ZERO)? {
                let evt = event::read()?;
                if should_exit_event(&evt, options.exit_on_escape) {
                    break 'main;
                }
                needs_repaint |= state.on_event(&evt).repaint;
            }
        }

        if !needs_repaint && Instant::now() < next_repaint_at {
            continue;
        }

        let mut frame = Frame::default();
        let input = state.take_egui_input();
        let output = ctx.run(input, |ctx| app(ctx, &mut frame));

        let repaint_delay = output
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .map(|v| v.repaint_delay)
            .unwrap_or(Duration::ZERO);

        let (width, height) = state.screen_size_pixels();
        let (screen_cols, screen_rows) = state.screen_size_cells();
        if width > 0 && height > 0 {
            let frame_rgba =
                renderer.render(&ctx, &output, width, height, state.pixels_per_point())?;
            kitty::write_frame(&mut stdout, &frame_rgba, screen_cols, screen_rows)?;
            stdout.flush()?;
        }

        state.handle_platform_output(output.platform_output);

        if frame.should_close {
            break;
        }

        next_repaint_at = Instant::now() + repaint_delay;
        needs_repaint = repaint_delay.is_zero();
    }

    Ok(())
}

fn should_exit_event(event: &Event, exit_on_escape: bool) -> bool {
    let Event::Key(key) = event else {
        return false;
    };

    if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
        return false;
    }

    if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('c' | 'C'))
    {
        return true;
    }

    exit_on_escape && matches!(key.code, KeyCode::Esc)
}

struct TerminalGuard;

impl TerminalGuard {
    fn enter(stdout: &mut io::Stdout) -> Result<Self> {
        crossterm::terminal::enable_raw_mode()?;
        execute!(
            stdout,
            EnterAlternateScreen,
            Clear(ClearType::All),
            MoveTo(0, 0),
            EnableMouseCapture,
            Hide,
        )?;
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = crossterm::terminal::disable_raw_mode();
        let mut stdout = io::stdout();
        let _ = execute!(stdout, Show, DisableMouseCapture, LeaveAlternateScreen);
    }
}
