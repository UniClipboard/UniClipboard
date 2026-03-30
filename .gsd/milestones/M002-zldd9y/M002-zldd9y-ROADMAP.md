# M002-zldd9y:

## Vision

Add daemon HTTP endpoints for settings (GET/PUT), encryption state management (get state, unlock, lock), and storage statistics (stats, clear cache). These complete the daemon API surface needed for frontend direct connection.

## Slice Overview

| ID  | Slice                                                | Risk   | Depends | Done | After this                                                                                                                                     |
| --- | ---------------------------------------------------- | ------ | ------- | ---- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| S01 | Foundation: Permissions, Constants & Unlock Use Case | low    | —       | ✅   | PermissionLevel L3/L4 variants exist, daemon_api_strings has all Phase 76 constants, UnlockEncryptionWithPassphrase use case passes unit tests |
| S02 | Settings & Encryption HTTP Handlers                  | medium | S01     | ✅   | GET /settings, PUT /settings, GET /encryption/state, POST /encryption/unlock, POST /encryption/lock all respond correctly                      |
| S03 | Storage Stats & Clear Cache HTTP Handlers            | medium | S01     | ✅   | GET /storage/stats returns all 5 fields; POST /storage/clear-cache with confirmed:true clears cache; without confirmed returns 400             |
