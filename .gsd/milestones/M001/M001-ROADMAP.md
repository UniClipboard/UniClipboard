# M001 Roadmap: Storage Management

- [x] **S01: Retention Policy Settings UI** `risk:low` `depends:[]`
      Connect existing backend `RetentionPolicy` to StorageSection UI. No new backend code needed.

- [x] **S02: Storage Stats & Visibility** `risk:medium` `depends:[]`
      Add `get_storage_stats` Tauri command and display disk usage in StorageSection.

- [x] **S03: Manual Cleanup Operations** `risk:medium` `depends:[S02]`
      Add clear cache, clear all history, and open data directory actions.
