import XCTest
import UniClipboardModels
@testable import UniClipboardNetwork

final class SyncClipboardClientTests: XCTestCase {

    override func tearDown() {
        MockURLProtocol.reset()
        super.tearDown()
    }

    // MARK: - URL normalization (§1.1)

    func test_normalizeBaseURL_appendsTrailingSlashWhenMissing() throws {
        let url = try SyncClipboardClient.normalizeBaseURL("https://example.com")
        XCTAssertEqual(url.absoluteString, "https://example.com/")
    }

    func test_normalizeBaseURL_isIdempotentWhenAlreadySlashed() throws {
        let url = try SyncClipboardClient.normalizeBaseURL("https://example.com/")
        XCTAssertEqual(url.absoluteString, "https://example.com/")
    }

    func test_normalizeBaseURL_trimsSurroundingWhitespace() throws {
        let url = try SyncClipboardClient.normalizeBaseURL("  https://example.com  ")
        XCTAssertEqual(url.absoluteString, "https://example.com/")
    }

    func test_normalizeBaseURL_rejectsEmptyString() {
        XCTAssertThrowsError(try SyncClipboardClient.normalizeBaseURL("")) { e in
            XCTAssertEqual((e as? SyncError)?.kind, .invalidURL)
        }
    }

    func test_normalizeBaseURL_rejectsNonHTTPScheme() {
        XCTAssertThrowsError(try SyncClipboardClient.normalizeBaseURL("ftp://example.com")) { e in
            XCTAssertEqual((e as? SyncError)?.kind, .invalidURL)
        }
    }

    func test_normalizeBaseURL_rejectsHostlessString() {
        XCTAssertThrowsError(try SyncClipboardClient.normalizeBaseURL("not-a-url")) { e in
            XCTAssertEqual((e as? SyncError)?.kind, .invalidURL)
        }
    }

    func test_normalizeBaseURL_acceptsPortAndPath() throws {
        let url = try SyncClipboardClient.normalizeBaseURL("https://nas.local:5033/sync")
        XCTAssertEqual(url.absoluteString, "https://nas.local:5033/sync/")
    }

    // MARK: - Basic auth header (§1.2)

    func test_basicAuthHeader_matchesSpecExample() {
        // base64("alice:secret") = "YWxpY2U6c2VjcmV0"
        XCTAssertEqual(
            SyncClipboardClient.basicAuthHeader(username: "alice", password: "secret"),
            "Basic YWxpY2U6c2VjcmV0"
        )
    }

    func test_basicAuthHeader_handlesUTF8Credentials() {
        // base64(utf8("用户:密码"))
        let header = SyncClipboardClient.basicAuthHeader(username: "用户", password: "密码")
        let expected = "Basic " + Data("用户:密码".utf8).base64EncodedString()
        XCTAssertEqual(header, expected)
    }

    // MARK: - GET SyncClipboard.json (§2.1)

    func test_getClipboard_decodesHappyPath() async throws {
        let payload: [String: Any] = [
            "type": "Text",
            "text": "hello",
            "hasData": false,
            "size": 5
        ]
        MockURLProtocol.handler = { req in
            let body = try JSONSerialization.data(withJSONObject: payload)
            let resp = HTTPURLResponse(url: req.url!, statusCode: 200, httpVersion: "HTTP/1.1", headerFields: ["Content-Type": "application/json"])!
            return (resp, body)
        }
        let client = try makeClient()
        let clip = try await client.getClipboard()
        XCTAssertEqual(clip.type, .text)
        XCTAssertEqual(clip.text, "hello")
        XCTAssertEqual(clip.size, 5)
        XCTAssertFalse(clip.hasData)
    }

    func test_getClipboard_attachesBasicAuthHeader() async throws {
        MockURLProtocol.handler = { req in
            let resp = HTTPURLResponse(url: req.url!, statusCode: 404, httpVersion: nil, headerFields: nil)!
            return (resp, nil)
        }
        let client = try makeClient(username: "alice", password: "secret")
        do {
            _ = try await client.getClipboard()
            XCTFail("expected notFound")
        } catch let e as SyncError {
            XCTAssertEqual(e.kind, .notFound)
        }
        let header = MockURLProtocol.lastRequest?.value(forHTTPHeaderField: "Authorization")
        XCTAssertEqual(header, "Basic YWxpY2U6c2VjcmV0")
    }

