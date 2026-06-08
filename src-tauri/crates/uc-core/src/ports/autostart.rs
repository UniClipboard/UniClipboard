use anyhow::Result;

pub trait AutostartPort: Send + Sync {
    fn is_enabled(&self) -> Result<bool>;
    fn enable(&self) -> Result<()>;
    fn disable(&self) -> Result<()>;
}
