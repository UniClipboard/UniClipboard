//! Wire shapes and frame parsing for the mobile-sync SSE push channel
//! (`GET /api/sse/clipboard`).
//!
//! Single source of truth for the event names, JSON payload shapes, and the
//! heartbeat cadence shared by the daemon endpoint (`uc-webserver`, the
//! serializer) and the mobile client (`uc-mobile`, the deserializer). The
//! channel carries **signals only** — never clipboard content; every event
//! tells the client to pull `GET /SyncClipboard.json` unconditionally.
//!
//! Framing: the server writes each event as `event: <name>\n` +
//! `data: <json>\n` + a blank-line terminator, and heartbeats as a bare
//! comment line (`: ping`). [`find_frame_end`] and [`parse_sse_frame`]
//! implement the client side of exactly that subset of the SSE grammar
//! (single `data:` line, no `id:`/`retry:`, LF or CRLF line endings).

use serde::{Deserialize, Serialize};

/// SSE event name for the connect handshake frame.
pub const SSE_EVENT_HELLO: &str = "hello";
/// SSE event name for an active-clipboard register advance.
pub const SSE_EVENT_UPDATE: &str = "update";
/// SSE event name for "the server dropped signal(s); pull once to catch up".
pub const SSE_EVENT_RESYNC: &str = "resync";

/// Server heartbeat comment cadence in seconds. The server emits `: ping`
/// this often; the client treats "no bytes for 2× this" as a dead connection.
/// Both sides derive from this constant so the cadence cannot drift apart.
pub const SSE_HEARTBEAT_INTERVAL_SECS: u64 = 25;

/// Payload of the [`SSE_EVENT_HELLO`] frame sent once at connect time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SseHello {
    /// Server clock at connect time, Unix epoch milliseconds.
    pub server_time_ms: i64,
}

/// Payload of an [`SSE_EVENT_UPDATE`] frame: the active-clipboard register
/// advanced on the server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SseUpdate {
    /// The new register's `snapshot_hash` (`blake3v1:<hex>`), opaque to the
    /// client — a signal to pull, never content to apply.
    pub content_id: String,
    /// The register's activation timestamp, Unix epoch milliseconds.
    pub server_time_ms: i64,
}

/// Payload of an [`SSE_EVENT_RESYNC`] frame: the connection's broadcast
/// receiver lagged on the server, so update(s) may have been dropped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SseResync {
    /// Server clock when the lag was detected, Unix epoch milliseconds.
    pub server_time_ms: i64,
}

/// One parsed frame of the channel, as the client consumes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SseEvent {
    Hello(SseHello),
    Update(SseUpdate),
    /// A resync signal. The payload's `server_time_ms` is diagnostic only,
    /// so a resync frame with a missing or malformed payload still parses —
    /// the client's reaction (pull once) does not depend on it.
    Resync,
}