    func test_getClipboard_hitsExpectedPath() async throws {
        MockURLProtocol.handler = { req in
            let resp = HTTPURLResponse(url: req.url!, statusCode: 404, httpVersion: nil, headerFields: nil)!
            return (resp, nil)
        }
        let client = try makeClient(baseURLString: "https://nas.local:5033/")
        _ = try? await client.getClipboard()
        XCTAssertEqual(MockURLProtocol.lastRequest?.url?.absoluteString,
                       "https://nas.local:5033/SyncClipboard.json")
    }

    func test_getClipboard_returns401AsAuthFailed() async {
        MockURLProtocol.handler = { req in
            let resp = HTTPURLResponse(url: req.url!, statusCode: 401, httpVersion: nil, headerFields: nil)!
            return (resp, nil)
        }
        await assertThrowsKind(.authFailed) { try await self.makeClient().getClipboard() }
    }

    func test_getClipboard_returns404AsNotFound() async {
        MockURLProtocol.handler = { req in
            let resp = HTTPURLResponse(url: req.url!, statusCode: 404, httpVersion: nil, headerFields: nil)!
            return (resp, nil)
        }
        await assertThrowsKind(.notFound) { try await self.makeClient().getClipboard() }
    }

    func test_getClipboard_returns500AsServerError() async {
        MockURLProtocol.handler = { req in
            let resp = HTTPURLResponse(url: req.url!, statusCode: 500, httpVersion: nil, headerFields: nil)!
            return (resp, nil)
        }
        await assertThrowsKind(.serverError(500)) { try await self.makeClient().getClipboard() }
    }

    func test_getClipboard_returnsOther4xxAsProtocolError() async {
        MockURLProtocol.handler = { req in
            let resp = HTTPURLResponse(url: req.url!, statusCode: 418, httpVersion: nil, headerFields: nil)!
            return (resp, nil)
        }
        await assertThrowsKind(.protocolError(418)) { try await self.makeClient().getClipboard() }
    }

    func test_getClipboard_malformedJSONFailsAsDecodingFailed() async {
        MockURLProtocol.handler = { req in
            let resp = HTTPURLResponse(url: req.url!, statusCode: 200, httpVersion: nil, headerFields: nil)!
            return (resp, Data("not-json".utf8))
        }
        await assertThrowsKind(.decodingFailed) { try await self.makeClient().getClipboard() }
    }

    // MARK: - PUT SyncClipboard.json (§2.2)

    private func ok2xxHandler(status: Int = 200) -> MockURLProtocol.Handler {
        { req in
            let resp = HTTPURLResponse(url: req.url!, statusCode: status, httpVersion: nil, headerFields: nil)!
            return (resp, nil)
        }
    }

    private static let shortClip = Clipboard(
        type: .text,
        hash: "3F4E62D9F184380BAD1B0F94B5518DCBF35ACB79B34F6D6E34F3DAB16CD7BC8F",
        text: "Hello, SyncClipboard!",
        hasData: false,
        size: 21
    )

    func test_P1_putClipboard_usesPUT_atSyncClipboardJsonPath() async throws {
        MockURLProtocol.handler = ok2xxHandler()
        let client = try makeClient(baseURLString: "https://nas.local:5033/")
        try await client.putClipboard(Self.shortClip)
        XCTAssertEqual(MockURLProtocol.lastRequest?.httpMethod, "PUT")
        XCTAssertEqual(MockURLProtocol.lastRequest?.url?.absoluteString,
                       "https://nas.local:5033/SyncClipboard.json")
    }

    func test_P2_putClipboard_setsAuthAndJsonContentType() async throws {
        MockURLProtocol.handler = ok2xxHandler()
        let client = try makeClient(username: "alice", password: "secret")
        try await client.putClipboard(Self.shortClip)
        let auth = MockURLProtocol.lastRequest?.value(forHTTPHeaderField: "Authorization")
        let ct = MockURLProtocol.lastRequest?.value(forHTTPHeaderField: "Content-Type")
        XCTAssertEqual(auth, "Basic YWxpY2U6c2VjcmV0")
        XCTAssertEqual(ct, "application/json")
    }

