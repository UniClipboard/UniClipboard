//! GUI 端 daemon 协调类型（ADR-008）。
//!
//! daemon runtime + host entry points（run_mode、workers、装配链、main loop、
//! `run` / `run_standalone_from_env`）全部住在 `uc-daemon`（产出 `uniclipd`
//! 二进制）。GUI 进程从不在进程内跑 daemon 代码，所以本模块只拥有
//! [`DaemonOwnership`]——GUI 内存里"是否已 attach 到外部 daemon"的轻量标记。
//!
//! 本模块**刻意不依赖 `uc-daemon`**：历史上 `uc-desktop` 仅为了 re-export 这个
//! ~40 行的 ownership 类型而 `path`-依赖整个 `uc-daemon`，导致 GUI 构建被迫编译
//! 整棵 daemon runtime 依赖树（uc-webserver / uc-application 等）。把类型下沉到
//! 这里、删掉死 re-export 后，依赖边断开，GUI 构建不再编 daemon runtime。

mod ownership;

pub use ownership::DaemonOwnership;
