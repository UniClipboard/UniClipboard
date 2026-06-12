use crate::DeviceId;

pub trait DeviceIdentityPort: Send + Sync {
    fn current_device_id(&self) -> DeviceId;
}
