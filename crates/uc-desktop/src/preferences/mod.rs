//! Local window preferences, independent of daemon settings and GUI frameworks.
//!
//! Only geometry and display hints are stored. Plaintext storage of these desktop
//! preferences was explicitly approved for issue #1736; no user content belongs here.

mod storage;
pub use storage::PreferencesStore;

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Size {
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

/// Physical work area plus a best-effort name hint, never a stable display ID.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Monitor {
    pub name: Option<String>,
    pub origin: Point,
    pub size: Size,
    pub scale_factor: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Placement {
    pub monitor: Monitor,
    /// Logical offset of the outer frame from the monitor work area's origin.
    pub offset: Point,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// Normal client size is logical; placement refers to the outer frame.
pub struct NormalGeometry {
    pub inner_size: Size,
    /// Absent when absolute placement is unavailable (notably Wayland).
    pub placement: Option<Placement>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WindowPreferences {
    pub version: u32,
    pub normal: NormalGeometry,
    pub maximized: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Document {
    pub schema_version: u32,
    pub windows: BTreeMap<String, WindowPreferences>,
}

impl Default for Document {
    fn default() -> Self {
        Self {
            schema_version: 1,
            windows: BTreeMap::new(),
        }
    }
}

fn positive(value: f64) -> bool {
    value.is_finite() && value > 0.0 && value <= 100_000.0
}
fn valid_size(size: Size) -> bool {
    positive(size.width) && positive(size.height)
}
fn valid_point(point: Point) -> bool {
    point.x.is_finite()
        && point.y.is_finite()
        && point.x.abs() <= 1_000_000.0
        && point.y.abs() <= 1_000_000.0
}
impl Monitor {
    pub fn valid(&self) -> bool {
        valid_size(self.size)
            && valid_point(self.origin)
            && self.scale_factor.is_finite()
            && (0.1..=16.0).contains(&self.scale_factor)
            && self.name.as_ref().is_none_or(|name| name.len() <= 256)
    }
}
impl WindowPreferences {
    pub fn valid(&self) -> bool {
        self.version == 1
            && valid_size(self.normal.inner_size)
            && self
                .normal
                .placement
                .as_ref()
                .is_none_or(|p| p.monitor.valid() && valid_point(p.offset))
    }
}
impl Document {
    pub fn valid(&self) -> bool {
        self.schema_version == 1
            && self.windows.len() <= 32
            && self
                .windows
                .iter()
                .all(|(key, value)| !key.is_empty() && key.len() <= 128 && value.valid())
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Constraints {
    pub minimum: Size,
    pub maximum: Size,
}

#[derive(Debug, PartialEq)]
pub struct Restoration {
    pub scale_factor: Option<f64>,
    pub inner_size: Size,
    pub minimum: Size,
    pub outer_position: Option<Point>,
    pub maximized: bool,
}

/// Project preferred geometry onto today's displays without changing the preference.
/// `fallback` is the current/primary monitor index supplied by the shell.
pub fn restore(
    preferred: &WindowPreferences,
    monitors: &[Monitor],
    fallback: usize,
    constraints: Constraints,
    frame: Size,
    can_position: bool,
) -> Restoration {
    let placement = preferred.normal.placement.as_ref().filter(|_| can_position);
    let matched = placement.and_then(|p| {
        monitors.iter().position(|m| m == &p.monitor).or_else(|| {
            let mut names = monitors
                .iter()
                .enumerate()
                .filter(|(_, m)| m.name.is_some() && m.name == p.monitor.name);
            let first = names.next();
            // Duplicate names are not reliable identities.
            first
                .filter(|_| names.next().is_none())
                .map(|(index, _)| index)
        })
    });
    let monitor = monitors
        .get(matched.unwrap_or(fallback))
        .filter(|m| m.valid());
    let available = monitor
        .map(|m| Size {
            width: (m.size.width / m.scale_factor - frame.width).max(1.0),
            height: (m.size.height / m.scale_factor - frame.height).max(1.0),
        })
        .unwrap_or(Size {
            width: f64::INFINITY,
            height: f64::INFINITY,
        });
    let minimum = Size {
        width: constraints.minimum.width.min(available.width),
        height: constraints.minimum.height.min(available.height),
    };
    let inner_size = Size {
        width: preferred
            .normal
            .inner_size
            .width
            .max(minimum.width)
            .min(constraints.maximum.width)
            .min(available.width),
        height: preferred
            .normal
            .inner_size
            .height
            .max(minimum.height)
            .min(constraints.maximum.height)
            .min(available.height),
    };
    let outer_position = monitor.filter(|_| can_position).map(|m| {
        let room = Size {
            width: (m.size.width - (inner_size.width + frame.width) * m.scale_factor).max(0.0),
            height: (m.size.height - (inner_size.height + frame.height) * m.scale_factor).max(0.0),
        };
        let offset = placement
            .filter(|_| matched.is_some())
            .map(|p| Point {
                x: (p.offset.x * m.scale_factor).clamp(0.0, room.width),
                y: (p.offset.y * m.scale_factor).clamp(0.0, room.height),
            })
            .unwrap_or(Point {
                x: room.width / 2.0,
                y: room.height / 2.0,
            });
        Point {
            x: m.origin.x + offset.x,
            y: m.origin.y + offset.y,
        }
    });
    Restoration {
        scale_factor: monitor.map(|m| m.scale_factor),
        inner_size,
        minimum,
        outer_position,
        maximized: preferred.maximized,
    }
}

/// A stable native observation. Context changes are system operations, not intent.
#[derive(Clone, Debug, PartialEq)]
pub struct Observation {
    pub normal: NormalGeometry,
    pub maximized: bool,
    pub transient: bool,
    pub monitors: Vec<Monitor>,
    pub scale_factor: f64,
    pub frame: Size,
}

/// Keeps preferred layout separate from temporary, display-constrained geometry.
/// Event origin is best effort: later stable changes outside known operations are
/// accepted because portable window APIs do not identify user-originated events.
pub struct WindowTracker {
    preferred: Option<WindowPreferences>,
    effective: Option<Observation>,
}
impl WindowTracker {
    pub fn new(preferred: Option<WindowPreferences>) -> Self {
        Self {
            preferred,
            effective: None,
        }
    }
    pub fn preferred(&self) -> Option<&WindowPreferences> {
        self.preferred.as_ref()
    }

    pub fn observe(
        &mut self,
        next: Observation,
        known_operation: bool,
    ) -> Option<WindowPreferences> {
        let candidate = WindowPreferences {
            version: 1,
            normal: next.normal.clone(),
            maximized: next.maximized,
        };
        if next.transient
            || !candidate.valid()
            || !next.scale_factor.is_finite()
            || next.scale_factor <= 0.0
        {
            return None;
        }
        let context_changed = self.effective.as_ref().is_some_and(|old| {
            old.monitors != next.monitors
                || old.scale_factor != next.scale_factor
                || (old.maximized == next.maximized && old.frame != next.frame)
        });
        let previous = self.effective.replace(next.clone());
        if self.preferred.is_none() {
            if !next.maximized {
                self.preferred = Some(WindowPreferences {
                    version: 1,
                    normal: next.normal.clone(),
                    maximized: false,
                });
            }
            return None;
        }
        if known_operation || context_changed {
            return None;
        }
        let previous = previous?;
        let preferred = self.preferred.as_mut()?;
        let before = preferred.clone();
        if next.maximized != previous.maximized {
            // Maximizing/unmaximizing must never adopt a clamped normal rectangle.
            preferred.maximized = next.maximized;
        } else if !next.maximized
            && geometry_changed(&previous.normal, &next.normal, next.scale_factor)
        {
            let placement = next
                .normal
                .placement
                .clone()
                .or_else(|| preferred.normal.placement.clone());
            preferred.normal = NormalGeometry {
                inner_size: next.normal.inner_size,
                placement,
            };
        }
        (preferred != &before && preferred.valid()).then(|| preferred.clone())
    }
}
fn geometry_changed(a: &NormalGeometry, b: &NormalGeometry, scale: f64) -> bool {
    let tolerance = 0.5 / scale.max(0.1);
    let changed = |a: f64, b: f64| (a - b).abs() > tolerance;
    changed(a.inner_size.width, b.inner_size.width)
        || changed(a.inner_size.height, b.inner_size.height)
        || match (&a.placement, &b.placement) {
            (Some(a), Some(b)) => {
                a.monitor != b.monitor
                    || changed(a.offset.x, b.offset.x)
                    || changed(a.offset.y, b.offset.y)
            }
            _ => false,
        }
}

#[cfg(test)]
mod tests;
