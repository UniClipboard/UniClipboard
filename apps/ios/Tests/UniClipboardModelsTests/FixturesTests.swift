import XCTest
@testable import UniClipboardModels

final class FixturesTests: XCTestCase {

    // MARK: - helpers

    /// Resolves to the repo's `docs/examples/` directory at test-runtime via
    /// the source location of this file. Keeps the fixtures single-sourced
    /// (no copy/symlink dance) and lets `swift test` work from the repo root.
    private static let fixturesDir: URL = {
        URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()                // Tests/UniClipboardModelsTests/
            .deletingLastPathComponent()                // Tests/
            .deletingLastPathComponent()                // <repo root>/
            .appendingPathComponent("docs/examples", isDirectory: true)
    }()

    private func loadFixture(_ name: String) throws -> Data {
        let url = Self.fixturesDir.appendingPathComponent("\(name).json")
        guard FileManager.default.fileExists(atPath: url.path) else {
            XCTFail("Fixture not found at \(url.path)")
            return Data()
        }
        return try Data(contentsOf: url)
    }

    private func keys(_ data: Data) throws -> Set<String> {
        let obj = try JSONSerialization.jsonObject(with: data) as? [String: Any] ?? [:]
        return Set(obj.keys)
    }

    private func nestedConfigs(_ data: Data) throws -> [[String: Any]] {
        let obj = try JSONSerialization.jsonObject(with: data) as? [String: Any] ?? [:]
        return obj["configs"] as? [[String: Any]] ?? []
    }

    // MARK: - Clipboard wire fixtures (§3)

    func test_clipboardTextShort_decodes_and_roundTripsWithoutNullKeys() throws {
        let data = try loadFixture("clipboard_text_short")
        let entry = try JSONDecoder().decode(Clipboard.self, from: data)

        XCTAssertEqual(entry.type, .text)
        XCTAssertEqual(entry.hash,
            "3F4E62D9F184380BAD1B0F94B5518DCBF35ACB79B34F6D6E34F3DAB16CD7BC8F")
        XCTAssertEqual(entry.text, "Hello, SyncClipboard!")
        XCTAssertEqual(entry.size, 21)
        XCTAssertFalse(entry.hasData)
        XCTAssertNil(entry.dataName)

        let re = try JSONEncoder().encode(entry)
        XCTAssertFalse(String(data: re, encoding: .utf8)!.contains("null"))
        XCTAssertEqual(try keys(re), ["type", "hash", "text", "hasData", "size"])
    }

    func test_clipboardTextLong_hasDataTrueWithDataName() throws {
        let data = try loadFixture("clipboard_text_long")
        let entry = try JSONDecoder().decode(Clipboard.self, from: data)

        XCTAssertEqual(entry.type, .text)
        XCTAssertTrue(entry.hasData)
        XCTAssertEqual(entry.dataName,
            "text_B7E8C3D4F5A6071829304152637485A6B7C8D9E0F1A2B3C4D5E6F70819203142.txt")
        XCTAssertEqual(entry.size, 23457)

        let re = try JSONEncoder().encode(entry)
        XCTAssertEqual(try keys(re), ["type", "hash", "text", "hasData", "dataName", "size"])
    }

    func test_clipboardImage_decodesImageKind() throws {
        let data = try loadFixture("clipboard_image")
        let entry = try JSONDecoder().decode(Clipboard.self, from: data)

        XCTAssertEqual(entry.type, .image)
        XCTAssertEqual(entry.dataName, "photo_2026.png")
        XCTAssertEqual(entry.size, 184320)
        XCTAssertTrue(entry.hasData)

        let re = try JSONEncoder().encode(entry)
        XCTAssertEqual(try keys(re), ["type", "hash", "text", "hasData", "dataName", "size"])
    }

    func test_clipboardFile_decodesFileKind() throws {
        let data = try loadFixture("clipboard_file")
        let entry = try JSONDecoder().decode(Clipboard.self, from: data)

        XCTAssertEqual(entry.type, .file)
        XCTAssertEqual(entry.dataName, "report.pdf")

        let re = try JSONEncoder().encode(entry)
        XCTAssertEqual(try keys(re), ["type", "hash", "text", "hasData", "dataName", "size"])
    }

