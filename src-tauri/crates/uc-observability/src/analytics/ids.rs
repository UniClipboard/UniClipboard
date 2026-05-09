//! `anonymous_user_id` / `analytics_device_id` 的持久化。
//!
//! 与 schema doc §3 对应。两个 ID 都是 UUIDv7、随机生成，与 `uc-core` 的
//! 业务 `DeviceId` 完全 disjoint——本模块**不允许**读取或派生自任何业务标识。
//!
//! ## 文件布局
//!
//! ```text
//! <analytics_dir>/
//! ├── installation_id        # 文本，单行 UUID
//! └── analytics_device_id    # 同上
//! ```
//!
//! 调用方负责选择 `analytics_dir`（推荐放在 `app_data_root_dir/analytics/`）。
//! 本模块不感知 `AppPaths`，纯函数易测。
//!
//! ## 原子性
//!
//! 写入走 "写 `<file>.tmp` → rename" 的两步操作。POSIX `rename(2)` 是
//! 原子操作，进程崩溃最多留下一个 `.tmp` 文件，下次启动会被覆盖。
//!
//! ## 并发
//!
//! 同一进程内只允许 init 阶段调用一次 [`load_or_create`]。多个调用者并发
//! 调用会各自生成不同的 ID 后相互覆盖——本模块**不**做文件锁，由调用方
//! 保证序列化（`uc-bootstrap` 的 init 时序天然满足）。

use std::fs;
use std::io;
use std::path::Path;

use anyhow::{Context, Result};
use uuid::Uuid;

const INSTALLATION_ID_FILE: &str = "installation_id";
const ANALYTICS_DEVICE_ID_FILE: &str = "analytics_device_id";

/// 持久化的 analytics 标识对。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnalyticsIds {
    /// `anonymous_user_id` —— 留存计算的"用户"。
    pub anonymous_user_id: Uuid,
    /// `analytics_device_id` —— 设备级切片用。**不要**与业务 `DeviceId` 关联。
    pub analytics_device_id: Uuid,
    /// 仅当本次调用同时新生成了**两个** ID 时为 `true`。
    ///
    /// 即：之前从未运行过本应用，或用户主动调用 [`reset`] 后再次启动。
    /// 任意一个 ID 已经存在的情况下都是 `false`，避免把"分区损坏后修复"
    /// 误算成"首次安装"。
    pub is_first_run: bool,
}

/// 读取或首次生成两个 ID。
///
/// 行为表：
///
/// | 文件状态 | anonymous_user_id | analytics_device_id | is_first_run |
/// |---|---|---|---|
/// | 都存在且可解析 | 沿用 | 沿用 | `false` |
/// | 都不存在 | 新生成 | 新生成 | `true` |
/// | 仅一个缺失 / 损坏 | 缺失方新生成 | 同左 | `false`（已有 ID 仍代表老用户） |
///
/// 解析失败的处理：写 `tracing::warn!` 后视为缺失。原始字节不会出现在日志里。
pub fn load_or_create(analytics_dir: &Path) -> Result<AnalyticsIds> {
    fs::create_dir_all(analytics_dir)
        .with_context(|| format!("create analytics dir {}", analytics_dir.display()))?;

    let installation_path = analytics_dir.join(INSTALLATION_ID_FILE);
    let device_path = analytics_dir.join(ANALYTICS_DEVICE_ID_FILE);

    let existing_installation = read_uuid(&installation_path)?;
    let existing_device = read_uuid(&device_path)?;

    let is_first_run = existing_installation.is_none() && existing_device.is_none();

    let anonymous_user_id = match existing_installation {
        Some(id) => id,
        None => {
            let id = Uuid::now_v7();
            atomic_write(&installation_path, &id.to_string())?;
            id
        }
    };

    let analytics_device_id = match existing_device {
        Some(id) => id,
        None => {
            let id = Uuid::now_v7();
            atomic_write(&device_path, &id.to_string())?;
            id
        }
    };

    Ok(AnalyticsIds {
        anonymous_user_id,
        analytics_device_id,
        is_first_run,
    })
}

