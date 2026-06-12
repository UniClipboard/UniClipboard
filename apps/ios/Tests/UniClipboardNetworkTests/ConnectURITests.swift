import Foundation
import Testing
@testable import UniClipboardNetwork

/// Golden vector from `docs/architecture/mobile-sync-connect-uri.md` §7.1.
/// MUST equal the string in `connect_uri.rs:GOLDEN_URI` and
/// `mobileSyncConnectUri.test.ts`. If any of the three sides drift, this
/// test (or its peers) breaks and the diff points at the offender.
private let goldenURI =
    "uniclipboard://connect?v=1&svc=mobile-sync&p=eyJ2IjoxLCJ1cmwiOiJodHRwOi8vMTkyLjE2OC4xLjU6NDI3MjAiLCJ1c2VyIjoibW9iaWxlX2FhYmJjY2RkIiwicHdkIjoiQWJDZEVmR2hJaktsTW5PcFFyU3QiLCJvIjp7ImRpZCI6ImRpZF8wMTIzYWJjZCIsImxhYmVsIjoiVGVzdCIsInByb3RvIjoic3luY2NsaXBib2FyZCJ9fQ"

/// base64url-no-pad a JSON payload into a full connect URI — mirrors the
/// inline encoding the negative-vector tests do by hand.
private func makeURI(_ jsonPayload: String) -> String {
    let p64 = Data(jsonPayload.utf8).base64EncodedString()
        .replacingOccurrences(of: "+", with: "-")
        .replacingOccurrences(of: "/", with: "_")
        .replacingOccurrences(of: "=", with: "")
    return "uniclipboard://connect?v=1&svc=mobile-sync&p=\(p64)"
}

@Test
func parsesTheGoldenVector() throws {
    let p = try ConnectURI.parse(goldenURI)
    #expect(p.url == "http://192.168.1.5:42720")
    #expect(p.user == "mobile_aabbccdd")
    #expect(p.pwd == "AbCdEfGhIjKlMnOpQrSt")
    #expect(p.deviceId == "did_0123abcd")
    #expect(p.label == "Test")
    #expect(p.proto == "syncclipboard")
}

@Test(arguments: [
    ("https://example.com/connect?v=1&svc=mobile-sync&p=eyJ2IjoxfQ",
        ConnectURI.ParseError.invalidScheme),
    ("uniclipboard://connect?v=2&svc=mobile-sync&p=eyJ2IjoxfQ",
        .unsupportedVersion(found: 2)),
    ("uniclipboard://connect?v=1&svc=other&p=eyJ2IjoxfQ",
        .unsupportedService(found: "other")),
    ("uniclipboard://connect?v=1&svc=mobile-sync&p=not-valid-base64!@",
        .payloadDecodeFailed(detail: "invalid base64url")),
])
func rejectsNegativeVectors(input: String, expected: ConnectURI.ParseError) {
    #expect(throws: expected) { try ConnectURI.parse(input) }
}

@Test
func rejectsMissingRequiredFields() throws {
    // payload = {"v":1,"url":"http://x","user":"u"} — pwd missing
    let payload = #"{"v":1,"url":"http://x","user":"u"}"#
    let p64 = Data(payload.utf8).base64EncodedString()
        .replacingOccurrences(of: "+", with: "-")
        .replacingOccurrences(of: "/", with: "_")
        .replacingOccurrences(of: "=", with: "")
    let uri = "uniclipboard://connect?v=1&svc=mobile-sync&p=\(p64)"
    #expect(throws: ConnectURI.ParseError.missingField(name: "pwd")) {
        try ConnectURI.parse(uri)
    }
}

@Test
func rejectsNonHTTPURL() throws {
    // payload = {"v":1,"url":"ftp://x","user":"u","pwd":"p"}
    let payload = #"{"v":1,"url":"ftp://x","user":"u","pwd":"p"}"#
    let p64 = Data(payload.utf8).base64EncodedString()
        .replacingOccurrences(of: "+", with: "-")
        .replacingOccurrences(of: "/", with: "_")
        .replacingOccurrences(of: "=", with: "")
    let uri = "uniclipboard://connect?v=1&svc=mobile-sync&p=\(p64)"
    #expect(throws: ConnectURI.ParseError.invalidURL(detail: "ftp://x")) {
        try ConnectURI.parse(uri)
    }
}

@Test
func dropsNonStringValuesInOtherDict() throws {
    // payload = {"v":1,"url":"http://x","user":"u","pwd":"p","o":{"label":"Hi","ttl":3600}}
    let payload =
        #"{"v":1,"url":"http://x","user":"u","pwd":"p","o":{"label":"Hi","ttl":3600}}"#
    let p64 = Data(payload.utf8).base64EncodedString()
        .replacingOccurrences(of: "+", with: "-")
        .replacingOccurrences(of: "/", with: "_")
        .replacingOccurrences(of: "=", with: "")
    let uri = "uniclipboard://connect?v=1&svc=mobile-sync&p=\(p64)"
    let p = try ConnectURI.parse(uri)
    #expect(p.label == "Hi")
    #expect(p.other["ttl"] == nil)   // numeric value silently dropped
    #expect(p.other.count == 1)
}

// MARK: - §4 urls candidate list

@Test
func parsesUrlsCandidateList() throws {
    let payload =
        #"{"v":1,"url":"https://wan.example.com","urls":["https://wan.example.com","http://192.168.1.5:42720","http://100.64.0.5:42720"],"user":"u","pwd":"p"}"#
    let p = try ConnectURI.parse(makeURI(payload))
    #expect(p.url == "https://wan.example.com")
    #expect(p.urls == [
        "https://wan.example.com",
        "http://192.168.1.5:42720",
        "http://100.64.0.5:42720",
    ])
}

@Test
func urlsFallsBackToSingleUrlWhenAbsent() throws {
    // The golden vector carries no `urls` (single-URL pairing); it must
    // surface as `[url]` so consumers always read a non-empty list.
    let p = try ConnectURI.parse(goldenURI)
    #expect(p.urls == ["http://192.168.1.5:42720"])
}

@Test
func dropsMalformedUrlsEntriesAndFallsBackIfAllDropped() throws {
    // Non-http(s) candidates are filtered out; the canonical `url` is unaffected.
    let mixed =
        #"{"v":1,"url":"https://ok.example.com","urls":["ftp://nope","https://ok.example.com","http://10.0.0.2"],"user":"u","pwd":"p"}"#
    #expect(try ConnectURI.parse(makeURI(mixed)).urls == ["https://ok.example.com", "http://10.0.0.2"])
    // An all-bad list collapses to `[url]` via the Payload fallback.
    let allBad =
        #"{"v":1,"url":"https://ok.example.com","urls":["ftp://nope"],"user":"u","pwd":"p"}"#
    #expect(try ConnectURI.parse(makeURI(allBad)).urls == ["https://ok.example.com"])
}
