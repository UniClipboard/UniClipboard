use crate::ids::RepresentationId;
use std::fmt::Display;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionPolicyVersion {
    V1,
}

impl Display for SelectionPolicyVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SelectionPolicyVersion::V1 => write!(f, "v1"),
        }
    }
}

impl FromStr for SelectionPolicyVersion {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "v1" => Ok(SelectionPolicyVersion::V1),
            _ => Err(format!("Invalid SelectionPolicyVersion: {}", s)),
        }
    }
}

/// 选择目标：UI 预览 vs 默认粘贴
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionTarget {
    UiPreview,
    DefaultPaste,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardSelection {
    pub primary_rep_id: RepresentationId,
    pub secondary_rep_ids: Vec<RepresentationId>,
    pub preview_rep_id: RepresentationId,
    pub paste_rep_id: RepresentationId,
    pub policy_version: SelectionPolicyVersion,
}