/// Byte offset just past the first frame terminator (a blank line: `\n\n` or
/// `\r\n\r\n`), or `None` if `buf` does not yet contain a complete frame.
pub fn find_frame_end(buf: &[u8]) -> Option<usize> {
    let lf = buf.windows(2).position(|w| w == b"\n\n").map(|i| i + 2);
    let crlf = buf.windows(4).position(|w| w == b"\r\n\r\n").map(|i| i + 4);
    match (lf, crlf) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

/// Parse one complete frame (with or without its trailing blank line) into an
/// [`SseEvent`]. Returns `None` for a pure heartbeat comment frame (`: ping`),
/// an unknown event name, or a hello/update payload that fails to decode —
/// all of which the client ignores by design.
pub fn parse_sse_frame(frame: &str) -> Option<SseEvent> {
    let mut event_name: Option<&str> = None;
    let mut data: Option<&str> = None;
    for line in frame.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.is_empty() || line.starts_with(':') {
            continue; // blank or comment line (heartbeat) — nothing to parse
        }
        let Some(colon) = line.find(':') else {
            continue;
        };
        let (name, rest) = line.split_at(colon);
        let value = rest[1..].strip_prefix(' ').unwrap_or(&rest[1..]);
        match name {
            "event" => event_name = Some(value),
            "data" => data = Some(value),
            _ => {}
        }
    }
    match (event_name, data) {
        (Some(SSE_EVENT_HELLO), Some(data)) => serde_json::from_str::<SseHello>(data)
            .ok()
            .map(SseEvent::Hello),
        (Some(SSE_EVENT_UPDATE), Some(data)) => serde_json::from_str::<SseUpdate>(data)
            .ok()
            .map(SseEvent::Update),
        (Some(SSE_EVENT_RESYNC), _) => Some(SseEvent::Resync),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_frame_end_lf_and_crlf() {
        assert_eq!(find_frame_end(b"event: hello\ndata: {}\n\nrest"), Some(23));
        assert_eq!(
            find_frame_end(b"event: hello\r\ndata: {}\r\n\r\nrest"),
            Some(26)
        );
        assert_eq!(find_frame_end(b"event: hello\ndata: {}"), None);
        assert_eq!(find_frame_end(b""), None);
    }

    #[test]
    fn find_frame_end_picks_the_earliest_terminator() {
        // An LF frame followed by a CRLF frame: the first frame's end wins.
        assert_eq!(find_frame_end(b"a\n\nb\r\n\r\n"), Some(3));
        // And the other way around.
        assert_eq!(find_frame_end(b"a\r\n\r\nb\n\n"), Some(5));
    }

    #[test]
    fn parses_hello() {
        let frame = "event: hello\ndata: {\"server_time_ms\":42}";
        assert_eq!(
            parse_sse_frame(frame),
            Some(SseEvent::Hello(SseHello { server_time_ms: 42 }))
        );
    }

    #[test]
    fn parses_update() {
        let frame = "event: update\ndata: {\"content_id\":\"blake3v1:aa\",\"server_time_ms\":7}";
        assert_eq!(
            parse_sse_frame(frame),
            Some(SseEvent::Update(SseUpdate {
                content_id: "blake3v1:aa".into(),
                server_time_ms: 7,
            }))
        );
    }

    #[test]
    fn parses_resync_even_with_malformed_payload() {
        assert_eq!(
            parse_sse_frame("event: resync\ndata: {\"server_time_ms\":1}"),
            Some(SseEvent::Resync)
        );
        assert_eq!(
            parse_sse_frame("event: resync\ndata: garbage"),
            Some(SseEvent::Resync)
        );
        assert_eq!(parse_sse_frame("event: resync"), Some(SseEvent::Resync));
    }

    #[test]
    fn crlf_line_endings_parse_identically() {
        let frame = "event: update\r\ndata: {\"content_id\":\"x\",\"server_time_ms\":1}\r\n";
        assert_eq!(
            parse_sse_frame(frame),
            Some(SseEvent::Update(SseUpdate {
                content_id: "x".into(),
                server_time_ms: 1,
            }))
        );
    }

    #[test]
    fn heartbeat_comment_and_unknown_events_parse_to_none() {
        assert_eq!(parse_sse_frame(": ping"), None);
        assert_eq!(parse_sse_frame("event: nonsense\ndata: {}"), None);
        assert_eq!(parse_sse_frame("data: {\"server_time_ms\":1}"), None);
    }

    #[test]
    fn malformed_hello_and_update_payloads_parse_to_none() {
        assert_eq!(parse_sse_frame("event: hello\ndata: garbage"), None);
        assert_eq!(parse_sse_frame("event: update\ndata: {}"), None);
        assert_eq!(parse_sse_frame("event: hello"), None);
    }

    #[test]
    fn round_trip_through_serde_matches_the_server_serializer() {
        // The server writes `data: <serde_json::to_string(payload)>`; feed
        // that exact byte shape back through the parser.
        let update = SseUpdate {
            content_id: "blake3v1:bb".into(),
            server_time_ms: 99,
        };
        let frame = format!(
            "event: update\ndata: {}\n\n",
            serde_json::to_string(&update).unwrap()
        );
        assert_eq!(parse_sse_frame(&frame), Some(SseEvent::Update(update)));
    }
}