    func test_clipboardGroup_decodesGroupKind() throws {
        let data = try loadFixture("clipboard_group")
        let entry = try JSONDecoder().decode(Clipboard.self, from: data)

        XCTAssertEqual(entry.type, .group)
        XCTAssertEqual(entry.dataName, "screenshots.zip")

        let re = try JSONEncoder().encode(entry)
        XCTAssertEqual(try keys(re), ["type", "hash", "text", "hasData", "dataName", "size"])
    }

    func test_clipboardNoHash_optionalKeysAreOmittedNotNullified() throws {
        let data = try loadFixture("clipboard_no_hash")
        let entry = try JSONDecoder().decode(Clipboard.self, from: data)

        XCTAssertEqual(entry.type, .text)
        XCTAssertNil(entry.hash)
        XCTAssertNil(entry.dataName)
        XCTAssertNil(entry.size)
        XCTAssertFalse(entry.hasData)

        let re = try JSONEncoder().encode(entry)
        let str = String(data: re, encoding: .utf8)!
        XCTAssertFalse(str.contains("null"),
            "optional fields must be omitted, not encoded as null")
        XCTAssertEqual(try keys(re), ["type", "text", "hasData"])
    }

    func test_clipboard_hashWhitespaceNormalizesToNil() throws {
        let json = #"{"type":"Text","hash":"   ","text":"x","hasData":false}"#
        let entry = try JSONDecoder().decode(Clipboard.self, from: Data(json.utf8))
        XCTAssertNil(entry.hash)
    }

    func test_clipboard_kindRawValuesMatchWire() {
        XCTAssertEqual(Clipboard.Kind.text.rawValue,  "Text")
        XCTAssertEqual(Clipboard.Kind.image.rawValue, "Image")
        XCTAssertEqual(Clipboard.Kind.file.rawValue,  "File")
        XCTAssertEqual(Clipboard.Kind.group.rawValue, "Group")
    }

    func test_hashMatches_nilOrEmptyExpectedMatchesAnything() {
        XCTAssertTrue(Clipboard.hashMatches(expected: nil, actual: "DEADBEEF"))
        XCTAssertTrue(Clipboard.hashMatches(expected: "", actual: "DEADBEEF"))
        XCTAssertTrue(Clipboard.hashMatches(expected: "  ", actual: "DEADBEEF"))
        XCTAssertTrue(Clipboard.hashMatches(expected: "deadbeef", actual: "DEADBEEF"))
        XCTAssertFalse(Clipboard.hashMatches(expected: "AAA", actual: "BBB"))
    }

    // MARK: - ServerConfig persistence fixtures (§5)

    func test_serverConfigList_decodesThreeConfigs() throws {
        let data = try loadFixture("server_config_list")
        let list = try JSONDecoder().decode(ServerConfigList.self, from: data)

        XCTAssertEqual(list.configs.count, 3)
        XCTAssertEqual(list.activeConfigId, "ff112233-4455-6677-8899-aabbccddeeff")

        XCTAssertEqual(list.configs[0].name, "Home NAS")
        XCTAssertEqual(list.configs[0].urls, [
            "https://203-0-113-10.sslip.io",
            "http://192.168.1.10:5033",
            "http://100.64.0.8:5033",
        ])
        XCTAssertEqual(list.configs[0].url, "https://203-0-113-10.sslip.io",
                       "canonical url is urls[0]")
        XCTAssertEqual(list.configs[1].urls, ["http://192.168.10.20:5033"])
        XCTAssertNil(list.configs[2].name)
        XCTAssertEqual(list.configs[2].urls, ["https://clip.example.com"])

        XCTAssertEqual(list.activeConfig?.id, list.configs[2].id)
    }

