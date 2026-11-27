# Sensitive Content Guard (`builtin.security`)

Sensitive Content Guard now ships built into unic at
`internal/plugins/builtinsecurity` under the reserved built-in ID
`builtin.security` (Guard stage, enabled by default and configurable in
Settings → Plugins → Sensitive Content Guard or
`[plugins.instances."builtin.security"]` in `config.toml`).

The standalone external plugin reference repository remains
[UniClipboard/unic-plugin-security](https://github.com/UniClipboard/unic-plugin-security).

By default (`mode = "block"`), `builtin.security` detects private keys, API
tokens/JWTs, credentialed connection strings, password/secret assignments,
known password manager source applications, sensitive filenames, and user
custom rules in the `guard` processor stage ahead of standard processors and
all sync backends, setting `SetPersist(false)` and `SetSync(false)` so
sensitive items stay out of local history and cross-device sync.
