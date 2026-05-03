// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Tracing and config are handled inside build_gui_app()
    let ctx = match uc_bootstrap::build_gui_app() {
        Ok(ctx) => ctx,
        Err(e) => {
            eprintln!("Bootstrap failed: {}", e);
            std::process::exit(1);
        }
    };

    // `tauri::generate_context!()` 必须在 bin crate 调用——它读取 bin
    // 的 Cargo.toml 同目录下的 tauri.conf.json。
    uc_tauri::run(ctx, tauri::generate_context!());
}
