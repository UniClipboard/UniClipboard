import Foundation

/// Intercepts URLSession traffic in tests. Set `Self.handler` per-case
/// before issuing the request; the handler receives the URLRequest and
/// returns the (response, body) to feed back.
final class MockURLProtocol: URLProtocol {
    typealias Handler = (URLRequest) throws -> (HTTPURLResponse, Data?)

    nonisolated(unsafe) static var handler: Handler?
    nonisolated(unsafe) static var lastRequest: URLRequest?
    /// URLSession converts `httpBody` to a stream before the URLProtocol
    /// sees the request; reading the stream in `startLoading()` is the
    /// only reliable way to recover the body.
    nonisolated(unsafe) static var lastBody: Data?

    static func reset() {
        handler = nil
        lastRequest = nil
        lastBody = nil
    }

    static func session() -> URLSession {
        let cfg = URLSessionConfiguration.ephemeral
        cfg.protocolClasses = [MockURLProtocol.self]
        return URLSession(configuration: cfg)
    }

    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }

    override func startLoading() {
        Self.lastRequest = request
        Self.lastBody = Self.readBody(from: request)
        guard let h = Self.handler else {
            client?.urlProtocol(self, didFailWithError: URLError(.badURL))
            return
        }
        do {
            let (response, data) = try h(request)
            client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
            if let data { client?.urlProtocol(self, didLoad: data) }
            client?.urlProtocolDidFinishLoading(self)
        } catch {
            client?.urlProtocol(self, didFailWithError: error)
        }
    }

    override func stopLoading() {}

    private static func readBody(from req: URLRequest) -> Data? {
        if let body = req.httpBody, !body.isEmpty { return body }
        guard let stream = req.httpBodyStream else { return nil }
        stream.open()
        defer { stream.close() }
        var data = Data()
        let bufSize = 4096
        var buf = [UInt8](repeating: 0, count: bufSize)
        while stream.hasBytesAvailable {
            let n = buf.withUnsafeMutableBufferPointer {
                stream.read($0.baseAddress!, maxLength: bufSize)
            }
            if n <= 0 { break }
            data.append(buf, count: n)
        }
        return data.isEmpty ? nil : data
    }
}
