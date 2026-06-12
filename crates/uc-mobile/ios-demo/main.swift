// B1 demo: exercise the uc-mobile UniFFI surface from Swift on an iOS
// simulator. Deliberately a bare command-line binary (spawned via
// `simctl spawn`), NOT a target inside the production uc-ios app — the spike
// plan keeps the pipeline proof out of product code.
//
// Asserted here:
//   1. sync pure function through FFI (golden vector from spec §7.1)
//   2. Rust Err -> Swift throw with the mapped error case
//   3. with_foreign PlatformBridge as a constructor argument, and the
//      Rust -> Swift callback round trip (spike seam 2)
//
// Prints UC-MOBILE-B1-DEMO-OK and exits 0 on success; exits non-zero with a
// message on any mismatch.

import Foundation

func fail(_ msg: String) -> Never {
    print("DEMO-FAIL: \(msg)")
    exit(1)
}

// Golden vector — byte-identical to uc-mobile-proto spec §7.1.
let goldenUri = "uniclipboard://connect?v=1&svc=mobile-sync&p=eyJ2IjoxLCJ1cmwiOiJodHRwOi8vMTkyLjE2OC4xLjU6NDI3MjAiLCJ1c2VyIjoibW9iaWxlX2FhYmJjY2RkIiwicHdkIjoiQWJDZEVmR2hJaktsTW5PcFFyU3QiLCJvIjp7ImRpZCI6ImRpZF8wMTIzYWJjZCIsImxhYmVsIjoiVGVzdCIsInByb3RvIjoic3luY2NsaXBib2FyZCJ9fQ"

// 1. sync pure function through the FFI
let payload: ConnectPayload
do {
    payload = try parseConnectUri(uri: goldenUri)
} catch {
    fail("golden vector failed to parse: \(error)")
}
guard payload.v == 1,
      payload.url == "http://192.168.1.5:42720",
      payload.urls.isEmpty,
      payload.user == "mobile_aabbccdd",
      payload.pwd == "AbCdEfGhIjKlMnOpQrSt",
      payload.other["did"] == "did_0123abcd",
      payload.other["label"] == "Test",
      payload.other["proto"] == "syncclipboard",
      payload.other.count == 3
else {
    fail("golden vector field mismatch: \(payload)")
}
print("1/3 parseConnectUri golden vector OK")

// 2. Rust Err surfaces as a typed Swift throw
do {
    _ = try parseConnectUri(uri: "uniclip://connect?v=1&svc=mobile-sync&p=eyJ2IjoxfQ")
    fail("uniclip:// alias must be rejected")
} catch let e as ConnectUriError {
    print("2/3 error mapping OK: \(e)")
} catch {
    fail("expected ConnectUriError, got \(error)")
}

// 3. with_foreign bridge as constructor argument + Rust->Swift round trip
final class DemoBridge: PlatformBridge {
    func appGroupDir() -> String { "group.app.uniclipboard.demo" }
}
let client = MobileSyncClient(bridge: DemoBridge())
guard client.bridgeProbe() == "group.app.uniclipboard.demo" else {
    fail("bridge round trip returned unexpected value")
}
print("3/3 PlatformBridge constructor + round trip OK")

print("UC-MOBILE-B1-DEMO-OK")
