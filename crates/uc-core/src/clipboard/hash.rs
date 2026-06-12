use serde::{Deserialize, Serialize};
use std::fmt;
use std::hash::Hash;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
pub enum HashAlgorithm {
    Blake3V1,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ContentHash {
    pub alg: HashAlgorithm,
    pub bytes: [u8; 32],
}

impl fmt::Display for ContentHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let alg = match self.alg {
            HashAlgorithm::Blake3V1 => "blake3v1",
        };

        write!(f, "{}:{}", alg, hex::encode(self.bytes))
    }
}

impl From<String> for ContentHash {
    fn from(s: String) -> Self {
        // Parse the string format "algorithm:hex_bytes"
        if let Some((alg_part, hex_part)) = s.split_once(':') {
            let alg = match alg_part {
                "blake3v1" => HashAlgorithm::Blake3V1,
                _ => panic!("unsupported hash algorithm: {}", alg_part),
            };

            let bytes = hex::decode(hex_part)
                .expect("invalid hex encoding for content hash")
                .try_into()
                .expect("invalid byte length for content hash");

            Self { alg, bytes }
        } else {
            panic!("invalid content hash format: {}", s);
        }
    }
}

impl From<&str> for ContentHash {
    fn from(s: &str) -> Self {
        s.to_string().into()
    }
}

// from bytes (array reference)
impl From<&[u8; 32]> for ContentHash {
    fn from(bytes: &[u8; 32]) -> Self {
        let alg = HashAlgorithm::Blake3V1;
        Self { alg, bytes: *bytes }
    }
}

// from bytes (slice)
impl From<&[u8]> for ContentHash {
    fn from(bytes: &[u8]) -> Self {
        let alg = HashAlgorithm::Blake3V1;
        let bytes = bytes
            .try_into()
            .expect("invalid byte length for content hash");
        Self { alg, bytes }
    }
}

// as bytes
impl AsRef<[u8]> for ContentHash {
    fn as_ref(&self) -> &[u8] {
        &self.bytes
    }
}
