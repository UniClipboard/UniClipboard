use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum DeviceStatus {
    Online = 0,
    Offline = 1,
    Unknown = 2,
    Recovering = 3,
}

impl PartialEq for DeviceStatus {
    fn eq(&self, other: &Self) -> bool {
        *self as i32 == *other as i32
    }
}

impl Eq for DeviceStatus {}

impl PartialOrd for DeviceStatus {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for DeviceStatus {
    fn cmp(&self, other: &Self) -> Ordering {
        (*self as i32).cmp(&(*other as i32))
    }
}

impl From<DeviceStatus> for i32 {
    fn from(status: DeviceStatus) -> Self {
        status as i32
    }
}

impl TryFrom<i32> for DeviceStatus {
    type Error = ();

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(DeviceStatus::Online),
            1 => Ok(DeviceStatus::Offline),
            2 => Ok(DeviceStatus::Unknown),
            3 => Ok(DeviceStatus::Recovering),
            _ => Err(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovering_variant_round_trips_through_i32() {
        let status = DeviceStatus::Recovering;
        let as_i32: i32 = status.into();
        assert_eq!(as_i32, 3);
        let back: DeviceStatus = DeviceStatus::try_from(3).expect("3 should decode");
        assert_eq!(back, DeviceStatus::Recovering);
    }

    #[test]
    fn recovering_orders_after_unknown() {
        // Ordering is by discriminant; Recovering must not silently collide
        // with existing variants.
        assert!(DeviceStatus::Online < DeviceStatus::Recovering);
        assert!(DeviceStatus::Offline < DeviceStatus::Recovering);
        assert!(DeviceStatus::Unknown < DeviceStatus::Recovering);
    }
}
