import XCTest
@testable import UniClipboardModels

final class ServerNameGeneratorTests: XCTestCase {

    /// Deterministic RNG so collision branches can be exercised reliably.
    private struct SeededRNG: RandomNumberGenerator {
        var state: UInt64
        mutating func next() -> UInt64 {
            state = state &* 6364136223846793005 &+ 1442695040888963407
            return state
        }
    }

    func test_generate_returnsAdjectiveDashNoun_format() {
        let name = ServerNameGenerator.generate()
        let parts = name.split(separator: "-")
        XCTAssertEqual(parts.count, 2, "Expected '<adj>-<noun>', got \(name)")
        XCTAssertTrue(
            ServerNameGenerator.adjectives.contains(String(parts[0])),
            "\(parts[0]) is not in adjective dictionary"
        )
        XCTAssertTrue(
            ServerNameGenerator.nouns.contains(String(parts[1])),
            "\(parts[1]) is not in noun dictionary"
        )
    }

    func test_generate_avoidsExistingNames() {
        var rng = SeededRNG(state: 42)
        var taken: Set<String> = []
        for _ in 0..<50 {
            let next = ServerNameGenerator.generate(avoiding: taken, using: &rng)
            XCTAssertFalse(taken.contains(next), "Got duplicate \(next)")
            taken.insert(next)
        }
    }

    func test_generate_fallsBackToNumericSuffixWhenDictionaryExhausted() {
        // Seed `existing` with every adj×noun pair so the function must
        // append a numeric suffix.
        var exhausted: Set<String> = []
        for adj in ServerNameGenerator.adjectives {
            for noun in ServerNameGenerator.nouns {
                exhausted.insert("\(adj)-\(noun)")
            }
        }
        let name = ServerNameGenerator.generate(avoiding: exhausted)
        XCTAssertFalse(exhausted.contains(name))
        XCTAssertTrue(
            name.hasSuffix("-2") || name.contains("-") && name.split(separator: "-").count == 3,
            "Expected numeric-suffix fallback, got \(name)"
        )
    }
}
