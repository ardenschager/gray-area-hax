//! Which displays are connected — so the performance output can land on
//! the projector / second screen by itself.
//!
//! egui/eframe don't expose the monitor list, so this asks the OS
//! directly. macOS: CoreGraphics (no extra crates — the framework is
//! already linked by AppKit). Coordinates are global, in points, origin at
//! the top-left of the main display, y down — the same space winit uses
//! for window positions on macOS. Other platforms report nothing yet, and
//! the output falls back to fullscreen on the current display.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Display {
    /// Top-left in global points.
    pub origin: [f32; 2],
    /// Size in points.
    pub size: [f32; 2],
    /// The display with the menu bar (where the control window lives).
    pub main: bool,
}

/// Every active, non-mirrored display.
#[cfg(target_os = "macos")]
pub fn list() -> Vec<Display> {
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CGPoint {
        x: f64,
        y: f64,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CGSize {
        width: f64,
        height: f64,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CGRect {
        origin: CGPoint,
        size: CGSize,
    }
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGGetActiveDisplayList(max: u32, displays: *mut u32, count: *mut u32) -> i32;
        fn CGMainDisplayID() -> u32;
        fn CGDisplayBounds(display: u32) -> CGRect;
        fn CGDisplayMirrorsDisplay(display: u32) -> u32;
    }
    let mut ids = [0u32; 16];
    let mut n = 0u32;
    // SAFETY: plain CoreGraphics queries into a correctly sized buffer.
    unsafe {
        if CGGetActiveDisplayList(ids.len() as u32, ids.as_mut_ptr(), &mut n) != 0 {
            return Vec::new();
        }
        let main = CGMainDisplayID();
        ids[..(n as usize).min(ids.len())]
            .iter()
            .filter(|&&id| CGDisplayMirrorsDisplay(id) == 0)
            .map(|&id| {
                let b = CGDisplayBounds(id);
                Display {
                    origin: [b.origin.x as f32, b.origin.y as f32],
                    size: [b.size.width as f32, b.size.height as f32],
                    main: id == main,
                }
            })
            .collect()
    }
}

#[cfg(not(target_os = "macos"))]
pub fn list() -> Vec<Display> {
    Vec::new()
}

/// Where the performance output should go: the largest display that is
/// not the main one (projector / second screen), if any.
pub fn pick_output(displays: &[Display]) -> Option<Display> {
    displays
        .iter()
        .filter(|d| !d.main)
        .max_by(|a, b| (a.size[0] * a.size[1]).total_cmp(&(b.size[0] * b.size[1])))
        .copied()
}

/// Largest rect of `aspect` (w/h) centered inside `outer` (letterbox /
/// pillarbox): the picture is never cropped or stretched.
pub fn fit(outer: eframe::egui::Rect, aspect: f32) -> eframe::egui::Rect {
    let aspect = aspect.max(1e-3);
    let (w, h) = (outer.width(), outer.height());
    let size = if w / h.max(1e-3) > aspect { eframe::egui::vec2(h * aspect, h) } else { eframe::egui::vec2(w, w / aspect) };
    eframe::egui::Rect::from_center_size(outer.center(), size)
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{pos2, vec2, Rect};

    #[test]
    fn output_goes_to_the_biggest_non_main_display() {
        let laptop = Display { origin: [0.0, 0.0], size: [1512.0, 982.0], main: true };
        let small = Display { origin: [-800.0, 0.0], size: [800.0, 600.0], main: false };
        let projector = Display { origin: [1512.0, -200.0], size: [1920.0, 1080.0], main: false };
        assert_eq!(pick_output(&[laptop]), None, "single display: no external target");
        assert_eq!(pick_output(&[laptop, small, projector]), Some(projector));
    }

    #[test]
    fn fit_letterboxes_without_cropping() {
        let screen = Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 1000.0));
        let r = fit(screen, 16.0 / 9.0);
        assert!((r.width() - 1000.0).abs() < 1e-3 && (r.height() - 562.5).abs() < 1e-3);
        assert!((r.center() - screen.center()).length() < 1e-3);
        let wide = Rect::from_min_size(pos2(10.0, 0.0), vec2(3000.0, 1000.0));
        let r = fit(wide, 16.0 / 9.0);
        assert!((r.height() - 1000.0).abs() < 1e-3 && r.min.x > 10.0, "pillarbox");
    }

    #[test]
    fn display_list_is_sane() {
        // Whatever is attached (CI may report none), at most one is main
        // and every size is positive.
        let ds = list();
        assert!(ds.iter().filter(|d| d.main).count() <= 1);
        assert!(ds.iter().all(|d| d.size[0] > 0.0 && d.size[1] > 0.0));
    }
}
