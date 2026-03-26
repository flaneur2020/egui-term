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

        Self {
            clicks: 0,
            value: 42.0,
            text: "type here".to_owned(),
            auto_quit_after_frames,
            frame_count: 0,
            autotest_exit_on_click,
            autotest_log_clicks,
        }
    }

    fn update(&mut self, ctx: &egui::Context, frame: &mut egui_term::Frame) {
        self.frame_count += 1;
        let previous_clicks = self.clicks;

        CentralPanel::default().show(ctx, |ui| {
            ui.heading("egui-term demo");
            ui.label("Kitty graphics + egui + crossterm");

            if ui.button("Click me").clicked() {
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
            if self.autotest_exit_on_click {
                frame.close();
            }
        }

        if self
            .auto_quit_after_frames
            .is_some_and(|limit| self.frame_count >= limit)
        {
            frame.close();
        }

        ctx.request_repaint_after(Duration::from_millis(16));
    }
}
