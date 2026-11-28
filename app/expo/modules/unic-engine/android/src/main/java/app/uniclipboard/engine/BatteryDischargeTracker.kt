package app.uniclipboard.engine

/**
 * Pure Kotlin battery discharge segment continuity tracker shared between production
 * Android resource collection and deterministic event-sequence verification.
 *
 * Emits a rate only after >=10 minutes of continuous unplugged discharge under identical
 * app_state and low_power_mode without battery percentage increase, observation gaps,
 * sleep jumps, or suspension.
 */
internal class BatteryDischargeTracker(
  private val minSegmentDurationMs: Long = 600_000L,
  private val maxObservationGapMs: Long = 180_000L,
) {
  data class Segment(
    var startPct: Double,
    var prevPct: Double,
    var startElapsedMs: Long,
    var lastEventElapsedMs: Long,
    var appState: String,
    var lowPowerMode: Boolean,
  )

  data class Result(
    val drainRate: Double?,
    val probeStatus: String,
  )

  var segment: Segment? = null
    private set

  fun reset() {
    segment = null
  }

  fun onObservation(
    powerSource: String,
    batteryPct: Double?,
    lowPowerMode: Boolean,
    appState: String,
    elapsedMs: Long,
    active: Boolean,
  ) {
    if (!active ||
      powerSource != "battery" ||
      batteryPct == null ||
      batteryPct < 0.0 ||
      batteryPct > 100.0 ||
      appState == "suspended"
    ) {
      segment = null
      return
    }
    val seg = segment
    if (seg == null) {
      segment = Segment(
        startPct = batteryPct,
        prevPct = batteryPct,
        startElapsedMs = elapsedMs,
        lastEventElapsedMs = elapsedMs,
        appState = appState,
        lowPowerMode = lowPowerMode,
      )
      return
    }
    if (elapsedMs < seg.lastEventElapsedMs ||
      (elapsedMs - seg.lastEventElapsedMs) > maxObservationGapMs ||
      batteryPct > seg.prevPct + 1e-6 ||
      seg.appState != appState ||
      seg.lowPowerMode != lowPowerMode
    ) {
      segment = Segment(
        startPct = batteryPct,
        prevPct = batteryPct,
        startElapsedMs = elapsedMs,
        lastEventElapsedMs = elapsedMs,
        appState = appState,
        lowPowerMode = lowPowerMode,
      )
      return
    }
    seg.prevPct = batteryPct
    seg.lastEventElapsedMs = elapsedMs
  }

  fun evaluateSample(
    powerSource: String,
    batteryPct: Double?,
    lowPowerMode: Boolean,
    appState: String,
    elapsedMs: Long,
    sleepJumped: Boolean,
    resetAfter: Boolean,
  ): Result {
    if (sleepJumped ||
      resetAfter ||
      powerSource != "battery" ||
      batteryPct == null ||
      batteryPct < 0.0 ||
      batteryPct > 100.0 ||
      appState == "suspended"
    ) {
      segment = null
      val status = if (powerSource == "unknown" || batteryPct == null || batteryPct < 0.0 || batteryPct > 100.0) {
        "unsupported"
      } else {
        "ok"
      }
      return Result(drainRate = null, probeStatus = status)
    }

    onObservation(
      powerSource = powerSource,
      batteryPct = batteryPct,
      lowPowerMode = lowPowerMode,
      appState = appState,
      elapsedMs = elapsedMs,
      active = true,
    )
    val seg = segment ?: return Result(drainRate = null, probeStatus = "ok")
    val elapsedSegMs = elapsedMs - seg.startElapsedMs
    if (elapsedSegMs < minSegmentDurationMs) {
      return Result(drainRate = null, probeStatus = "ok")
    }
    val rate = maxOf(0.0, (seg.startPct - batteryPct) * 3600.0 / (elapsedSegMs.toDouble() / 1000.0))
    seg.startPct = batteryPct
    seg.prevPct = batteryPct
    seg.startElapsedMs = elapsedMs
    seg.lastEventElapsedMs = elapsedMs
    return Result(drainRate = rate, probeStatus = "ok")
  }
}
