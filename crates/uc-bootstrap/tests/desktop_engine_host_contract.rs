use uc_bootstrap::{prepare_desktop_engine_host, DesktopEngineHost};
use uc_core::config::AppConfig;

#[test]
fn desktop_engine_host_has_a_single_preparation_entry() {
    let _prepare: fn(&AppConfig) -> uc_bootstrap::WiringResult<DesktopEngineHost> =
        prepare_desktop_engine_host;
}

#[test]
fn desktop_engine_host_preparation_does_not_assemble_the_core() {
    let source = include_str!("../src/wiring/desktop_host.rs");
    assert!(!source.contains("wire_dependencies("));
    assert!(!source.contains("Engine::start("));
}
