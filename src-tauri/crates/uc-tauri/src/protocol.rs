use tauri::http::{Request, StatusCode};
use uc_core::ids::RepresentationId;
use uc_core::BlobId;

/// # Behavior / 行为
///
/// Parsed UC protocol route.
/// 解析后的 UC 协议路由。
///
/// Represents the parsed resource type from a UC protocol URI (`uc://host/id`).
/// 表示从 UC 协议 URI (`uc://host/id`) 解析出的资源类型。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UcRoute {
    Blob { blob_id: BlobId },
    Thumbnail { representation_id: RepresentationId },
}

/// # Behavior / 行为
///
/// Errors when parsing UC protocol requests.
/// 解析 UC 协议请求的错误。
///
/// Represents validation failures during UC protocol URI parsing.
/// 表示 UC 协议 URI 解析过程中的验证失败。
#[derive(Debug, thiserror::Error)]
pub enum UcRequestError {
    #[error("Unsupported uc URI host")]
    UnsupportedHost,
    #[error("Missing resource id")]
    MissingId,
    #[error("Invalid resource id")]
    InvalidId,
}

impl UcRequestError {
    pub fn status_code(&self) -> StatusCode {
        StatusCode::BAD_REQUEST
    }

    pub fn response_message(&self) -> &'static str {
        match self {
            UcRequestError::UnsupportedHost => "Unsupported uc URI host",
            UcRequestError::MissingId => "Missing resource id",
            UcRequestError::InvalidId => "Invalid resource id",
        }
    }
}

/// # Behavior / 行为
///
/// Parse UC protocol request into route information.
/// 将 UC 协议请求解析为路由信息。
///
/// Extracts the host and resource ID from the URI and validates the format.
/// 从 URI 中提取主机和资源 ID 并验证格式。
///
/// # Examples / 示例
///
/// ```no_run
/// use tauri::http::Request;
/// use uc_tauri::protocol::parse_uc_request;
///
/// let request = Request::builder()
///     .uri("uc://blob/blob-123")
///     .body(Vec::new())
///     .unwrap();
///
/// let route = parse_uc_request(&request)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn parse_uc_request(request: &Request<Vec<u8>>) -> Result<UcRoute, UcRequestError> {
    let uri = request.uri();
    let host = uri.host().unwrap_or_default();
    let path = uri.path();

    // Two URL formats depending on platform:
    //   macOS/Linux (direct scheme):  uc://thumbnail/rep-1  → host="thumbnail", path="/rep-1"
    //   Windows (HTTP proxy):         http://uc.localhost/thumbnail/rep-1  → host="localhost", path="/thumbnail/rep-1"
    //   Frontend with convertFileSrc: uc://localhost/thumbnail/rep-1      → host="localhost", path="/thumbnail/rep-1"
    let (resource_type, resource_id) = if host == "localhost" || host == "uc.localhost" {
        // Windows / convertFileSrc format: resource type is the first path segment
        let trimmed = path.trim_start_matches('/');
        trimmed.split_once('/').ok_or(UcRequestError::MissingId)?
    } else {
        // macOS/Linux direct scheme format: resource type is the host
        let resource_id = path.trim_start_matches('/');
        if resource_id.is_empty() {
            return Err(UcRequestError::MissingId);
        }
        (host, resource_id)
    };

    if resource_id.is_empty() {
        return Err(UcRequestError::MissingId);
    }

    if resource_id.contains('/') {
        return Err(UcRequestError::InvalidId);
    }

    match resource_type {
        "blob" => Ok(UcRoute::Blob {
            blob_id: BlobId::from(resource_id),
        }),
        "thumbnail" => Ok(UcRoute::Thumbnail {
            representation_id: RepresentationId::from(resource_id),
        }),
        _ => Err(UcRequestError::UnsupportedHost),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_thumbnail_direct_scheme_format() {
        // macOS/Linux format: uc://thumbnail/rep-1 (host=thumbnail)
        let request = Request::builder()
            .uri("uc://thumbnail/rep-1")
            .body(Vec::new())
            .expect("build request");

        let route = parse_uc_request(&request).expect("expected uc request route");

        assert!(matches!(
            route,
            UcRoute::Thumbnail {
                representation_id,
            } if representation_id == RepresentationId::from("rep-1")
        ));
    }

    #[test]
    fn test_thumbnail_localhost_format() {
        // Frontend convertFileSrc format: uc://localhost/thumbnail/rep-1
        let request = Request::builder()
            .uri("uc://localhost/thumbnail/rep-1")
            .body(Vec::new())
            .expect("build request");

        let route = parse_uc_request(&request).expect("expected uc request route");

        assert!(matches!(
            route,
            UcRoute::Thumbnail {
                representation_id,
            } if representation_id == RepresentationId::from("rep-1")
        ));
    }

    #[test]
    fn test_blob_direct_scheme_format() {
        let request = Request::builder()
            .uri("uc://blob/blob-123")
            .body(Vec::new())
            .expect("build request");

        let route = parse_uc_request(&request).expect("expected uc request route");

        assert!(matches!(
            route,
            UcRoute::Blob {
                blob_id,
            } if blob_id == BlobId::from("blob-123")
        ));
    }

    #[test]
    fn test_blob_localhost_format() {
        let request = Request::builder()
            .uri("uc://localhost/blob/blob-123")
            .body(Vec::new())
            .expect("build request");

        let route = parse_uc_request(&request).expect("expected uc request route");

        assert!(matches!(
            route,
            UcRoute::Blob {
                blob_id,
            } if blob_id == BlobId::from("blob-123")
        ));
    }

    #[test]
    fn test_unsupported_host() {
        let request = Request::builder()
            .uri("uc://unknown/id-1")
            .body(Vec::new())
            .expect("build request");

        assert!(matches!(
            parse_uc_request(&request),
            Err(UcRequestError::UnsupportedHost)
        ));
    }

    #[test]
    fn test_missing_id_localhost_format() {
        let request = Request::builder()
            .uri("uc://localhost/thumbnail")
            .body(Vec::new())
            .expect("build request");

        assert!(parse_uc_request(&request).is_err());
    }
}
