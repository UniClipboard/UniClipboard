import { Sentry, sentryEnabled } from '@/observability/sentry'

export interface TraceContext {
  traceId: string
  startTime: number
  operation: string
  sentrySpan?: ReturnType<typeof Sentry.startInactiveSpan>
}

class TraceManager {
  private currentTrace: TraceContext | null = null

  startTrace(operation: string): TraceContext {
    const span = sentryEnabled
      ? Sentry.startInactiveSpan({
          name: operation,
          op: 'ui.action',
        })
      : undefined

    this.currentTrace = {
      traceId: span?.spanContext().traceId ?? crypto.randomUUID(),
      startTime: Date.now(),
      operation,
      sentrySpan: span,
    }
    return this.currentTrace
  }

  getCurrentTrace(): TraceContext | null {
    return this.currentTrace
  }

  endTrace(): void {
    this.currentTrace?.sentrySpan?.end()
    this.currentTrace = null
  }
}

export const traceManager = new TraceManager()
