# Sensitive Content Guard plugin (`unic.security`)

The independent implementation is
[UniClipboard/unic-plugin-security](https://github.com/UniClipboard/unic-plugin-security).
Implemented revision:
[`0f23b17ee8922fdbe5e5e47016e0a70f979f1185`](https://github.com/UniClipboard/unic-plugin-security/tree/0f23b17ee8922fdbe5e5e47016e0a70f979f1185)
(release [`v0.1.0`](https://github.com/UniClipboard/unic-plugin-security/releases/tag/v0.1.0) with prebuilt platform assets).
This directory references that repository; it does not duplicate its code.

The plugin registers processor `unic.security` on `clipboard.captured` and
`clipboard.received`. By default (`mode = "block"`), it detects private keys,
API tokens/JWTs, credentialed connection strings, password/secret assignments,
known password manager source applications, sensitive filenames, and user
custom rules, then sets `SetPersist(false)` and `SetSync(false)` so sensitive
items stay out of local history and cross-device sync.

```sh
unic plugin install https://github.com/UniClipboard/unic-plugin-security --ref 0f23b17ee8922fdbe5e5e47016e0a70f979f1185 --enable
```

Restart the service after enabling the plugin. See the independent
[acceptance record](https://github.com/UniClipboard/unic-plugin-security/blob/0f23b17ee8922fdbe5e5e47016e0a70f979f1185/ACCEPTANCE.md)
for verified process E2E boundaries and release details.
