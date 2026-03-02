# Implementation Plan: Download Progress Display

## Overview

Add download progress tracking to the app update flow. The current `install_update` command uses empty callbacks in `download_and_install(|_, _| {}, || {})` — this plan bridges those callbacks to the React frontend via `tauri::ipc::Channel`.

## Changes Summary

| File                                                | Change                                       |
| --------------------------------------------------- | -------------------------------------------- |
| `src-tauri/crates/uc-tauri/src/commands/updater.rs` | Add `DownloadEvent` enum + `Channel` param   |
| `src/api/updater.ts`                                | Create Channel, accept progress callback     |
| `src/contexts/update-context.ts`                    | Add `DownloadProgress` type + context fields |
| `src/contexts/UpdateContext.tsx`                    | Manage progress state, wire Channel events   |
| `src/components/setting/AboutSection.tsx`           | Add progress bar to update dialog            |
| `src/components/layout/Sidebar.tsx`                 | Add progress bar to update dialog            |
| `src/i18n/locales/en-US.json`                       | Add `downloading` / `installing` keys        |
| `src/i18n/locales/zh-CN.json`                       | Add Chinese translations                     |

## Step-by-Step

### Step 1: Rust — Add `DownloadEvent` and Channel to `install_update`

**File:** `src-tauri/crates/uc-tauri/src/commands/updater.rs`

