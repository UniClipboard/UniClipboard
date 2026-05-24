//! 文件接收 HUD 的纯逻辑状态机。
//!
//! 这个模块不接 AppKit,也不接 host event bus —— 只是一个 in-memory 模
//! 型,接受"事件 + 当前时间"作为输入,产出"当前应该渲染哪些行"作为输出。
//! 这样可以在没有 macOS 环境的情况下,对核心边界(buffered 阶段 entry_id
//! 缺失、IncomingPending 后到回填、Cancelled/Completed 终态保留、Sending
//! 方向丢弃、速度/ETA 滑窗)写完整单测。
//!
//! ## 行键约定
//!
//! 行键固定使用 `transfer_id`。协议层约定 `transfer_id == receiver_entry_id`
//! (见 `uc-application/src/facade/blob_transfer/facade.rs` 中的注释),因
//! 此 `entry_id` 可以与 `transfer_id` 互换索引。这里只暴露 `transfer_id`
//! 这一面,避免上游事件里 `entry_id: Option<String>` 的 None 分支(buffered
//! 阶段)污染索引语义。
//!
//! ## 方向过滤
//!
//! 状态机只对 `Receiving` 方向的 Progress 建/更新行。Sending 方向直接丢
//! 弃 —— 接收方 HUD 不显示出站传输。`StatusChanged` 不带 direction,所以
//! 用"行不存在就忽略"作为隐式过滤:出站 transfer 从来没有插入过行,
//! 它的 Completed/Cancelled 也就不会更新出新行。

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use uc_core::file_transfer::FileTransferDirection;

use super::clock::Clock;

/// 行的生命周期状态。
///
/// `Receiving` 是进行态;`Completed` / `Failed` / `Cancelled` 是终态,
/// 保留若干秒后由 [`ActivityHudState::sweep`] 移除。`CancelPending` 是
/// 乐观 UI 状态:用户点击取消按钮后立即进入,等后端发回
/// `StatusChanged: cancelled` 才落到真正的 `Cancelled`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowState {
    Receiving,
    Completed,
    Failed { reason: Option<String> },
    Cancelled { reason: Option<String> },
    CancelPending,
}

impl RowState {
    /// 终态:行最终会被 sweep 走,不应再被 progress 事件更新。
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            RowState::Completed | RowState::Failed { .. } | RowState::Cancelled { .. }
        )
    }
}

/// 速度滑窗的最大保留时长。窗口里只保留 `(timestamp_ms, bytes)` 对,
/// 用窗口首尾计算字节增量除以时间增量得到瞬时速度。
const SPEED_WINDOW_MS: u64 = 3_000;
/// 滑窗采样点上限。Progress 事件突发时(短时间内连发)避免无界增长。
const SPEED_WINDOW_MAX_SAMPLES: usize = 32;
/// 终态保留时长:Completed 后保留多久才被 sweep 移除。配 UI 上"打勾
/// 停一下再淡出"的视觉节奏。
pub const COMPLETED_RETAIN_MS: u64 = 2_000;
/// Failed / Cancelled 终态保留时长。用户需要更多时间读失败原因。
pub const FAILED_RETAIN_MS: u64 = 4_000;

/// 单条传输行的对外快照。`speed_window` 是内部状态,不进 snapshot。
#[derive(Debug, Clone, PartialEq)]
pub struct ActivityHudRow {
    pub transfer_id: String,
    pub peer_id: String,
    /// 从 `IncomingPending.filenames` 缓存来。事件比 first Progress 晚到
    /// 时为 `None`,UI 应显示 "正在接收文件…" 占位文案。
    pub filenames: Option<Vec<String>>,
    pub bytes_transferred: u64,
    pub total_bytes: Option<u64>,
    pub state: RowState,
    /// 瞬时速度(字节/秒)。不足两次 progress 时为 `None`。
    pub speed_bps: Option<f64>,
    /// 预计还需毫秒数。`total_bytes` 或 `speed_bps` 缺失、或剩余为零时
    /// 为 `None`。
    pub eta_ms: Option<u64>,
    /// 行进入当前状态(`Receiving` 或终态)的时间戳。`sweep` 用这个
    /// 判断终态行是否过保留期。
    pub state_entered_at_ms: u64,
}

