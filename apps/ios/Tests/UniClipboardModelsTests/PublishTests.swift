import XCTest
@testable import UniClipboardModels

/// §3.4 long-text overflow transform via `Clipboard.publishText`.
final class PublishTests: XCTestCase {

    private static let threshold = 10_240

    // MARK: - Short path (≤ threshold)

    func test_Pub1_shortText_hasDataFalse_payloadNil_textIsFull() {
        let (clip, payload) = Clipboard.publishText("hi")
        XCTAssertFalse(clip.hasData)
        XCTAssertNil(payload)
        XCTAssertEqual(clip.text, "hi")
        XCTAssertEqual(clip.size, 2)
        XCTAssertEqual(clip.type, .text)
        XCTAssertNil(clip.dataName)
        XCTAssertEqual(clip.hash, Clipboard.computeTextHash("hi"))
    }

    func test_Pub2_atThreshold_staysInline() {
        let s = String(repeating: "a", count: Self.threshold)
        let (clip, payload) = Clipboard.publishText(s)
        XCTAssertFalse(clip.hasData, "exactly threshold-length must be inline (rule is `> threshold`)")
        XCTAssertNil(payload)
        XCTAssertEqual(clip.text.count, Self.threshold)
        XCTAssertEqual(clip.size, Self.threshold)
    }

    func test_Pub8_emptyString_isInline() {
        let (clip, payload) = Clipboard.publishText("")
        XCTAssertFalse(clip.hasData)
        XCTAssertNil(payload)
        XCTAssertEqual(clip.size, 0)
        XCTAssertEqual(clip.hash, Clipboard.computeTextHash(""))
    }

    // MARK: - Long path (> threshold)

    func test_Pub3_aboveThreshold_overflowTriggers() {
        let s = String(repeating: "a", count: Self.threshold + 1)
        let (clip, payload) = Clipboard.publishText(s)
        XCTAssertTrue(clip.hasData)
        XCTAssertNotNil(payload)
    }

    func test_Pub4_dataNameBindsToHash() {
        let s = String(repeating: "z", count: Self.threshold + 100)
        let (clip, _) = Clipboard.publishText(s)
        let hash = try! XCTUnwrap(clip.hash)
        XCTAssertEqual(clip.dataName, "text_\(hash).txt")
    }

    func test_Pub5_textIsExactlyFirstThresholdChars() {
        let s = String(repeating: "x", count: Self.threshold + 500) + "_TAIL"
        let (clip, _) = Clipboard.publishText(s)
        XCTAssertEqual(clip.text.count, Self.threshold)
        XCTAssertEqual(clip.text, String(s.prefix(Self.threshold)))
        XCTAssertFalse(clip.text.contains("_TAIL"))
    }

    func test_Pub6_payloadIsFullUTF8Bytes() {
        let s = String(repeating: "你", count: Self.threshold + 10) // multi-byte UTF-8
        let (_, payload) = Clipboard.publishText(s)
        XCTAssertEqual(payload, Data(s.utf8))
        // sanity: each "你" is 3 UTF-8 bytes; 10250 chars × 3 = 30750 bytes
        XCTAssertEqual(payload?.count, (Self.threshold + 10) * 3)
    }

    func test_Pub7_sizeIsFullCharacterCount_notPreviewLength() {
        let full = Self.threshold + 777
        let s = String(repeating: "a", count: full)
        let (clip, _) = Clipboard.publishText(s)
        XCTAssertEqual(clip.size, full, "size MUST be the full text count, not the preview length")
    }
}
