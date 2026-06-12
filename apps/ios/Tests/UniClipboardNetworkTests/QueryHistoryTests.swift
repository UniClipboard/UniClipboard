import XCTest
import UniClipboardModels
@testable import UniClipboardNetwork

/// Tests for `SyncClipboardClient.queryHistory` (§2.7) and the
/// `HistoryQuery` multipart-encoding contract that backs it.
final class QueryHistoryTests: XCTestCase {

    override func tearDown() {
        MockURLProtocol.reset()
        super.tearDown()
    }

    // MARK: - HistoryQuery encoding (no network)

    func test_historyQuery_emptyEncodesToClosingDelimiterOnly() {
        let q = HistoryQuery()
        let body = q.multipartEncoded(boundary: "BND")
        // No fields → multipart body is just the trailing boundary.
        let encoded = String(data: body.encoded(), encoding: .utf8)
        XCTAssertEqual(encoded, "--BND--\r\n")
    }

    func test_historyQuery_pageOnlyEncodesAsIntString() {
        let q = HistoryQuery(page: 3)
        let body = q.multipartEncoded(boundary: "BND")
        let encoded = String(data: body.encoded(), encoding: .utf8)!
        XCTAssertTrue(encoded.contains("name=\"page\"\r\n\r\n3\r\n"))
    }

    func test_historyQuery_typesEncodesBitmaskAsString() {
        let q = HistoryQuery(types: HistoryQuery.TypeMask.all)
        let body = q.multipartEncoded(boundary: "BND")
        let encoded = String(data: body.encoded(), encoding: .utf8)!
        XCTAssertTrue(encoded.contains("name=\"types\"\r\n\r\n15\r\n"))
    }