/// 内部行(带滑窗)。`snapshot()` 返回时剥成 [`ActivityHudRow`]。
#[derive(Debug, Clone)]
struct InternalRow {
    transfer_id: String,
    peer_id: String,
    filenames: Option<Vec<String>>,
    bytes_transferred: u64,
    total_bytes: Option<u64>,
    state: RowState,
    state_entered_at_ms: u64,
    /// 滑窗 (timestamp_ms, bytes_transferred)。
    speed_window: VecDeque<(u64, u64)>,
    /// 行首次插入顺序号,用于 snapshot 稳定排序。
    insert_order: u64,
}

impl InternalRow {
    fn compute_speed_bps(&self) -> Option<f64> {
        if self.speed_window.len() < 2 {
            return None;
        }
        let (t0, b0) = *self.speed_window.front()?;
        let (t1, b1) = *self.speed_window.back()?;
        if t1 <= t0 {
            return None;
        }
        if b1 < b0 {
            // 理论上不可能,bytes_transferred 单调不减。防御性返回 None
            // 而不是 panic,避免 publisher 偶发乱序时把整个状态机搞挂。
            return None;
        }
        let bytes = (b1 - b0) as f64;
        let secs = (t1 - t0) as f64 / 1_000.0;
        Some(bytes / secs)
    }

    fn compute_eta_ms(&self, speed_bps: Option<f64>) -> Option<u64> {
        let total = self.total_bytes?;
        let speed = speed_bps?;
        if speed <= 0.0 || self.bytes_transferred >= total {
            return None;
        }
        let remaining = (total - self.bytes_transferred) as f64;
        Some((remaining / speed * 1_000.0) as u64)
    }

    fn snapshot(&self) -> ActivityHudRow {
        let speed_bps = self.compute_speed_bps();
        let eta_ms = self.compute_eta_ms(speed_bps);
        ActivityHudRow {
            transfer_id: self.transfer_id.clone(),
            peer_id: self.peer_id.clone(),
            filenames: self.filenames.clone(),
            bytes_transferred: self.bytes_transferred,
            total_bytes: self.total_bytes,
            state: self.state.clone(),
            speed_bps,
            eta_ms,
            state_entered_at_ms: self.state_entered_at_ms,
        }
    }
}

/// HUD 状态机。线程不安全:由 emitter 用 `Mutex` 包起来访问。
pub struct ActivityHudState {
    clock: Arc<dyn Clock>,
    rows: HashMap<String, InternalRow>,
    /// `IncomingPending` 比 first Progress 早到时,文件名先缓存这里;
    /// Progress 到达时按 transfer_id 查询并 drain 到行。也支持反向:
    /// Progress 先到、IncomingPending 后到时,从这里查不到,落到
    /// `apply_incoming_pending` 的"行已存在则直接回填"分支。
    pending_filenames: HashMap<String, Vec<String>>,
    /// 单调递增,赋给每个新插入的 `InternalRow.insert_order`,决定
    /// snapshot 的稳定顺序。
    next_insert_order: u64,
}

