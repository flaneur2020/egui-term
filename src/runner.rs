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

use crate::{debug_log, kitty, OffscreenRenderer, Result, State, StateOptions};

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
    debug_log::log(format!(
        "runner.start: cols={} rows={} ppp={} cell={}x{}",
        cols,
        rows,
        options.state.pixels_per_point,
        options.state.cell_width_px,
        options.state.cell_height_px
    ));

    let ctx = egui::Context::default();
    let mut state = State::new(ctx.clone(), cols, rows, options.state);
    let mut renderer = OffscreenRenderer::new()?;
    let mut presenter = kitty::KittyPresenter::default();

    let mut needs_repaint = true;
    let mut next_repaint_at = Instant::now();
    let mut frame_index = 0_u64;

    'main: loop {
        let timeout = if needs_repaint {
            Duration::ZERO
        } else {
            next_repaint_at.saturating_duration_since(Instant::now())
        };

        if event::poll(timeout)? {
            let evt = event::read()?;
            debug_log::log(format!("runner.event: {:?}", evt));
            if should_exit_event(&evt, options.exit_on_escape) {
                debug_log::log("runner.exit: by key");
                break;
            }
            let response = state.on_event(&evt);
            needs_repaint |= response.repaint;
            debug_log::log(format!(
                "runner.event_response: repaint={} consumed={}",
                response.repaint, response.consumed
            ));

            while event::poll(Duration::ZERO)? {
                let evt = event::read()?;
                debug_log::log(format!("runner.event.batch: {:?}", evt));
                if should_exit_event(&evt, options.exit_on_escape) {
                    debug_log::log("runner.exit: by key(batch)");
                    break 'main;
                }
                let response = state.on_event(&evt);
                needs_repaint |= response.repaint;
                debug_log::log(format!(
                    "runner.event_response.batch: repaint={} consumed={}",
                    response.repaint, response.consumed
                ));
            }
        }

        if !needs_repaint && Instant::now() < next_repaint_at {
            continue;
        }

        let mut frame = Frame::default();
        let input = state.take_egui_input();
        let output = ctx.run(input, |ctx| app(ctx, &mut frame));
        frame_index = frame_index.saturating_add(1);
        debug_log::log(format!(
            "runner.frame: idx={} shapes={} textures_set={} textures_free={}",
            frame_index,
            output.shapes.len(),
            output.textures_delta.set.len(),
            output.textures_delta.free.len()
        ));

        let repaint_delay = output
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .map(|v| v.repaint_delay)
            .unwrap_or(Duration::ZERO);
        debug_log::log(format!(
            "runner.repaint_delay_ms: {}",
            repaint_delay.as_millis()
        ));

        let (width, height) = state.screen_size_pixels();
        let (screen_cols, screen_rows) = state.screen_size_cells();
        if width > 0 && height > 0 {
            let frame_rgba =
                renderer.render(&ctx, &output, width, height, state.pixels_per_point())?;
            debug_log::log(format!(
                "runner.rendered: {}x{} cells={}x{}",
                width, height, screen_cols, screen_rows
            ));
            presenter.present(&mut stdout, &frame_rgba, screen_cols, screen_rows)?;
            stdout.flush()?;
        }

        state.handle_platform_output(output.platform_output);

        if frame.should_close {
            debug_log::log("runner.exit: frame.close");
            break;
        }

        next_repaint_at = Instant::now() + repaint_delay;
        needs_repaint = repaint_delay.is_zero();
    }

    presenter.clear(&mut stdout)?;
    stdout.flush()?;
    debug_log::log("runner.done");

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
