use serde::Serialize;
use utoipa::ToSchema;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LocalDeviceInfoDto {
    pub peer_id: String,
    pub device_name: String,
}
