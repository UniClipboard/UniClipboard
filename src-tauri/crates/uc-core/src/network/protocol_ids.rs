#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolId {
    Pairing,
    PairingStream,
    Business,
    FileTransfer,
    FileTransferV2,
}

impl ProtocolId {
    pub const fn as_str(&self) -> &'static str {
        match self {
            ProtocolId::Pairing => "/uc-pairing/1.0.0",
            ProtocolId::PairingStream => "/uniclipboard/pairing-stream/1.0.0",
            ProtocolId::Business => "/uniclipboard/business/1.0.0",
            ProtocolId::FileTransfer => "/uniclipboard/file-transfer/1.0.0",
            ProtocolId::FileTransferV2 => "/uniclipboard/file-transfer/2.0.0",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ProtocolId;

    #[test]
    fn protocol_id_strings_match_expected_values() {
        assert_eq!(ProtocolId::Pairing.as_str(), "/uc-pairing/1.0.0");
        assert_eq!(
            ProtocolId::PairingStream.as_str(),
            "/uniclipboard/pairing-stream/1.0.0"
        );
        assert_eq!(
            ProtocolId::Business.as_str(),
            "/uniclipboard/business/1.0.0"
        );
        assert_eq!(
            ProtocolId::FileTransfer.as_str(),
            "/uniclipboard/file-transfer/1.0.0"
        );
        assert_eq!(
            ProtocolId::FileTransferV2.as_str(),
            "/uniclipboard/file-transfer/2.0.0"
        );
    }
}
