//! Hidden CLI diagnostic for capturing explicit file and directory paths.

use std::path::PathBuf;

use serde::Serialize;
use uc_engine::{
    DevCapturedFileSet, DevCapturedFileSetLine, DevOperation, DevOperationResult,
    FileSyncSettingsPatch, Operation, OperationResult, SettingsPatch,
};

use crate::commands::app_session::{build_app_session, refuse_if_daemon_running, CliAppSession};
use crate::{exit_codes, output, ui};

pub struct CaptureFilesArgs {
    pub paths: Vec<PathBuf>,
    pub max_members: Option<u64>,
    pub max_bytes: Option<u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CaptureFilesOutput {
    entry_id: String,
    deduplicated: bool,
    directory_structure: bool,
    content_digest_count: usize,
    lines: Vec<CaptureFilesLineOutput>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CaptureFilesLineOutput {
    line_index: i64,
    root_index: Option<i64>,
    root_name: Option<String>,
    relative_path: Option<String>,
    member_kind: Option<String>,
    line_kind: String,
    exclude_reason: Option<String>,
}

impl From<DevCapturedFileSet> for CaptureFilesOutput {
    fn from(value: DevCapturedFileSet) -> Self {
        Self {
            entry_id: value.entry_id,
            deduplicated: value.deduplicated,
            directory_structure: value.directory_structure,
            content_digest_count: value.content_digest_count,
            lines: value.lines.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<DevCapturedFileSetLine> for CaptureFilesLineOutput {
    fn from(value: DevCapturedFileSetLine) -> Self {
        Self {
            line_index: value.line_index,
            root_index: value.root_index,
            root_name: value.root_name,
            relative_path: value.relative_path,
            member_kind: value.member_kind,
            line_kind: value.line_kind,
            exclude_reason: value.exclude_reason,
        }
    }
}

pub async fn run(args: CaptureFilesArgs, json: bool, verbose: bool) -> i32 {
    if !json {
        ui::header("Capture file set");
    }
    if let Err(code) = refuse_if_daemon_running().await {
        return code;
    }

    let bundle = match build_app_session(verbose).await {
        Ok(bundle) => bundle,
        Err(code) => return code,
    };
    if let Err(code) = resume_session(&bundle).await {
        bundle.shutdown().await;
        return code;
    }

    let original_caps = match bundle.engine().execute(Operation::QuerySettings).await {
        Ok(OperationResult::Settings(settings)) => (
            settings.file_sync.max_file_set_member_count,
            settings.file_sync.max_file_set_total_bytes,
        ),
        Ok(_) => {
            ui::error("Failed to load file-set caps: unexpected engine response");
            bundle.shutdown().await;
            return exit_codes::EXIT_ERROR;
        }
        Err(err) => {
            ui::error(&format!("Failed to load file-set caps: {err}"));
            bundle.shutdown().await;
            return exit_codes::EXIT_ERROR;
        }
    };
    let overrides_requested = args.max_members.is_some() || args.max_bytes.is_some();
    if overrides_requested {
        let patch = SettingsPatch {
            file_sync: Some(FileSyncSettingsPatch {
                max_file_set_member_count: args.max_members,
                max_file_set_total_bytes: args.max_bytes,
                ..Default::default()
            }),
            ..Default::default()
        };
        if let Err(err) = bundle
            .engine()
            .execute(Operation::UpdateSettings(Box::new(patch)))
            .await
        {
            ui::error(&format!("Failed to apply temporary file-set caps: {err}"));
            bundle.shutdown().await;
            return exit_codes::EXIT_ERROR;
        }
    }

    let capture = bundle
        .engine()
        .execute_dev(DevOperation::CaptureFilePaths { paths: args.paths })
        .await;
    let restore = if overrides_requested {
        bundle
            .engine()
            .execute(Operation::UpdateSettings(Box::new(SettingsPatch {
                file_sync: Some(FileSyncSettingsPatch {
                    max_file_set_member_count: Some(original_caps.0),
                    max_file_set_total_bytes: Some(original_caps.1),
                    ..Default::default()
                }),
                ..Default::default()
            })))
            .await
            .map(|_| ())
    } else {
        Ok(())
    };

    let result = match capture {
        Ok(DevOperationResult::FilePathsCaptured(result)) => result,
        Ok(_) => {
            ui::error("Failed to capture file set: unexpected engine response");
            if let Err(restore_err) = restore {
                ui::error(&format!("Failed to restore file-set caps: {restore_err}"));
            }
            bundle.shutdown().await;
            return exit_codes::EXIT_ERROR;
        }
        Err(err) => {
            ui::error(&format!("Failed to capture file set: {err}"));
            if let Err(restore_err) = restore {
                ui::error(&format!("Failed to restore file-set caps: {restore_err}"));
            }
            bundle.shutdown().await;
            return exit_codes::EXIT_ERROR;
        }
    };
    if let Err(err) = restore {
        ui::error(&format!("Failed to restore file-set caps: {err}"));
        bundle.shutdown().await;
        return exit_codes::EXIT_ERROR;
    }

    let output_view = CaptureFilesOutput::from(result);
    let exit = if json {
        output::emit_json(&output_view, "captured file set")
    } else {
        ui::info("entry_id", &output_view.entry_id);
        ui::info("line_count", &output_view.lines.len().to_string());
        ui::info(
            "directory_structure",
            &output_view.directory_structure.to_string(),
        );
        for line in &output_view.lines {
            ui::info("member", line.relative_path.as_deref().unwrap_or("-"));
        }
        exit_codes::EXIT_SUCCESS
    };

    bundle.shutdown().await;
    exit
}

async fn resume_session(bundle: &CliAppSession) -> Result<(), i32> {
    match bundle.recover_session().await {
        Ok(true) => Ok(()),
        Ok(false) => {
            ui::error(
                "This device is not set up yet. Use `uniclip space init` or `uniclip space join` first.",
            );
            Err(exit_codes::EXIT_ERROR)
        }
        Err(error) => {
            ui::error(&format!("Resume failed: {error}"));
            Err(exit_codes::EXIT_ERROR)
        }
    }
}
