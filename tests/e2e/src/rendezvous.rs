use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

type TicketVault = Arc<Mutex<HashMap<String, String>>>;

pub struct LocalRendezvous {
    server: MockServer,
}

impl LocalRendezvous {
    pub async fn start() -> Self {
        let server = MockServer::start().await;
        let tickets = TicketVault::default();
        let counter = Arc::new(AtomicU32::new(1));

        Mock::given(method("POST"))
            .and(path("/v1/pairings"))
            .respond_with(CreatePairing {
                tickets: Arc::clone(&tickets),
                counter,
            })
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/pairings/resolve"))
            .respond_with(ResolvePairing {
                tickets: Arc::clone(&tickets),
            })
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/pairings/consume"))
            .respond_with(ConsumePairing { tickets })
            .mount(&server)
            .await;

        Self { server }
    }

    pub fn uri(&self) -> String {
        self.server.uri()
    }
}

struct CreatePairing {
    tickets: TicketVault,
    counter: Arc<AtomicU32>,
}

impl Respond for CreatePairing {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let body: serde_json::Value = match serde_json::from_slice(&request.body) {
            Ok(body) => body,
            Err(_) => return ResponseTemplate::new(400),
        };
        let Some(ticket) = body.get("sponsorTicket").and_then(|value| value.as_str()) else {
            return ResponseTemplate::new(400);
        };
        let sequence = self.counter.fetch_add(1, Ordering::Relaxed);
        let code = format!("E2E0-{sequence:04}");
        self.tickets
            .lock()
            .expect("ticket vault lock")
            .insert(code.clone(), ticket.to_string());
        ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "code": code,
            "expiresAtMs": 4_102_444_800_000_i64
        }))
    }
}

struct ResolvePairing {
    tickets: TicketVault,
}

impl Respond for ResolvePairing {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let code = request_code(request);
        let ticket = code.and_then(|code| {
            self.tickets
                .lock()
                .expect("ticket vault lock")
                .get(&code)
                .cloned()
        });
        match ticket {
            Some(ticket) => ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "sponsorTicket": ticket,
                "sponsorEndpointId": "ignored",
                "expiresAtMs": 4_102_444_800_000_i64
            })),
            None => ResponseTemplate::new(404),
        }
    }
}

struct ConsumePairing {
    tickets: TicketVault,
}

impl Respond for ConsumePairing {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        if let Some(code) = request_code(request) {
            self.tickets
                .lock()
                .expect("ticket vault lock")
                .remove(&code);
        }
        ResponseTemplate::new(204)
    }
}

fn request_code(request: &Request) -> Option<String> {
    serde_json::from_slice::<serde_json::Value>(&request.body)
        .ok()?
        .get("code")?
        .as_str()
        .map(str::to_string)
}