/// 删除两个 ID 文件。下次 [`load_or_create`] 会重新生成，等价于"首次运行"。
///
/// 幂等：文件不存在不视为错误。
pub fn reset(analytics_dir: &Path) -> Result<()> {
    for filename in [INSTALLATION_ID_FILE, ANALYTICS_DEVICE_ID_FILE] {
        let path = analytics_dir.join(filename);
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => {
                return Err(anyhow::Error::from(e).context(format!("remove {}", path.display())));
            }
        }
    }
    Ok(())
}

/// 读 UUID。文件不存在或解析失败都返回 `Ok(None)`——调用方按"需要重建"处理。
///
/// 真正的 IO 错误（权限、IO 故障等）才往上抛 `Err`。
fn read_uuid(path: &Path) -> Result<Option<Uuid>> {
    match fs::read_to_string(path) {
        Ok(content) => match Uuid::parse_str(content.trim()) {
            Ok(id) => Ok(Some(id)),
            Err(e) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %e,
                    "analytics ID 文件无法解析，将重新生成"
                );
                Ok(None)
            }
        },
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(anyhow::Error::from(e).context(format!("read {}", path.display()))),
    }
}

/// 写到 `<file>.tmp` 后 rename，保证崩溃下不会留下半截内容。
fn atomic_write(path: &Path, content: &str) -> Result<()> {
    let tmp_path = path.with_extension("tmp");
    fs::write(&tmp_path, content).with_context(|| format!("write tmp {}", tmp_path.display()))?;
    fs::rename(&tmp_path, path)
        .with_context(|| format!("rename {} -> {}", tmp_path.display(), path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn fresh_dir() -> TempDir {
        TempDir::new().expect("create tempdir")
    }

    // —— 首次运行 vs 复用 ————————————————————————————————————

    #[test]
    fn first_run_generates_both_ids_and_marks_is_first_run() {
        let dir = fresh_dir();
        let ids = load_or_create(dir.path()).unwrap();

        assert!(ids.is_first_run);
        // UUIDv7 的 version nibble 是 0x7。
        assert_eq!(ids.anonymous_user_id.get_version_num(), 7);
        assert_eq!(ids.analytics_device_id.get_version_num(), 7);
        assert_ne!(ids.anonymous_user_id, ids.analytics_device_id);

        assert!(dir.path().join(INSTALLATION_ID_FILE).exists());
        assert!(dir.path().join(ANALYTICS_DEVICE_ID_FILE).exists());
    }

    #[test]
    fn second_run_returns_same_ids_and_clears_is_first_run() {
        let dir = fresh_dir();
        let first = load_or_create(dir.path()).unwrap();
        let second = load_or_create(dir.path()).unwrap();

        assert_eq!(first.anonymous_user_id, second.anonymous_user_id);
        assert_eq!(first.analytics_device_id, second.analytics_device_id);
        assert!(first.is_first_run);
        assert!(!second.is_first_run);
    }

    // —— 部分损坏 / 缺失 ————————————————————————————————————

    #[test]
    fn missing_one_file_regenerates_only_that_one_without_first_run_flag() {
        let dir = fresh_dir();
        let original = load_or_create(dir.path()).unwrap();

        // 单删 installation_id，模拟分区故障 / 用户手动删一个文件。
        fs::remove_file(dir.path().join(INSTALLATION_ID_FILE)).unwrap();

        let recovered = load_or_create(dir.path()).unwrap();
        assert!(
            !recovered.is_first_run,
            "已有 analytics_device_id 时不应被算作首次运行"
        );
        assert_ne!(
            recovered.anonymous_user_id, original.anonymous_user_id,
            "丢失的 ID 必须重生成"
        );
        assert_eq!(
            recovered.analytics_device_id, original.analytics_device_id,
            "未丢失的 ID 必须沿用"
        );
    }

    #[test]
    fn corrupted_id_file_is_regenerated_silently() {
        let dir = fresh_dir();
        let original = load_or_create(dir.path()).unwrap();

        // 用垃圾内容覆盖 device id 文件。
        fs::write(dir.path().join(ANALYTICS_DEVICE_ID_FILE), "not-a-uuid").unwrap();

        let recovered = load_or_create(dir.path()).unwrap();
        assert!(!recovered.is_first_run);
        assert_eq!(recovered.anonymous_user_id, original.anonymous_user_id);
        assert_ne!(recovered.analytics_device_id, original.analytics_device_id);
        assert_eq!(recovered.analytics_device_id.get_version_num(), 7);
    }

    #[test]
    fn id_files_contain_canonical_hyphenated_uuid_form() {
        let dir = fresh_dir();
        let ids = load_or_create(dir.path()).unwrap();

        let installation_text = fs::read_to_string(dir.path().join(INSTALLATION_ID_FILE)).unwrap();
        let device_text = fs::read_to_string(dir.path().join(ANALYTICS_DEVICE_ID_FILE)).unwrap();

        // UUID 标准 36 字符形式（含 4 个连字符），便于人工排查。
        assert_eq!(installation_text.trim().len(), 36);
        assert_eq!(device_text.trim().len(), 36);
        assert_eq!(
            Uuid::parse_str(installation_text.trim()).unwrap(),
            ids.anonymous_user_id
        );
        assert_eq!(
            Uuid::parse_str(device_text.trim()).unwrap(),
            ids.analytics_device_id
        );
    }

    #[test]
    fn whitespace_around_uuid_is_tolerated() {
        // 防御性：手写 / 跨平台编辑器可能引入 \r\n 或末尾空格。
        let dir = fresh_dir();
        let id = Uuid::now_v7();
        fs::write(
            dir.path().join(INSTALLATION_ID_FILE),
            format!("\r\n{id}  \n"),
        )
        .unwrap();
        fs::write(dir.path().join(ANALYTICS_DEVICE_ID_FILE), format!(" {id} ")).unwrap();

        let recovered = load_or_create(dir.path()).unwrap();
        assert_eq!(recovered.anonymous_user_id, id);
        assert_eq!(recovered.analytics_device_id, id);
    }

    // —— reset ————————————————————————————————————

    #[test]
    fn reset_clears_both_files_and_next_load_is_first_run() {
        let dir = fresh_dir();
        load_or_create(dir.path()).unwrap();

        reset(dir.path()).unwrap();
        assert!(!dir.path().join(INSTALLATION_ID_FILE).exists());
        assert!(!dir.path().join(ANALYTICS_DEVICE_ID_FILE).exists());

        let after = load_or_create(dir.path()).unwrap();
        assert!(after.is_first_run);
    }

    #[test]
    fn reset_is_idempotent() {
        let dir = fresh_dir();
        // 在没有任何 ID 文件的目录上调 reset 不应失败。
        reset(dir.path()).unwrap();
        reset(dir.path()).unwrap();

        // 生成后再连续 reset 两次也应成功。
        load_or_create(dir.path()).unwrap();
        reset(dir.path()).unwrap();
        reset(dir.path()).unwrap();
    }

    // —— 目录创建 ————————————————————————————————————

    #[test]
    fn load_or_create_makes_missing_parent_directory() {
        let dir = fresh_dir();
        let nested = dir.path().join("a").join("b").join("analytics");
        assert!(!nested.exists());

        let ids = load_or_create(&nested).unwrap();
        assert!(ids.is_first_run);
        assert!(nested.exists());
        assert!(nested.join(INSTALLATION_ID_FILE).exists());
    }

    // —— 原子性 ————————————————————————————————————

    #[test]
    fn no_tmp_files_remain_after_successful_load() {
        let dir = fresh_dir();
        load_or_create(dir.path()).unwrap();

        let entries: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name())
            .collect();
        for name in &entries {
            let s = name.to_string_lossy();
            assert!(
                !s.ends_with(".tmp"),
                "成功路径下不应该留下 {s} 这种 tmp 文件"
            );
        }
    }
}