impl ActivityHudState {
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        Self {
            clock,
            rows: HashMap::new(),
            pending_filenames: HashMap::new(),
            next_insert_order: 0,
        }
    }

    /// 应用一条 Progress 事件。返回 true 表示行集合或行内容有变化,调
    /// 用方应通知 listener 重绘;false 表示被静默丢弃(Sending 方向 /
    /// 终态行)。
    pub fn apply_progress(
        &mut self,
        transfer_id: &str,
        peer_id: &str,
        direction: FileTransferDirection,
        bytes_transferred: u64,
        total_bytes: Option<u64>,
    ) -> bool {
        if direction != FileTransferDirection::Receiving {
            return false;
        }
        let now_ms = self.clock.now_ms();

        if let Some(row) = self.rows.get_mut(transfer_id) {
            if row.state.is_terminal() {
                // 行已进入终态(可能是 publisher 乱序、或 sweep 还没到):
                // 不再倒退状态,也不再更新字节数。返回 false 让上层别
                // 因这条事件重绘。
                return false;
            }
            // CancelPending 也允许进度继续推进 —— 后端可能还在传几个字
            // 节才把 cancel 落地,UI 显示进度仍在涨 + "取消中..."文案比
            // 直接冻结进度更诚实。
            row.bytes_transferred = bytes_transferred;
            // total_bytes 在 buffered 阶段可能为 None,后续 progress 才补;
            // 一旦补上就锁定,后面不允许再倒退回 None。
            if total_bytes.is_some() {
                row.total_bytes = total_bytes;
            }
            push_speed_sample(&mut row.speed_window, now_ms, bytes_transferred);
            return true;
        }

        // first Progress:插入新行。文件名从 pending_filenames 拿(可能为空)。
        let filenames = self.pending_filenames.remove(transfer_id);
        let insert_order = self.next_insert_order;
        self.next_insert_order = self.next_insert_order.wrapping_add(1);
        let mut speed_window = VecDeque::new();
        push_speed_sample(&mut speed_window, now_ms, bytes_transferred);
        self.rows.insert(
            transfer_id.to_string(),
            InternalRow {
                transfer_id: transfer_id.to_string(),
                peer_id: peer_id.to_string(),
                filenames,
                bytes_transferred,
                total_bytes,
                state: RowState::Receiving,
                state_entered_at_ms: now_ms,
                speed_window,
                insert_order,
            },
        );
        true
    }

    /// 应用一条 StatusChanged 事件。终态(`completed` / `failed` /
    /// `cancelled`)会把对应行切到终态并记录进入时间。行不存在时返回
    /// false —— 出站 transfer 永远不会经由本状态机插入行,所以它们的
    /// StatusChanged 在这里被自然丢弃。
    pub fn apply_status_changed(
        &mut self,
        transfer_id: &str,
        status: &str,
        reason: Option<String>,
    ) -> bool {
        let now_ms = self.clock.now_ms();
        let Some(row) = self.rows.get_mut(transfer_id) else {
            return false;
        };
        let new_state = match status {
            "completed" => RowState::Completed,
            "failed" => RowState::Failed { reason },
            "cancelled" => RowState::Cancelled { reason },
            // "transferring" / "pending" 不在 HUD 关心范围内 —— 进度由
            // Progress 事件驱动,这里只用 StatusChanged 处理终态。
            _ => return false,
        };
        if row.state == new_state {
            return false;
        }
        row.state = new_state;
        row.state_entered_at_ms = now_ms;
        true
    }

    /// 应用一条 IncomingPending 事件,把文件名回填到行(若已存在),否
    /// 则缓存到 `pending_filenames` 等 first Progress 到达时再取。
    pub fn apply_incoming_pending(
        &mut self,
        transfer_id: &str,
        filenames: Vec<String>,
        total_bytes: Option<u64>,
    ) -> bool {
        if filenames.is_empty() && total_bytes.is_none() {
            return false;
        }
        if let Some(row) = self.rows.get_mut(transfer_id) {
            let mut changed = false;
            if !filenames.is_empty() && row.filenames.as_ref().map(Vec::is_empty).unwrap_or(true) {
                row.filenames = Some(filenames);
                changed = true;
            }
            if row.total_bytes.is_none() && total_bytes.is_some() {
                row.total_bytes = total_bytes;
                changed = true;
            }
            return changed;
        }
        // 行还不存在:把 filenames 缓存起来等 first Progress。total_bytes
        // 不缓存(IncomingPending 的 total 是 envelope 声明值,Progress 自
        // 己也会携带真实值);只缓存 IncomingPending 独有的 filenames。
        if !filenames.is_empty() {
            self.pending_filenames
                .insert(transfer_id.to_string(), filenames);
            return true;
        }
        false
    }

    /// 用户在 HUD 上点击了某行的取消按钮 —— 乐观把状态切到
    /// [`RowState::CancelPending`],UI 上立刻显示"取消中…"。真正的
    /// `Cancelled` 由后续 `StatusChanged: cancelled` 落地。
    ///
    /// 行不存在 / 已是终态时返回 false —— 调用方不该响应这次点击。
    pub fn mark_cancel_pending(&mut self, transfer_id: &str) -> bool {
        let now_ms = self.clock.now_ms();
        let Some(row) = self.rows.get_mut(transfer_id) else {
            return false;
        };
        if row.state.is_terminal() || matches!(row.state, RowState::CancelPending) {
            return false;
        }
        row.state = RowState::CancelPending;
        row.state_entered_at_ms = now_ms;
        true
    }

    /// 把过保留期的终态行扫掉。`completed_retain_ms` / `failed_retain_ms`
    /// 用模块常量 [`COMPLETED_RETAIN_MS`] / [`FAILED_RETAIN_MS`]。返回
    /// true 表示有行被移除。
    pub fn sweep(&mut self) -> bool {
        let now_ms = self.clock.now_ms();
        let before = self.rows.len();
        self.rows.retain(|_, row| {
            let retain_ms = match row.state {
                RowState::Receiving | RowState::CancelPending => return true,
                RowState::Completed => COMPLETED_RETAIN_MS,
                RowState::Failed { .. } | RowState::Cancelled { .. } => FAILED_RETAIN_MS,
            };
            now_ms.saturating_sub(row.state_entered_at_ms) < retain_ms
        });
        self.rows.len() != before
    }

    /// 返回当前所有行的快照,按插入顺序稳定排序。UI 应拿这份去重绘。
    pub fn snapshot(&self) -> Vec<ActivityHudRow> {
        let mut rows: Vec<_> = self.rows.values().collect();
        rows.sort_by_key(|r| r.insert_order);
        rows.iter().map(|r| r.snapshot()).collect()
    }

    /// 当前活跃行数(任何状态)。UI 用来判断"行数是否归零、是否该
    /// auto-hide"。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// 把 (timestamp, bytes) 加进滑窗,然后裁剪掉超出 `SPEED_WINDOW_MS` 的旧
