import Foundation

/// Pure Foundation battery discharge segment continuity tracker shared between
/// iOS production collection and deterministic event-sequence verification.
///
/// Emits a drain rate only after >=10 minutes of continuous unplugged discharge under
/// identical app_state and low_power_mode without percentage rise, observation gap
/// (>180s), sleep jump, or suspension.
final class BatteryDischargeTracker {
  struct Segment {
    var startPct: Double
    var prevPct: Double
    var startContSec: Double
    var lastEventContSec: Double
    var appState: String
    var lowPowerMode: Bool
  }

  struct Result {
    let drainRate: Double?
    let probeStatus: String
  }

  private let minSegmentDurationSec: Double
  private let maxObservationGapSec: Double
  private(set) var segment: Segment?

  init(minSegmentDurationSec: Double = 600.0, maxObservationGapSec: Double = 180.0) {
    self.minSegmentDurationSec = minSegmentDurationSec
    self.maxObservationGapSec = maxObservationGapSec
  }

  func reset() {
    segment = nil
  }

  func onObservation(
    powerSource: String,
    batteryPct: Double?,
    lowPowerMode: Bool,
    appState: String,
    contSec: Double,
    active: Bool
  ) {
    guard active,
          powerSource == "battery",
          let pct = batteryPct,
          pct.isFinite,
          pct >= 0.0,
          pct <= 100.0,
          appState != "suspended" else {
      segment = nil
      return
    }
    guard var seg = segment else {
      segment = Segment(
        startPct: pct,
        prevPct: pct,
        startContSec: contSec,
        lastEventContSec: contSec,
        appState: appState,
        lowPowerMode: lowPowerMode
      )
      return
    }
    if contSec < seg.lastEventContSec ||
       (contSec - seg.lastEventContSec) > maxObservationGapSec ||
       pct > seg.prevPct + 1e-6 ||
       seg.appState != appState ||
       seg.lowPowerMode != lowPowerMode {
      segment = Segment(
        startPct: pct,
        prevPct: pct,
        startContSec: contSec,
        lastEventContSec: contSec,
        appState: appState,
        lowPowerMode: lowPowerMode
      )
      return
    }
    seg.prevPct = pct
    seg.lastEventContSec = contSec
    segment = seg
  }

  func evaluateSample(
    powerSource: String,
    batteryPct: Double?,
    lowPowerMode: Bool,
    appState: String,
    contSec: Double,
    sleepJumped: Bool,
    resetAfter: Bool
  ) -> Result {
    guard !sleepJumped,
          !resetAfter,
          powerSource == "battery",
          let pct = batteryPct,
          pct.isFinite,
          pct >= 0.0,
          pct <= 100.0,
          appState != "suspended" else {
      segment = nil
      let unsupported = powerSource == "unknown" ||
        batteryPct == nil ||
        !(batteryPct?.isFinite ?? false) ||
        (batteryPct ?? -1) < 0.0 ||
        (batteryPct ?? 101) > 100.0
      return Result(drainRate: nil, probeStatus: unsupported ? "unsupported" : "ok")
    }

    onObservation(
      powerSource: powerSource,
      batteryPct: pct,
      lowPowerMode: lowPowerMode,
      appState: appState,
      contSec: contSec,
      active: true
    )
    guard var seg = segment else {
      return Result(drainRate: nil, probeStatus: "ok")
    }
    let elapsed = contSec - seg.startContSec
    guard elapsed >= minSegmentDurationSec else {
      return Result(drainRate: nil, probeStatus: "ok")
    }
    let rate = max(0.0, (seg.startPct - pct) * 3600.0 / elapsed)
    seg.startPct = pct
    seg.prevPct = pct
    seg.startContSec = contSec
    seg.lastEventContSec = contSec
    segment = seg
    return Result(drainRate: rate, probeStatus: "ok")
  }
}