1. Add `use tauri::ipc::Channel;`
2. Define `DownloadEvent` enum (matches plugin's own pattern):
   ```rust
   #[derive(Debug, Clone, Serialize)]
   #[serde(tag = "event", content = "data")]
   pub enum DownloadEvent {
       #[serde(rename_all = "camelCase")]
       Started { content_length: Option<u64> },
       #[serde(rename_all = "camelCase")]
       Progress { chunk_length: usize },
       Finished,
   }
   ```
3. Add `on_event: Channel<DownloadEvent>` parameter to `install_update`
4. Replace empty closures with Channel-emitting closures using `first_chunk` pattern:
   ```rust
   let mut first_chunk = true;
   update
       .download_and_install(
           |chunk_length, content_length| {
               if first_chunk {
                   first_chunk = false;
                   let _ = on_event.send(DownloadEvent::Started { content_length });
               }
               let _ = on_event.send(DownloadEvent::Progress { chunk_length });
           },
           || {
               let _ = on_event.send(DownloadEvent::Finished);
           },
       )
       .await
       .map_err(|e| e.to_string())?;
   ```

**Key notes:**

- Use `let _ = on_event.send(...)` to ignore send errors (frontend might close dialog)
- `std::sync::Mutex` is fine — guard is dropped before any await
- No new dependencies needed

### Step 2: Frontend API — Update `installUpdate` with Channel

**File:** `src/api/updater.ts`

1. Add imports: `Channel` from `@tauri-apps/api/core`
2. Define `DownloadEvent` type (discriminated union matching Rust serde output)
3. Export `DownloadProgress` interface
4. Update `installUpdate` to accept `onProgress` callback and create a `Channel`:

   ```typescript
   export type DownloadEvent =
     | { event: 'Started'; data: { contentLength: number | null } }
     | { event: 'Progress'; data: { chunkLength: number } }
     | { event: 'Finished' }

   export interface DownloadProgress {
     downloaded: number
     total: number | null
     phase: 'idle' | 'downloading' | 'installing'
   }

   export async function installUpdate(
     onProgress?: (progress: DownloadProgress) => void
   ): Promise<void> {
     const onEvent = new Channel<DownloadEvent>()
     let downloaded = 0
     let total: number | null = null

     onEvent.onmessage = message => {
       switch (message.event) {
         case 'Started':
           total = message.data.contentLength
           onProgress?.({ downloaded: 0, total, phase: 'downloading' })
           break
         case 'Progress':
           downloaded += message.data.chunkLength
           onProgress?.({ downloaded, total, phase: 'downloading' })
           break
         case 'Finished':
           onProgress?.({ downloaded, total, phase: 'installing' })
           break
       }
     }

     await invokeWithTrace('install_update', { onEvent })
   }
   ```

**Key notes:**

- `invokeWithTrace` spreads args into `invoke`, Channel passes through safely
- Rust `on_event` → JS `onEvent` (Tauri auto camelCase conversion)
- `onProgress` is optional for backward compat

### Step 3: Update Context — Add progress state

**File:** `src/contexts/update-context.ts`

Add `DownloadProgress` import and `downloadProgress` field:

```typescript
import type { DownloadProgress } from '@/api/updater'

export interface UpdateContextType {
  updateInfo: UpdateMetadata | null
  isCheckingUpdate: boolean
  downloadProgress: DownloadProgress
  checkForUpdates: () => Promise<UpdateMetadata | null>
  installUpdate: () => Promise<void>
}
```

**File:** `src/contexts/UpdateContext.tsx`

1. Add `DownloadProgress` import
2. Add `downloadProgress` state with default `{ downloaded: 0, total: null, phase: 'idle' }`
3. Update `doInstallUpdate` to pass progress callback and reset on error:

   ```typescript
   const [downloadProgress, setDownloadProgress] = useState<DownloadProgress>({
     downloaded: 0,
     total: null,
     phase: 'idle',
   })

   const doInstallUpdate = useCallback(async () => {
     setDownloadProgress({ downloaded: 0, total: null, phase: 'downloading' })
     try {
       await apiInstallUpdate(progress => {
         setDownloadProgress(progress)
       })
     } catch (error) {
       setDownloadProgress({ downloaded: 0, total: null, phase: 'idle' })
       throw error
     }
   }, [])
   ```

4. Include `downloadProgress` in the context value

### Step 4: UI — Add progress bar to update dialogs

Both `AboutSection.tsx` and `Sidebar.tsx` have duplicate update dialogs. Add progress display to both.

**Pattern (shared between both files):**

1. Import `Progress` from `@/components/ui/progress`
2. Get `downloadProgress` from `useUpdate()` hook
3. Remove local `isInstallingUpdate` state — derive from `downloadProgress.phase !== 'idle'`
4. Add progress section inside AlertDialogDescription, after the version info:
   ```tsx
   {
     downloadProgress.phase !== 'idle' && (
       <div className="space-y-2 pt-2">
         <div className="flex justify-between text-xs text-muted-foreground">
           <span>
             {downloadProgress.phase === 'installing'
               ? t('update.installing')
               : t('update.downloading')}
           </span>
           {downloadProgress.total !== null && (
             <span>
               {Math.round((downloadProgress.downloaded / downloadProgress.total) * 100)}%
             </span>
           )}
         </div>
         <Progress
           value={
             downloadProgress.total !== null
               ? (downloadProgress.downloaded / downloadProgress.total) * 100
               : undefined
           }
           className={cn('h-2', downloadProgress.total === null && 'animate-pulse')}
         />
       </div>
     )
   }
   ```
5. Disable Cancel and Update buttons when `downloadProgress.phase !== 'idle'`

**AboutSection.tsx specific:**

- Remove `isInstallingUpdate` local state
- Simplify `handleInstallUpdate` (no local state management)
- Use `downloadProgress.phase !== 'idle'` for disabled states

**Sidebar.tsx specific:**

- Remove `isInstallingUpdate` local state
- Same simplification as above

### Step 5: i18n — Add translation keys

**File:** `src/i18n/locales/en-US.json`

Add to `"update"` section:

```json
"downloading": "Downloading...",
"installing": "Installing..."
```

**File:** `src/i18n/locales/zh-CN.json`

Add to `"update"` section:

```json
"downloading": "正在下载...",
"installing": "正在安装..."
```

## Edge Cases

1. **Unknown content length** (`total === null`): Show indeterminate progress bar with pulse animation
2. **Frontend closes dialog during download**: `let _ = on_event.send(...)` ignores errors, download continues
3. **Very fast downloads**: Progress bar might flash briefly — acceptable, no special handling needed
4. **Network error during download**: `download_and_install` returns `Err`, caught in try/catch, progress resets to idle
5. **`Finished` event ≠ update complete**: Show "Installing..." after Finished, app will auto-restart

## Verification

1. Build: `cd src-tauri && cargo check` (Rust compiles)
2. Build: `bun run build` (frontend compiles)
3. Manual test: trigger update and verify progress bar appears
