import Foundation
#if canImport(Darwin)
import Darwin
#endif

/// A snapshot of the device's current network, fed to the §5.3 auto-switch
/// resolver. Pure Foundation/POSIX so both the app and the keyboard extension
/// build it: interface type comes from `NWPathMonitor` and the Tailscale check
/// from `getifaddrs` (neither needs an entitlement); the SSID name comes from
/// the App Group, which the app writes.
public struct NetworkContext: Equatable, Sendable {
    /// Normalized §5.1 SSID, or `nil` when not on a named Wi-Fi. The name is
    /// no longer matched for auto-switch (§5.3 is URL-based now); it survives
    /// only as a "which Wi-Fi" cross-process signal and as a fallback wifi
    /// indicator for clients that don't populate `isWifi`.
    public var ssid: String?
    /// The primary path uses Wi-Fi. Comes from the OS network path
    /// (`NWPath.usesInterfaceType(.wifi)`) — no entitlement, unlike the SSID
    /// *name*. Drives the §5.3 URL ordering's "prefer the LAN URL on Wi-Fi"
    /// rule even when the SSID name can't be read (Location denied).
    public var isWifi: Bool
    /// The primary path is cellular data.
    public var isCellular: Bool
    /// A Tailscale virtual network is up — a local interface holds an IPv4 in
    /// 100.64.0.0/10. Highest-priority tier: it overlays the physical link, so
    /// when it's up it wins over the Wi-Fi the device is physically on.
    public var isTailscale: Bool

    public init(
        ssid: String? = nil,
        isWifi: Bool = false,
        isCellular: Bool = false,
        isTailscale: Bool = false
    ) {
        self.ssid = ssid
        self.isWifi = isWifi
        self.isCellular = isCellular
        self.isTailscale = isTailscale
    }
}

/// Detects an active Tailscale virtual network by looking for a local
/// interface with an IPv4 address in Tailscale's CGNAT range 100.64.0.0/10.
///
/// Pure POSIX (`getifaddrs`) — no entitlement, usable from the app and any
/// extension (the keyboard included). 100.64/10 is Tailscale's standard
/// node-address range, so a hit pins it to Tailscale rather than just "some
/// VPN is up". Best-effort: a hand-rolled CGNAT VPN on the same range would
/// also match, which is an acceptable false-positive for an opt-in rule.
public enum TailscaleDetector {
    public static func isActive() -> Bool {
        var head: UnsafeMutablePointer<ifaddrs>?
        guard getifaddrs(&head) == 0 else { return false }
        defer { freeifaddrs(head) }
        var ptr = head
        while let cur = ptr {
            defer { ptr = cur.pointee.ifa_next }
            guard let sa = cur.pointee.ifa_addr,
                  sa.pointee.sa_family == sa_family_t(AF_INET) else { continue }
            let ipv4 = sa.withMemoryRebound(to: sockaddr_in.self, capacity: 1) {
                UInt32(bigEndian: $0.pointee.sin_addr.s_addr)
            }
            // 100.64.0.0/10 → network 0x64400000, mask 0xFFC00000.
            if (ipv4 & 0xFFC0_0000) == 0x6440_0000 { return true }
        }
        return false
    }
}
