import Foundation
import Testing
@testable import UniClipboardModels

@Suite("SyncLoopGuard")
struct SyncLoopGuardTests {
    @Test("not tripped on empty guard")
    func empty() {
        let g = SyncLoopGuard()
        #expect(!g.tripped())
    }

    @Test("not tripped when same direction repeats")
    func sameDirectionRepeated() {
        var g = SyncLoopGuard(window: 30, flipThreshold: 3)
        let t0 = Date(timeIntervalSinceReferenceDate: 1000)
        for i in 0..<10 {
            g.record(.pushed, hash: "AABB", at: t0.addingTimeInterval(Double(i)))
        }
        #expect(!g.tripped())
    }

    @Test("trips on 3 flips of same hash within window")
    func flipsOnSameHash() {
        var g = SyncLoopGuard(window: 30, flipThreshold: 3)
        let t0 = Date(timeIntervalSinceReferenceDate: 1000)
        g.record(.pulled, hash: "AABB", at: t0)
        #expect(!g.tripped())
        g.record(.pushed, hash: "AABB", at: t0.addingTimeInterval(1))
        #expect(!g.tripped())  // 1 flip
        g.record(.pulled, hash: "AABB", at: t0.addingTimeInterval(2))
        #expect(!g.tripped())  // 2 flips
        g.record(.pushed, hash: "AABB", at: t0.addingTimeInterval(3))
        #expect(g.tripped())   // 3 flips → trip
    }

    @Test("flips on different hashes are not counted together")
    func differentHashesDoNotCombine() {
        var g = SyncLoopGuard(window: 30, flipThreshold: 3)
        let t0 = Date(timeIntervalSinceReferenceDate: 1000)
        g.record(.pulled, hash: "AAAA", at: t0)
        g.record(.pushed, hash: "BBBB", at: t0.addingTimeInterval(1))
        g.record(.pulled, hash: "CCCC", at: t0.addingTimeInterval(2))
        g.record(.pushed, hash: "DDDD", at: t0.addingTimeInterval(3))
        #expect(!g.tripped())
    }

    @Test("old events outside window are dropped on record")
    func windowEviction() {
        var g = SyncLoopGuard(window: 5, flipThreshold: 3)
        let t0 = Date(timeIntervalSinceReferenceDate: 1000)
        g.record(.pulled, hash: "AABB", at: t0)
        g.record(.pushed, hash: "AABB", at: t0.addingTimeInterval(1))
        g.record(.pulled, hash: "AABB", at: t0.addingTimeInterval(2))
        // Three flips would normally trip, but new event is > window away —
        // the prior events get evicted at record-time, so only the new one
        // remains and there are 0 flips.
        g.record(.pushed, hash: "AABB", at: t0.addingTimeInterval(100))
        #expect(!g.tripped())
        #expect(g.snapshot.count == 1)
    }

    @Test("hash is case-normalized on record")
    func caseInsensitive() {
        var g = SyncLoopGuard(window: 30, flipThreshold: 3)
        let t0 = Date(timeIntervalSinceReferenceDate: 1000)
        g.record(.pulled, hash: "aabb", at: t0)
        g.record(.pushed, hash: "AABB", at: t0.addingTimeInterval(1))
        g.record(.pulled, hash: "AaBb", at: t0.addingTimeInterval(2))
        g.record(.pushed, hash: "aaBB", at: t0.addingTimeInterval(3))
        #expect(g.tripped())
    }

    @Test("nil and empty hash are ignored")
    func nilAndEmptyHashIgnored() {
        var g = SyncLoopGuard(window: 30, flipThreshold: 3)
        let t0 = Date(timeIntervalSinceReferenceDate: 1000)
        g.record(.pulled, hash: nil, at: t0)
        g.record(.pushed, hash: "", at: t0.addingTimeInterval(1))
        g.record(.pulled, hash: nil, at: t0.addingTimeInterval(2))
        #expect(g.snapshot.isEmpty)
        #expect(!g.tripped())
    }

    @Test("reset clears the buffer and untrips")
    func resetClears() {
        var g = SyncLoopGuard(window: 30, flipThreshold: 3)
        let t0 = Date(timeIntervalSinceReferenceDate: 1000)
        g.record(.pulled, hash: "AABB", at: t0)
        g.record(.pushed, hash: "AABB", at: t0.addingTimeInterval(1))
        g.record(.pulled, hash: "AABB", at: t0.addingTimeInterval(2))
        g.record(.pushed, hash: "AABB", at: t0.addingTimeInterval(3))
        #expect(g.tripped())
        g.reset()
        #expect(!g.tripped())
        #expect(g.snapshot.isEmpty)
    }
}