    func test_serverConfigList_roundTripPreservesMissingNameAsAbsentKey() throws {
        let data = try loadFixture("server_config_list")
        let list = try JSONDecoder().decode(ServerConfigList.self, from: data)
        let re = try JSONEncoder().encode(list)

        XCTAssertFalse(String(data: re, encoding: .utf8)!.contains("\"name\":null"))
        let configs = try nestedConfigs(re)
        XCTAssertEqual(configs.count, 3)
        XCTAssertNotNil(configs[0]["name"])
        XCTAssertNotNil(configs[1]["name"])
        XCTAssertNil(configs[2]["name"], "third config has no name → key must be absent")
    }

    func test_activeConfig_fallsBackToFirstWhenIdIsStale() {
        let cfg = ServerConfig(id: "alpha", url: "http://x", username: "u", password: "p")
        let list = ServerConfigList(configs: [cfg], activeConfigId: "stale-id")
        XCTAssertEqual(list.activeConfig?.id, "alpha")
    }

    func test_activeConfig_isNilWhenConfigsIsEmpty() {
        XCTAssertNil(ServerConfigList().activeConfig)
        XCTAssertNil(ServerConfigList(configs: [], activeConfigId: "anything").activeConfig)
    }

    func test_serverConfig_displayLabelFallsBackToURLWhenNameEmpty() {
        let cfg = ServerConfig(id: "x", name: "  ", url: "http://h", username: "u", password: "p")
        XCTAssertEqual(cfg.displayLabel, "http://h")
    }

    // MARK: - §5.1 codec: urls / legacy url fallback

    func test_serverConfig_decode_urlsTakesPrecedenceOverLegacyUrl() throws {
        let json = #"{"id":"a","url":"https://first","urls":["https://x","http://192.168.0.2"],"username":"u","password":"p"}"#
        let cfg = try JSONDecoder().decode(ServerConfig.self, from: Data(json.utf8))
        XCTAssertEqual(cfg.urls, ["https://x", "http://192.168.0.2"])
        XCTAssertEqual(cfg.url, "https://x", "url accessor is urls[0]")
    }

    func test_serverConfig_decode_fallsBackToLegacyUrlWhenUrlsAbsentOrEmpty() throws {
        let onlyURL = #"{"id":"a","url":"https://h","username":"u","password":"p"}"#
        XCTAssertEqual(try JSONDecoder().decode(ServerConfig.self, from: Data(onlyURL.utf8)).urls, ["https://h"])
        let emptyURLs = #"{"id":"a","url":"https://h","urls":[],"username":"u","password":"p"}"#
        XCTAssertEqual(try JSONDecoder().decode(ServerConfig.self, from: Data(emptyURLs.utf8)).urls, ["https://h"])
    }

    func test_serverConfig_decode_throwsWhenNeitherUrlsNorUrl() {
        let json = #"{"id":"a","username":"u","password":"p"}"#
        XCTAssertThrowsError(try JSONDecoder().decode(ServerConfig.self, from: Data(json.utf8)))
    }

    func test_serverConfig_encode_emitsBothUrlAndUrls() throws {
        let cfg = ServerConfig(id: "a", urls: ["https://x", "http://192.168.0.2"], username: "u", password: "p")
        let obj = try XCTUnwrap(JSONSerialization.jsonObject(with: JSONEncoder().encode(cfg)) as? [String: Any])
        XCTAssertEqual(obj["url"] as? String, "https://x", "url == urls[0] for old readers")
        XCTAssertEqual(obj["urls"] as? [String], ["https://x", "http://192.168.0.2"])
    }

    func test_serverConfig_decode_ignoresLegacyAutoSwitchKeys() throws {
        // The pre-multi-URL per-config auto-switch keys are tolerated on read
        // and never re-encoded — auto-switch is between a profile's URLs now.
        let json = #"{"id":"a","url":"http://a","username":"u","password":"p","autoSwitchWifiNames":["Home"],"autoSwitchStrategy":"wifi"}"#
        let cfg = try JSONDecoder().decode(ServerConfig.self, from: Data(json.utf8))
        XCTAssertEqual(cfg.urls, ["http://a"])
        let reencoded = String(data: try JSONEncoder().encode(cfg), encoding: .utf8)!
        XCTAssertFalse(reencoded.contains("autoSwitch"), "dropped legacy keys must not be re-encoded")
    }

