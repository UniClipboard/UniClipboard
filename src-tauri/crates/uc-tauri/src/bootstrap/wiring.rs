//! # Dependency Injection / 依赖注入模块
//!
//! ## Responsibilities / 职责
//!
//! - ✅ Create infra implementations (db, fs, secure storage) / 创建 infra 层具体实现
//! - ✅ Create platform implementations (clipboard, network) / 创建 platform 层具体实现
//! - ✅ Inject all dependencies into App / 将所有依赖注入到 App
//!
//! ## Prohibited / 禁止事项
//!
//! ❌ **No business logic / 禁止包含任何业务逻辑**
//! - Do not decide "what to do if encryption uninitialized"
//! - 不判断"如果加密未初始化就怎样"
//! - Do not handle "what to do if device not registered"
//! - 不处理"如果设备未注册就怎样"
//!
//! ❌ **No configuration validation / 禁止做配置验证**
//! - Config already loaded in config.rs
//! - 配置已在 config.rs 加载
//! - Validation should be in use case or upper layer
//! - 验证应在 use case 或上层
//!
//! ❌ **No direct concrete implementation usage / 禁止直接使用具体实现**
//! - Must inject through Port traits
//! - 必须通过 Port trait 注入
//! - Do not call implementation methods directly after App construction
//! - 不在 App 构造后直接调用实现方法
//!
//! ## Architecture Principle / 架构原则
//!
//! > **This is the only place allowed to depend on uc-infra + uc-platform + uc-app simultaneously.**
//! > **这是唯一允许同时依赖 uc-infra、uc-platform 和 uc-app 的地方。**
//! > But this privilege is only for "assembly", not for "decision making".
//! > 但这种特权仅用于"组装"，不用于"决策"。

// Re-export assembly types from uc-bootstrap.
pub use uc_bootstrap::assembly::{
    get_storage_paths, resolve_pairing_device_name, wire_dependencies, WiredDependencies,
    WiringError, WiringResult,
};

// Re-export BackgroundRuntimeDeps from uc-bootstrap (definition moved in Phase 40).
pub use uc_bootstrap::BackgroundRuntimeDeps;

// 后台任务调度已下沉到 uc-desktop —— 多 shell 共享。
// 这里 re-export 以保持 `uc_tauri::bootstrap::start_background_tasks` 的历史
// import 路径。新代码请直接 `use uc_desktop::background::*;`。
pub use uc_desktop::background::start_file_cache_cleanup as start_background_tasks;
pub use uc_desktop::background::start_gui_pairing_lease as start_gui_pairing_lease_task;
