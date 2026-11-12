//! chromagrain-ui: the play-first studio UI and the GPU canvas.
//!
//! * [`studio::Studio`] — the three views + Deep drawer, drawn with egui
//!   against a [`host::Host`] (the standalone app or the plugin).
//! * [`gpu`] — the wgpu canvas pipeline (render once per frame offscreen,
//!   present in any number of egui viewports via paint callbacks).
//! * [`surface`] — egui + wgpu on a raw window / `CAMetalLayer` (or an
//!   offscreen texture): what a plugin editor, an AUv3 view or a test uses
//!   when there is no eframe.

pub mod atlas;
pub mod envelope;
pub mod gpu;
pub mod host;
pub mod studio;
pub mod surface;
pub mod theme;
pub mod wave;
pub mod widgets;

pub use host::{AddRoute, AddTarget, Added, BrowseItem, Caps, Host, Pad, PadKind, Pending, PerformHost};
pub use studio::{DeepTab, DeepTarget, Studio, View};

/// Largest rect of `aspect` (w/h) centered inside `outer` (letterbox /
/// pillarbox): the picture is never cropped or stretched.
pub fn fit(outer: egui::Rect, aspect: f32) -> egui::Rect {
    let aspect = aspect.max(1e-3);
    let (w, h) = (outer.width(), outer.height());
    let size = if w / h.max(1e-3) > aspect { egui::vec2(h * aspect, h) } else { egui::vec2(w, w / aspect) };
    egui::Rect::from_center_size(outer.center(), size)
}