    func test_serverConfig_codable_roundTrips() throws {
        let cfg = ServerConfig(id: "a", name: "N",
                               urls: ["https://x", "http://10.0.0.5"], username: "u", password: "p")
        let back = try JSONDecoder().decode(ServerConfig.self, from: JSONEncoder().encode(cfg))
        XCTAssertEqual(back, cfg)
    }

    // MARK: - §5.1 URL classification (host shape)

    func test_classifyURL() {
        XCTAssertEqual(ServerConfig.classifyURL("http://192.168.1.5:42720"), .lan)
        XCTAssertEqual(ServerConfig.classifyURL("http://10.0.0.5"), .lan)
        XCTAssertEqual(ServerConfig.classifyURL("http://172.16.3.4"), .lan)
        XCTAssertEqual(ServerConfig.classifyURL("http://172.32.0.1"), .wan, "outside the /12")
        XCTAssertEqual(ServerConfig.classifyURL("http://169.254.1.1"), .lan)
        XCTAssertEqual(ServerConfig.classifyURL("https://nas.local:5033"), .lan)
        XCTAssertEqual(ServerConfig.classifyURL("http://100.64.0.5:42720"), .tailscale)
        XCTAssertEqual(ServerConfig.classifyURL("http://100.128.0.5"), .wan, "outside the /10")
        XCTAssertEqual(ServerConfig.classifyURL("https://box.tail-abc.ts.net"), .tailscale)
        XCTAssertEqual(ServerConfig.classifyURL("https://203-0-113-10.sslip.io"), .wan)
        XCTAssertEqual(ServerConfig.classifyURL("https://8.8.8.8"), .wan)
    }

    // MARK: - §5.3 network-based URL ordering

    private func net(_ ssid: String? = nil, wifi: Bool = false,
                     cellular: Bool = false, tailscale: Bool = false) -> NetworkContext {
        NetworkContext(ssid: ssid, isWifi: wifi, isCellular: cellular, isTailscale: tailscale)
    }

    /// A profile reachable at [wan, lan, tailscale] in publisher order.
    private var multiURL: ServerConfig {
        ServerConfig(id: "m",
                     urls: ["https://wan.example.com",
                            "http://192.168.1.10:5033",
                            "http://100.64.0.8:5033"],
                     username: "u", password: "p")
    }

    func test_orderedURLs_onWifiPrefersLAN() {
        XCTAssertEqual(multiURL.orderedURLs(network: net(wifi: true)),
                       ["http://192.168.1.10:5033", "http://100.64.0.8:5033", "https://wan.example.com"])
    }

    func test_orderedURLs_ssidNameAloneCountsAsWifi() {
        // A readable SSID implies on-Wi-Fi even when `isWifi` wasn't populated.
        XCTAssertEqual(multiURL.orderedURLs(network: net("Home")).first, "http://192.168.1.10:5033")
    }

    func test_orderedURLs_offWifiTailscaleLeads() {
        // Off Wi-Fi (e.g. cellular) with Tailscale up → the Tailscale URL leads,
        // then WAN, then the unreachable LAN URL last.
        XCTAssertEqual(multiURL.orderedURLs(network: net(tailscale: true)),
                       ["http://100.64.0.8:5033", "https://wan.example.com", "http://192.168.1.10:5033"])
    }

    func test_orderedURLs_onWifiLANBeatsTailscale() {
        // On Wi-Fi the direct LAN path wins even when Tailscale is also up —
        // probing demotes LAN on a foreign network, so ranking it first is safe.
        XCTAssertEqual(multiURL.orderedURLs(network: net(wifi: true, tailscale: true)),
                       ["http://192.168.1.10:5033", "http://100.64.0.8:5033", "https://wan.example.com"])
    }

    func test_orderedURLs_onCellularPrefersWAN() {
        XCTAssertEqual(multiURL.orderedURLs(network: net(cellular: true)),
                       ["https://wan.example.com", "http://100.64.0.8:5033", "http://192.168.1.10:5033"])
    }

