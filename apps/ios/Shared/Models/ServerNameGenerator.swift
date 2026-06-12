import Foundation

/// Generates short, friendly server aliases of the form `happy-otter` —
/// adjective + animal noun, joined by a hyphen. Style picked during the
/// multi-server UX redesign so the toolbar chip can avoid leaking full
/// server URLs to the user.
public enum ServerNameGenerator {
    public static let adjectives: [String] = [
        "happy", "quiet", "bright", "curious", "swift", "brave",
        "calm", "clever", "eager", "gentle", "jolly", "lucky",
        "mighty", "polite", "proud", "silent", "smart", "sunny",
        "wise", "bold", "witty", "lively",
    ]

    public static let nouns: [String] = [
        "otter", "falcon", "sparrow", "fox", "badger", "panda",
        "koala", "lion", "wolf", "rabbit", "owl", "dolphin",
        "raven", "hawk", "hare", "lynx", "heron", "ibex",
        "marmot", "puma", "finch", "gecko",
    ]

    /// Pick a random `<adjective>-<noun>` that is not in `existing`.
    ///
    /// Strategy: a few random throws (the common case on first use),
    /// then fall back to a deterministic scan of the cartesian product
    /// so a collision can't return the same alias twice. If the entire
    /// dictionary is exhausted (484 unique names), tack a numeric
    /// suffix on the first pair so we still return *something*.
    public static func generate<R: RandomNumberGenerator>(
        avoiding existing: Set<String> = [],
        using rng: inout R
    ) -> String {
        for _ in 0..<8 {
            let candidate = "\(adjectives.randomElement(using: &rng)!)-\(nouns.randomElement(using: &rng)!)"
            if !existing.contains(candidate) { return candidate }
        }
        for adj in adjectives {
            for noun in nouns {
                let candidate = "\(adj)-\(noun)"
                if !existing.contains(candidate) { return candidate }
            }
        }
        let base = "\(adjectives[0])-\(nouns[0])"
        var n = 2
        while existing.contains("\(base)-\(n)") { n += 1 }
        return "\(base)-\(n)"
    }

    public static func generate(avoiding existing: Set<String> = []) -> String {
        var rng = SystemRandomNumberGenerator()
        return generate(avoiding: existing, using: &rng)
    }
}
