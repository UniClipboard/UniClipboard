//! OpenAPI document routes and their Swagger UI configuration.

use std::sync::Arc;

use axum::body::Bytes;
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, MethodRouter};
use axum::Router;
use utoipa::{openapi::OpenApi as OpenApiDocument, OpenApi};
use utoipa_swagger_ui::{Config, SwaggerUi};

use super::openapi::ApiDoc;
#[cfg(debug_assertions)]
use super::ApiDocDev;

const PUBLIC_DOCUMENT_URL: &str = "/api-docs/openapi.json";
#[cfg(debug_assertions)]
const DEVELOPMENT_DOCUMENT_URL: &str = "/api-docs/openapi-dev.json";

pub(super) fn router<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    let routes = Router::new().route(PUBLIC_DOCUMENT_URL, document_route(ApiDoc::openapi()));
    #[cfg(debug_assertions)]
    let routes = routes.route(
        DEVELOPMENT_DOCUMENT_URL,
        document_route(ApiDocDev::openapi()),
    );

    #[cfg(debug_assertions)]
    let urls = [PUBLIC_DOCUMENT_URL, DEVELOPMENT_DOCUMENT_URL];
    #[cfg(not(debug_assertions))]
    let urls = [PUBLIC_DOCUMENT_URL];

    routes.merge(SwaggerUi::new("/swagger-ui").config(Config::new(urls)))
}

fn document_route<S>(document: OpenApiDocument) -> MethodRouter<S>
where
    S: Clone + Send + Sync + 'static,
{
    // Drop the schema tree after serialization; route and response clones share the bytes.
    let encoded = Arc::new(serde_json::to_vec(&document).map(Bytes::from));
    get(move || {
        let encoded = Arc::clone(&encoded);
        async move {
            match encoded.as_ref() {
                Ok(body) => {
                    ([(header::CONTENT_TYPE, "application/json")], body.clone()).into_response()
                }
                // Preserve Json's observable serialization failure response.
                Err(error) => {
                    (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response()
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use axum::body::{to_bytes, Body, Bytes};
    use axum::http::{header, HeaderMap, Method, Request, StatusCode};
    use futures_util::future::join_all;
    use serde_json::Value;
    use tower::ServiceExt;

    use super::*;

    async fn fetch(app: Router, path: &str, method: Method) -> (StatusCode, HeaderMap, Bytes) {
        let response = app
            .oneshot(
                Request::builder()
                    .uri(path)
                    .method(method)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let (parts, body) = response.into_parts();
        (
            parts.status,
            parts.headers,
            to_bytes(body, usize::MAX).await.unwrap(),
        )
    }

    #[tokio::test]
    async fn public_document_matches_the_generated_contract() {
        let (status, headers, body) = fetch(router(), "/api-docs/openapi.json", Method::GET).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers[header::CONTENT_TYPE], "application/json");
        assert_eq!(
            serde_json::from_slice::<Value>(&body).unwrap(),
            serde_json::to_value(ApiDoc::openapi()).unwrap()
        );
    }

    #[tokio::test]
    async fn cloned_routes_and_concurrent_requests_share_document_bytes() {
        let app = router();
        let responses =
            join_all((0..8).map(|_| fetch(app.clone(), "/api-docs/openapi.json", Method::GET)))
                .await;
        let first = &responses[0].2;
        assert!(!first.is_empty());
        for (status, _, body) in &responses {
            assert_eq!(*status, StatusCode::OK);
            assert_eq!(body, first);
            assert_eq!(
                body.as_ptr(),
                first.as_ptr(),
                "document body was allocated again"
            );
        }
    }

    #[tokio::test]
    async fn head_preserves_document_headers_without_a_body() {
        let app = router();
        let (_, get_headers, body) =
            fetch(app.clone(), "/api-docs/openapi.json", Method::GET).await;
        let (status, head_headers, head_body) =
            fetch(app, "/api-docs/openapi.json", Method::HEAD).await;
        assert_eq!(status, StatusCode::OK);
        assert!(head_body.is_empty());
        assert_eq!(
            head_headers[header::CONTENT_TYPE],
            get_headers[header::CONTENT_TYPE]
        );
        assert_eq!(head_headers[header::CONTENT_LENGTH], body.len().to_string());
    }

    #[tokio::test]
    async fn swagger_redirect_assets_and_document_urls_are_preserved() {
        let app = router();
        let (status, headers, _) = fetch(app.clone(), "/swagger-ui", Method::GET).await;
        assert_eq!(status, StatusCode::SEE_OTHER);
        assert_eq!(headers[header::LOCATION], "/swagger-ui/");
        let (status, headers, _) = fetch(app.clone(), "/swagger-ui/", Method::GET).await;
        assert_eq!(status, StatusCode::OK);
        assert!(headers[header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .contains("text/html"));
        let (status, _, body) = fetch(app, "/swagger-ui/swagger-initializer.js", Method::GET).await;
        assert_eq!(status, StatusCode::OK);
        let script = std::str::from_utf8(&body).unwrap();
        assert!(script.contains("/api-docs/openapi.json"));
        assert_eq!(
            script.contains("/api-docs/openapi-dev.json"),
            cfg!(debug_assertions)
        );
    }

    #[tokio::test]
    async fn development_document_follows_the_build_gate() {
        let (status, _, body) = fetch(router(), "/api-docs/openapi-dev.json", Method::GET).await;
        #[cfg(debug_assertions)]
        {
            use super::super::ApiDocDev;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(
                serde_json::from_slice::<Value>(&body).unwrap(),
                serde_json::to_value(ApiDocDev::openapi()).unwrap()
            );
        }
        #[cfg(not(debug_assertions))]
        {
            assert_eq!(status, StatusCode::NOT_FOUND);
            assert!(body.is_empty());
        }
    }
}