    func test_orderedURLs_noSignalKeepsPublisherOrder() {
        XCTAssertEqual(multiURL.orderedURLs(network: net()), multiURL.urls)
    }

    func test_orderedURLs_stableWithinClass() {
        // Two LAN URLs keep their original relative order on Wi-Fi.
        let cfg = ServerConfig(id: "c",
                               urls: ["http://192.168.1.2", "https://wan.example.com", "http://192.168.1.3"],
                               username: "u", password: "p")
        XCTAssertEqual(cfg.orderedURLs(network: net(wifi: true)),
                       ["http://192.168.1.2", "http://192.168.1.3", "https://wan.example.com"])
    }

    // MARK: - §5.3 preferredURLs (probe verdict layered on shape order)

    func test_preferredURLs_liveLeadsRestFollowShapeOrder() {
        // Live = WAN (e.g. last probe ran on cellular), now on Wi-Fi:
        // the confirmed URL still leads, fallbacks in Wi-Fi shape order.
        XCTAssertEqual(
            multiURL.preferredURLs(live: "https://wan.example.com", network: net(wifi: true)),
            ["https://wan.example.com", "http://192.168.1.10:5033", "http://100.64.0.8:5033"]
        )
    }

    func test_preferredURLs_liveAlreadyFirstIsIdentity() {
        XCTAssertEqual(
            multiURL.preferredURLs(live: "http://192.168.1.10:5033", network: net(wifi: true)),
            multiURL.orderedURLs(network: net(wifi: true))
        )
    }

    func test_preferredURLs_nilLiveFallsBackToShapeOrder() {
        XCTAssertEqual(
            multiURL.preferredURLs(live: nil, network: net(cellular: true)),
            multiURL.orderedURLs(network: net(cellular: true))
        )
    }

    func test_preferredURLs_staleLiveNotInUrlsIsIgnored() {
        // The config was edited since the probe wrote the live URL — the
        // removed candidate must not resurrect.
        XCTAssertEqual(
            multiURL.preferredURLs(live: "http://10.0.0.9:5033", network: net(wifi: true)),
            multiURL.orderedURLs(network: net(wifi: true))
        )
    }

    func test_effectiveActiveConfig_reordersActiveConfigUrls() {
        let list = ServerConfigList(configs: [multiURL], activeConfigId: "m")
        let eff = list.effectiveActiveConfig(network: net(wifi: true))
        XCTAssertEqual(eff?.id, "m")
        XCTAssertEqual(eff?.url, "http://192.168.1.10:5033",
                       "effective url is the network-preferred candidate")
    }

    func test_effectiveActiveConfig_keepsActiveProfileNeverCrossesProfiles() {
        // Auto-switch no longer crosses profiles: the active profile stays put,
        // only its own URLs reorder.
        let a = ServerConfig(id: "a", urls: ["http://10.0.0.1"], username: "u", password: "p")
        let list = ServerConfigList(configs: [a, multiURL], activeConfigId: "a")
        XCTAssertEqual(list.effectiveActiveConfig(network: net(tailscale: true))?.id, "a")
    }

    func test_effectiveActiveConfig_nilWhenEmpty() {
        XCTAssertNil(ServerConfigList().effectiveActiveConfig(network: net(wifi: true)))
    }

    func test_legacyManualOverride_promotedToActiveConfigOnDecode() throws {
        // Pre-unification installs persisted a home-chip "pin" in
        // `manualOverrideConfigId`. On decode it must be promoted to
        // `activeConfigId` (the user's last explicit pick becomes current).
        let json = #"{"configs":[{"id":"a","url":"http://a","username":"u","password":"p"},{"id":"b","url":"http://b","username":"u","password":"p"}],"activeConfigId":"a","manualOverrideConfigId":"b"}"#
        let decoded = try JSONDecoder().decode(ServerConfigList.self, from: Data(json.utf8))
        XCTAssertEqual(decoded.activeConfigId, "b",
                       "a resolvable legacy pin must become the current server")
    }

