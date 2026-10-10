// Host adapter for `@tauri-apps/api/core`. Host commands are not routed here any more: the shared frontend calls the
// Wails-generated `HostService` bindings through `apps/gui/src/lib/ipc.ts`. What remains is the platform surface
// the Tauri core module exposed besides `invoke`.

// The shared frontend uses this flag for "running inside the desktop shell".
export const isTauri = (): boolean => true

export const convertFileSrc = (path: string): string =>
  `/host-file?path=${encodeURIComponent(path)}`
