import Foundation

/// Minimal `multipart/form-data` builder. Used by §2.7 (`POST /api/history/query`)
/// and §2.9 (`POST /api/history`) — the two endpoints whose JSON-encoder
/// path won't work because they're spec'd as multipart.
///
/// Pure Foundation; no URLSession entanglement. Tests assert byte-for-byte
/// output, so the implementation MUST keep `\r\n` line terminators and the
/// exact boundary delimiters (`--<boundary>` between parts, `--<boundary>--`
/// at the very end) — that's what RFC 7578 + every HTTP server expects.
public struct MultipartBody: Sendable {
    public let boundary: String
    public var contentType: String { "multipart/form-data; boundary=\(boundary)" }

    private var parts: [Data] = []

    public init(boundary: String = "UCB-\(UUID().uuidString)") {
        self.boundary = boundary
    }

    /// Append a text field. Empty / nil values are still emitted as
    /// zero-byte parts — the server distinguishes "field present but
    /// empty" from "field absent", so callers MUST omit by not calling
    /// rather than passing `""`.
    public mutating func append(name: String, value: String) {
        var part = Data()
        part.append(crlf(prefix: "--\(boundary)"))
        part.append(crlf(prefix: "Content-Disposition: form-data; name=\"\(quoted(name))\""))
        part.append(Self.crlf)                                // header/body separator
        part.append(value.data(using: .utf8) ?? Data())
        part.append(Self.crlf)
        parts.append(part)
    }

    /// Append a binary field with a filename and `Content-Type`. Used by
    /// §2.9 to attach the payload bytes alongside the metadata fields.
    public mutating func append(
        name: String,
        filename: String,
        contentType: String,
        body: Data
    ) {
        var part = Data()
        part.append(crlf(prefix: "--\(boundary)"))
        part.append(crlf(prefix:
            "Content-Disposition: form-data; name=\"\(quoted(name))\"; "
            + "filename=\"\(quoted(filename))\""))
        part.append(crlf(prefix: "Content-Type: \(contentType)"))
        part.append(Self.crlf)
        part.append(body)
        part.append(Self.crlf)
        parts.append(part)
    }

    /// Finalize the body. A multipart with zero fields is legal — the
    /// closing delimiter alone is a valid (if useless) body — but callers
    /// generally want at least one part.
    public func encoded() -> Data {
        var data = Data()
        for part in parts { data.append(part) }
        data.append(crlf(prefix: "--\(boundary)--"))
        return data
    }

    // MARK: - Internals

    private static let crlf = Data([0x0D, 0x0A])              // \r\n

    private func crlf(prefix: String) -> Data {
        var d = prefix.data(using: .utf8) ?? Data()
        d.append(Self.crlf)
        return d
    }

    /// Per RFC 7578 §4.2, escape `"` and CR/LF in the disposition's
    /// `name=` / `filename=` parameters. Backslash-escape `"`, drop any
    /// embedded CR/LF (no reasonable form field name has them).
    private func quoted(_ s: String) -> String {
        s.replacingOccurrences(of: "\\", with: "\\\\")
         .replacingOccurrences(of: "\"", with: "\\\"")
         .replacingOccurrences(of: "\r", with: "")
         .replacingOccurrences(of: "\n", with: "")
    }
}