    func test_legacyManualOverride_ignoredWhenUnresolvableAndNotReEncoded() throws {
        // A phantom legacy pin keeps the persisted activeConfigId, and the
        // old key is never written back out.
        let json = #"{"configs":[{"id":"a","url":"http://a","username":"u","password":"p"}],"activeConfigId":"a","manualOverrideConfigId":"ghost"}"#
        let decoded = try JSONDecoder().decode(ServerConfigList.self, from: Data(json.utf8))
        XCTAssertEqual(decoded.activeConfigId, "a")
        let reencoded = String(data: try JSONEncoder().encode(decoded), encoding: .utf8)!
        XCTAssertFalse(reencoded.contains("manualOverrideConfigId"),
                       "the legacy key must not be re-encoded")
    }

    func test_serverConfig_normalizeSSID_stripsQuotesAndRejectsPlaceholders() {
        XCTAssertEqual(ServerConfig.normalizeSSID("\"Home-5G\""), "Home-5G")
        XCTAssertEqual(ServerConfig.normalizeSSID("  Home-5G  "), "Home-5G")
        XCTAssertNil(ServerConfig.normalizeSSID(""))
        XCTAssertNil(ServerConfig.normalizeSSID(nil))
        XCTAssertNil(ServerConfig.normalizeSSID("<unknown ssid>"))
        XCTAssertNil(ServerConfig.normalizeSSID("0x"))
    }

    func test_effectiveActiveConfig_fromFixtureKeepsActiveProfileAcrossNetworks() throws {
        let data = try loadFixture("server_config_list")
        let list = try JSONDecoder().decode(ServerConfigList.self, from: data)
        let remoteId = "ff112233-4455-6677-8899-aabbccddeeff"
        // The active profile (#3) is the user's pick and never changes with the
        // network now — only a profile's own URL order does. #3 is single-URL,
        // so its effective config is identical on every network.
        XCTAssertEqual(list.effectiveActiveConfig(network: net(wifi: true))?.id, remoteId)
        XCTAssertEqual(list.effectiveActiveConfig(network: net(cellular: true))?.id, remoteId)
        XCTAssertEqual(list.effectiveActiveConfig(network: net())?.id, remoteId)
    }

    func test_orderedURLs_fromFixtureMultiURLConfig() throws {
        let data = try loadFixture("server_config_list")
        let list = try JSONDecoder().decode(ServerConfigList.self, from: data)
        // Home NAS (#1) carries [wan, lan, tailscale]; the network floats the
        // matching candidate to the front.
        XCTAssertEqual(list.configs[0].orderedURLs(network: net(wifi: true)).first,
                       "http://192.168.1.10:5033")
        XCTAssertEqual(list.configs[0].orderedURLs(network: net(tailscale: true)).first,
                       "http://100.64.0.8:5033")
    }

    func test_legacyServerConfig_migrationProducesActiveSingleConfig() throws {
        let data = try loadFixture("server_config_legacy")
        let legacy = try JSONDecoder().decode(LegacyServerConfig.self, from: data)
        XCTAssertEqual(legacy.url, "https://clip.home.lan:5033/")
        XCTAssertEqual(legacy.username, "alice")
        XCTAssertEqual(legacy.password, "p4ssw0rd!")

        let migrated = legacy.migrated(idProvider: { "fixed-uuid" })
        XCTAssertEqual(migrated.configs.count, 1)
        XCTAssertEqual(migrated.activeConfigId, "fixed-uuid")
        XCTAssertEqual(migrated.configs[0].id, "fixed-uuid")
        XCTAssertNil(migrated.configs[0].name)
        XCTAssertEqual(migrated.configs[0].urls, ["https://clip.home.lan:5033/"],
                       "legacy single url becomes the one-element candidate list")
    }

    // MARK: - AppSettings persistence fixtures (§5.4)

    func test_appSettings_full_roundTripsIgnoredVersion() throws {
        let data = try loadFixture("app_settings")
        let settings = try JSONDecoder().decode(AppSettings.self, from: data)

        XCTAssertFalse(settings.trustInsecureCert)
        XCTAssertTrue(settings.autoCheckUpdate)
        XCTAssertTrue(settings.manualUploadDialogShown)
        XCTAssertEqual(settings.downloadRelativePath, "SyncClipboard/Inbox")
        XCTAssertEqual(settings.logViewLevelFilter, "info")
        XCTAssertEqual(settings.ignoredVersion, "0.3.2")

        let re = try JSONEncoder().encode(settings)
        XCTAssertTrue(try keys(re).contains("ignoredVersion"))
    }

