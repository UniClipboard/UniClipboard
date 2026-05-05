/**
 * Shared test helpers for DaemonClient API integration tests.
 *
 * Provides a shared mock for daemonClient.request that all daemon API tests use.
 * The vi.mock for @/api/daemon/client is defined HERE (in _test-helpers.ts) so
 * that only ONE mock is applied for this module across all test files.
 *
 * Usage in each test file:
 *   import { mockDaemonClient, setupMockClient, teardownMockClient } from './_test-helpers'
 *
 *   beforeEach(() => { mockDaemonClient.request.mockReset() })
 *   afterEach(() => { teardownMockClient() })
 */

import { vi } from 'vitest'
import type { ClipboardEntryDto } from '@/api/daemon/clipboard'
import type { EncryptionStateResponse } from '@/api/daemon/encryption'
import { DaemonErrorCode, DaemonApiError } from '@/api/daemon/errors'
import type { DaemonApiError as DaemonApiErrorType } from '@/api/daemon/errors'
import type { Settings } from '@/api/daemon/settings'

// ── Mock daemonClient ──────────────────────────────────────────
// MUST be at the top of this file so vi.mock can reference it before hoisting.
const mockRequest = vi.fn()
const mockRefreshSession = vi.fn()

export const mockDaemonClient = {
  initialize: vi.fn(),
  destroy: vi.fn(),
  request: mockRequest,
  refreshSession: mockRefreshSession,
  session: null as { token: string; expiresAt: number; encryptionReady: boolean } | null,
  get initialized() {
    return true
  },
  get wsUrl() {
    return null
  },
  get currentSession() {
    return null
  },
}

// Hoisted mock — this runs before any import of @/api/daemon/client
vi.mock('@/api/daemon/client', () => ({
  daemonClient: mockDaemonClient,
}))

// ── Helper functions ───────────────────────────────────────────

/**
 * Reset mockDaemonClient.request between tests.
 * Call this in beforeEach.
 */
export function setupMockClient(): void {
  mockDaemonClient.request.mockReset()
  mockDaemonClient.request.mockResolvedValue(undefined)
  mockDaemonClient.initialize.mockReset()
  mockDaemonClient.destroy.mockReset()
}

/**
 * Restore all mocks.
 * Call this in afterEach.
 */
export function teardownMockClient(): void {
  vi.restoreAllMocks()
}

// ── Legacy fetch-based helpers (for pre-existing tests) ───────────
// These are kept for backward compatibility with pre-existing tests that
// used vi.spyOn(globalThis, 'fetch') via setupFetchMock/teardownFetchMock.

/**
 * @deprecated Use setupMockClient + mockDaemonClient.request.mockResolvedValueOnce instead.
 */
export function setupFetchMock(): { mockFetch: ReturnType<typeof vi.spyOn> } {
  const mockFetch = vi.spyOn(globalThis, 'fetch')
  return { mockFetch }
}

/**
 * @deprecated Use teardownMockClient instead.
 */
export function teardownFetchMock(): void {
  vi.restoreAllMocks()
}

// ── Mock fetch response builders ──────────────────────────────

/** Build a Response that returns the given JSON payload and status. */
export function mockResponse<T>(payload: T, status = 200): Response {
  return new Response(JSON.stringify(payload), {
    status,
    headers: { 'Content-Type': 'application/json' },
  })
}

/** Build a Response representing a daemon error (non-ok status). */
export function mockErrorResponse(status: number, body?: unknown): Response {
  return new Response(JSON.stringify(body ?? { error: `HTTP ${status}` }), {
    status,
    headers: { 'Content-Type': 'application/json' },
  })
}

// ── Error factories ────────────────────────────────────────────

export function makeNotFoundError(
  message = '404 on /clipboard/entries/test-id'
): DaemonApiErrorType {
  return new DaemonApiError(DaemonErrorCode.NOT_FOUND, message)
}

export function makeUnauthorizedError(message = '401 Unauthorized'): DaemonApiErrorType {
  return new DaemonApiError(DaemonErrorCode.UNAUTHORIZED, message)
}

export function makeValidationError(
  message = 'validation failed',
  details?: unknown
): DaemonApiErrorType {
  return new DaemonApiError(DaemonErrorCode.INTERNAL_ERROR, message, details)
}

// ── Mock data factories ────────────────────────────────────────

export function makeEntryDto(overrides: Partial<ClipboardEntryDto> = {}): ClipboardEntryDto {
  return {
    id: 'entry-1',
    preview: 'Hello, world!',
    hasDetail: false,
    sizeBytes: 13,
    capturedAt: 1710000000000,
    contentType: 'text/plain',
    thumbnailUrl: null,
    isEncrypted: false,
    isFavorited: false,
    updatedAt: 1710000000000,
    activeTime: 1710000000000,
    fileTransferStatus: null,
    fileTransferReason: null,
    linkUrls: null,
    linkDomains: null,
    fileSizes: null,
    ...overrides,
  }
}

/**
 * Create a Settings object populated with sensible defaults for tests.
 *
 * @param overrides - Partial `Settings` that will replace the default fields provided by this factory.
 * @returns A `Settings` object with defaults applied; values in `overrides` take precedence.
 */
export function makeSettingsDto(overrides: Partial<Settings> = {}): Settings {
  return {
    schemaVersion: 1,
    general: {
      autoStart: false,
      silentStart: false,
      autoCheckUpdate: true,
      theme: 'system',
      themeColor: null,
      language: null,
      deviceName: 'Test Device',
      updateChannel: null,
      telemetryEnabled: true,
    },
    sync: {
      autoSync: true,
      syncFrequency: 'realtime',
      contentTypes: {
        text: true,
        image: true,
        link: true,
        file: true,
        codeSnippet: true,
        richText: true,
      },
    },
    retentionPolicy: {
      enabled: false,
      rules: [],
      skipPinned: true,
      evaluation: 'anyMatch',
    },
    security: {
      encryptionEnabled: true,
      passphraseConfigured: false,
      autoUnlockEnabled: false,
    },
    pairing: {
      stepTimeout: 30,
      userVerificationTimeout: 60,
      sessionTimeout: 3600,
      maxRetries: 3,
      protocolVersion: '1.0.0',
    },
    keyboardShortcuts: {},
    fileSync: {
      fileSyncEnabled: true,
      smallFileThreshold: 1024,
      maxFileSize: 1024 * 1024 * 100,
      fileCacheQuotaPerDevice: 1024 * 1024 * 500,
      fileRetentionHours: 168,
      fileAutoCleanup: true,
    },
    network: {
      allowRelayFallback: true,
      allowOverlayNetworkAddrs: false,
    },
    ...overrides,
  }
}

export function makeEncryptionStateDto(
  overrides: Partial<EncryptionStateResponse> = {}
): EncryptionStateResponse {
  return {
    initialized: true,
    sessionReady: false,
    ...overrides,
  }
}

export function makeStorageStatsDto(
  overrides: Partial<{
    totalBytes: number
    databaseBytes: number
    vaultBytes: number
    cacheBytes: number
    logsBytes: number
  }> = {}
): {
  totalBytes: number
  databaseBytes: number
  vaultBytes: number
  cacheBytes: number
  logsBytes: number
} {
  return {
    totalBytes: 1024 * 512,
    databaseBytes: 1024 * 128,
    vaultBytes: 1024 * 256,
    cacheBytes: 1024 * 64,
    logsBytes: 1024 * 64,
    ...overrides,
  }
}
