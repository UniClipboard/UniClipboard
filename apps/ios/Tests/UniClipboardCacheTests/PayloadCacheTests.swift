import XCTest
@testable import UniClipboardCache

final class PayloadCacheTests: XCTestCase {

    var tempDir: URL!

    override func setUp() async throws {
        try await super.setUp()
        tempDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("PayloadCacheTests-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: tempDir, withIntermediateDirectories: true)
    }

    override func tearDown() async throws {
        if let tempDir { try? FileManager.default.removeItem(at: tempDir) }
        tempDir = nil
        try await super.tearDown()
    }

    private func makeCache(maxBytes: Int = 1_000_000, concurrencyLimit: Int = 3) -> PayloadCache {
        PayloadCache(directory: tempDir, maxBytes: maxBytes, concurrencyLimit: concurrencyLimit)
    }

    // MARK: basic read/write

    func test_writeThenRead_returnsSameBytes() async throws {
        let cache = makeCache()
        let bytes = Data([0x01, 0x02, 0x03, 0x04])
        try await cache.write(profileId: "Image-AAAA", bytes: bytes)
        let readBack = await cache.read(profileId: "Image-AAAA")
        XCTAssertEqual(readBack, bytes)
    }

    func test_read_missingProfileId_returnsNil() async {
        let cache = makeCache()
        let result = await cache.read(profileId: "Image-DOESNTEXIST")
        XCTAssertNil(result)
    }

    func test_write_isAtomic_noTmpLeftOnSuccess() async throws {
        let cache = makeCache()
        try await cache.write(profileId: "Image-AAAA", bytes: Data([0xff]))
        let entries = try FileManager.default.contentsOfDirectory(atPath: tempDir.path)
        // Exactly one regular file with our key as the name. No `.tmp`, no
        // hidden Foundation scratch artifacts.
        XCTAssertEqual(entries.count, 1)
        XCTAssertEqual(entries.first, "Image-AAAA")
    }

    func test_write_setsExcludeFromBackupAttribute() async throws {
        let cache = makeCache()
        try await cache.write(profileId: "Image-AAAA", bytes: Data([0xab]))
        let url = tempDir.appendingPathComponent("Image-AAAA")
        let values = try url.resourceValues(forKeys: [.isExcludedFromBackupKey])
        XCTAssertEqual(values.isExcludedFromBackup, true)
    }

    func test_write_overExistingFile_replacesAtomically() async throws {
        let cache = makeCache()
        try await cache.write(profileId: "Image-AAAA", bytes: Data([0x01]))
        try await cache.write(profileId: "Image-AAAA", bytes: Data([0x02, 0x03]))
        let readBack = await cache.read(profileId: "Image-AAAA")
        XCTAssertEqual(readBack, Data([0x02, 0x03]))
    }

    func test_delete_removesFile() async throws {
        let cache = makeCache()
        try await cache.write(profileId: "Image-AAAA", bytes: Data([0xab]))
        await cache.delete(profileId: "Image-AAAA")
        let readBack = await cache.read(profileId: "Image-AAAA")
        XCTAssertNil(readBack)
    }

    func test_delete_missingProfileId_isNoOp() async {
        let cache = makeCache()
        // No throw, no crash.
        await cache.delete(profileId: "Image-NOPE")
    }

    func test_purgeAll_emptiesDirectory() async throws {
        let cache = makeCache()
        try await cache.write(profileId: "Image-A", bytes: Data([0x01]))
        try await cache.write(profileId: "Image-B", bytes: Data([0x02]))
        try await cache.write(profileId: "Image-C", bytes: Data([0x03]))
        await cache.purgeAll()
        let entries = try FileManager.default.contentsOfDirectory(atPath: tempDir.path)
        XCTAssertEqual(entries.count, 0)
    }

    func test_totalSize_sumsFileSizes() async throws {
        let cache = makeCache()
        try await cache.write(profileId: "Image-A", bytes: Data(repeating: 0x01, count: 100))
        try await cache.write(profileId: "Image-B", bytes: Data(repeating: 0x02, count: 250))
        let total = await cache.totalSize()
        XCTAssertEqual(total, 350)
    }

    // MARK: mtime

    func test_read_touchesMtime() async throws {
        let cache = makeCache()
        try await cache.write(profileId: "Image-A", bytes: Data([0x01]))
        let path = tempDir.appendingPathComponent("Image-A").path
        // `URL.resourceValues(...)` caches across reads from the SAME URL
        // instance — querying once and checking again returns the stale value
        // even if the file's mtime changed. `FileManager.attributesOfItem`
        // hits the filesystem every call, so we use that for verification.
        let before = try FileManager.default.attributesOfItem(atPath: path)[.modificationDate] as! Date
        try await Task.sleep(for: .milliseconds(1100))
        _ = await cache.read(profileId: "Image-A")
        let after = try FileManager.default.attributesOfItem(atPath: path)[.modificationDate] as! Date
        XCTAssertGreaterThan(after, before)
    }

    // MARK: LRU

    func test_LRU_evictsOldestUntilUnderCap() async throws {
        // Cap of 200 bytes. Write three 100-byte entries. After the third
        // write, total = 300 > 200 → evict oldest (A) → total = 200.
        let cache = makeCache(maxBytes: 200)
        try await cache.write(profileId: "Image-A", bytes: Data(repeating: 0x01, count: 100))
        try await Task.sleep(for: .milliseconds(50))
        try await cache.write(profileId: "Image-B", bytes: Data(repeating: 0x02, count: 100))
        try await Task.sleep(for: .milliseconds(50))
        try await cache.write(profileId: "Image-C", bytes: Data(repeating: 0x03, count: 100))

        let bytesA = await cache.read(profileId: "Image-A")
        let bytesB = await cache.read(profileId: "Image-B")
        let bytesC = await cache.read(profileId: "Image-C")
        XCTAssertNil(bytesA, "oldest entry should have been evicted")
        XCTAssertNotNil(bytesB)
        XCTAssertNotNil(bytesC)
    }

    func test_LRU_doesNotEvictWhenUnderCap() async throws {
        // Cap of 250_000 bytes. Write four 50_000-byte entries → total
        // 200_000 ≤ cap. Nothing evicted.
        let cache = makeCache(maxBytes: 250_000)
        for key in ["Image-A", "Image-B", "Image-C", "Image-D"] {
            try await cache.write(profileId: key, bytes: Data(repeating: 0x42, count: 50_000))
        }
        for key in ["Image-A", "Image-B", "Image-C", "Image-D"] {
            let bytes = await cache.read(profileId: key)
            XCTAssertNotNil(bytes, "\(key) should still be present")
        }
    }

    // MARK: fetchAndStore

    func test_fetchAndStore_dedupsConcurrentCallers() async throws {
        let cache = makeCache()
        let counter = Counter()
        let latch = Latch()

        async let r1 = cache.fetchAndStore(profileId: "Image-DEDUP") { @Sendable in
            await counter.increment()
            await latch.wait()
            return Data([0xde, 0xdc])
        }
        async let r2 = cache.fetchAndStore(profileId: "Image-DEDUP") { @Sendable in
            await counter.increment()
            await latch.wait()
            return Data([0xff, 0xff])
        }

        try await Task.sleep(for: .milliseconds(50))
        await latch.release()

        let bytes1 = try await r1
        let bytes2 = try await r2
        let count = await counter.value()

        XCTAssertEqual(count, 1, "fetcher should run exactly once for racing callers")
        XCTAssertEqual(bytes1, bytes2, "both callers should see the same bytes")
    }

    func test_fetchAndStore_returnsCachedOnSecondCall() async throws {
        let cache = makeCache()
        let counter = Counter()

        let first = try await cache.fetchAndStore(profileId: "Image-CACHED") { @Sendable in
            await counter.increment()
            return Data([0x10, 0x20])
        }
        let second = try await cache.fetchAndStore(profileId: "Image-CACHED") { @Sendable in
            await counter.increment()
            return Data([0xff, 0xff])
        }
        let count = await counter.value()

        XCTAssertEqual(first, Data([0x10, 0x20]))
        XCTAssertEqual(second, Data([0x10, 0x20]), "second call should read from disk, not re-fetch")
        XCTAssertEqual(count, 1, "fetcher should not run when bytes are already cached")
    }

    func test_fetchAndStore_throttlesConcurrency() async throws {
        let cache = makeCache(concurrencyLimit: 3)
        let counter = ConcurrencyCounter()
        let latch = Latch()

        await withTaskGroup(of: Void.self) { group in
            for i in 0..<4 {
                group.addTask {
                    _ = try? await cache.fetchAndStore(profileId: "Image-T\(i)") { @Sendable in
                        await counter.enter()
                        await latch.wait()
                        await counter.leave()
                        return Data([UInt8(i)])
                    }
                }
            }
            try? await Task.sleep(for: .milliseconds(100))
            let peak = await counter.peak()
            XCTAssertLessThanOrEqual(peak, 3, "no more than 3 fetchers should be running at once")
            XCTAssertGreaterThanOrEqual(peak, 3, "the first 3 fetchers should be running (sanity)")
            await latch.release()
        }
    }

    // MARK: setMaxBytes (PR 3 — settings-driven cap)

    func test_setMaxBytes_shrinkBelowOccupancy_evictsImmediately() async throws {
        // Cap 1 MiB, write three 200-byte entries → total 600 ≤ cap. Then
        // shrink to 250 bytes → expect oldest two evicted (only Image-C
        // survives; 200 ≤ 250).
        let cache = makeCache(maxBytes: 1_000_000)
        try await cache.write(profileId: "Image-A", bytes: Data(repeating: 0x01, count: 200))
        try await Task.sleep(for: .milliseconds(50))
        try await cache.write(profileId: "Image-B", bytes: Data(repeating: 0x02, count: 200))
        try await Task.sleep(for: .milliseconds(50))
        try await cache.write(profileId: "Image-C", bytes: Data(repeating: 0x03, count: 200))

        await cache.setMaxBytes(250)

        let bytesA = await cache.read(profileId: "Image-A")
        let bytesB = await cache.read(profileId: "Image-B")
        let bytesC = await cache.read(profileId: "Image-C")
        let cap = await cache.currentMaxBytes()
        XCTAssertNil(bytesA)
        XCTAssertNil(bytesB)
        XCTAssertNotNil(bytesC)
        XCTAssertEqual(cap, 250)
    }

    func test_setMaxBytes_grow_keepsAllEntries() async throws {
        // Cap 600, write three 200-byte entries → exactly at cap. Then
        // grow to 10 MiB → nothing evicted.
        let cache = makeCache(maxBytes: 600)
        try await cache.write(profileId: "Image-A", bytes: Data(repeating: 0x01, count: 200))
        try await cache.write(profileId: "Image-B", bytes: Data(repeating: 0x02, count: 200))
        try await cache.write(profileId: "Image-C", bytes: Data(repeating: 0x03, count: 200))

        await cache.setMaxBytes(10 * 1024 * 1024)

        let bytesA = await cache.read(profileId: "Image-A")
        let bytesB = await cache.read(profileId: "Image-B")
        let bytesC = await cache.read(profileId: "Image-C")
        XCTAssertNotNil(bytesA)
        XCTAssertNotNil(bytesB)
        XCTAssertNotNil(bytesC)
    }

    // MARK: key validation

    func test_invalidKey_writeThrows() async {
        let cache = makeCache()
        for bad in ["", "../escape", "with/slash", "with\\backslash", ".", ".."] {
            do {
                try await cache.write(profileId: bad, bytes: Data([0x00]))
                XCTFail("should have rejected \(bad.debugDescription)")
            } catch PayloadCache.CacheError.invalidKey {
                // expected
            } catch {
                XCTFail("unexpected error for \(bad.debugDescription): \(error)")
            }
        }
    }
}

// MARK: - Test helpers

private actor Counter {
    private var n = 0
    func increment() { n += 1 }
    func value() -> Int { n }
}

private actor ConcurrencyCounter {
    private var current = 0
    private var maxSeen = 0
    func enter() {
        current += 1
        if current > maxSeen { maxSeen = current }
    }
    func leave() {
        current -= 1
    }
    func peak() -> Int { maxSeen }
}

/// One-shot async latch. Callers `wait()` and suspend until the first call to
/// `release()` resumes all of them.
private actor Latch {
    private var released = false
    private var waiters: [CheckedContinuation<Void, Never>] = []
    func wait() async {
        if released { return }
        await withCheckedContinuation { cont in
            waiters.append(cont)
        }
    }
    func release() {
        released = true
        let snapshot = waiters
        waiters.removeAll()
        for c in snapshot { c.resume() }
    }
}
