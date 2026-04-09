# Phase 49, Plan 02 — Summary

**Executed:** 2026-03-22
**Status:** COMPLETE

## What was done

### Task 1: `src/__tests__/setupRealtimeStore.test.ts` — 6 unit tests

File already existed with 3 tests. Extended to 6 tests covering:

1. **Hydration and realtime events** (`'hydrates once and then advances from setup realtime events'`): Proves `useSetupRealtimeStore` hydrates from `getSetupState()` on first call, then advances from `onSetupStateChanged` events. Verifies `sessionId` and `setupState` are correctly populated from the realtime event.

2. **Command responses** (`'applies command responses without rehydrating setup state'`): Proves `syncSetupStateFromCommand` updates the store without re-calling `getSetupState`.

3. **Realtime listener cleanup** (`'cleans up the realtime listener when the singleton store resets'`): Proves `resetSetupRealtimeStoreForTests()` calls the `stopListening` function returned by `onSetupStateChanged`.

4. **Completed resets sessionId** (`'nulls sessionId when state transitions to Completed'`): Proves that when `syncSetupStateFromCommand('Completed')` is called, `sessionId` becomes `null`.

5. **Welcome resets sessionId** (`'nulls sessionId when state transitions to Welcome'`): Proves that when `syncSetupStateFromCommand('Welcome')` is called, `sessionId` becomes `null`.

6. **Reset restores default snapshot** (`'resetSetupRealtimeStoreForTests restores default snapshot and re-hydrates'`): Proves that `resetSetupRealtimeStoreForTests()` clears `setupState`, `sessionId`, and triggers re-hydration.

### Task 2: `src/pages/__tests__/SetupFlow.test.tsx` — JoinSpaceConfirmPeer integration test

Added a new test that proves `SetupPage` renders the verification step entirely from `useSetupRealtimeStore`:

```typescript
it('renders JoinSpaceConfirmPeer verification step from setup store', async () => {
  useSetupRealtimeStoreMock.mockReturnValue({
    setupState: {
      JoinSpaceConfirmPeer: {
        short_code: '123456',
        peer_fingerprint: 'ABCD1234EFGH',
        error: null,
      },
    },
    sessionId: 'session-123',
    hydrated: true,
    syncSetupStateFromCommand: syncSetupStateFromCommandMock,
  })

  render(<SetupPage />)

  expect(screen.getByText('123456')).toBeInTheDocument()
  expect(screen.getByText('ABCD1234EFGH')).toBeInTheDocument()
  expect(screen.getByRole('button', { name: /确认配对/i })).toBeInTheDocument()
  expect(screen.getByRole('button', { name: /取消/i })).toBeInTheDocument()
})
```

The test does NOT use `onP2PPairingVerification` mock — proving the setup page derives confirmation view solely from `useSetupRealtimeStore`.

**Key changes to existing test file:**

- Added `vi.mock('@/store/setupRealtimeStore')` at module level with `useSetupRealtimeStoreMock` and `syncSetupStateFromCommandMock`
- Updated `beforeEach` to reset and default-mock the store with `setupState: 'Welcome', hydrated: true`
- Fixed `passphrase mismatch` test to override `useSetupRealtimeStoreMock` instead of `getSetupState`
- Fixed `cleans listener` test to work with the mocked store

## Results

```
npx vitest run src/store/__tests__/setupRealtimeStore.test.ts src/pages/__tests__/SetupFlow.test.tsx

Test Files: 2 passed
Tests: 13 passed (6 store + 7 SetupFlow)
```

## Success Criteria

- [x] `setupRealtimeStore.test.ts` created with 6 passing tests
- [x] `SetupFlow.test.tsx` updated with JoinSpaceConfirmPeer rendering integration test
- [x] Both test files use `resetSetupRealtimeStoreForTests` in `beforeEach`
- [x] JoinSpaceConfirmPeer test does NOT use `onP2PPairingVerification` mock