    func test_historyQuery_starredEncodesAsTrueFalseString() {
        let trueQuery = HistoryQuery(starred: true)
        let falseQuery = HistoryQuery(starred: false)
        let trueEncoded = String(data: trueQuery.multipartEncoded(boundary: "B").encoded(), encoding: .utf8)!
        let falseEncoded = String(data: falseQuery.multipartEncoded(boundary: "B").encoded(), encoding: .utf8)!
        XCTAssertTrue(trueEncoded.contains(#"name="starred""# + "\r\n\r\ntrue\r\n"))
        XCTAssertTrue(falseEncoded.contains(#"name="starred""# + "\r\n\r\nfalse\r\n"))
    }

    func test_historyQuery_modifiedAfterEncodesAsFractionalISO() {
        // Build the Date from an ISO-8601 string so fractional seconds
        // are exact (Date(timeIntervalSince1970:) round-trips imprecisely).
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        let date = f.date(from: "2026-05-17T16:43:21.420Z")!
        let q = HistoryQuery(modifiedAfter: date)
        let encoded = String(data: q.multipartEncoded(boundary: "B").encoded(), encoding: .utf8)!
        XCTAssertTrue(encoded.contains("name=\"modifiedAfter\"\r\n\r\n2026-05-17T16:43:21.420Z\r\n"),
                      "modifiedAfter must be ISO-8601 with fractional seconds and Z suffix.\nGot: \(encoded)")
    }

    func test_historyQuery_omitsNilFields() {
        let q = HistoryQuery(page: 1)
        let encoded = String(data: q.multipartEncoded(boundary: "B").encoded(), encoding: .utf8)!
        XCTAssertFalse(encoded.contains("name=\"modifiedAfter\""))
        XCTAssertFalse(encoded.contains("name=\"types\""))
        XCTAssertFalse(encoded.contains("name=\"starred\""))
        XCTAssertFalse(encoded.contains("name=\"searchText\""))
    }

    // MARK: - queryHistory request shape

    func test_queryHistory_usesPOSTAtApiHistoryQueryPath() async throws {
        MockURLProtocol.handler = { req in
            let resp = HTTPURLResponse(url: req.url!, statusCode: 200, httpVersion: nil, headerFields: nil)!
            return (resp, Data("[]".utf8))
        }
        let client = try makeClient(baseURLString: "https://nas.local:5033/")
        _ = try await client.queryHistory()

        XCTAssertEqual(MockURLProtocol.lastRequest?.httpMethod, "POST")
        XCTAssertEqual(MockURLProtocol.lastRequest?.url?.absoluteString,
                       "https://nas.local:5033/api/history/query")
    }

    func test_queryHistory_setsAuthAndMultipartContentType() async throws {
        MockURLProtocol.handler = { req in
            let resp = HTTPURLResponse(url: req.url!, statusCode: 200, httpVersion: nil, headerFields: nil)!
            return (resp, Data("[]".utf8))
        }
        let client = try makeClient(username: "alice", password: "secret")
        _ = try await client.queryHistory(HistoryQuery(page: 1))

        let auth = MockURLProtocol.lastRequest?.value(forHTTPHeaderField: "Authorization")
        let ct = MockURLProtocol.lastRequest?.value(forHTTPHeaderField: "Content-Type") ?? ""
        XCTAssertEqual(auth, "Basic YWxpY2U6c2VjcmV0")
        XCTAssertTrue(ct.hasPrefix("multipart/form-data; boundary="),
                      "Content-Type must announce multipart with a boundary, got: \(ct)")
    }

    func test_queryHistory_bodyContainsRequestedFilters() async throws {
        MockURLProtocol.handler = { req in
            let resp = HTTPURLResponse(url: req.url!, statusCode: 200, httpVersion: nil, headerFields: nil)!
            return (resp, Data("[]".utf8))
        }
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        try await makeClient().queryHistory(HistoryQuery(
            page: 2,
            modifiedAfter: f.date(from: "2026-05-17T16:43:21.420Z")!,
            types: HistoryQuery.TypeMask.text | HistoryQuery.TypeMask.image,
            starred: true
        ))
        let raw = try XCTUnwrap(MockURLProtocol.lastBody)
        let body = String(data: raw, encoding: .utf8) ?? ""
        XCTAssertTrue(body.contains("name=\"page\"\r\n\r\n2\r\n"))
        XCTAssertTrue(body.contains("name=\"modifiedAfter\"\r\n\r\n2026-05-17T16:43:21.420Z\r\n"))
        XCTAssertTrue(body.contains("name=\"types\"\r\n\r\n3\r\n"))      // 1 | 2 = 3
        XCTAssertTrue(body.contains("name=\"starred\"\r\n\r\ntrue\r\n"))
    }

    // MARK: - response parsing

    func test_queryHistory_decodesArrayOfRecords() async throws {
        let payload: [[String: Any]] = [
            ["hash": "AA", "type": "Text", "text": "first", "size": 5, "isDeleted": false],
            ["hash": "BB", "type": "Image", "hasData": true, "size": 1024, "createTime": "2026-05-17T10:00:00Z"],
        ]
        MockURLProtocol.handler = { req in
            let body = try JSONSerialization.data(withJSONObject: payload)
            let resp = HTTPURLResponse(url: req.url!, statusCode: 200, httpVersion: nil, headerFields: nil)!
            return (resp, body)
        }
        let records = try await makeClient().queryHistory()
        XCTAssertEqual(records.count, 2)
        XCTAssertEqual(records[0].hash, "AA")
        XCTAssertEqual(records[0].type, .text)
        XCTAssertEqual(records[1].type, .image)
        XCTAssertTrue(records[1].hasData)
        XCTAssertNotNil(records[1].createTime)
    }

    func test_queryHistory_emptyArrayIsEndOfList_notAnError() async throws {
        MockURLProtocol.handler = { req in
            let resp = HTTPURLResponse(url: req.url!, statusCode: 200, httpVersion: nil, headerFields: nil)!
            return (resp, Data("[]".utf8))
        }
        let records = try await makeClient().queryHistory()
        XCTAssertEqual(records, [])
    }

    func test_queryHistory_malformedJSONFailsAsDecodingFailed() async {
        MockURLProtocol.handler = { req in
            let resp = HTTPURLResponse(url: req.url!, statusCode: 200, httpVersion: nil, headerFields: nil)!
            return (resp, Data("not json".utf8))
        }
        await assertThrowsKind(.decodingFailed) { try await self.makeClient().queryHistory() }
    }

    // MARK: - HTTP status mapping

    func test_queryHistory_returns401AsAuthFailed() async {
        MockURLProtocol.handler = { req in
            let resp = HTTPURLResponse(url: req.url!, statusCode: 401, httpVersion: nil, headerFields: nil)!
            return (resp, nil)
        }
        await assertThrowsKind(.authFailed) { try await self.makeClient().queryHistory() }
    }

    func test_queryHistory_returns500AsServerError() async {
        MockURLProtocol.handler = { req in
            let resp = HTTPURLResponse(url: req.url!, statusCode: 500, httpVersion: nil, headerFields: nil)!
            return (resp, nil)
        }
        await assertThrowsKind(.serverError(500)) { try await self.makeClient().queryHistory() }
    }

    // MARK: - Helpers

    private func makeClient(
        baseURLString: String = "https://example.com/",
        username: String = "u",
        password: String = "p"
    ) throws -> SyncClipboardClient {
        let cfg = ServerConfig(
            id: "test-id",
            name: nil,
            url: baseURLString,
            username: username,
            password: password
        )
        return try SyncClipboardClient(server: cfg, trustInsecureCert: false, session: MockURLProtocol.session())
    }

    private func assertThrowsKind(
        _ expected: SyncError.Kind,
        file: StaticString = #file, line: UInt = #line,
        _ body: () async throws -> Void
    ) async {
        do {
            try await body()
            XCTFail("expected SyncError.\(expected)", file: file, line: line)
        } catch let e as SyncError {
            XCTAssertEqual(e.kind, expected, file: file, line: line)
        } catch {
            XCTFail("expected SyncError, got \(error)", file: file, line: line)
        }
    }
}

// Convenience operator for the type bitmask in test setup. Lives in the
// test target — keep the production `TypeMask` API minimal.
private func | (lhs: HistoryQuery.TypeMask, rhs: HistoryQuery.TypeMask) -> Int {
    lhs.rawValue | rhs.rawValue
}
