# Sensitive Content Guard plugin (`unic.security`)

The independent implementation is
[UniClipboard/unic-plugin-security](https://github.com/UniClipboard/unic-plugin-security).
Implemented revision:
[`fb674abede1980e233310b26921efdee62faa91c`](https://github.com/UniClipboard/unic-plugin-security/tree/fb674abede1980e233310b26921efdee62faa91c).
This directory references that repository; it does not duplicate its code.

The plugin registers processor `unic.security` on `clipboard.captured` and
`clipboard.received`. By default (`mode = "block"`), it detects private keys,
API tokens/JWTs, credentialed connection strings, password/secret assignments,
known password manager source applications, sensitive filenames, and user
custom rules, then sets `SetPersist(false)` and `SetSync(false)` so sensitive
items stay out of local history and cross-device sync.

```sh
unic plugin install https://github.com/UniClipboard/unic-plugin-security --ref fb674abede1980e233310b26921efdee62faa91c --build --enable
```

Restart the service after enabling the plugin. See the independent
[acceptance record](https://github.com/UniClipboard/unic-plugin-security/blob/fb674abede1980e233310b26921efdee62faa91c/ACCEPTANCE.md)
for verified process E2E boundaries and known host limits.
