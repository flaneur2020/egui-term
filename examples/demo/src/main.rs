use std::fs::OpenOptions;
use std::io::Write;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use egui::{CentralPanel, Key, ScrollArea, Slider};

fn main() -> anyhow::Result<()> {
    let mut app = DemoApp::new();

    egui_term::run_with(egui_term::RunOptions::default(), move |ctx, frame| {
        app.update(ctx, frame);
    })?;

    Ok(())
}

struct DemoApp {
    clicks: u32,
    value: f32,
    text: String,
    auto_quit_after_frames: Option<u32>,
    frame_count: u32,
    autotest_exit_on_click: bool,
    autotest_log_clicks: bool,
    debug_log_enabled: bool,
}

impl DemoApp {
    fn new() -> Self {
        let auto_quit_after_frames = std::env::var("EGUI_TERM_AUTOTEST_FRAMES")
            .ok()
            .and_then(|value| value.parse::<u32>().ok())
            .filter(|v| *v > 0);
        let autotest_exit_on_click = std::env::var("EGUI_TERM_AUTOTEST_EXIT_ON_CLICK")
            .map(|v| v == "1")
            .unwrap_or(false);
        let autotest_log_clicks = std::env::var("EGUI_TERM_AUTOTEST_LOG_CLICKS")
            .map(|v| v == "1")
            .unwrap_or(false);
        let debug_log_enabled = std::env::var("EGUI_TERM_DEMO_LOG")
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false);
        if debug_log_enabled {
            demo_log(format!(
                "demo.init: auto_quit={:?} exit_on_click={} log_clicks={}",
                auto_quit_after_frames, autotest_exit_on_click, autotest_log_clicks
            ));
        }

        Self {
            clicks: 0,
            value: 42.0,
            text: "type here".to_owned(),
            auto_quit_after_frames,
            frame_count: 0,
            autotest_exit_on_click,
            autotest_log_clicks,
            debug_log_enabled,
        }
    }

    fn update(&mut self, ctx: &egui::Context, frame: &mut egui_term::Frame) {
        self.frame_count += 1;
        let previous_clicks = self.clicks;
        let mut click_button_rect = None;
        let mut click_button_hovered = false;

        CentralPanel::default().show(ctx, |ui| {
            ui.heading("egui-term demo");
            ui.label("Kitty graphics + egui + crossterm");

            let click_button = ui.button("Click me");
            click_button_rect = Some(click_button.rect);
            click_button_hovered = click_button.hovered();
            if click_button.clicked() {
                self.clicks = self.clicks.saturating_add(1);
            }
            ui.label(format!("Clicks: {}", self.clicks));

            ui.separator();
            ui.label("Text input");
            ui.text_edit_singleline(&mut self.text);

            ui.add(Slider::new(&mut self.value, 0.0..=100.0).text("value"));

            ui.separator();
            ui.label("Scrollable area");
            ScrollArea::vertical().max_height(160.0).show(ui, |ui| {
                for i in 0..30 {
                    ui.label(format!("row #{i}"));
                }
            });

            ui.separator();
            ui.label("Press q or Esc to exit");
            if ui.button("Quit").clicked() {
                frame.close();
            }
        });

        if ctx.input(|i| i.key_pressed(Key::Q)) {
            frame.close();
        }

        if self.clicks != previous_clicks {
            if self.autotest_log_clicks {
                eprintln!("AUTOTEST_CLICK={}", self.clicks);
            }
            demo_log(format!("demo.clicks_changed: {}", self.clicks));
            if self.autotest_exit_on_click {
                frame.close();
            }
        }

        if self
            .auto_quit_after_frames
            .is_some_and(|limit| self.frame_count >= limit)
        {
            demo_log(format!(
                "demo.frame_close: by_frame_limit frame={}",
                self.frame_count
            ));
            frame.close();
        }

        if self.debug_log_enabled && self.frame_count % 20 == 0 {
            let input_summary = ctx.input(|i| {
                format!(
                    "pointer_pos={:?} primary_down={} events={} modifiers={:?}",
                    i.pointer.hover_pos(),
                    i.pointer.primary_down(),
                    i.events.len(),
                    i.modifiers
                )
            });
            demo_log(format!(
                "demo.frame={} clicks={} hovered={} button_rect={:?} {} wants_pointer={} wants_keyboard={} using_pointer={}",
                self.frame_count,
                self.clicks,
                click_button_hovered,
                click_button_rect,
                input_summary,
                ctx.wants_pointer_input(),
                ctx.wants_keyboard_input(),
                ctx.is_using_pointer(),
            ));
        }

        ctx.request_repaint_after(Duration::from_millis(16));
    }
}

fn demo_log(message: impl AsRef<str>) {
    static DEMO_LOG_FILE: OnceLock<Option<Mutex<std::fs::File>>> = OnceLock::new();
    let Some(file_mutex) = DEMO_LOG_FILE.get_or_init(|| {
        let path = std::env::var("EGUI_TERM_DEMO_LOG").ok()?;
        if path.trim().is_empty() {
            return None;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .ok()?;
        Some(Mutex::new(file))
    }) else {
        return;
    };

    let Ok(mut file) = file_mutex.lock() else {
        return;
    };
    let _ = writeln!(file, "{}", message.as_ref());
}
