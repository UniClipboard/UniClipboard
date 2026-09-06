import { startDiagnosticTrace } from '@/observability/diagnostics'

export interface TraceContext {
  traceId: string
  startTime: number
  operation: string
}

class TraceManager {
  private activeTraces = new Map<TraceContext, () => void>()

  startTrace(operation: string): TraceContext {
    const backendTrace = startDiagnosticTrace(operation)
    const trace = {
      traceId: backendTrace.traceId,
      startTime: Date.now(),
      operation,
    }
    this.activeTraces.set(trace, backendTrace.finish)
    return trace
  }

  getCurrentTrace(): TraceContext | null {
    // Overlapping calls have no unambiguous ambient owner in a WebView.
    // Their explicit command breadcrumbs/errors still carry their own IDs.
    return this.activeTraces.size === 1 ? this.activeTraces.keys().next().value! : null
  }

  endTrace(trace: TraceContext): void {
    const finish = this.activeTraces.get(trace)
    this.activeTraces.delete(trace)
    finish?.()
  }
}

export const traceManager = new TraceManager()
