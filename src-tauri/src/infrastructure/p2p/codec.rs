use async_trait::async_trait;
use futures::prelude::*;
use libp2p::{request_response::Codec, StreamProtocol};
use serde::{Deserialize, Serialize};

const PROTOCOL_NAME: &str = "/uniclipboard/1.0.0";

/// Request-response codec for pairing protocol
#[derive(Debug, Clone, Default)]
pub struct UniClipboardCodec;

/// Pairing request wrapper for request-response protocol
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingRequest {
    pub message: Vec<u8>,
}

/// Pairing response wrapper for request-response protocol
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingResponse {
    pub message: Vec<u8>,
}

#[async_trait]
impl Codec for UniClipboardCodec {
    type Protocol = StreamProtocol;
    type Request = PairingRequest;
    type Response = PairingResponse;

    async fn read_request<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
    ) -> std::io::Result<Self::Request>
    where
        T: futures::AsyncRead + Unpin + Send,
    {
        let mut buf = Vec::new();
        let mut limited = io.take(1024 * 64); // 64KB limit
        limited.read_to_end(&mut buf).await?;
        serde_json::from_slice(&buf)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    async fn read_response<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
    ) -> std::io::Result<Self::Response>
    where
        T: futures::AsyncRead + Unpin + Send,
    {
        let mut buf = Vec::new();
        let mut limited = io.take(1024 * 64); // 64KB limit
        limited.read_to_end(&mut buf).await?;
        serde_json::from_slice(&buf)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    async fn write_request<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
        req: Self::Request,
    ) -> std::io::Result<()>
    where
        T: futures::AsyncWrite + Unpin + Send,
    {
        let bytes = serde_json::to_vec(&req)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        io.write_all(&bytes).await?;
        io.close().await?;
        Ok(())
    }

    async fn write_response<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
        res: Self::Response,
    ) -> std::io::Result<()>
    where
        T: futures::AsyncWrite + Unpin + Send,
    {
        let bytes = serde_json::to_vec(&res)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        io.write_all(&bytes).await?;
        io.close().await?;
        Ok(())
    }
}
