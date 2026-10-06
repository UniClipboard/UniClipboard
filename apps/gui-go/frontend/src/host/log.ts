// Host adapter for `@tauri-apps/plugin-log`: host logs stay in the Go process.
export const attachConsole = async (): Promise<() => void> => () => undefined
