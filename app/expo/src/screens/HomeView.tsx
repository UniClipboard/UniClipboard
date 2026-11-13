// Platform dispatcher (see AGENTS.md「Platform-Specific Component Pattern」):
// Metro resolves HomeView.ios.tsx on iOS; this base file is the Android/default fallback.
export * from './HomeView.android';
