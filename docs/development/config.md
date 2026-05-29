# Configuration Guide

## Production Mode (Default)

When `config.toml` is absent, the application uses system-default paths:

### macOS

```
~/Library/Application Support/app.uniclipboard.desktop[-<profile>]/
├── uniclipboard.db          # Database
├── vault/                   # Encryption vault
│   ├── key
│   └── snapshot
├── logs/                    # Daily JSON logs
└── settings.json            # User settings
```

### Linux

```
~/.local/share/app.uniclipboard.desktop[-<profile>]/
├── uniclipboard.db
├── vault/
│   ├── key
│   └── snapshot
├── logs/
└── settings.json
```

### Windows

```
%LOCALAPPDATA%\app.uniclipboard.desktop[-<profile>]\
├── uniclipboard.db
├── vault\
│   ├── key
│   └── snapshot
├── logs\
└── settings.json
```

`[-<profile>]` means the suffix is present only when `UC_PROFILE` is set, for example `app.uniclipboard.desktop-dev`.

## Development Mode (Optional)

Developers can create `config.toml` in the project root to override default paths for testing purposes.

**Example `config.toml`:**

```toml
[general]
device_name = "DevDevice"

[storage]
database_path = "/tmp/uniclipboard-dev.db"

[security]
vault_key_path = "/tmp/vault/key"
vault_snapshot_path = "/tmp/vault/snapshot"
```

**Important Notes:**

- `config.toml` is **ONLY for development use**
- Users never need to create or modify `config.toml`
- Production deployments should not include `config.toml`
- All user-configurable settings are managed through the UI and stored in `settings.json`

## Configuration Architecture

The application has two separate configuration systems:

### 1. AppConfig (`config.toml`, optional)

- **Purpose**: Infrastructure paths (database, vault locations)
- **Usage**: Development-only overrides
- **Production**: Uses system defaults automatically
- **Managed by**: Developers only

### 2. Settings (`settings.json`)

- **Purpose**: User-configurable application settings
- **Includes**: Theme, sync preferences, retention policies, device name, pairing timers
- **Storage**: JSON file in data directory
- **Managed by**: User through the UI

### Pairing settings

`settings.json` includes a `pairing` block. Timer values are expressed in seconds.

```json
"pairing": {
  "step_timeout": 15,
  "user_verification_timeout": 120,
  "session_timeout": 300,
  "max_retries": 3,
  "protocol_version": "1.0.0"
}
```

## Environment Variables (advanced)

A few runtime knobs are read from environment variables at daemon/CLI startup
rather than from `settings.json`. They default to today's behavior when unset.

### iroh direct reachability (server / VPS nodes)

For a node behind NAT or a Docker bridge that should be dialable by remote
peers at a known public address — without depending on a relay for
reflexive-address discovery — set both of these (see
[`ADR-007`](../architecture/adr-007-headless-server-node-deployment.md) §2.4):

| Variable | Value | Effect |
| --- | --- | --- |
| `UC_IROH_BIND_PORT` | UDP port `1`–`65535` | Pins the iroh IPv4 UDP socket to `0.0.0.0:<port>` (stable across restarts) instead of a random ephemeral port, so the port can be port-forwarded / firewall-allowed. |
| `UC_IROH_PUBLIC_ADDR` | `ip:port`, e.g. `203.0.113.7:51820` | Advertises this socket address as one of the node's own direct-connection candidates, injected before the first address exchange so paired peers store a reachable public address. |

Notes:

- Typically used together (the advertised port should match the forwarded
  pinned port), with the relay disabled (LAN-only Mode on, i.e.
  `allow_relay_fallback = false`) for pure direct connectivity.
- Only the IPv4 socket is pinned; the IPv6 default bind is unaffected.
- Invalid or empty values are logged at `WARN` and ignored (the node falls
  back to the default ephemeral port / no advertised address); they never
  abort startup. `UC_IROH_BIND_PORT=0` is treated as "ephemeral" and ignored.

## Migrating from Development to Production

When you're ready to test production behavior:

1. **Remove** `config.toml` from the project root
2. **Restart** the application
3. **Verify** files are created in the system data directory
4. **Test** all functionality to ensure paths work correctly

To restore development mode:

1. **Create** `config.toml` with your custom paths
2. **Restart** the application
3. **Verify** files are created at your specified locations
