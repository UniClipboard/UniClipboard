//! Composition-root core: the wiring output bundles and the
//! `wire_dependencies` orchestrator that fills them.

pub mod deps;
pub(crate) mod desktop;
pub(crate) mod desktop_host;
pub mod wire;

#[cfg(test)]
mod tests {
    #[test]
    fn desktop_wiring_is_owned_by_desktop_module() {
        let _ = super::desktop::wire_dependencies;
    }
}
