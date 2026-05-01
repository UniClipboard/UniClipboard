//! `uniclip members` — list paired devices + presence (Slice 2 Phase 1 · T10).
//!
//! Self-contained direct-mode command (no daemon), mirroring the `init` /
//! `invite` / `join` pattern in this crate. Builds a `SpaceSetupAssembly`,
//! silently resumes the cached session, forces one `ensure_reachable_all`
//! probe pass (so a B-restart-then-query window shows `online` within
//! ≤ 10 s per plan §1.1 / §12.4), then prints the roster.
//!
//! Human output:
//!
//! ```text
//!   laptop (online) [local]
//!   phone (offline)
//!   workstation (unknown)
//! ```
//!
//! JSON output: array of `{device_id, device_name, is_local, state}`.

use serde::Serialize;
use uc_application::facade::roster::{RosterEntry, RosterError};
use uc_application::facade::space_setup::TryResumeSessionError;
use uc_core::ports::ReachabilityState;

use crate::commands::app_session::{build_app_session, refuse_if_daemon_running};
use crate::exit_codes;
use crate::ui;

pub async fn run(json: bool, verbose: bool) -> i32 {
    ui::header("Members");

    if let Err(code) = refuse_if_daemon_running().await {
        return code;
    }

    let cli = match build_app_session(verbose).await {
        Ok(bundle) => bundle,
        Err(code) => return code,
    };

    // 静默 resume:roster 只读成员 + presence 缓存,严格来说不需要解锁
    // session,但 `member_repo` 的后备 adapter 未来可能要求 active space,
    // 且与 `invite` 保持一致的"先 resume 再操作"节奏让用户感受一致。
    // 找不到可 resume 的空间时直接说明并退出,避免下游空列表让人误以为
    // 配对失败。
    let resume_spinner = ui::spinner("Resuming space session...");
    match cli.app_facade().try_resume_session().await {
        Ok(true) => {
            ui::spinner_finish_success(&resume_spinner, "Session resumed");
        }
        Ok(false) => {
            ui::spinner_finish_error(
                &resume_spinner,
                "No space on this profile — run `init` or `join` first.",
            );
            cli.shutdown().await;
            return exit_codes::EXIT_ERROR;
        }
        Err(TryResumeSessionError::CorruptedKeyMaterial) => {
            ui::spinner_finish_error(
                &resume_spinner,
                "Key material is corrupted — consider resetting this profile.",
            );
            cli.shutdown().await;
            return exit_codes::EXIT_ERROR;
        }
        Err(TryResumeSessionError::KeyringMiss) => {
            ui::spinner_finish_error(
                &resume_spinner,
                "Keychain cannot silently unlock this space. Run a future \
                 `uniclip unlock` (not yet shipped) or re-init.",
            );
            cli.shutdown().await;
            return exit_codes::EXIT_ERROR;
        }
        Err(TryResumeSessionError::Internal(msg)) => {
            ui::spinner_finish_error(&resume_spinner, &format!("Resume failed: {msg}"));
            cli.shutdown().await;
            return exit_codes::EXIT_ERROR;
        }
    }

    // plan §12.4 / §8 T3 修订决策:查询前跑一轮 ensure_reachable_all,保
    // 证"B 重启后下一次 CLI 查询 ≤ 10s 内看见 online"的验收条款。单个
    // peer 失败不 fatal —— 进 report.errors,摘要里展示个数即可。
    let probe_spinner = ui::spinner("Probing paired peers...");
    match cli.app_facade().refresh_presence().await {
        Ok(report) => {
            ui::spinner_finish_success(
                &probe_spinner,
                &format!(
                    "Probed {} peer(s): {} online, {} offline, {} error(s)",
                    report.total,
                    report.online,
                    report.offline,
                    report.errors.len()
                ),
            );
        }
        Err(err) => {
            // peer_addr_repo 故障属 infra fatal 但 roster 仍可展示缓存里的
            // 已知状态(全是 Unknown / 过期状态),不 abort——改打 warn。
            ui::spinner_finish_error(
                &probe_spinner,
                &format!("Probe round failed: {err} (showing last-known state)"),
            );
        }
    }

    let entries = match cli.app_facade().list_roster_entries().await {
        Ok(entries) => entries,
        Err(err) => {
            let msg = match &err {
                RosterError::MemberRepository(m) => format!("list members failed: {m}"),
                RosterError::LocalIdentity(m) => format!("local identity read failed: {m}"),
                RosterError::PeerAddressRepository(m) => {
                    format!("peer address cleanup failed: {m}")
                }
                RosterError::TrustedPeerRepository(m) => {
                    format!("trusted peer cleanup failed: {m}")
                }
                RosterError::NotFound(m) => format!("member not found: {m}"),
                RosterError::Unavailable => "member roster unavailable".to_string(),
            };
            ui::error(&msg);
            cli.shutdown().await;
            return exit_codes::EXIT_ERROR;
        }
    };

    if json {
        let dtos: Vec<MemberDto> = entries.iter().map(MemberDto::from).collect();
        match serde_json::to_string_pretty(&dtos) {
            Ok(json_str) => println!("{json_str}"),
            Err(err) => {
                ui::error(&format!("Failed to serialize roster: {err}"));
                cli.shutdown().await;
                return exit_codes::EXIT_ERROR;
            }
        }
    } else {
        render_human(&entries);
    }

    cli.shutdown().await;
    exit_codes::EXIT_SUCCESS
}

fn render_human(entries: &[RosterEntry]) {
    ui::bar();
    if entries.is_empty() {
        ui::info("members", "(none)");
    } else {
        for entry in entries {
            let local_tag = if entry.is_local { " [local]" } else { "" };
            // 与 `info()` 左侧 gutter 对齐:`"│  "` 前缀 + "name (state) [local]"
            let line = format!(
                "{} ({}){}",
                entry.device_name,
                format_state(entry.state),
                local_tag,
            );
            // 直接用 info 的 label/value 组合不合适(这里没有 label),所以
            // 走 Term::stderr write_line 会更合适——但为了最小依赖,复用
            // ui::info 的视觉风格,label 填"·",value 填整行。
            ui::info("·", &line);
        }
    }
    ui::bar();
}

fn format_state(state: ReachabilityState) -> &'static str {
    match state {
        ReachabilityState::Online => "online",
        ReachabilityState::Offline => "offline",
        ReachabilityState::Unknown => "unknown",
    }
}

#[derive(Serialize)]
struct MemberDto {
    device_id: String,
    device_name: String,
    is_local: bool,
    state: &'static str,
}

impl From<&RosterEntry> for MemberDto {
    fn from(entry: &RosterEntry) -> Self {
        Self {
            device_id: entry.device_id.as_str().to_string(),
            device_name: entry.device_name.clone(),
            is_local: entry.is_local,
            state: format_state(entry.state),
        }
    }
}