/// 采样,并限制点数不超过 `SPEED_WINDOW_MAX_SAMPLES`。两个保护一起做避
/// 免 progress 事件突发时窗口无界增长。
fn push_speed_sample(window: &mut VecDeque<(u64, u64)>, now_ms: u64, bytes: u64) {
    window.push_back((now_ms, bytes));
    let cutoff = now_ms.saturating_sub(SPEED_WINDOW_MS);
    while window.len() > 1 {
        if let Some(&(t, _)) = window.front() {
            if t < cutoff {
                window.pop_front();
                continue;
            }
        }
        break;
    }
    while window.len() > SPEED_WINDOW_MAX_SAMPLES {
        window.pop_front();
    }
}

#[cfg(test)]
mod tests {
    use super::super::clock::ManualClock;
    use super::*;

    fn make_state() -> (ActivityHudState, Arc<ManualClock>) {
        let clock = Arc::new(ManualClock::new());
        let state = ActivityHudState::new(clock.clone() as Arc<dyn Clock>);
        (state, clock)
    }

    #[test]
    fn sending_progress_is_ignored() {
        let (mut state, _clock) = make_state();
        let changed = state.apply_progress(
            "t1",
            "peer-1",
            FileTransferDirection::Sending,
            100,
            Some(1000),
        );
        assert!(!changed);
        assert!(state.is_empty());
    }

