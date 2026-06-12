import XCTest
@testable import UniClipboardModels

/// Spec §4.1 (text hash) + `Clipboard.fromText` factory. The wire fixture
/// `docs/examples/clipboard_text_short.json` ships a hash for the exact
/// string `"Hello, SyncClipboard!"`, so H1 is a cross-check between the
/// fixture and our implementation.
final class HashTests: XCTestCase {

    // MARK: - computeTextHash

    func test_H1_computeTextHash_helloFixture_matchesSpecValue() {
        let h = Clipboard.computeTextHash("Hello, SyncClipboard!")
        XCTAssertEqual(
            h,
            "3F4E62D9F184380BAD1B0F94B5518DCBF35ACB79B34F6D6E34F3DAB16CD7BC8F"
        )
    }

    func test_H2_computeTextHash_emptyString_matchesKnownSHA256() {
        // SHA-256("") is a universally-known constant.
        XCTAssertEqual(
            Clipboard.computeTextHash(""),
            "E3B0C44298FC1C149AFBF4C8996FB92427AE41E4649B934CA495991B7852B855"
        )
    }

    func test_H5_computeTextHash_isUppercase64HexChars() {
        let h = Clipboard.computeTextHash("any input")
        XCTAssertEqual(h.count, 64)
        XCTAssertEqual(h, h.uppercased(), "hash output must be uppercase")
        XCTAssertTrue(
            h.allSatisfy { $0.isHexDigit && (!$0.isLetter || $0.isUppercase) },
            "hash must be uppercase hex digits only"
        )
    }

    // MARK: - fromText factory

    func test_H3_fromText_setsTypeHashSize_andHasDataFalse() {
        let c = Clipboard.fromText("hi")
        XCTAssertEqual(c.type, .text)
        XCTAssertEqual(c.hash, Clipboard.computeTextHash("hi"))
        XCTAssertEqual(c.text, "hi")
        XCTAssertEqual(c.size, 2)
        XCTAssertFalse(c.hasData)
        XCTAssertNil(c.dataName)
    }

    func test_H4_fromText_longString_doesNotApplySection34Transform() {
        // > 10240 chars. §3.4's preview/payload split is upload-time only;
        // observe-time fromText must keep the full text and hasData=false.
        let long = String(repeating: "a", count: 10_500)
        let c = Clipboard.fromText(long)
        XCTAssertEqual(c.text.count, 10_500, "must keep full text, not truncate to preview")
        XCTAssertFalse(c.hasData, "§3.4 transform must NOT be applied at observe time")
        XCTAssertNil(c.dataName)
        XCTAssertEqual(c.hash, Clipboard.computeTextHash(long))
    }

    // MARK: - computeBytesHash (§4.1 over UTF-8 bytes)

    func test_H6_computeBytesHash_emptyData_matchesKnownSHA256() {
        XCTAssertEqual(
            Clipboard.computeBytesHash(Data()),
            "E3B0C44298FC1C149AFBF4C8996FB92427AE41E4649B934CA495991B7852B855"
        )
    }

    func test_H7_computeBytesHash_abc_matchesNISTTestVector() {
        // FIPS 180-2 test vector for SHA-256 of the three-byte string "abc".
        XCTAssertEqual(
            Clipboard.computeBytesHash(Data("abc".utf8)),
            "BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD"
        )
    }

    /// Parity invariant the text-overflow download verifier relies on:
    /// `computeBytesHash(Data(text.utf8)) == computeTextHash(text)`. Without
    /// this equality, downloading the file payload of a text-overflow entry
    /// and hashing the bytes would not match the metadata's `hash` field
    /// (which is the §4.1 text hash). Both must compute SHA-256 of the same
    /// UTF-8 bytes — same input, same digest.
    func test_H8_computeBytesHash_matchesComputeTextHash_forSameUTF8() {
        for sample in ["hello, world", "你好,世界", "", "🍎🍌"] {
            XCTAssertEqual(
                Clipboard.computeBytesHash(Data(sample.utf8)),
                Clipboard.computeTextHash(sample),
                "parity broken for \(sample.debugDescription)"
            )
        }
    }

    // MARK: - File/image hash (§4.2) + publishImage

    /// 8×8 red-square PNG, ~70 bytes. Same fixture the simctl stub uses,
    /// so cross-recipe consistency is locked in: the device's local
    /// `publishImage` of these bytes produces the same §4.2 hash as the
    /// stub server's `_bytes_hash` of the served file.
    private static let red8x8PNG = Data(base64Encoded:
        "iVBORw0KGgoAAAANSUhEUgAAAAgAAAAIAQMAAAD+wSzIAAAABGdBTUEAALGP" +
        "C/xhBQAAAAFzUkdCAK7OHOkAAAAGUExURf8AAP///8jJRKEAAAAOSURBVAjX" +
        "Y/jPwMDAAAAEAQEAQYxqNwAAAABJRU5ErkJggg=="
    )!

    func test_F1_fileHash_isRawBytesSHA256_nameDoesNotParticipate() {
        // §4.2 — the file/image hash is plain SHA256(bytes). If publish
        // ever drifts back to the basename-bound two-step (pre-unification
        // bug), the published hash stops matching computeBytesHash and
        // this fails.
        let bytes = Self.red8x8PNG
        let (asImage, _) = Clipboard.publishImage(bytes: bytes, ext: "png")
        let (asFile, _) = Clipboard.publishFile(name: "anything.png", bytes: bytes)
        XCTAssertEqual(asImage.hash, Clipboard.computeBytesHash(bytes))
        XCTAssertEqual(asFile.hash, Clipboard.computeBytesHash(bytes))
        XCTAssertEqual(
            asImage.hash, asFile.hash,
            "filename must NOT bind into the hash — same bytes, same hash"
        )
    }

    func test_F3_fileHash_differentBytes_differentHash() {
        let h1 = Clipboard.computeBytesHash(Self.red8x8PNG)
        let h2 = Clipboard.computeBytesHash(Data([0xFF, 0xD8, 0xFF])) // not red8x8
        XCTAssertNotEqual(h1, h2)
    }

    func test_F5_publishImage_producesCorrectShape() {
        let bytes = Self.red8x8PNG
        let (clip, _) = Clipboard.publishImage(bytes: bytes, ext: "png")
        XCTAssertEqual(clip.type, .image)
        XCTAssertTrue(clip.hasData)
        XCTAssertEqual(clip.dataName, "image.png")
        XCTAssertEqual(clip.text, "image.png", "non-text text-field must be the basename per §3.3")
        XCTAssertEqual(clip.size, bytes.count)
        XCTAssertEqual(clip.hash, Clipboard.computeBytesHash(bytes))
    }

    /// Bytes-preservation discipline. `publishImage` must NOT re-encode
    /// the input — the payload returned must equal the bytes given. If
    /// this ever fails, the §4.2 hash on the wire won't match what
    /// downstream readers compute, and image sync silently breaks.
    func test_F6_publishImage_payload_isByteIdenticalToInput() {
        let (_, payload) = Clipboard.publishImage(bytes: Self.red8x8PNG, ext: "png")
        XCTAssertEqual(payload, Self.red8x8PNG)
    }
}
