import XCTest
import UniClipboardModels
@testable import UniClipboardNetwork

/// §5.3 multi-URL reachability probe. The handler routes on the request's
/// host so one MockURLProtocol session can answer differently per
/// candidate — probes run concurrently, so the handler must stay a pure
/// function of the request.
final class ConnectionTesterProbeTests: XCTestCase {

    override func tearDown() {
        MockURLProtocol.reset()
        super.tearDown()
    }

    private func probe(
        _ urls: [String],
        username: String = "u",
        password: String = "p"
    ) async -> [String: ConnectionTester.Result] {
        await ConnectionTester.probe(
            urls: urls,
            username: username,
            password: password,
            trustInsecureCert: false,
            session: MockURLProtocol.session()
        )
    }

    private static func status(_ code: Int, for req: URLRequest) -> (HTTPURLResponse, Data?) {
        let resp = HTTPURLResponse(
            url: req.url!, statusCode: code, httpVersion: "HTTP/1.1", headerFields: nil
        )!
        return (resp, nil)
    }

    // MARK: - Per-URL status mapping

    func test_probe_mapsStatusPerCandidate() async {
        MockURLProtocol.handler = { req in
            switch req.url?.host {
            case "ok.example":       return Self.status(200, for: req)
            case "empty.example":    return Self.status(404, for: req)
            case "badauth.example":  return Self.status(401, for: req)
            case "broken.example":   return Self.status(500, for: req)
            case "gone.example":     throw URLError(.cannotConnectToHost)
            default:                 throw URLError(.badURL)
            }
        }
        let results = await probe([
            "https://ok.example",
            "https://empty.example",
            "https://badauth.example",
            "https://broken.example",
            "https://gone.example",
        ])
        XCTAssertEqual(results["https://ok.example"], .success)
        XCTAssertEqual(results["https://empty.example"], .success)      // 404 = reachable (§2.1)
        XCTAssertEqual(results["https://badauth.example"], .authFailed) // 401 = reachable, creds wrong
        XCTAssertEqual(results["https://broken.example"], .unreachable)
        XCTAssertEqual(results["https://gone.example"], .unreachable)
        XCTAssertEqual(results.count, 5)
    }

    func test_probe_requestTargetsSyncClipboardJSONWithBasicAuth() async {
        MockURLProtocol.handler = { req in
            XCTAssertEqual(req.url?.absoluteString, "https://nas.example:5033/sub/SyncClipboard.json")
            XCTAssertEqual(req.httpMethod, "GET")
            XCTAssertEqual(
                req.value(forHTTPHeaderField: "Authorization"),
                "Basic " + Data("u:p".utf8).base64EncodedString()
            )
            return Self.status(200, for: req)
        }
        let results = await probe(["https://nas.example:5033/sub"])
        XCTAssertEqual(results["https://nas.example:5033/sub"], .success)
    }

    // MARK: - Degenerate inputs

    func test_probe_emptyCredentials_allMissingFields_withoutNetwork() async {
        // No handler installed: a network attempt would surface as
        // .unreachable, so .missingFields proves we never issued one.
        let results = await probe(["https://a.example", "https://b.example"], password: "")
        XCTAssertEqual(results["https://a.example"], .missingFields)
        XCTAssertEqual(results["https://b.example"], .missingFields)
    }

    func test_probe_emptyList_returnsEmpty() async {
        let results = await probe([])
        XCTAssertTrue(results.isEmpty)
    }

    func test_probe_malformedURL_isUnreachable_blankURL_isMissingFields() async {
        MockURLProtocol.handler = { req in Self.status(200, for: req) }
        let results = await probe(["not-a-url", "   "])
        XCTAssertEqual(results["not-a-url"], .unreachable)
        XCTAssertEqual(results["   "], .missingFields)
    }

    func test_probe_dedupesRepeatedCandidates() async {
        MockURLProtocol.handler = { req in Self.status(200, for: req) }
        let results = await probe(["https://a.example", "https://a.example"])
        XCTAssertEqual(results, ["https://a.example": .success])
    }

    // MARK: - firstReachable pick (§5.3: first reachable in shape order)

    func test_firstReachable_skipsUnreachableHead() {
        let ordered = ["https://lan.example", "https://ts.example", "https://wan.example"]
        let results: [String: ConnectionTester.Result] = [
            "https://lan.example": .unreachable,
            "https://ts.example": .success,
            "https://wan.example": .success,
        ]
        XCTAssertEqual(
            ConnectionTester.firstReachable(in: ordered, results: results),
            "https://ts.example"
        )
    }

    func test_firstReachable_authFailedCountsAsReachable() {
        let ordered = ["https://lan.example", "https://wan.example"]
        let results: [String: ConnectionTester.Result] = [
            "https://lan.example": .authFailed,
            "https://wan.example": .success,
        ]
        XCTAssertEqual(
            ConnectionTester.firstReachable(in: ordered, results: results),
            "https://lan.example"
        )
    }

    func test_firstReachable_orderDecidesWhenBothReachable() {
        let ordered = ["https://lan.example", "https://wan.example"]
        let results: [String: ConnectionTester.Result] = [
            "https://lan.example": .success,
            "https://wan.example": .success,
        ]
        XCTAssertEqual(
            ConnectionTester.firstReachable(in: ordered, results: results),
            "https://lan.example"
        )
    }

    func test_firstReachable_nilWhenNothingReachable() {
        let ordered = ["https://lan.example", "https://wan.example"]
        let results: [String: ConnectionTester.Result] = [
            "https://lan.example": .unreachable,
            "https://wan.example": .missingFields,
        ]
        XCTAssertNil(ConnectionTester.firstReachable(in: ordered, results: results))
    }

    func test_firstReachable_missingProbeEntryIsNotReachable() {
        // A URL the probe never saw (e.g. filtered out upstream) must not
        // be picked just because the dictionary lookup is nil.
        XCTAssertNil(
            ConnectionTester.firstReachable(
                in: ["https://lan.example"],
                results: [:]
            )
        )
    }

    // MARK: - End-to-end pick over a probed candidate set

    func test_probeThenPick_choosesFirstReachableInShapeOrder() async {
        MockURLProtocol.handler = { req in
            switch req.url?.host {
            case "192.168.1.9":     throw URLError(.cannotConnectToHost) // LAN down
            case "host.ts.net":     return Self.status(404, for: req)    // TS up, empty
            case "wan.example":     return Self.status(200, for: req)
            default:                throw URLError(.badURL)
            }
        }
        let config = ServerConfig(
            id: "c1",
            urls: ["https://wan.example", "http://192.168.1.9:5033", "https://host.ts.net"],
            username: "u",
            password: "p"
        )
        let ordered = config.orderedURLs(network: NetworkContext(isWifi: true))
        XCTAssertEqual(
            ordered,
            ["http://192.168.1.9:5033", "https://host.ts.net", "https://wan.example"]
        )
        let results = await probe(config.urls)
        XCTAssertEqual(
            ConnectionTester.firstReachable(in: ordered, results: results),
            "https://host.ts.net"
        )
    }
}
