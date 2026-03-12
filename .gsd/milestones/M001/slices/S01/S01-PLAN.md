# S01 Plan: Retention Policy Settings UI

## Goal
Replace the StorageSection placeholder with functional retention policy controls, using the existing backend `RetentionPolicy` model.

## Tasks
- [x] **T01: Implement StorageSection with retention policy controls** `est:1h`
  - Enable/disable auto-cleanup (switch)
  - History retention period selector (ByAge rule)
  - Max history items selector (ByCount rule) 
  - Skip pinned items toggle
  - Update i18n keys as needed

## Backend Status
All backend support exists:
- `RetentionPolicy` in `uc-core/src/settings/model.rs`
- `updateRetentionPolicy` in `SettingContext`
- `Settings.retention_policy` field
- Frontend types in `src/types/setting.ts`

## No backend changes required.
