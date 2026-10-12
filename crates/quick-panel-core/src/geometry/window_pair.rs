/// History window: 380 wide; a 48 search row, nine 30 rows with 6 padding (282), a 34 footer and
/// the 1 px border on both sides.
pub const PANEL_WIDTH: f64 = 380.;
pub const PANEL_HEIGHT: f64 = 366.;
/// Preview body width, not counting the pointer.
pub const PREVIEW_WIDTH: f64 = 400.;
/// Body width of the action list, which the preview window shows in place of a preview.
pub const ACTIONS_WIDTH: f64 = 380.;
pub const WINDOW_GAP: f64 = 8.;
pub const POINTER_DEPTH: f64 = 8.;
pub const POINTER_HALF_HEIGHT: f64 = 7.;
pub const MIN_PREVIEW_HEIGHT: f64 = 96.;
pub const MAX_PREVIEW_HEIGHT: f64 = 480.;
const SCREEN_INSET: f64 = 8.;
const POINTER_INSET: f64 = 20.;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
impl Rect {
    pub fn right(self) -> f64 {
        self.x + self.width
    }
    pub fn bottom(self) -> f64 {
        self.y + self.height
    }
    pub fn center_y(self) -> f64 {
        self.y + self.height / 2.
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PreviewSide {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PreviewAnchor {
    pub history: Rect,
    pub item: Rect,
    pub screen: Rect,
    pub scale: f64,
    /// Unscaled pointer gutter; zero for a rectangular preview window.
    pub pointer_depth: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PreviewPlacement {
    pub frame: Rect,
    pub side: PreviewSide,
    pub pointer_y: f64,
}

impl PreviewAnchor {
    /// The anchor of a show session, in the global space of `screen`. The history window is where
    /// the panel put it (`origin`, `panel` size), and `item` is relative to that window. Reading
    /// the frame back from the toolkit would not do: GPUI on macOS reports a window frame
    /// relative to its own display, not in the global space the work area is in.
    pub fn for_session(
        origin: (f64, f64),
        panel: (f64, f64),
        item: Rect,
        screen: Rect,
        scale: f64,
        pointer_depth: f64,
    ) -> Self {
        Self {
            history: Rect {
                x: origin.0,
                y: origin.1,
                width: panel.0,
                height: panel.1,
            },
            item: Rect {
                x: origin.0 + item.x,
                y: origin.1 + item.y,
                ..item
            },
            screen,
            scale,
            pointer_depth,
        }
    }
}

pub fn preview_placement(anchor: PreviewAnchor, measured_height: f64) -> PreviewPlacement {
    preview_placement_with_width(anchor, PREVIEW_WIDTH, measured_height)
}

/// Placement for a body of `body_width` (unscaled), whose content is `measured_height` tall.
pub fn preview_placement_with_width(
    anchor: PreviewAnchor,
    body_width: f64,
    measured_height: f64,
) -> PreviewPlacement {
    let scale = anchor.scale;
    let inset = (SCREEN_INSET * scale).min(anchor.screen.height / 4.);
    let available_height = (anchor.screen.height - 2. * inset).max(1.);
    let maximum = (MAX_PREVIEW_HEIGHT * scale).min(available_height);
    let minimum = (MIN_PREVIEW_HEIGHT * scale).min(maximum);
    let height = measured_height.clamp(minimum, maximum);
    preview_placement_for_size(anchor, body_width * scale, height)
}

pub fn preview_capacity(anchor: PreviewAnchor) -> (f64, f64) {
    let clearance = (WINDOW_GAP + anchor.pointer_depth) * anchor.scale;
    let left = anchor.history.x - anchor.screen.x - clearance;
    let right = anchor.screen.right() - anchor.history.right() - clearance;
    let inset = (SCREEN_INSET * anchor.scale).min(anchor.screen.height / 4.);
    (
        left.max(right).max(1.),
        (anchor.screen.height - 2. * inset).max(1.),
    )
}

pub fn preview_placement_for_size(
    anchor: PreviewAnchor,
    body_width: f64,
    body_height: f64,
) -> PreviewPlacement {
    let scale = anchor.scale;
    let inset = (SCREEN_INSET * scale).min(anchor.screen.height / 4.);
    let height = body_height
        .max(1.)
        .min((anchor.screen.height - 2. * inset).max(1.));
    let wanted = (body_width + anchor.pointer_depth * scale).min(anchor.screen.width);
    let gap = WINDOW_GAP * scale;
    let right_room = anchor.screen.right() - anchor.history.right() - gap;
    let left_room = anchor.history.x - gap - anchor.screen.x;
    // Beside the history window on the side that holds the preview. When neither does, the preview
    // shrinks into the larger side, which keeps it on the display and clear of history for as long
    // as that side still has room for a body behind the pointer gutter.
    let larger_room = right_room.max(left_room);
    let (side, x, width) = if wanted <= right_room {
        (PreviewSide::Right, anchor.history.right() + gap, wanted)
    } else if wanted <= left_room {
        (PreviewSide::Left, anchor.history.x - gap - wanted, wanted)
    } else if larger_room > anchor.pointer_depth * scale {
        if right_room >= left_room {
            (PreviewSide::Right, anchor.history.right() + gap, right_room)
        } else {
            (
                PreviewSide::Left,
                anchor.history.x - gap - left_room,
                left_room,
            )
        }
    } else {
        // The display cannot hold the history window (fixed size) and a preview body side by
        // side. Staying on the display wins over staying clear of history: the preview covers part
        // of it, flush with the display edge on the side with more room.
        let side = if right_room >= left_room {
            PreviewSide::Right
        } else {
            PreviewSide::Left
        };
        let x = match side {
            PreviewSide::Right => anchor.screen.right() - wanted,
            PreviewSide::Left => anchor.screen.x,
        };
        (side, x.max(anchor.screen.x), wanted)
    };
    let y = (anchor.item.center_y() - height / 2.).clamp(
        anchor.screen.y + inset,
        anchor.screen.bottom() - inset - height,
    );
    let pointer_inset = (POINTER_INSET * scale).min(height / 2.);
    let pointer_y = (anchor.item.center_y() - y).clamp(pointer_inset, height - pointer_inset);
    PreviewPlacement {
        frame: Rect {
            x,
            y,
            width,
            height,
        },
        side,
        pointer_y,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn anchor(y: f64) -> PreviewAnchor {
        PreviewAnchor {
            history: Rect {
                x: 400.,
                y: 200.,
                width: 380.,
                height: 366.,
            },
            item: Rect {
                x: 406.,
                y,
                width: 368.,
                height: 30.,
            },
            screen: Rect {
                x: 0.,
                y: 24.,
                width: 1920.,
                height: 1030.,
            },
            scale: 1.,
            pointer_depth: POINTER_DEPTH,
        }
    }
    #[test]
    fn short_preview_tracks_the_item_instead_of_the_history_top() {
        let a = anchor(300.);
        let p = preview_placement(a, 140.);
        assert_eq!(
            p.frame,
            Rect {
                x: 788.,
                y: 245.,
                width: 408.,
                height: 140.
            }
        );
        assert_eq!(p.frame.y + p.pointer_y, a.item.center_y());
    }
    #[test]
    fn long_content_is_capped_without_moving_history() {
        let a = anchor(500.);
        let p = preview_placement(a, 4000.);
        assert_eq!(p.frame.height, MAX_PREVIEW_HEIGHT);
        assert_eq!(p.frame.y + p.pointer_y, a.item.center_y());
        assert_eq!(a.history.x, 400.);
    }
    #[test]
    fn bottom_edge_moves_the_window_but_keeps_the_pointer_on_the_row() {
        let a = anchor(980.);
        let p = preview_placement(a, 300.);
        assert_eq!(p.frame.bottom(), a.screen.bottom() - SCREEN_INSET);
        assert_eq!(p.frame.y + p.pointer_y, a.item.center_y());
    }
    #[test]
    fn top_edge_moves_the_window_but_keeps_the_pointer_on_the_row() {
        let a = anchor(64.);
        let p = preview_placement(a, 400.);
        assert_eq!(p.frame.y, a.screen.y + SCREEN_INSET);
        assert_eq!(p.frame.y + p.pointer_y, a.item.center_y());
    }
    #[test]
    fn right_edge_flips_the_pointer_toward_history() {
        let mut a = anchor(300.);
        a.history.x = 1500.;
        let p = preview_placement(a, 140.);
        assert_eq!(p.side, PreviewSide::Left);
        assert_eq!(p.frame.right(), 1492.);
    }
    #[test]
    fn negative_coordinates_and_ui_scale_are_supported() {
        let mut a = anchor(400.);
        a.history.x = -1800.;
        a.history.width = 475.;
        a.screen.x = -1920.;
        a.scale = 1.25;
        let p = preview_placement(a, 200.);
        assert_eq!(p.frame.x, -1315.);
        assert_eq!(p.frame.width, 510.);
        assert_eq!(p.frame.y + p.pointer_y, a.item.center_y());
    }
    #[test]
    fn small_screen_caps_the_height_and_keeps_the_frame_inside() {
        let mut a = anchor(100.);
        a.screen.height = 180.;
        let p = preview_placement(a, 4000.);
        assert!(p.frame.y >= a.screen.y);
        assert!(p.frame.bottom() <= a.screen.bottom());
        assert_eq!(p.frame.height, 164.);
    }
    #[test]
    fn empty_content_still_has_room_for_the_pointer_and_metadata() {
        let p = preview_placement(anchor(300.), 0.);
        assert_eq!(p.frame.height, MIN_PREVIEW_HEIGHT);
    }
    #[test]
    fn the_action_list_is_as_wide_as_the_history_window() {
        let a = anchor(300.);
        let preview = preview_placement(a, 200.);
        let actions = preview_placement_with_width(a, ACTIONS_WIDTH, 200.);
        assert_eq!(preview.frame.width, PREVIEW_WIDTH + POINTER_DEPTH);
        assert_eq!(actions.frame.width, ACTIONS_WIDTH + POINTER_DEPTH);
        assert_eq!(actions.frame.height, preview.frame.height);
    }

    #[test]
    fn rectangular_preview_has_no_pointer_gutter_at_any_scale() {
        for scale in [0.8, 1., 1.5] {
            let mut a = anchor(300.);
            a.scale = scale;
            a.pointer_depth = 0.;
            let p = preview_placement(a, 140.);
            assert_eq!(p.frame.width, PREVIEW_WIDTH * scale);
            assert_eq!(p.frame.x, a.history.right() + WINDOW_GAP * scale);
            a.history.x = 1500.;
            let p = preview_placement(a, 140.);
            assert_eq!(p.side, PreviewSide::Left);
            assert_eq!(p.frame.right(), a.history.x - WINDOW_GAP * scale);
        }
    }

    #[test]
    fn rectangular_image_capacity_does_not_reserve_an_unused_pointer() {
        let mut a = anchor(300.);
        let shaped = preview_capacity(a);
        a.pointer_depth = 0.;
        let rectangular = preview_capacity(a);
        assert_eq!(rectangular.0, shaped.0 + POINTER_DEPTH * a.scale);
        assert_eq!(rectangular.1, shaped.1);
        let p = preview_placement_for_size(a, rectangular.0, rectangular.1);
        assert_eq!(p.frame.right(), a.screen.right());
    }

    #[test]
    fn preview_never_leaves_the_session_work_area_on_a_display_with_an_offset_origin() {
        // A display stacked above the primary one: its work area has negative y.
        let mut a = anchor(-500.);
        a.screen = Rect {
            x: 2560.,
            y: -1000.,
            width: 1920.,
            height: 975.,
        };
        a.history.x = 3000.;
        a.history.y = -700.;
        let p = preview_placement(a, 300.);
        assert!(p.frame.x >= a.screen.x && p.frame.right() <= a.screen.right());
        assert!(p.frame.y >= a.screen.y && p.frame.bottom() <= a.screen.bottom());
    }

    #[test]
    fn preview_shrinks_into_the_larger_side_instead_of_overlapping_history() {
        let mut a = anchor(300.);
        a.screen.width = 1000.;
        a.history.x = 310.;
        let p = preview_placement(a, 140.);
        assert!(p.frame.x >= a.screen.x && p.frame.right() <= a.screen.right());
        let clear_of_history = p.frame.right() <= a.history.x || p.frame.x >= a.history.right();
        assert!(clear_of_history, "{:?} overlaps {:?}", p.frame, a.history);
    }

    #[test]
    fn flip_left_on_a_display_whose_origin_is_not_zero_stays_on_that_display() {
        let mut a = anchor(300.);
        a.screen.x = 1920.;
        a.screen.width = 1920.;
        a.history.x = 1920. + 1500.;
        let p = preview_placement(a, 140.);
        assert_eq!(p.side, PreviewSide::Left);
        assert!(p.frame.x >= a.screen.x);
        assert_eq!(p.frame.right(), a.history.x - WINDOW_GAP);
    }

    /// Failure modes of a work area too narrow for both windows. The history window is fixed at
    /// `PANEL_WIDTH * scale` (it is not resizable), so only the preview can give way.
    fn narrow(width: f64, history_x: f64, scale: f64, depth: f64) -> PreviewAnchor {
        let mut a = anchor(300.);
        a.scale = scale;
        a.pointer_depth = depth;
        a.screen.x = 100.;
        a.screen.width = width;
        a.history.x = history_x;
        a.history.width = PANEL_WIDTH * scale;
        a
    }

    fn assert_grouped(a: PreviewAnchor, p: PreviewPlacement) {
        assert!(
            p.frame.x >= a.screen.x && p.frame.right() <= a.screen.right(),
            "{:?} leaves {:?}",
            p.frame,
            a.screen
        );
        assert!(
            p.frame.right() <= a.history.x || p.frame.x >= a.history.right(),
            "{:?} overlaps {:?}",
            p.frame,
            a.history
        );
    }

    #[test]
    fn room_for_a_narrow_body_keeps_the_preview_inside_and_clear_of_history() {
        // 380 history + 8 gap leaves 12 on the right: 4 for the body behind the 8 pointer gutter.
        let a = narrow(400., 100., 1., POINTER_DEPTH);
        let p = preview_placement(a, 140.);
        assert_grouped(a, p);
        assert_eq!(p.frame.width, 12.);
    }

    #[test]
    fn no_room_for_any_body_stays_on_the_display_instead_of_a_one_pixel_sliver() {
        for (width, scale, depth) in [
            (385., 1., POINTER_DEPTH),
            (380., 1., POINTER_DEPTH),
            (300., 1., POINTER_DEPTH),
            (300., 0.8, 0.),
            (388., 1.25, 0.),
        ] {
            let a = narrow(width, 100., scale, depth);
            for item_y in [64., 300., 980.] {
                let mut a = a;
                a.item.y = item_y;
                let p = preview_placement(a, 140.);
                assert!(
                    p.frame.x >= a.screen.x && p.frame.right() <= a.screen.right(),
                    "{width} {scale} {depth}: {:?} leaves {:?}",
                    p.frame,
                    a.screen
                );
                assert!(p.frame.width > 1., "{:?}", p.frame);
            }
        }
    }

    #[test]
    fn rectangular_preview_in_a_narrow_area_is_grouped_down_to_its_last_pixel() {
        let a = narrow(390., 100., 1., 0.);
        let p = preview_placement(a, 140.);
        assert_grouped(a, p);
        assert_eq!(p.frame.width, 2.);
    }

    #[test]
    fn image_capacity_never_promises_more_than_the_larger_side_has() {
        let a = narrow(500., 150., 1., POINTER_DEPTH);
        let (width, _) = preview_capacity(a);
        let p = preview_placement_for_size(a, width, 200.);
        assert_grouped(a, p);
    }

    #[test]
    fn a_session_on_a_display_with_an_offset_origin_keeps_both_windows_there() {
        let screen = Rect {
            x: 2560.,
            y: -300.,
            width: 1920.,
            height: 1055.,
        };
        let item = Rect {
            x: 6.,
            y: 100.,
            width: 368.,
            height: 30.,
        };
        let a = PreviewAnchor::for_session((3000., -100.), (380., 366.), item, screen, 1., 8.);
        assert_eq!(
            a.history,
            Rect {
                x: 3000.,
                y: -100.,
                width: 380.,
                height: 366.
            }
        );
        assert_eq!(
            a.item,
            Rect {
                x: 3006.,
                y: 0.,
                width: 368.,
                height: 30.
            }
        );
        let p = preview_placement(a, 140.);
        assert!(p.frame.x >= screen.x && p.frame.right() <= screen.right());
        assert!(p.frame.y >= screen.y && p.frame.bottom() <= screen.bottom());
        assert_eq!(p.frame.x, a.history.right() + WINDOW_GAP);
    }
}