    func test_P3_putClipboard_bodyIsJSONEncodedClipboard() async throws {
        MockURLProtocol.handler = ok2xxHandler()
        try await makeClient().putClipboard(Self.shortClip)
        let body = try XCTUnwrap(MockURLProtocol.lastBody)
        let decoded = try JSONDecoder().decode(Clipboard.self, from: body)
        XCTAssertEqual(decoded, Self.shortClip)
        // §3.1 — null fields must be omitted, not serialized as null.
        let raw = try XCTUnwrap(String(data: body, encoding: .utf8))
        XCTAssertFalse(raw.contains("null"), "encoded body must not contain null fields: \(raw)")
        XCTAssertFalse(raw.contains("dataName"), "dataName MUST be omitted when nil: \(raw)")
    }

    func test_P4_putClipboard_acceptsAll2xxAsSuccess() async throws {
        for status in [200, 201, 204] {
            MockURLProtocol.handler = ok2xxHandler(status: status)
            try await makeClient().putClipboard(Self.shortClip)
        }
    }

    func test_P5a_putClipboard_returns401AsAuthFailed() async {
        MockURLProtocol.handler = ok2xxHandler(status: 401)
        await assertThrowsKind(.authFailed) {
            try await self.makeClient().putClipboard(Self.shortClip)
        }
    }

    func test_P5b_putClipboard_returns500AsServerError() async {
        MockURLProtocol.handler = ok2xxHandler(status: 500)
        await assertThrowsKind(.serverError(500)) {
            try await self.makeClient().putClipboard(Self.shortClip)
        }
    }

    // MARK: - PUT file/<name> (§2.3)

    func test_P6_putFile_usesPUT_atFilePath_withCorrectHeaders() async throws {
        MockURLProtocol.handler = ok2xxHandler()
        let client = try makeClient(baseURLString: "https://nas.local:5033/")
        let bytes = Data([0xDE, 0xAD, 0xBE, 0xEF])
        try await client.putFile(name: "text_ABC.txt", body: bytes)
        XCTAssertEqual(MockURLProtocol.lastRequest?.httpMethod, "PUT")
        XCTAssertEqual(MockURLProtocol.lastRequest?.url?.absoluteString,
                       "https://nas.local:5033/file/text_ABC.txt")
        XCTAssertEqual(
            MockURLProtocol.lastRequest?.value(forHTTPHeaderField: "Content-Type"),
            "application/octet-stream"
        )
        XCTAssertEqual(
            MockURLProtocol.lastRequest?.value(forHTTPHeaderField: "Content-Length"),
            "4"
        )
    }

    func test_P7_putFile_bodyIsRawBytes() async throws {
        MockURLProtocol.handler = ok2xxHandler()
        let bytes = Data((0..<256).map { UInt8($0) })
        try await makeClient().putFile(name: "blob.bin", body: bytes)
        XCTAssertEqual(MockURLProtocol.lastBody, bytes)
    }

    func test_P8_putFile_rejectsBadFilenamesBeforeNetworkCall() async {
        MockURLProtocol.handler = { _ in
            XCTFail("network must NOT be called for invalid filenames")
            return (HTTPURLResponse(), nil)
        }
        for bad in ["a/b", "..\\b", ""] {
            await assertThrowsKind(.invalidURL) {
                try await self.makeClient().putFile(name: bad, body: Data())
            }
        }
    }

    func test_P9_putFile_returns401AsAuthFailed() async {
        MockURLProtocol.handler = ok2xxHandler(status: 401)
        await assertThrowsKind(.authFailed) {
            try await self.makeClient().putFile(name: "x.bin", body: Data([0]))
        }
    }

    // MARK: - GET file/<name> (§2.4)

    private func bytesHandler(_ bytes: Data, status: Int = 200) -> MockURLProtocol.Handler {
        { req in
            let resp = HTTPURLResponse(url: req.url!, statusCode: status, httpVersion: nil, headerFields: nil)!
            return (resp, bytes)
        }
    }

