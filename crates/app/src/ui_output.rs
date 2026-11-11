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
    /// `O`: the picture, fullscreen, wherever it can't freeze. With a
    /// second display: the output window, fullscreen there. With one
    /// display: THIS window goes fullscreen (picture only) — a second
    /// window fullscreened over the controls would put them in another
    /// macOS Space, where they stop redrawing and the output (painted from
    /// their frame loop) would freeze.
    pub(crate) fn toggle_output(&mut self) {
        if self.output.open {
            self.output.open = false;
            self.status = "output window closed".into();
            return;
        }
        let ds = displays::list();
        match plan_output(&ds, None) {
            OutputPlan::Window { display: Some(d), fullscreen } => {
                self.status = format!("output fullscreen on the second display ({:.0}×{:.0}) — O closes it", d.size[0], d.size[1]);
                self.output = OutputWin { open: true, fullscreen, target: Some(d), age: 0 };
            }
            _ => self.enter_single_screen_output(),
        }
    }

    /// Open the output as a normal window (Perform → "Display 2"): it goes
    /// fullscreen on a second display, or stays a window you can drag to
    /// a projector when there is only one.
    pub(crate) fn open_output_window(&mut self) {
        if self.output.open {
            return;
        }
        let target = displays::pick_output(&displays::list());
        self.status = match target {
            Some(_) => "output fullscreen on the second display — O closes it".into(),
            None => "output window — drag it to the projector, then F (or double-click)".into(),
        };
        self.output = OutputWin { open: true, fullscreen: target.is_some(), target, age: 0 };
    }

    fn enter_single_screen_output(&mut self) {
        self.status = "fullscreen on this display — esc comes back".into();
        self.pending_fullscreen = true;
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
            // Created fullscreen on the display it is positioned on (only
            // ever a non-main display). The builder stays fixed while
            // open; later toggles are commands.
            .with_fullscreen(self.output.fullscreen && self.output.target.is_some());
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
            if self.output.fullscreen {
                self.output.fullscreen = false;
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
            } else {
                // Fullscreen only where it can't cover (and freeze) the
                // controls: the display this window is on now must not be
                // the main one. Otherwise the main window takes over.
                let center = ctx.input(|i| i.viewport().outer_rect).map(|r| r.center());
                match plan_output(&displays::list(), center) {
                    OutputPlan::Window { display: Some(_), .. } => {
                        self.output.fullscreen = true;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
                    }
                    _ => {
                        self.output.open = false;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        self.enter_single_screen_output();
                        return;
                    }
                }
            }
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

/// Where the fullscreen picture goes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OutputPlan {
    /// The output window, on `display` (None = no external display).
    Window { display: Option<displays::Display>, fullscreen: bool },
    /// The main window itself goes fullscreen (single display).
    MainFullscreen,
}

/// Decide where fullscreen output goes. `window_center` = where the output
/// window currently is (global points), if open: fullscreen is allowed on
/// the display under it unless that's the main (controls) display.
pub fn plan_output(ds: &[displays::Display], window_center: Option<egui::Pos2>) -> OutputPlan {
    let on = |d: &displays::Display, p: egui::Pos2| {
        p.x >= d.origin[0] && p.x < d.origin[0] + d.size[0] && p.y >= d.origin[1] && p.y < d.origin[1] + d.size[1]
    };
    match window_center {
        Some(c) => match ds.iter().find(|d| on(d, c)) {
            Some(d) if !d.main => OutputPlan::Window { display: Some(*d), fullscreen: true },
            _ => OutputPlan::MainFullscreen,
        },
        None => match displays::pick_output(ds) {
            Some(d) => OutputPlan::Window { display: Some(d), fullscreen: true },
            None => OutputPlan::MainFullscreen,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use displays::Display;

    #[test]
    fn single_screen_output_never_covers_the_controls_with_a_second_window() {
        let laptop = Display { origin: [0.0, 0.0], size: [1512.0, 982.0], main: true };
        let projector = Display { origin: [1512.0, 0.0], size: [1920.0, 1080.0], main: false };
        assert_eq!(plan_output(&[laptop], None), OutputPlan::MainFullscreen);
        assert_eq!(plan_output(&[], None), OutputPlan::MainFullscreen, "unknown displays: safe default");
        assert_eq!(plan_output(&[laptop, projector], None), OutputPlan::Window { display: Some(projector), fullscreen: true });
        // A window dragged onto the projector may go fullscreen there…
        assert_eq!(
            plan_output(&[laptop, projector], Some(egui::pos2(2000.0, 400.0))),
            OutputPlan::Window { display: Some(projector), fullscreen: true }
        );
        // …but one still on the laptop hands over to the main window.
        assert_eq!(plan_output(&[laptop, projector], Some(egui::pos2(500.0, 400.0))), OutputPlan::MainFullscreen);
    }
}
