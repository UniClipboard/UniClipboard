pub const PANEL_WIDTH: f64 = 360.;
pub const PANEL_HEIGHT: f64 = 420.;
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
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PreviewPlacement {
    pub frame: Rect,
    pub side: PreviewSide,
    pub pointer_y: f64,
}

pub fn preview_placement(anchor: PreviewAnchor, measured_height: f64) -> PreviewPlacement {
    let scale = anchor.scale;
    let inset = (SCREEN_INSET * scale).min(anchor.screen.height / 4.);
    let available_height = (anchor.screen.height - 2. * inset).max(1.);
    let maximum = (MAX_PREVIEW_HEIGHT * scale).min(available_height);
    let minimum = (MIN_PREVIEW_HEIGHT * scale).min(maximum);
    let height = measured_height.clamp(minimum, maximum);
    let width = ((PANEL_WIDTH + POINTER_DEPTH) * scale).min(anchor.screen.width);
    let right = anchor.history.right() + WINDOW_GAP * scale;
    let (side, x) = if right + width <= anchor.screen.right() {
        (PreviewSide::Right, right)
    } else {
        (
            PreviewSide::Left,
            (anchor.history.x - WINDOW_GAP * scale - width).max(anchor.screen.x),
        )
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
                width: 360.,
                height: 420.,
            },
            item: Rect {
                x: 406.,
                y,
                width: 348.,
                height: 32.,
            },
            screen: Rect {
                x: 0.,
                y: 24.,
                width: 1920.,
                height: 1030.,
            },
            scale: 1.,
        }
    }
    #[test]
    fn short_preview_tracks_the_item_instead_of_the_history_top() {
        let a = anchor(300.);
        let p = preview_placement(a, 140.);
        assert_eq!(
            p.frame,
            Rect {
                x: 768.,
                y: 246.,
                width: 368.,
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
        a.history.width = 450.;
        a.screen.x = -1920.;
        a.scale = 1.25;
        let p = preview_placement(a, 200.);
        assert_eq!(p.frame.x, -1340.);
        assert_eq!(p.frame.width, 460.);
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
}
