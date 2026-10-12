//! Where the history window goes for one show session.
//!
//! The platform measures the pointer and the usable area of the display that holds it once, when
//! the panel is asked to show. Both windows of the pair are then laid out inside that one area
//! (see `window_pair`), so they cannot be pulled onto different displays by clamping each to
//! whatever display it happens to be on.

use super::window_pair::Rect;

/// Gap between the pointer and the history window.
const POINTER_GAP: f64 = 6.;

/// What the platform reports for one show session, in the logical pixels of one global space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SessionScreen {
    /// Pointer position when the panel was asked to show.
    pub pointer: (f64, f64),
    /// Usable area of the display that holds the pointer: work area on Windows, visible frame on
    /// macOS. Menu bar, Dock and taskbar are outside it.
    pub work: Rect,
}

/// Top-left corner of the history window of `panel` size.
///
/// `preview_reserve` is the width the preview window needs beside the history window (gap,
/// pointer gutter and body). The history window is kept where at least one side can hold it, as
/// far as the display is wide enough to do so.
pub fn panel_origin(
    screen: SessionScreen,
    cursor_anchored: bool,
    panel: (f64, f64),
    preview_reserve: f64,
) -> (f64, f64) {
    let work = screen.work;
    let (width, height) = panel;
    if !cursor_anchored {
        return (
            work.x + (work.width - width) / 2.,
            work.y + (work.height - height) / 2.,
        );
    }
    let x = anchored_axis(screen.pointer.0, work.x, work.width, width);
    let y = anchored_axis(screen.pointer.1, work.y, work.height, height);
    (fit_preview_side(x, work, width, preview_reserve), y)
}

/// Puts the panel just after the pointer, before it when there is no room, and inside the work
/// area either way (the pointer may be over a menu bar or taskbar, outside the work area).
fn anchored_axis(pointer: f64, origin: f64, extent: f64, panel: f64) -> f64 {
    let preferred = if pointer + POINTER_GAP + panel <= origin + extent {
        pointer + POINTER_GAP
    } else {
        pointer - POINTER_GAP - panel
    };
    preferred.min(origin + extent - panel).max(origin)
}

/// Moves `x` to the nearest position where the preview fits on the right or on the left of the
/// panel. Leaves it alone when the work area cannot hold both windows at all; the preview then
/// shrinks into the larger side (see `window_pair::preview_placement_for_size`).
fn fit_preview_side(x: f64, work: Rect, panel: f64, reserve: f64) -> f64 {
    let lowest = work.x;
    let highest = work.right() - panel;
    if highest - lowest < reserve {
        return x;
    }
    let right_fits_up_to = highest - reserve;
    let left_fits_from = lowest + reserve;
    if x <= right_fits_up_to || x >= left_fits_from {
        x
    } else if x - right_fits_up_to <= left_fits_from - x {
        right_fits_up_to
    } else {
        left_fits_from
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(pointer: (f64, f64), work: Rect) -> SessionScreen {
        SessionScreen { pointer, work }
    }
    const PRIMARY: Rect = Rect {
        x: 0.,
        y: 25.,
        width: 1920.,
        height: 1055.,
    };
    const PANEL: (f64, f64) = (380., 366.);
    const RESERVE: f64 = 416.;

    fn inside(origin: (f64, f64), panel: (f64, f64), work: Rect) -> bool {
        origin.0 >= work.x
            && origin.1 >= work.y
            && origin.0 + panel.0 <= work.right()
            && origin.1 + panel.1 <= work.bottom()
    }

    #[test]
    fn panel_follows_the_pointer_when_there_is_room() {
        let origin = panel_origin(screen((500., 400.), PRIMARY), true, PANEL, RESERVE);
        assert_eq!(origin, (506., 406.));
    }

    #[test]
    fn panel_flips_before_the_pointer_at_the_far_edges() {
        let origin = panel_origin(screen((1900., 1070.), PRIMARY), true, PANEL, RESERVE);
        assert_eq!(origin, (1900. - 6. - 380., 1070. - 6. - 366.));
    }

    #[test]
    fn centered_mode_centers_in_the_work_area_not_the_display() {
        let origin = panel_origin(screen((10., 10.), PRIMARY), false, PANEL, RESERVE);
        assert_eq!(origin, (770., 25. + (1055. - 366.) / 2.));
    }

    #[test]
    fn display_left_of_and_above_the_primary_has_negative_coordinates() {
        let work = Rect {
            x: -1440.,
            y: -900.,
            width: 1440.,
            height: 875.,
        };
        let origin = panel_origin(screen((-700., -500.), work), true, PANEL, RESERVE);
        assert_eq!(origin, (-694., -494.));
        assert!(inside(origin, PANEL, work));
    }

    #[test]
    fn pointer_over_the_menu_bar_or_taskbar_keeps_the_panel_in_the_work_area() {
        let above = panel_origin(screen((500., 3.), PRIMARY), true, PANEL, RESERVE);
        assert!(inside(above, PANEL, PRIMARY), "{above:?}");
        let below = panel_origin(screen((500., 1079.), PRIMARY), true, PANEL, RESERVE);
        assert!(inside(below, PANEL, PRIMARY), "{below:?}");
    }

    #[test]
    fn pointer_outside_every_edge_still_lands_inside() {
        let origin = panel_origin(screen((-50., 5000.), PRIMARY), true, PANEL, RESERVE);
        assert!(inside(origin, PANEL, PRIMARY), "{origin:?}");
    }

    #[test]
    fn panel_in_the_middle_of_a_narrow_display_moves_to_where_a_side_holds_the_preview() {
        let work = Rect {
            x: 0.,
            y: 0.,
            width: 1000.,
            height: 800.,
        };
        // 1000 wide: history at 310 leaves 310 on each side, 106 short of the preview.
        let origin = panel_origin(screen((304., 100.), work), true, PANEL, RESERVE);
        let right_room = work.right() - (origin.0 + PANEL.0);
        let left_room = origin.0 - work.x;
        assert!(right_room >= RESERVE || left_room >= RESERVE, "{origin:?}");
        assert!(inside(origin, PANEL, work));
    }

    #[test]
    fn a_display_too_narrow_for_both_windows_still_keeps_the_panel_inside() {
        let work = Rect {
            x: 2560.,
            y: 0.,
            width: 600.,
            height: 800.,
        };
        let origin = panel_origin(screen((2900., 100.), work), true, PANEL, RESERVE);
        assert!(inside(origin, PANEL, work), "{origin:?}");
    }
}
