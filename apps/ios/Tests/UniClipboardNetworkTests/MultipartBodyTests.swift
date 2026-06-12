import XCTest
@testable import UniClipboardNetwork

final class MultipartBodyTests: XCTestCase {

    // MARK: - shape / contentType

    func test_contentType_includesBoundary() {
        let body = MultipartBody(boundary: "TESTBND")
        XCTAssertEqual(body.contentType, "multipart/form-data; boundary=TESTBND")
    }

    func test_emptyBody_isOnlyTheClosingDelimiter() {
        let body = MultipartBody(boundary: "TESTBND")
        let encoded = String(data: body.encoded(), encoding: .utf8)
        XCTAssertEqual(encoded, "--TESTBND--\r\n")
    }

    // MARK: - text fields

    func test_singleTextField_encodesWithCRLFLineEndings() {
        var body = MultipartBody(boundary: "BND")
        body.append(name: "page", value: "1")

        let want = [
            "--BND\r\n",
            "Content-Disposition: form-data; name=\"page\"\r\n",
            "\r\n",
            "1\r\n",
            "--BND--\r\n",
        ].joined()
        XCTAssertEqual(String(data: body.encoded(), encoding: .utf8), want)
    }

    func test_multipleTextFields_areOrderedAsAppended() {
        var body = MultipartBody(boundary: "B")
        body.append(name: "page", value: "1")
        body.append(name: "types", value: "15")
        body.append(name: "modifiedAfter", value: "2026-05-17T00:00:00Z")

        let encoded = String(data: body.encoded(), encoding: .utf8) ?? ""
        // Ordering: page comes before types comes before modifiedAfter.
        let pageIdx     = encoded.range(of: "name=\"page\"")!
        let typesIdx    = encoded.range(of: "name=\"types\"")!
        let modAfterIdx = encoded.range(of: "name=\"modifiedAfter\"")!
        XCTAssertLessThan(pageIdx.lowerBound, typesIdx.lowerBound)
        XCTAssertLessThan(typesIdx.lowerBound, modAfterIdx.lowerBound)
    }

    func test_unicodeValuesArePassedThroughAsUTF8() {
        var body = MultipartBody(boundary: "B")
        body.append(name: "searchText", value: "搜索 — emoji ✨")

        let raw = body.encoded()
        let asString = String(data: raw, encoding: .utf8)!
        XCTAssertTrue(asString.contains("搜索 — emoji ✨"))
        // Spot-check the UTF-8 bytes (e.g., "搜" is E6 90 9C in UTF-8).
        let needle = Data([0xE6, 0x90, 0x9C])
        XCTAssertTrue(raw.range(of: needle) != nil)
    }

    // MARK: - file fields

    func test_fileField_emitsFilenameContentTypeAndBody() {
        var body = MultipartBody(boundary: "B")
        body.append(
            name: "file",
            filename: "snap.png",
            contentType: "application/octet-stream",
            body: Data([0x00, 0xFF, 0x42])
        )

        let raw = body.encoded()
        // The body contains 0xFF — not valid ASCII or UTF-8 in isolation.
        // Latin-1 round-trips any byte 1:1, so it's the safe lens for
        // header substring checks even when the body is binary.
        let head = String(data: raw, encoding: .isoLatin1) ?? ""
        XCTAssertTrue(head.contains("Content-Disposition: form-data; name=\"file\"; filename=\"snap.png\"\r\n"))
        XCTAssertTrue(head.contains("Content-Type: application/octet-stream\r\n"))
        // Raw bytes survive intact — the binary part is a fixed offset
        // before the closing boundary. Search by the trailing CRLF +
        // closing boundary so we don't depend on header text length.
        let closer = "\r\n--B--\r\n".data(using: .utf8)!
        guard let closerRange = raw.range(of: closer) else {
            return XCTFail("Closer not found in encoded body")
        }
        let bodyRange = (closerRange.lowerBound - 3)..<closerRange.lowerBound
        XCTAssertEqual(raw.subdata(in: bodyRange), Data([0x00, 0xFF, 0x42]))
    }

    // MARK: - escaping

    func test_disposition_escapesQuotesInFieldNamesAndFilenames() {
        var body = MultipartBody(boundary: "B")
        body.append(
            name: "weird\"name",
            filename: "a\"b.txt",
            contentType: "text/plain",
            body: Data()
        )

        let head = String(data: body.encoded(), encoding: .utf8) ?? ""
        XCTAssertTrue(head.contains(#"name="weird\"name""#))
        XCTAssertTrue(head.contains(#"filename="a\"b.txt""#))
    }

    func test_disposition_stripsEmbeddedNewlines() {
        var body = MultipartBody(boundary: "B")
        body.append(name: "bad\r\nfield", value: "x")

        let head = String(data: body.encoded(), encoding: .utf8) ?? ""
        XCTAssertTrue(head.contains(#"name="badfield""#),
                      "CR/LF in field name MUST be stripped, not preserved")
    }

    // MARK: - closing

    func test_encoded_endsExactlyOnClosingBoundary() {
        var body = MultipartBody(boundary: "Z")
        body.append(name: "a", value: "1")
        body.append(name: "b", value: "2")

        let encoded = String(data: body.encoded(), encoding: .utf8) ?? ""
        XCTAssertTrue(encoded.hasSuffix("--Z--\r\n"),
                      "Final delimiter must be `--<boundary>--\\r\\n`")
    }
}
