//! `uniclip mobile-sync shortcut ...` —— SyncClipboard EX iCloud 模板的添加流程。
//!
//! 唯一子命令 `add --label <LABEL>`:在 daemon **未运行**时颁发一对独立
//! (username, password) 凭据,渲染 SyncClipboard "Clipboard EX" iCloud
//! 共享链接的 ASCII 二维码并打印到终端,user 用 iPhone 扫码安装,然后
//! 在 SyncClipboard shortcut 里编辑 url / username / password 三项。
//!
//! 详见 SPEC §14.2 + §7.

use clap::Subcommand;
use serde::Serialize;

use uc_application::facade::RegisterMobileShortcutDeviceInput;

use crate::commands::mobile_sync::shared;
use crate::exit_codes;
use crate::ui;

#[derive(Subcommand)]
pub enum ShortcutCommands {
    /// Add a new iPhone (mints credentials and renders the install QR).
    Add {
        /// Human-readable label, e.g. "My iPhone 15".
        #[arg(long)]
        label: String,
    },
}

pub async fn run(command: ShortcutCommands, json: bool, verbose: bool) -> i32 {
    match command {
        ShortcutCommands::Add { label } => add(label, json, verbose).await,
    }
}

#[derive(Serialize)]
struct AddDeviceDto {
    device_id: String,
    label: String,
    base_url: String,
    username: String,
    password: String,
    install_url: String,
    qr_code_ascii: String,
}

async fn add(label: String, json: bool, verbose: bool) -> i32 {
    let ctx = match shared::enter_write("Add iPhone (SyncClipboard EX)", json, verbose).await {
        Ok(c) => c,
        Err(code) => return code,
    };

    let result = ctx
        .facade
        .register_device(RegisterMobileShortcutDeviceInput {
            label: label.clone(),
        })
        .await;

    match result {
        Ok(out) => {
            if json {
                let dto = AddDeviceDto {
                    device_id: out.device.device_id.as_str().to_string(),
                    label: out.device.label.clone(),
                    base_url: out.base_url.clone(),
                    username: out.username.clone(),
                    password: out.password.clone(),
                    install_url: out.install_url.clone(),
                    qr_code_ascii: out.qr_code_ascii.clone(),
                };
                shared::finish_json(ctx, &dto).await
            } else {
                ui::success(&format!("Registered device: {}", out.device.label));
                ui::info("deviceId", out.device.device_id.as_str());
                ui::info("baseUrl", &out.base_url);
                ui::info("username", &out.username);
                ui::info("password (one-time)", &out.password);
                ui::info("installUrl", &out.install_url);
                ui::bar();
                println!();
                println!("{}", out.qr_code_ascii);
                println!();
                ui::info(
                    "next",
                    "Scan the QR with iPhone Camera, install the SyncClipboard \
                     shortcut, then edit url / username / password fields.",
                );
                ui::warn("The password above will NOT be shown again. Copy it now.");
                ui::warn(
                    "Run `uniclip start` so the LAN listener accepts requests \
                     from this device.",
                );
                shared::finish(ctx, exit_codes::EXIT_SUCCESS).await
            }
        }
        Err(err) => {
            ui::error(&shared::render_register_error(&err));
            shared::finish(ctx, exit_codes::EXIT_ERROR).await
        }
    }
}
