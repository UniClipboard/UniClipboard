import XCTest
import UniClipboardModels
@testable import UniClipboardNetwork

/// Tests for `SyncClipboardClient.getHistoryPayload(profileId:)` (§2.11).
final class GetHistoryPayloadTests: XCTestCase {

    override func tearDown() {
        MockURLProtocol.reset()
        super.tearDown()
    }

    private static let profileId =
        "Image-4DD7CC4227AA3FB2FDAC2597CB4F88EAC6F69A10BC1994F6B87CF8890C345AFC"

    func test_usesGET_atCompositeIdDataPath() async throws {
        MockURLProtocol.handler = { req in
            let resp = HTTPURLResponse(url: req.url!, statusCode: 200, httpVersion: nil, headerFields: nil)!
            return (resp, Data())
        }
        let client = try makeClient(baseURLString: "https://nas.local:5033/")
        _ = try await client.getHistoryPayload(profileId: Self.profileId)

        XCTAssertEqual(MockURLProtocol.lastRequest?.httpMethod, "GET")
        XCTAssertEqual(MockURLProtocol.lastRequest?.url?.absoluteString,
                       "https://nas.local:5033/api/history/\(Self.profileId)/data")
    }

    func test_setsAuthHeader() async throws {
        MockURLProtocol.handler = { req in
            let resp = HTTPURLResponse(url: req.url!, statusCode: 200, httpVersion: nil, headerFields: nil)!
            return (resp, Data())
        }
        let client = try makeClient(username: "alice", password: "secret")
        _ = try await client.getHistoryPayload(profileId: Self.profileId)
        let auth = MockURLProtocol.lastRequest?.value(forHTTPHeaderField: "Authorization")
        XCTAssertEqual(auth, "Basic YWxpY2U6c2VjcmV0")
    }

    func test_returnsResponseBytesVerbatim() async throws {
        let payload = Data([0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A])  // PNG sig
        MockURLProtocol.handler = { req in
            let resp = HTTPURLResponse(url: req.url!, statusCode: 200, httpVersion: nil, headerFields: nil)!
            return (resp, payload)
        }
        let bytes = try await makeClient().getHistoryPayload(profileId: Self.profileId)
        XCTAssertEqual(bytes, payload)
    }

    func test_returns401AsAuthFailed() async {
        MockURLProtocol.handler = { req in
            let resp = HTTPURLResponse(url: req.url!, statusCode: 401, httpVersion: nil, headerFields: nil)!
            return (resp, nil)
        }
        await assertThrowsKind(.authFailed) {
            _ = try await self.makeClient().getHistoryPayload(profileId: Self.profileId)
        }
    }

    func test_returns404AsNotFound() async {
        MockURLProtocol.handler = { req in
            let resp = HTTPURLResponse(url: req.url!, statusCode: 404, httpVersion: nil, headerFields: nil)!
            return (resp, nil)
        }
        await assertThrowsKind(.notFound) {
            _ = try await self.makeClient().getHistoryPayload(profileId: Self.profileId)
        }
    }

    func test_returns500AsServerError() async {
        MockURLProtocol.handler = { req in
            let resp = HTTPURLResponse(url: req.url!, statusCode: 500, httpVersion: nil, headerFields: nil)!
            return (resp, nil)
        }
        await assertThrowsKind(.serverError(500)) {
            _ = try await self.makeClient().getHistoryPayload(profileId: Self.profileId)
        }
    }

    func test_rejectsInvalidProfileIdsBeforeNetworkCall() async {
        let bads = ["", "Text/abc", "Text\\abc"]
        for bad in bads {
            MockURLProtocol.handler = { _ in
                XCTFail("network should not be hit for invalid profileId: \(bad)")
                throw URLError(.badURL)
            }
            do {
                _ = try await makeClient().getHistoryPayload(profileId: bad)
                XCTFail("expected throw for invalid id \(bad)")
            } catch let e as SyncError {
                XCTAssertEqual(e.kind, .invalidURL)
            } catch {
                XCTFail("expected SyncError.invalidURL, got \(error)")
            }
        }
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
