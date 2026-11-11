//! A deliberately thin egui editor: load a video, the four macros, and a
//! placeholder canvas. The real play-first UI is built in crates/app
//! (branch `overhaul-ui`) and gets embedded here later — don't grow this.

use crate::engine::{Job, Shared};
use crate::params::CgParams;
use nih_plug::prelude::*;
use nih_plug_egui::egui::{self, Color32, Sense, Vec2};
use nih_plug_egui::{create_egui_editor, widgets};
use std::sync::atomic::Ordering;
use std::sync::Arc;

pub fn create(params: Arc<CgParams>, shared: Arc<Shared>) -> Option<Box<dyn Editor>> {
    create_egui_editor(params.editor_state.clone(), (), |_, _| {}, move |ctx, setter, _| {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("chromagrain");
                if ui.button("Load video…").clicked() {
                    let pick = rfd::FileDialog::new()
                        .add_filter("media", &["mp4", "mov", "m4v", "webm", "mkv", "wav", "mp3", "m4a", "aif", "aiff"])
                        .pick_file();
                    if let Some(path) = pick {
                        shared.send(Job::Load(path));
                    }
                }
                ui.label(shared.status());
            });
            ui.separator();
            egui::Grid::new("macros").num_columns(2).show(ui, |ui| {
                for (label, p) in [("GRAIN", &params.grain), ("PITCH", &params.pitch), ("SMEAR", &params.smear), ("LOOK", &params.look)] {
                    ui.label(label);
                    ui.add(widgets::ParamSlider::for_param(p, setter));
                    ui.end_row();
                }
                ui.label("key");
                ui.horizontal(|ui| {
                    ui.add(widgets::ParamSlider::for_param(&params.root, setter));
                    ui.add(widgets::ParamSlider::for_param(&params.scale, setter));
                });
                ui.end_row();
                ui.label("sync");
                ui.add(widgets::ParamSlider::for_param(&params.sync, setter));
                ui.end_row();
                ui.label("gain");
                ui.add(widgets::ParamSlider::for_param(&params.gain, setter));
                ui.end_row();
            });
            ui.separator();
            // Placeholder canvas: the GPU picture lands here with the real UI.
            let size = Vec2::new(ui.available_width(), ui.available_height().max(120.0));
            let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
            ui.painter().rect_filled(rect, 4.0, Color32::from_gray(24));
            let text = format!(
                "canvas · {} source(s) · {} grain(s) on screen",
                shared.sources.load(Ordering::Relaxed),
                shared.visible_grains.load(Ordering::Relaxed)
            );
            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, text, egui::FontId::proportional(14.0), Color32::from_gray(160));
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        });
    })
}