    func test_appSettings_minimal_omitsNilIgnoredVersionOnReencode() throws {
        let data = try loadFixture("app_settings_minimal")
        let settings = try JSONDecoder().decode(AppSettings.self, from: data)

        XCTAssertNil(settings.ignoredVersion)

        let re = try JSONEncoder().encode(settings)
        let str = String(data: re, encoding: .utf8)!
        XCTAssertFalse(str.contains("ignoredVersion"))
        XCTAssertFalse(str.contains("null"))
        XCTAssertEqual(try keys(re), [
            "trustInsecureCert", "autoCheckUpdate", "manualUploadDialogShown",
            "downloadRelativePath", "logViewLevelFilter",
            // Cycle 9 — auto-sync engine. Always encoded (Bool, no nil
            // semantics); fixture omits it and decode falls back to true.
            "autoApplyServerChanges",
            // Consent-push cycle — auto-read/push of the device pasteboard.
            // Always encoded (Bool); fixture omits it and decode falls back
            // to false (manual PasteButton push is the default).
            "autoPushDeviceChanges",
            // Cycle 12 — PayloadCache settings. Same Bool/Int defaulting
            // rule: fixture omits them and decode falls back to defaults.
            "prefetchAttachments", "prefetchOnCellular", "payloadCacheMaxBytes",
            // UI appearance preference (light/dark/system). Always encoded
            // as a raw String; decode falls back to .system when missing.
            "appearance",
            // Keyboard-extension feedback toggles. Always encoded (Bool);
            // fixture omits them and decode falls back to true (stock-keyboard
            // feel). Read by the keyboard via the App Group.
            "keyboardSoundFeedback", "keyboardHapticFeedback",
            // First-run onboarding flag. Always encoded (Bool); fixture omits
            // it and decode falls back to false (fresh install shows onboarding).
            "onboardingShown",
            // Home paste-permission hint dismissal. Always encoded (Bool).
            "pastePermissionHintDismissed",
            // Post-pairing enhancements carousel "shown once" flag. Always
            // encoded (Bool); fixture omits it and decode falls back to false.
            "enhancementsPromptShown",
        ])
    }

