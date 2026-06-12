/// 策略错误：v1 只在完全没有可用 representation 时失败
#[derive(Debug, thiserror::Error)]
pub enum PolicyError {
    #[error("no usable representations in snapshot")]
    NoUsableRepresentation,
}
