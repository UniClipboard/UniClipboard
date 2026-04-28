//! [`UpgradeStatus`] —— 启动期版本游标比较的结构化结果。

/// 启动期一次性的"上次运行版本 vs 当前版本"判定结果。
///
/// 设计准则：
/// * 模块只负责给出"从哪到哪"，不嵌入产品策略。
/// * `Upgraded.from = None` 表示游标缺失或解析失败（典型为"P1 之前的老用户"），
///   消费者通常与 `Upgraded.from = Some(_)` 同等对待，需要时再细分。
/// * 不区分 alpha/beta/stable 等通道；通道是消费者决策的范畴。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpgradeStatus {
    /// 全新安装：游标缺失 **且** 没有 setup 痕迹。消费者应跳过任何
    /// 升级引导。
    FreshInstall,

    /// 检测到升级。`from = None` 表示游标缺失或损坏（按"非 fresh 即老用户"
    /// 策略归类到此）；`from = Some(_)` 表示读到了合法旧游标。
    Upgraded {
        from: Option<semver::Version>,
        to: semver::Version,
    },

    /// 游标版本与当前版本一致，无需任何升级动作。
    NoChange,

    /// 检测到回滚（游标版本 > 当前版本）。P1 不主动处理，仅记录；
    /// 消费者可决定是否警告。
    Downgraded {
        from: semver::Version,
        to: semver::Version,
    },
}