    func test_appSettings_acceptsUnknownKeys_forwardCompatibility() throws {
        let json = #"""
        {
          "trustInsecureCert": false,
          "autoCheckUpdate": true,
          "manualUploadDialogShown": false,
          "downloadRelativePath": "",
          "logViewLevelFilter": "info",
          "_schemaVersion": 7,
          "futureField": "tolerated"
        }
        """#
        let settings = try JSONDecoder().decode(AppSettings.self, from: Data(json.utf8))
        XCTAssertEqual(settings.logViewLevelFilter, "info")
    }

    func test_appSettings_fillsDefaultsForMissingKeys() throws {
        let settings = try JSONDecoder().decode(AppSettings.self, from: Data("{}".utf8))
        XCTAssertEqual(settings, .defaults)
    }

    func test_appSettings_persistenceKeysMatchSpec() {
        XCTAssertEqual(AppSettings.PersistenceKey.serverConfigList,   "server_config_list")
        XCTAssertEqual(AppSettings.PersistenceKey.appSettings,        "app_settings")
        XCTAssertEqual(AppSettings.PersistenceKey.legacyServerConfig, "server_config")
    }

    // MARK: - HistoryRecord wire fixtures (§3.6)

    func test_historyRecordText_decodesAllOptionalFields() throws {
        let data = try loadFixture("history_record_text")
        let record = try JSONDecoder().decode(HistoryRecord.self, from: data)

        XCTAssertEqual(record.type, .text)
        XCTAssertEqual(record.hash, "3F4E62D9F184380BAD1B0F94B5518DCBF35ACB79B34F6D6E34F3DAB16CD7BC8F")
        XCTAssertEqual(record.text, "Hello, SyncClipboard!")
        XCTAssertFalse(record.hasData)
        XCTAssertEqual(record.size, 21)
        XCTAssertNotNil(record.createTime)
        XCTAssertNotNil(record.lastModified)
        XCTAssertNotNil(record.lastAccessed)
        XCTAssertFalse(record.starred)
        XCTAssertFalse(record.pinned)
        XCTAssertEqual(record.version, 0)
        XCTAssertFalse(record.isDeleted)
    }

    func test_historyRecordText_roundTripPreservesISOTimestamps() throws {
        let data = try loadFixture("history_record_text")
        let decoded = try JSONDecoder().decode(HistoryRecord.self, from: data)
        let reEncoded = try JSONEncoder().encode(decoded)
        let redecoded = try JSONDecoder().decode(HistoryRecord.self, from: reEncoded)
        // Date equality holds to sub-millisecond because the fractional-ISO
        // formatter we encode through has millisecond resolution and the
        // fixture's timestamps already round to whole milliseconds.
        XCTAssertEqual(decoded, redecoded)
    }

    func test_historyRecordMinimal_fillsDefaults() throws {
        let data = try loadFixture("history_record_minimal")
        let record = try JSONDecoder().decode(HistoryRecord.self, from: data)

        XCTAssertEqual(record.type, .file)
        XCTAssertEqual(record.hash, "088EA33D054B64459EA2EB0CBD9F9152DD0BE4C38C6350963BBA00FDDC94CCEA")
        XCTAssertNil(record.text)
        XCTAssertFalse(record.hasData)
        XCTAssertNil(record.size)
        XCTAssertNil(record.createTime)
        XCTAssertNil(record.lastModified)
        XCTAssertNil(record.lastAccessed)
        XCTAssertFalse(record.starred)
        XCTAssertFalse(record.pinned)
        XCTAssertNil(record.version)
        XCTAssertFalse(record.isDeleted)
    }

    func test_historyRecordDeleted_isDeletedReadAndRoundTrip() throws {
        let data = try loadFixture("history_record_deleted")
        let record = try JSONDecoder().decode(HistoryRecord.self, from: data)

        XCTAssertTrue(record.isDeleted, "Read shape uses isDeleted (not isDelete)")
        XCTAssertEqual(record.version, 3)

        // Re-encoding must NOT introduce isDelete (no trailing d) — that
        // key is the PATCH-update-body convention, not the read shape.
        let reEncoded = try JSONEncoder().encode(record)
        let keys = try keys(reEncoded)
        XCTAssertTrue(keys.contains("isDeleted"))
        XCTAssertFalse(keys.contains("isDelete"))
    }

    func test_historyRecord_idIsCompositeTypeDashHash() {
        let r = HistoryRecord(hash: "ABCDEF", type: .text)
        XCTAssertEqual(r.id, "Text-ABCDEF")
        XCTAssertEqual(HistoryRecord.profileId(type: .image, hash: "XYZ"), "Image-XYZ")
    }

    /// The Android wire emits `…Z` timestamps; some hand-rolled servers
    /// truncate fractional seconds. Both shapes MUST decode.
    func test_historyRecord_decodesPlainISOWithoutFractionalSeconds() throws {
        let json = #"""
        {
          "hash": "AA",
          "type": "Text",
          "createTime": "2026-05-17T16:43:00Z"
        }
        """#
        let r = try JSONDecoder().decode(HistoryRecord.self, from: Data(json.utf8))
        XCTAssertNotNil(r.createTime)
    }

    /// Empty/whitespace timestamps decode to nil rather than throwing,
    /// matching the `hash` normalization on `Clipboard`.
    func test_historyRecord_emptyTimestampDecodesToNil() throws {
        let json = #"""
        {
          "hash": "AA",
          "type": "Text",
          "createTime": "",
          "lastModified": "   "
        }
        """#
        let r = try JSONDecoder().decode(HistoryRecord.self, from: Data(json.utf8))
        XCTAssertNil(r.createTime)
        XCTAssertNil(r.lastModified)
    }
}
