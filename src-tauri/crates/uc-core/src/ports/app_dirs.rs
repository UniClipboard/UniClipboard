use crate::app_dirs::AppDirs;
use crate::ports::errors::AppDirsError;

pub trait AppDirsPort: Send + Sync {
    fn get_app_dirs(&self) -> Result<AppDirs, AppDirsError>;
}
