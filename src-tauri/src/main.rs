// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // `tauri::generate_context!()` 必须在 bin crate 调用——它读取 bin
    // 的 Cargo.toml 同目录下的 tauri.conf.json。其它装配 + Tauri 事件
    // 循环全部交给 `uc_tauri::run`。
    if let Err(e) = uc_tauri::run(tauri::generate_context!()) {
        eprintln!("Tauri shell failed: {}", e);
        std::process::exit(1);
    }
}
