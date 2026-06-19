// Spike demo: exercise the uc-mobile UniFFI surface from Swift on an iOS
// simulator. Deliberately a bare command-line binary (spawned via
// `simctl spawn`), NOT a target inside the production uc-ios app — the spike
// plan keeps the pipeline proof out of product code.
//
// B1 probes (always run):
//   1. sync pure function through FFI (golden vector from spec §7.1)
//   2. Rust Err -> Swift throw with the mapped error case
//   3. with_foreign PlatformBridge as a constructor argument, and the
//      Rust -> Swift callback round trip (spike seam 2)
//
// B2 probes (run when argv[1] is a connect URI from `uniclip mobile-sync
// setup`; the simulator shares the host network stack, so a daemon bound on
// the host loopback is directly reachable):
//   4. async put_clipboard against the real daemon (tokio on device)
//   5. async get_latest round-trips the text written in (4)
//   6. wrong password surfaces as SyncError.Unauthorized
//   7. tls_probe completes a real TLS handshake (ring provider, seam 1)
//
// Prints UC-MOBILE-B1-DEMO-OK / UC-MOBILE-B2-DEMO-OK and exits 0 on success.

import Foundation

func fail(_ msg: String) -> Never {
    print("DEMO-FAIL: \(msg)")
    exit(1)
}

// ─── B1: sync FFI probes ────────────────────────────────────────────────

// Golden vector — byte-identical to uc-mobile-proto spec §7.1.
let goldenUri = "uniclipboard://connect?v=1&svc=mobile-sync&p=eyJ2IjoxLCJ1cmwiOiJodHRwOi8vMTkyLjE2OC4xLjU6NDI3MjAiLCJ1c2VyIjoibW9iaWxlX2FhYmJjY2RkIiwicHdkIjoiQWJDZEVmR2hJaktsTW5PcFFyU3QiLCJvIjp7ImRpZCI6ImRpZF8wMTIzYWJjZCIsImxhYmVsIjoiVGVzdCIsInByb3RvIjoic3luY2NsaXBib2FyZCJ9fQ"

let golden: ConnectPayload
do {
    golden = try parseConnectUri(uri: goldenUri)
} catch {
    fail("golden vector failed to parse: \(error)")
}
guard golden.v == 1,
      golden.url == "http://192.168.1.5:42720",
      golden.urls.isEmpty,
      golden.user == "mobile_aabbccdd",
      golden.pwd == "AbCdEfGhIjKlMnOpQrSt",
      golden.other["did"] == "did_0123abcd",
      golden.other["label"] == "Test",
      golden.other["proto"] == "syncclipboard",
      golden.other.count == 3
else {
    fail("golden vector field mismatch: \(golden)")
}
print("1/7 parseConnectUri golden vector OK")

do {
    _ = try parseConnectUri(uri: "uniclip://connect?v=1&svc=mobile-sync&p=eyJ2IjoxfQ")
    fail("uniclip:// alias must be rejected")
} catch ConnectUriError.InvalidScheme {
    print("2/7 error mapping OK: InvalidScheme")
} catch {
    fail("expected ConnectUriError.InvalidScheme, got \(error)")
}

final class DemoBridge: PlatformBridge {
    func appGroupDir() -> String { "group.app.uniclipboard.demo" }
}

// Seam 1 gate + seam 2 constructor probe.
ucMobileInit()
let client: MobileSyncClient
do {
    client = try MobileSyncClient(bridge: DemoBridge())
} catch {
    fail("constructor failed after ucMobileInit(): \(error)")
}
guard client.bridgeProbe() == "group.app.uniclipboard.demo" else {
    fail("bridge round trip returned unexpected value")
}
print("3/7 ucMobileInit + PlatformBridge constructor + round trip OK")

// ─── B2: async probes against a real daemon ─────────────────────────────

let args = CommandLine.arguments
guard args.count >= 2 else {
    print("UC-MOBILE-B1-DEMO-OK")
    print("(no connect URI argument; skipping B2 daemon probes)")
    exit(0)
}

let payload: ConnectPayload
do {
    payload = try parseConnectUri(uri: args[1])
} catch {
    fail("argv[1] is not a valid connect URI: \(error)")
}
let server = ServerConfig(
    baseUrl: payload.url,
    username: payload.user,
    password: payload.pwd
)
let tlsUrl = args.count >= 3 ? args[2] : "https://www.apple.com"

let marker = "uc-mobile B2 demo \(UInt64(Date().timeIntervalSince1970 * 1000))"

await_main: do {
    let semaphore = DispatchSemaphore(value: 0)
    Task {
        defer { semaphore.signal() }
        do {
            // 4. async PUT through tokio-on-device.
            let meta = ClipboardMeta(
                kind: .text,
                text: marker,
                dataName: nil,
                hasData: false,
                size: UInt64(marker.utf8.count),
                hash: nil
            )
            try await client.putClipboard(server: server, meta: meta, payload: nil)
            print("4/7 async put_clipboard against real daemon OK")

            // 5. GET must round-trip the marker once the daemon materializes
            // the inbound write (poll up to ~10s).
            var roundTripped = false
            for _ in 0..<50 {
                let latest = try await client.getLatest(server: server)
                if latest.kind == .text && latest.text == marker {
                    roundTripped = true
                    break
                }
                try await Task.sleep(nanoseconds: 200_000_000)
            }
            guard roundTripped else {
                fail("get_latest never returned the marker text written by put_clipboard")
            }
            print("5/7 async get_latest round-trips the marker OK")

            // 6. Wrong password must surface as Unauthorized.
            let badServer = ServerConfig(
                baseUrl: server.baseUrl,
                username: server.username,
                password: "definitely-wrong"
            )
            do {
                _ = try await client.getLatest(server: badServer)
                fail("wrong password must be rejected")
            } catch SyncError.Unauthorized {
                print("6/7 wrong password maps to SyncError.Unauthorized OK")
            }

            // 7. Real TLS handshake through the ring provider installed by
            // ucMobileInit(); any HTTP status proves the handshake completed.
            let status = try await client.tlsProbe(url: tlsUrl)
            print("7/7 tls_probe handshake OK (\(tlsUrl) -> HTTP \(status))")
        } catch {
            fail("B2 probe failed: \(error)")
        }
    }
    semaphore.wait()
}

print("UC-MOBILE-B2-DEMO-OK")
