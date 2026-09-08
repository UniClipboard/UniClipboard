//! One-shot, framework-independent hardware assessment for desktop visual effects.

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VisualCapability {
    Capable,
    Constrained,
    Unknown,
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GraphicsCapability {
    ModernHardware,
    LegacyHardware,
    Unavailable,
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
#[derive(Clone, Copy, Debug)]
struct Hardware {
    physical_cores: Option<usize>,
    memory_bytes: u64,
    graphics: GraphicsCapability,
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
fn classify(hardware: Hardware) -> VisualCapability {
    // Resource guardrails, not a frame-rate guarantee. Actual interaction samples
    // can subsequently lower the next-start choice, independently of these inputs.
    const MIN_MEMORY: u64 = 8 * 1024 * 1024 * 1024;
    const MIN_PHYSICAL_CORES: usize = 4;
    if hardware
        .physical_cores
        .is_some_and(|cores| cores > 0 && cores < MIN_PHYSICAL_CORES)
        || (hardware.memory_bytes > 0 && hardware.memory_bytes < MIN_MEMORY)
        || hardware.graphics == GraphicsCapability::LegacyHardware
    {
        return VisualCapability::Constrained;
    }
    if !hardware
        .physical_cores
        .is_some_and(|cores| cores >= MIN_PHYSICAL_CORES)
        || hardware.memory_bytes == 0
        || hardware.graphics == GraphicsCapability::Unavailable
    {
        return VisualCapability::Unknown;
    }
    VisualCapability::Capable
}

/// Read native capacity and graphics support without retaining hardware details.
/// Linux deliberately skips all probes; its automatic mode is always smooth.
pub fn detect() -> VisualCapability {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        let mut system = sysinfo::System::new();
        system.refresh_memory();
        let mut hardware = Hardware {
            physical_cores: sysinfo::System::physical_core_count(),
            memory_bytes: system.total_memory(),
            graphics: GraphicsCapability::Unavailable,
        };
        if classify(hardware) == VisualCapability::Constrained {
            return VisualCapability::Constrained;
        }
        #[cfg(target_os = "macos")]
        {
            hardware.graphics = macos::detect_graphics();
        }
        #[cfg(target_os = "windows")]
        {
            hardware.graphics = windows::detect_graphics();
        }
        classify(hardware)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        VisualCapability::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const GIB: u64 = 1024 * 1024 * 1024;

    #[test]
    fn visual_capabilities_m4_capacity_is_capable_with_modern_graphics() {
        assert_eq!(
            classify(Hardware {
                physical_cores: Some(10),
                memory_bytes: 24 * GIB,
                graphics: GraphicsCapability::ModernHardware
            }),
            VisualCapability::Capable
        );
    }

    #[test]
    fn visual_capabilities_require_all_three_signals() {
        let baseline = Hardware {
            physical_cores: Some(4),
            memory_bytes: 8 * GIB,
            graphics: GraphicsCapability::ModernHardware,
        };
        assert_eq!(classify(baseline), VisualCapability::Capable);
        assert_eq!(
            classify(Hardware {
                physical_cores: Some(3),
                ..baseline
            }),
            VisualCapability::Constrained
        );
        assert_eq!(
            classify(Hardware {
                memory_bytes: 8 * GIB - 1,
                ..baseline
            }),
            VisualCapability::Constrained
        );
        assert_eq!(
            classify(Hardware {
                graphics: GraphicsCapability::LegacyHardware,
                ..baseline
            }),
            VisualCapability::Constrained
        );
        assert_eq!(
            classify(Hardware {
                graphics: GraphicsCapability::Unavailable,
                ..baseline
            }),
            VisualCapability::Unknown
        );
        assert_eq!(
            classify(Hardware {
                physical_cores: None,
                ..baseline
            }),
            VisualCapability::Unknown
        );
        assert_eq!(
            classify(Hardware {
                physical_cores: Some(0),
                ..baseline
            }),
            VisualCapability::Unknown
        );
        assert_eq!(
            classify(Hardware {
                memory_bytes: 0,
                ..baseline
            }),
            VisualCapability::Unknown
        );
    }

    #[test]
    #[ignore = "run explicitly on a supported graphics-capable desktop"]
    fn visual_capabilities_live_host() {
        let started = std::time::Instant::now();
        let result = detect();
        println!(
            "native visual capability: {result:?}; elapsed: {:?}",
            started.elapsed()
        );
        assert_eq!(result, VisualCapability::Capable);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn visual_capabilities_linux_stays_unknown_without_probing() {
        assert_eq!(detect(), VisualCapability::Unknown);
    }
}
