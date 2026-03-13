# S02 Plan: Storage Stats & Visibility

## Goal

Add a Tauri command to compute storage statistics and display them in StorageSection.

## Tasks

- [ ] **T01: Add `get_storage_stats` Tauri command** `est:1h`
  - Add port in uc-core for app directory paths
  - Add use case in uc-app to compute dir sizes
  - Add Tauri command in uc-tauri
  - Register in main.rs invoke_handler

- [ ] **T02: Display storage stats in StorageSection** `est:30m`
  - Show DB size, vault size, cache size, logs size, total
  - Human-readable formatting (KB/MB/GB)
  - Loading state
