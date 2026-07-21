use uc_bootstrap::{prepare_desktop_engine_host, DesktopEngineHost, DesktopHostFileHandles};
use uc_core::config::AppConfig;
use uc_engine::HostFileAccess;

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

#[test]
fn desktop_host_file_handles_share_opaque_input_and_output_paths() {
    let temp = tempfile::tempdir().unwrap();
    let input_path = temp.path().join("private-input.txt");
    let output_path = temp.path().join("private-output.txt");
    std::fs::write(&input_path, b"input bytes").unwrap();

    let handles = DesktopHostFileHandles::default();
    let input = handles.register_input(input_path.clone()).unwrap();
    let output = handles.register_output(output_path.clone()).unwrap();

    assert_eq!(handles.read_chunk(&input, 0, 64).unwrap(), b"input bytes");
    handles.write_chunk(&output, 0, b"output ").unwrap();
    handles.write_chunk(&output, 7, b"bytes").unwrap();
    handles.finish_write(&output).unwrap();
    assert_eq!(std::fs::read(&output_path).unwrap(), b"output bytes");

    for handle in [input, output] {
        let debug = format!("{handle:?}");
        assert!(!debug.contains("private-input.txt"));
        assert!(!debug.contains("private-output.txt"));
        assert!(!debug.contains(temp.path().to_string_lossy().as_ref()));
    }
}
