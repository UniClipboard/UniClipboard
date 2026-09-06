/**
 * The application's diagnostics entry point. Sentry remains the sole provider.
 * Keep provider selection here; callers must not import SDKs or private adapters.
 * Product analytics has its own daemon-owned path and consent setting.
 */
export {
  initSentry as initializeDiagnostics,
  setFrontendSentryEnabled as setDiagnosticsEnabled,
  applyDeviceMetaToSentry as applyDiagnosticDeviceContext,
  sentryEnabled as diagnosticsConfigured,
  captureDiagnosticException,
  recordDiagnosticBreadcrumb,
  writeDiagnosticLog,
  submitDiagnosticFeedback,
  startDiagnosticTrace,
  createDiagnosticsEnhancer,
  DiagnosticsErrorBoundary,
  DiagnosticsRoutes,
} from './sentry'

export type {
  DiagnosticBreadcrumb,
  DiagnosticDeviceContext,
  DiagnosticExceptionContext,
  DiagnosticFeedback,
  DiagnosticLogLevel,
  DiagnosticTrace,
} from './types'