    func test_G1_getFile_usesGET_atFilePath_withAuth() async throws {
        MockURLProtocol.handler = bytesHandler(Data())
        let client = try makeClient(baseURLString: "https://nas.local:5033/", username: "alice", password: "secret")
        _ = try await client.getFile(name: "text_ABC.txt")
        XCTAssertEqual(MockURLProtocol.lastRequest?.httpMethod, "GET")
        XCTAssertEqual(MockURLProtocol.lastRequest?.url?.absoluteString,
                       "https://nas.local:5033/file/text_ABC.txt")
        XCTAssertEqual(
            MockURLProtocol.lastRequest?.value(forHTTPHeaderField: "Authorization"),
            "Basic YWxpY2U6c2VjcmV0"
        )
    }

    func test_G2_getFile_returnsResponseBytesVerbatim() async throws {
        let payload = Data((0..<256).map { UInt8($0) })
        MockURLProtocol.handler = bytesHandler(payload)
        let received = try await makeClient().getFile(name: "blob.bin")
        XCTAssertEqual(received, payload)
    }

    func test_G3a_getFile_returns401AsAuthFailed() async {
        MockURLProtocol.handler = bytesHandler(Data(), status: 401)
        await assertThrowsKind(.authFailed) {
            _ = try await self.makeClient().getFile(name: "x.bin")
        }
    }

    func test_G3b_getFile_returns404AsNotFound() async {
        MockURLProtocol.handler = bytesHandler(Data(), status: 404)
        await assertThrowsKind(.notFound) {
            _ = try await self.makeClient().getFile(name: "x.bin")
        }
    }

    func test_G3c_getFile_returns500AsServerError() async {
        MockURLProtocol.handler = bytesHandler(Data(), status: 500)
        await assertThrowsKind(.serverError(500)) {
            _ = try await self.makeClient().getFile(name: "x.bin")
        }
    }

    func test_G4_getFile_rejectsBadFilenamesBeforeNetworkCall() async {
        MockURLProtocol.handler = { _ in
            XCTFail("network must NOT be called for invalid filenames")
            return (HTTPURLResponse(), nil)
        }
        for bad in ["a/b", "..\\b", ""] {
            await assertThrowsKind(.invalidURL) {
                _ = try await self.makeClient().getFile(name: bad)
            }
        }
    }

    func test_G5_getFile_returnsEmptyDataFor200WithEmptyBody() async throws {
        MockURLProtocol.handler = bytesHandler(Data())
        let received = try await makeClient().getFile(name: "empty.bin")
        XCTAssertEqual(received, Data())
    }

    // MARK: - Helpers

    // MARK: - cancelInFlight (§5.3 network-epoch invalidation)

    func test_cancelInFlight_poisonsSubsequentCallsWithoutTouchingNetwork() async throws {
        let client = try makeClient()
        var handlerCalled = false
        MockURLProtocol.handler = { req in
            handlerCalled = true
            return (HTTPURLResponse(url: req.url!, statusCode: 200, httpVersion: "HTTP/1.1", headerFields: nil)!, Data())
        }
        client.cancelInFlight()
        await assertThrowsKind(.cancelled) {
            _ = try await client.getClipboard()
        }
        XCTAssertFalse(handlerCalled, "a cancelled client must refuse new requests before any network I/O")
    }

    func test_mapURLError_cancelledMapsToCancelledKind() {
        XCTAssertEqual(SyncError.mapURLError(URLError(.cancelled)).kind, .cancelled)
    }

    func test_perform_doesNotRetryAfterURLErrorCancelled() async throws {
        // The 300ms retry-once path covers .networkConnectionLost/.timedOut;
        // a cancelled task is a deliberate abort and must surface
        // immediately, exactly once.
        let client = try makeClient()
        var attempts = 0
        MockURLProtocol.handler = { _ in
            attempts += 1
            throw URLError(.cancelled)
        }
        await assertThrowsKind(.cancelled) {
            _ = try await client.getClipboard()
        }
        XCTAssertEqual(attempts, 1)
    }

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
