//! Performance output: a second OS window that shows ONLY the composited
//! canvas — black, aspect-correct letterbox, no chrome, no cursor — while
//! the control window stays usable on the laptop.
//!
//! * `O` (or the top-bar "output" button) opens/closes it.
//! * With a second display (projector, TV, external monitor) it opens
//!   fullscreen THERE; with one display it opens fullscreen on this one.
//! * In the output window: `esc` leaves fullscreen (it becomes a normal
//!   window you can drag to another screen), `F` or double-click toggles
//!   fullscreen, `O` closes it; space / enter / the perform pad keys still
//!   play, so you can perform with the output focused.
//!
//! Rendering: the window is an egui immediate viewport. eframe's wgpu
//! backend shares one device and one callback-resource map across all
//! viewports, so the output adds the same `gpu::PreviewCallback` as the
//! editor preview: the scene renders once per frame offscreen and each
//! window samples that texture (see `gpu.rs`).

use crate::{displays, ui_perform, App, AppMode};
use eframe::egui;
use std::sync::atomic::Ordering;

/// State of the output window.
#[derive(Debug, Default)]
pub struct OutputWin {
    pub open: bool,
    pub fullscreen: bool,
    /// Display it was sent to (None = the control window's display).
    pub target: Option<displays::Display>,
    /// Frames since it opened.
    age: u32,
}

fn output_id() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("chromagrain_performance_output")
}

impl App {
    pub(crate) fn toggle_output(&mut self) {
        if self.output.open {
            self.output.open = false;
            self.status = "output window closed".into();
            return;
        }
        let target = displays::pick_output(&displays::list());
        self.status = match target {
            Some(d) => format!(
                "output fullscreen on the second display ({:.0}×{:.0}) — O closes it",
                d.size[0], d.size[1]
            ),
            None => "output fullscreen on this display — esc makes it a window, O closes it".into(),
        };
        self.output = OutputWin { open: true, fullscreen: true, target, age: 0 };
    }

    /// Draw the output window (call every frame from `update`).
    pub(crate) fn output_ui(&mut self, ctx: &egui::Context) {
        if !self.output.open {
            return;
        }
        let mut builder = egui::ViewportBuilder::default()
            .with_title("chromagrain — output")
            .with_inner_size([960.0, 540.0])
            .with_min_inner_size([160.0, 90.0])
            .with_active(true)
            // Created fullscreen on the display it is positioned on. (The
            // builder stays fixed while open; later toggles are commands.)
            .with_fullscreen(true);
        if let Some(d) = self.output.target {
            builder = builder.with_position([d.origin[0] + 40.0, d.origin[1] + 40.0]);
        }
        ctx.show_viewport_immediate(output_id(), builder, |ctx, _class| self.output_contents(ctx));
    }

    fn output_contents(&mut self, ctx: &egui::Context) {
        self.output.age = self.output.age.saturating_add(1);
        if self.output.age == 2 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
        if ctx.input(|i| i.viewport().close_requested()) {
            self.output.open = false;
            return;
        }

        // Window keys first (consumed so nothing else sees them).
        let (esc, f, o) = ctx.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
                i.consume_key(egui::Modifiers::NONE, egui::Key::F)
                    || i.consume_key(egui::Modifiers::SHIFT, egui::Key::F),
                i.consume_key(egui::Modifiers::NONE, egui::Key::O),
            )
        });
        if o {
            self.output.open = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        let mut toggle = f || (esc && self.output.fullscreen);

        let (pw, ph) = (self.plan_project.width.max(1) as f32, self.plan_project.height.max(1) as f32);
        egui::CentralPanel::default().frame(egui::Frame::new().fill(egui::Color32::BLACK)).show(ctx, |ui| {
            let full = ui.max_rect();
            let resp = ui.allocate_rect(full, egui::Sense::click());
            let rect = displays::fit(full, pw / ph);
            if !self.gpu_failed.load(Ordering::Relaxed) {
                ui.painter().add(crate::gpu::PreviewCallback::shape(self.scene.clone(), rect));
            }
            if resp.hovered() {
                ctx.set_cursor_icon(egui::CursorIcon::None);
            }
            toggle |= resp.double_clicked();
        });
        if toggle {
            self.output.fullscreen = !self.output.fullscreen;
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.output.fullscreen));
        }
        self.output_play_keys(ctx);
    }

    /// The playing keys work with the output focused too (nothing that
    /// edits the project, since you can't see the editor from there).
    fn output_play_keys(&mut self, ctx: &egui::Context) {
        let (space, home, tab) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::Space),
                i.key_pressed(egui::Key::Home) || i.key_pressed(egui::Key::Enter),
                i.key_pressed(egui::Key::Tab) && !i.modifiers.any(),
            )
        });
        if space {
            self.toggle_play();
        }
        if home {
            self.seek(if self.loop_on { self.loop_start } else { 0.0 });
        }
        if tab {
            let m = if self.mode == AppMode::Perform { AppMode::Arrange } else { AppMode::Perform };
            self.set_mode(m);
        }
        if self.mode == AppMode::Perform {
            let hit: Vec<usize> = ctx.input(|i| {
                if i.modifiers.command || i.modifiers.ctrl || i.modifiers.alt {
                    return Vec::new();
                }
                ui_perform::PAD_KEYS.iter().enumerate().filter(|(_, (k, _))| i.key_pressed(*k)).map(|(n, _)| n).collect()
            });
            for pad in hit {
                self.toggle_pad(pad);
            }
        }
    }
}
