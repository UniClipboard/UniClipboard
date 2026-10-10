// Platform surface of the desktop host besides the commands. Host commands are not routed here: the frontend calls the
// Wails-generated `HostService` bindings through `src/lib/ipc.ts`.

// The shared frontend uses this flag for "running inside the desktop shell".
export const isDesktopHost = (): boolean => true

export const convertFileSrc = (path: string): string =>
  `/host-file?path=${encodeURIComponent(path)}`
