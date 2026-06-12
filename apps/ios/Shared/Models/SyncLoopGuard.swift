import Foundation

/// Cycle-detection state machine for the auto-sync loop. Pure value type so
/// it lives in `Shared/Models` and is testable via `swift test` without
/// pulling in UIKit / SwiftUI.
///
/// The engine `record(_:hash:at:)`s every successful Apply (`.pulled`) and
/// Push (`.pushed`) it performs. When the SAME hash flips between Apply
/// and Push more than `flipThreshold` times inside `window`, the guard
/// `tripped()` returns true and the engine should park itself in a
/// loop-detected state and stop auto-syncing until the user acknowledges.
///
/// Why count flips, not absolute counts: a healthy engine may legitimately
/// see N Pushes of the same hash in a row (e.g., on a flaky server that
/// keeps dropping the entry). It may also see N Pulls in a row (auto-apply
/// off, server keeps re-emitting). Only an *alternating* pattern is the
/// real pong — apply writes to pasteboard, pasteboard echoes back as a
/// push, push lands on server, server echoes back as a pull, etc.
public struct SyncLoopGuard: Equatable, Sendable {
    public enum Direction: String, Codable, Sendable {
        case pulled  // server → device (apply)
        case pushed  // device → server (push)
    }

    public struct Event: Equatable, Sendable {
        public let hash: String
        public let direction: Direction
        public let at: Date

        public init(hash: String, direction: Direction, at: Date) {
            self.hash = hash.uppercased()
            self.direction = direction
            self.at = at
        }
    }

    /// Time window in which flips are counted. 30s covers the worst
    /// realistic case (1Hz tick → ~30 alternations) and is short enough
    /// that an old harmless flip from earlier in the session doesn't keep
    /// the breaker armed forever.
    public let window: TimeInterval

    /// Minimum number of *flips* (Apply→Push or Push→Apply transitions) on
    /// the same hash inside `window` before tripping. A flip count of 3
    /// means at least 4 events of the same hash with alternating direction:
    /// e.g., Pulled → Pushed → Pulled → Pushed.
    public let flipThreshold: Int

    private var events: [Event] = []

    public init(window: TimeInterval = 30.0, flipThreshold: Int = 3) {
        self.window = window
        self.flipThreshold = flipThreshold
    }

    /// Append a sync event. Drops anything older than `window` relative to
    /// `at` so the buffer stays bounded by the cadence.
    public mutating func record(_ direction: Direction, hash: String?, at: Date = .now) {
        guard let hash, !hash.isEmpty else { return }
        let cutoff = at.addingTimeInterval(-window)
        events.removeAll { $0.at < cutoff }
        events.append(Event(hash: hash, direction: direction, at: at))
    }

    /// `true` when any single hash has alternated direction at least
    /// `flipThreshold` times inside the current window. Idempotent — does
    /// not mutate state.
    public func tripped() -> Bool {
        let grouped = Dictionary(grouping: events, by: \.hash)
        for (_, group) in grouped {
            let sorted = group.sorted { $0.at < $1.at }
            var flips = 0
            var lastDir: Direction?
            for ev in sorted {
                if let prev = lastDir, prev != ev.direction { flips += 1 }
                lastDir = ev.direction
            }
            if flips >= flipThreshold { return true }
        }
        return false
    }

    /// Wipe the buffer. Call after the user acknowledges the loop-detected
    /// banner — otherwise the next legitimate sync would re-trip the
    /// breaker by inheriting yesterday's events.
    public mutating func reset() {
        events.removeAll()
    }

    /// Test-only inspector — exposes the buffered events without forcing
    /// callers to know the storage shape. Read-only.
    public var snapshot: [Event] { events }
}