    #[test]
    fn first_receiving_progress_inserts_row() {
        let (mut state, _clock) = make_state();
        let changed = state.apply_progress(
            "t1",
            "peer-1",
            FileTransferDirection::Receiving,
            100,
            Some(1000),
        );
        assert!(changed);
        assert_eq!(state.len(), 1);
        let snap = state.snapshot();
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0].transfer_id, "t1");
        assert_eq!(snap[0].peer_id, "peer-1");
        assert_eq!(snap[0].bytes_transferred, 100);
        assert_eq!(snap[0].total_bytes, Some(1000));
        assert_eq!(snap[0].state, RowState::Receiving);
        assert!(snap[0].filenames.is_none());
        assert!(snap[0].speed_bps.is_none(), "first sample 不足以算速度");
    }

    #[test]
    fn second_progress_updates_bytes_and_yields_speed() {
        let (mut state, clock) = make_state();
        state.apply_progress(
            "t1",
            "peer",
            FileTransferDirection::Receiving,
            0,
            Some(2_000_000),
        );
        clock.advance(1_000);
        state.apply_progress(
            "t1",
            "peer",
            FileTransferDirection::Receiving,
            500_000,
            Some(2_000_000),
        );
        let snap = state.snapshot();
        assert_eq!(snap[0].bytes_transferred, 500_000);
        // 1 秒里跑了 500_000 字节 -> ~500_000 B/s。
        let speed = snap[0].speed_bps.expect("应能算出速度");
        assert!((speed - 500_000.0).abs() < 1.0, "speed = {}", speed);
        // ETA = 剩余 1.5MB / 500KB/s ≈ 3000ms。
        let eta = snap[0].eta_ms.expect("应能算 ETA");
        assert!((eta as i64 - 3_000).abs() < 50, "eta = {}", eta);
    }

    #[test]
    fn incoming_pending_before_progress_backfills_filenames() {
        let (mut state, _clock) = make_state();
        let changed = state.apply_incoming_pending(
            "t1",
            vec!["report.pdf".into(), "notes.txt".into()],
            Some(2048),
        );
        assert!(changed);
        assert!(state.is_empty(), "行还不应被插入");
        // first progress 到了,从 pending_filenames drain 到新行。
        state.apply_progress(
            "t1",
            "peer",
            FileTransferDirection::Receiving,
            0,
            Some(2048),
        );
        let snap = state.snapshot();
        assert_eq!(
            snap[0].filenames,
            Some(vec!["report.pdf".into(), "notes.txt".into()])
        );
    }

    #[test]
    fn incoming_pending_after_progress_backfills_filenames() {
        let (mut state, _clock) = make_state();
        state.apply_progress("t1", "peer", FileTransferDirection::Receiving, 0, None);
        let snap = state.snapshot();
        assert!(snap[0].filenames.is_none());
        let changed = state.apply_incoming_pending("t1", vec!["late.txt".into()], Some(1024));
        assert!(changed);
        let snap = state.snapshot();
        assert_eq!(snap[0].filenames, Some(vec!["late.txt".into()]));
        // total_bytes 之前是 None,IncomingPending 回填后应该有值。
        assert_eq!(snap[0].total_bytes, Some(1024));
    }

    #[test]
    fn status_changed_for_unknown_transfer_is_dropped() {
        // 出站 transfer:从来没插过行,Cancelled 进来直接被丢弃。
        let (mut state, _clock) = make_state();
        let changed = state.apply_status_changed("t-outbound", "cancelled", None);
        assert!(!changed);
        assert!(state.is_empty());
    }

    #[test]
    fn completed_status_moves_row_to_terminal_and_sweep_removes_it() {
        let (mut state, clock) = make_state();
        state.apply_progress(
            "t1",
            "peer",
            FileTransferDirection::Receiving,
            100,
            Some(100),
        );
        state.apply_status_changed("t1", "completed", None);
        assert_eq!(state.snapshot()[0].state, RowState::Completed);

        // 保留期内 sweep 不会动它。
        clock.advance(COMPLETED_RETAIN_MS - 100);
        assert!(!state.sweep());
        assert_eq!(state.len(), 1);

        // 过了保留期就被扫掉。
        clock.advance(200);
        assert!(state.sweep());
        assert!(state.is_empty());
    }

    #[test]
    fn failed_retains_longer_than_completed() {
        let (mut state, clock) = make_state();
        state.apply_progress(
            "t1",
            "peer",
            FileTransferDirection::Receiving,
            50,
            Some(100),
        );
        state.apply_status_changed("t1", "failed", Some("disk_full".into()));
        clock.advance(COMPLETED_RETAIN_MS + 100);
        assert!(!state.sweep(), "Failed 比 Completed 保留更久");
        clock.advance(FAILED_RETAIN_MS - COMPLETED_RETAIN_MS + 100);
        assert!(state.sweep());
    }

    #[test]
    fn cancel_pending_then_cancelled_status() {
        let (mut state, _clock) = make_state();
        state.apply_progress(
            "t1",
            "peer",
            FileTransferDirection::Receiving,
            50,
            Some(100),
        );
        let acked = state.mark_cancel_pending("t1");
        assert!(acked);
        assert_eq!(state.snapshot()[0].state, RowState::CancelPending);
        // 后端真的把 cancel 落地了,推送 status_changed: cancelled。
        state.apply_status_changed("t1", "cancelled", Some("local_user".into()));
        assert!(matches!(
            state.snapshot()[0].state,
            RowState::Cancelled { .. }
        ));
    }

    #[test]
    fn mark_cancel_pending_on_terminal_is_noop() {
        let (mut state, _clock) = make_state();
        state.apply_progress(
            "t1",
            "peer",
            FileTransferDirection::Receiving,
            100,
            Some(100),
        );
        state.apply_status_changed("t1", "completed", None);
        let acked = state.mark_cancel_pending("t1");
        assert!(!acked, "已经 Completed 的行不允许再 CancelPending");
        assert_eq!(state.snapshot()[0].state, RowState::Completed);
    }

    #[test]
    fn progress_after_terminal_is_ignored() {
        let (mut state, _clock) = make_state();
        state.apply_progress(
            "t1",
            "peer",
            FileTransferDirection::Receiving,
            50,
            Some(100),
        );
        state.apply_status_changed("t1", "cancelled", None);
        // 后端取消生效前可能还有一条尾随 progress —— 不能让它把状态拉回 Receiving。
        let changed = state.apply_progress(
            "t1",
            "peer",
            FileTransferDirection::Receiving,
            60,
            Some(100),
        );
        assert!(!changed);
        assert!(matches!(
            state.snapshot()[0].state,
            RowState::Cancelled { .. }
        ));
    }

    #[test]
    fn snapshot_order_follows_insert_order() {
        let (mut state, _clock) = make_state();
        state.apply_progress("t1", "a", FileTransferDirection::Receiving, 0, Some(100));
        state.apply_progress("t2", "b", FileTransferDirection::Receiving, 0, Some(100));
        state.apply_progress("t3", "c", FileTransferDirection::Receiving, 0, Some(100));
        // 后续 progress 不应改变排序。
        state.apply_progress("t1", "a", FileTransferDirection::Receiving, 10, Some(100));
        let snap = state.snapshot();
        assert_eq!(
            snap.iter()
                .map(|r| r.transfer_id.as_str())
                .collect::<Vec<_>>(),
            vec!["t1", "t2", "t3"]
        );
    }

    #[test]
    fn speed_window_drops_old_samples() {
        let (mut state, clock) = make_state();
        state.apply_progress(
            "t1",
            "peer",
            FileTransferDirection::Receiving,
            0,
            Some(1_000_000),
        );
        // 推进超出窗口,补一个新采样;此时窗口里只应保留这个新点,
        // 旧的 (0ms, 0 bytes) 被淘汰 -> speed 又算不出来。
        clock.advance(SPEED_WINDOW_MS + 1_000);
        state.apply_progress(
            "t1",
            "peer",
            FileTransferDirection::Receiving,
            100,
            Some(1_000_000),
        );
        // 注意:被淘汰会留至少 1 个点(>1 才淘汰),所以这里两个点之间
        // 时差不会 > SPEED_WINDOW_MS;但首个点已经是新的 cutoff 之后的
        // 采样,所以是合法窗口。再加一个采样验证速度能算出。
        clock.advance(1_000);
        state.apply_progress(
            "t1",
            "peer",
            FileTransferDirection::Receiving,
            200,
            Some(1_000_000),
        );
        let snap = state.snapshot();
        let speed = snap[0].speed_bps.expect("窗口内仍有 2+ 采样");
        assert!(speed > 0.0);
    }

    #[test]
    fn duplicate_status_change_returns_false() {
        let (mut state, _clock) = make_state();
        state.apply_progress(
            "t1",
            "peer",
            FileTransferDirection::Receiving,
            100,
            Some(100),
        );
        assert!(state.apply_status_changed("t1", "completed", None));
        assert!(!state.apply_status_changed("t1", "completed", None));
    }
}
