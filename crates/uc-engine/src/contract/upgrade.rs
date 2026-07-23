use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum UpgradeStatusSummary {
    FreshInstall { current: String },
    NoChange { current: String },
    Upgraded { from: Option<String>, to: String },
    Downgraded { from: String, to: String },
}
