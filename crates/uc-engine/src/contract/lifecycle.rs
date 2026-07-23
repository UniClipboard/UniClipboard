use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineState {
    Running,
    Quiescing,
    Quiesced,
    Suspended,
    ShuttingDown,
    Stopped,
}

impl EngineState {
    pub fn accepts_operations(self) -> bool {
        self == Self::Running
    }

    pub fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Running, Self::Quiescing)
                | (Self::Quiescing, Self::Quiesced)
                | (Self::Quiesced, Self::Suspended)
                | (Self::Suspended, Self::Running)
                | (Self::Running, Self::ShuttingDown)
                | (Self::Quiescing, Self::ShuttingDown)
                | (Self::Quiesced, Self::ShuttingDown)
                | (Self::Suspended, Self::ShuttingDown)
                | (Self::ShuttingDown, Self::Stopped)
        )
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleAction {
    Suspend,
    Resume,
}
