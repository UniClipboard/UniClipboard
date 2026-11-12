# Directory transfer acceptance

Local process acceptance passed. Native clipboard and real-device acceptance remain NOT_RUN.

`accepted/` records 17 scenarios. `current-source/` records nine focused scenarios with exactly matching CLI binary hashes. `platform-builds/` records final source compilation only. `production-headless/` preserves the production acceptance manifest. `checks/` records existing race/nonvisual checks.

Replay from the repository with `python3 tools/directorye2e/run.py --output /absolute/new/receipt`. Build receipts use `python3 tools/directorye2e/compile.py --output /absolute/new/build-receipt`. Raw logs, source copies, profiles and binaries are not retained. See the [execution receipt](../../0024-directory-copy-and-send.md) for coverage and limits.
