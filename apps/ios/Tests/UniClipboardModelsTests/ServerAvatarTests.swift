import XCTest
@testable import UniClipboardModels

final class ServerAvatarTests: XCTestCase {

    // MARK: - Determinism (the load-bearing property)

    func test_hue_isStableForSameId() {
        let s1 = ServerConfig(id: "abc-123", url: "https://a.example", username: "u", password: "p")
        let s2 = ServerConfig(id: "abc-123", name: "Different Name", url: "https://b.example", username: "x", password: "y")
        XCTAssertEqual(ServerAvatar.hue(for: s1), ServerAvatar.hue(for: s2),
                       "hue must depend only on id so the avatar tile stays visually stable")
    }

    func test_hue_isInUnitInterval() {
        for i in 0..<200 {
            let s = ServerConfig(id: "id-\(i)", url: "https://x", username: "u", password: "p")
            let h = ServerAvatar.hue(for: s)
            XCTAssertGreaterThanOrEqual(h, 0)
            XCTAssertLessThan(h, 1.0)
        }
    }

    func test_hue_differsForCommonIds() {
        // Not a strict collision-resistance test — just sanity that small id
        // perturbations don't all land on the same hue.
        let ids = (0..<32).map { "server-\($0)" }
        let hues = Set(ids.map { id -> Int in
            let s = ServerConfig(id: id, url: "x", username: "u", password: "p")
            return Int(ServerAvatar.hue(for: s) * 360)
        })
        XCTAssertGreaterThan(hues.count, 24, "Expected most ids to map to distinct hue buckets, got \(hues.count)/32")
    }

    // MARK: - Initials priority

    func test_initials_prefersNameOverHost() {
        let s = ServerConfig(id: "1", name: "  Home Lab  ", url: "https://nas.example", username: "u", password: "p")
        XCTAssertEqual(ServerAvatar.initials(for: s), "HL")
    }

    func test_initials_fallsBackToHostWhenNameEmpty() {
        let s = ServerConfig(id: "1", name: "   ", url: "https://nas.example.com", username: "u", password: "p")
        XCTAssertEqual(ServerAvatar.initials(for: s), "NA")
    }

    func test_initials_fallsBackToURLWhenNotParseable() {
        // No scheme → URL(string:).host is nil; we take prefix of the raw url.
        let s = ServerConfig(id: "1", name: nil, url: "abc", username: "u", password: "p")
        XCTAssertEqual(ServerAvatar.initials(for: s), "AB")
    }

    func test_initials_handlesChineseSingleWord() {
        // Two CJK characters with no separators → take prefix(2).
        let s = ServerConfig(id: "1", name: "服务器", url: "x", username: "u", password: "p")
        XCTAssertEqual(ServerAvatar.initials(for: s), "服务")
    }

    func test_initials_handlesHyphenatedAlias() {
        let s = ServerConfig(id: "1", name: "happy-otter", url: "x", username: "u", password: "p")
        XCTAssertEqual(ServerAvatar.initials(for: s), "HO")
    }

    func test_initials_handlesAllSeparatorString() {
        let s = ServerConfig(id: "1", name: "  --  ", url: "  ", username: "u", password: "p")
        XCTAssertEqual(ServerAvatar.initials(for: s), "?")
    }
}
