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
