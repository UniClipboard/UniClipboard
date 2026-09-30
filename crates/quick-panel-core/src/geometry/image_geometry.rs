#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Size {
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Offset {
    pub x: f64,
    pub y: f64,
}

pub fn image_body_size(pixels: Size, display_scale: f64, available: Size, ui_scale: f64) -> Size {
    let maximum = Size {
        width: available.width.min(640. * ui_scale).max(1.),
        height: available.height.min(560. * ui_scale).max(1.),
    };
    let native = Size {
        width: pixels.width / display_scale,
        height: pixels.height / display_scale,
    };
    let factor = (maximum.width / native.width.max(1.))
        .min(maximum.height / native.height.max(1.))
        .min(1.);
    Size {
        width: (native.width * factor).max((120. * ui_scale).min(maximum.width)),
        height: (native.height * factor).max((96. * ui_scale).min(maximum.height)),
    }
}

pub struct ImageViewport {
    pub native: Size,
    pub viewport: Size,
    pub actual_size: bool,
    pub pan: Offset,
}

impl ImageViewport {
    pub fn new(native: Size, viewport: Size) -> Self {
        Self {
            native,
            viewport,
            actual_size: false,
            pan: Offset::default(),
        }
    }
    pub fn displayed_size(&self) -> Size {
        let factor = if self.actual_size {
            1.
        } else {
            (self.viewport.width / self.native.width.max(1.))
                .min(self.viewport.height / self.native.height.max(1.))
                .min(1.)
        };
        Size {
            width: self.native.width * factor,
            height: self.native.height * factor,
        }
    }
    pub fn origin(&self) -> Offset {
        let size = self.displayed_size();
        Offset {
            x: (self.viewport.width - size.width) / 2. + self.pan.x,
            y: (self.viewport.height - size.height) / 2. + self.pan.y,
        }
    }
    pub fn toggle_at(&mut self, point: Offset) {
        if self.actual_size {
            self.actual_size = false;
            self.pan = Offset::default();
            return;
        }
        let size = self.displayed_size();
        let origin = self.origin();
        if size.width <= 0. || size.height <= 0. {
            return;
        }
        let u = ((point.x - origin.x) / size.width).clamp(0., 1.);
        let v = ((point.y - origin.y) / size.height).clamp(0., 1.);
        self.actual_size = true;
        self.pan_to(Offset {
            x: point.x - self.viewport.width / 2. - (u - 0.5) * self.native.width,
            y: point.y - self.viewport.height / 2. - (v - 0.5) * self.native.height,
        });
    }
    pub fn pan_to(&mut self, offset: Offset) {
        if !self.actual_size {
            self.pan = Offset::default();
            return;
        }
        let x = ((self.native.width - self.viewport.width) / 2.).max(0.);
        let y = ((self.native.height - self.viewport.height) / 2.).max(0.);
        self.pan = Offset {
            x: offset.x.clamp(-x, x),
            y: offset.y.clamp(-y, y),
        };
    }
    pub fn resize(&mut self, viewport: Size) {
        self.viewport = viewport;
        self.pan_to(self.pan);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn approx(a: f64, b: f64) {
        assert!((a - b).abs() < 0.001, "{a} != {b}");
    }
    #[test]
    fn landscape_fits_without_cropping() {
        let size = image_body_size(
            Size {
                width: 2560.,
                height: 1440.,
            },
            2.,
            Size {
                width: 800.,
                height: 800.,
            },
            1.,
        );
        assert_eq!(
            size,
            Size {
                width: 640.,
                height: 360.
            }
        );
    }
    #[test]
    fn portrait_uses_height_and_keeps_its_aspect_ratio() {
        let size = image_body_size(
            Size {
                width: 800.,
                height: 2400.,
            },
            2.,
            Size {
                width: 800.,
                height: 800.,
            },
            1.,
        );
        approx(size.width, 560. / 3.);
        approx(size.height, 560.);
    }
    #[test]
    fn small_image_is_not_upscaled_even_in_a_minimum_size_surface() {
        let body = image_body_size(
            Size {
                width: 64.,
                height: 64.,
            },
            2.,
            Size {
                width: 800.,
                height: 800.,
            },
            1.,
        );
        assert_eq!(
            body,
            Size {
                width: 120.,
                height: 96.
            }
        );
        let view = ImageViewport::new(
            Size {
                width: 32.,
                height: 32.,
            },
            body,
        );
        assert_eq!(
            view.displayed_size(),
            Size {
                width: 32.,
                height: 32.
            }
        );
    }
    #[test]
    fn constrained_screen_limits_the_surface() {
        let size = image_body_size(
            Size {
                width: 2400.,
                height: 1600.,
            },
            2.,
            Size {
                width: 220.,
                height: 180.,
            },
            1.5,
        );
        approx(size.width, 220.);
        approx(size.height, 220. * 2. / 3.);
    }
    #[test]
    fn zoom_preserves_the_point_under_the_pointer_and_toggle_resets() {
        let mut view = ImageViewport::new(
            Size {
                width: 1200.,
                height: 800.,
            },
            Size {
                width: 600.,
                height: 400.,
            },
        );
        view.toggle_at(Offset { x: 450., y: 200. });
        assert!(view.actual_size);
        assert_eq!(view.pan, Offset { x: -150., y: 0. });
        assert_eq!(view.origin(), Offset { x: -450., y: -200. });
        view.toggle_at(Offset { x: 450., y: 200. });
        assert!(!view.actual_size);
        assert_eq!(view.pan, Offset::default());
        assert_eq!(
            view.displayed_size(),
            Size {
                width: 600.,
                height: 400.
            }
        );
    }
    #[test]
    fn dragging_and_resize_cannot_reveal_empty_space_beyond_image_edges() {
        let mut view = ImageViewport::new(
            Size {
                width: 1200.,
                height: 800.,
            },
            Size {
                width: 600.,
                height: 400.,
            },
        );
        view.toggle_at(Offset { x: 300., y: 200. });
        view.pan_to(Offset {
            x: 9999.,
            y: -9999.,
        });
        assert_eq!(view.pan, Offset { x: 300., y: -200. });
        view.resize(Size {
            width: 1000.,
            height: 700.,
        });
        assert_eq!(view.pan, Offset { x: 100., y: -50. });
    }
}
