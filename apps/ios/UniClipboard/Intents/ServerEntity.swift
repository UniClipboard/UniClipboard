import AppIntents
import Foundation

/// An `AppEntity` projection of a `ServerConfig`, so the Shortcuts editor
/// (and Siri) can show a server *picker* on the Send / Receive actions.
///
/// This is the load-bearing half of "make the shortcut hit the right
/// backend". Without it the intents silently use whatever the persisted
/// baseline `activeConfig` is: a background intent can't resolve the §5.3
/// effective server, because it can't read the SSID
/// (`NEHotspotNetwork.fetchCurrent` needs foreground + Location auth — see
/// `CurrentSSIDProvider`) and only the foreground app / keyboard run the
/// network-rule resolver. With it, the user picks the destination explicitly
/// in the Shortcuts editor, or branches on it with the system "If Wi-Fi
/// network is …" action — the reliable way to get per-network routing out of
/// a background intent.
///
/// `id` is the `ServerConfig.id`, so a saved shortcut keeps pointing at the
/// same server across launches and config edits.
struct ServerEntity: AppEntity, Identifiable {
    let id: String
    let label: String
    let urlString: String

    init(_ config: ServerConfig) {
        self.id = config.id
        self.label = config.displayLabel
        self.urlString = config.url
    }

    static var typeDisplayRepresentation = TypeDisplayRepresentation(name: "服务器")

    var displayRepresentation: DisplayRepresentation {
        DisplayRepresentation(title: "\(label)", subtitle: "\(urlString)")
    }

    static var defaultQuery = ServerEntityQuery()

    /// Resolve the full `ServerConfig` (credentials included) the intent
    /// needs to talk to a server. A nil entity — the AppShortcut / Siri
    /// path where the user never picked one — falls back to `activeConfig`,
    /// preserving the pre-parameter behavior.
    ///
    /// Which *URL* of the resolved profile to try first is layered on per
    /// §5.3: a background intent never probes, so — like the keyboard and
    /// the share sheet — it reads the main app's last probe verdict
    /// (`live_urls`) and falls back to pure shape order. The network signal
    /// is the entitlement-free subset: the App-Group SSID stands in for
    /// "on Wi-Fi", Tailscale is checked live via `getifaddrs`.
    @MainActor
    static func resolveConfig(
        _ entity: ServerEntity?,
        in servers: ServerConfigList,
        store: SettingsStore
    ) -> ServerConfig? {
        var resolved: ServerConfig?
        if let entity, let hit = servers.configs.first(where: { $0.id == entity.id }) {
            resolved = hit
        } else {
            resolved = servers.activeConfig
        }
        guard var cfg = resolved else { return nil }
        cfg.urls = cfg.preferredURLs(
            live: store.loadLiveURL(configId: cfg.id),
            network: NetworkContext(
                ssid: store.loadLastKnownSSID(),
                isTailscale: TailscaleDetector.isActive()
            )
        )
        return cfg
    }
}

/// Feeds the server picker. Every read comes from the App-Group
/// `SettingsStore` so the Shortcuts process (which can be separate from the
/// main app) sees exactly the servers the app persists.
struct ServerEntityQuery: EntityQuery {
    @MainActor
    func entities(for identifiers: [String]) async throws -> [ServerEntity] {
        let configs = SettingsStore().loadServers().configs
        let wanted = Set(identifiers)
        return configs.filter { wanted.contains($0.id) }.map(ServerEntity.init)
    }

    @MainActor
    func suggestedEntities() async throws -> [ServerEntity] {
        SettingsStore().loadServers().configs.map(ServerEntity.init)
    }
}
