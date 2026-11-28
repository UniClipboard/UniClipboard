package app.uniclipboard.engine

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.net.TrafficStats
import android.os.BatteryManager
import android.os.Build
import android.os.Debug
import android.os.PowerManager
import android.os.Process
import android.os.SystemClock
import mobile.Engine
import mobile.HostResourceProvider
import org.json.JSONObject
import java.io.File

/**
 * Collects Android process CPU, PSS memory, voluntary context switches, UID network
 * bytes (aggregate across all transports), thermal residency, piecewise timestamped
 * background mode residency, and >=10-minute unplugged system battery drain segments.
 */
internal class AndroidResourcePowerCollector(
  private val appContext: Context,
) : HostResourceProvider {

  private data class StateSnapshot(
    val appState: String,
    val bgMode: String?,
    val screenState: String,
    val multicastHeld: Boolean,
    val elapsedMs: Long,
    val uptimeMs: Long,
    val seq: Long,
  )

  private data class PowerSnapshot(
    val powerSource: String,
    val elapsedMs: Long,
    val seq: Long,
  )

  private data class BatteryObsEvent(
    val epoch: Long,
    val powerSource: String,
    val batteryPct: Double?,
    val lowPowerMode: Boolean,
    val appState: String,
    val elapsedMs: Long,
    val active: Boolean,
    val seq: Long,
    val isLpmBroadcast: Boolean = false,
  )

  private data class ModeKey(
    val mode: String,
    val screenState: String,
    val multicastLock: Boolean,
  )

  companion object {
    private const val STATE_QUEUE_CAP = 64
    private const val MAX_OBSERVATION_GAP_MS = 180_000L
    private const val MIN_SPILL_MS = 25L
    private const val MAX_SPILL_MS = 250L
  }

  // Lock ordering: probeLock -> eventLock.
  // Callbacks (recordStateSnapshot, BroadcastReceiver.onReceive) ONLY acquire eventLock
  // and never execute OS probes, system service queries, blocking IPC, or wait on probeLock.
  private val probeLock = Any()
  private val eventLock = Any()
  @Volatile private var requestedEpoch: Long = 0L
  private var activeEventEpoch: Long = 0L

  @Volatile private var cachedScreenInteractive: Boolean = true
  @Volatile private var cachedPowerSource: String = "unknown"
  @Volatile private var cachedBatteryPct: Double? = null
  @Volatile private var cachedLowPowerMode: Boolean = false

  private var lastEpoch: Long = 0L
  private var suspendedEpoch: Long = 0L
  private var hasBaseline: Boolean = false
  private var prevUptimeMs: Long = 0L
  private var prevElapsedMs: Long = 0L
  private var prevCPUMs: Long = 0L
  private var prevVoluntaryCtxSwitches: Long? = null
  private var prevTxBytes: Long? = null
  private var prevRxBytes: Long? = null
  private val batteryTracker = BatteryDischargeTracker()
  private var batteryReceiver: BroadcastReceiver? = null

  private var baseAppState: String = resolveAppState("on")
  private var baseBgMode: String? = currentBgMode()
  private var baseScreenState: String = "on"
  private var baseMulticastHeld: Boolean = BackgroundSyncService.isMulticastLockHeld
  private var basePowerSource: String = "unknown"
  private var baseBatteryPct: Double? = null
  private var baseLowPowerMode: Boolean = false

  private var stateSeq: Long = 0L
  private var powerSeq: Long = 0L
  private var batteryObsSeq: Long = 0L
  private val pendingStates = mutableListOf<StateSnapshot>()
  private var stateOverflowed: Boolean = false
  private val pendingPower = mutableListOf<PowerSnapshot>()
  private var powerOverflowed: Boolean = false
  private val pendingBatteryObs = mutableListOf<BatteryObsEvent>()
  private var batteryObsOverflowed: Boolean = false

  fun onEngineStarted() {
    synchronized(probeLock) {
      suspendedEpoch = 0L
    }
  }

  /**
   * Synchronously records a timestamped host state/background-mode transition in memory
   * without blocking on JNI, IPC, system service queries, or waiting on [probeLock].
   */
  fun recordStateSnapshot(
    appState: String? = null,
    bgMode: String? = currentBgMode(),
    screenState: String? = null,
    multicastHeld: Boolean = BackgroundSyncService.isMulticastLockHeld,
    elapsedMs: Long = SystemClock.elapsedRealtime(),
    uptimeMs: Long = SystemClock.uptimeMillis(),
  ) {
    synchronized(eventLock) {
      if (screenState != null) {
        cachedScreenInteractive = screenState == "on"
      }
      val effScreen = screenState ?: if (cachedScreenInteractive) "on" else "off"
      val effApp = appState ?: resolveAppState(effScreen)
      if (activeEventEpoch <= 0L) {
        return
      }
      insertStateSortedLocked(effApp, bgMode, effScreen, multicastHeld, elapsedMs, uptimeMs)
    }
  }

  private fun insertStateSortedLocked(
    appState: String,
    bgMode: String?,
    screenState: String,
    multicastHeld: Boolean,
    elapsedMs: Long,
    uptimeMs: Long,
  ) {
    // Do not deduplicate against tail/predecessor at enqueue time because out-of-order
    // concurrent transitions (e.g. A -> B -> A) have not yet established their causal predecessor.
    var idx = pendingStates.size
    while (idx > 0 && pendingStates[idx - 1].elapsedMs > elapsedMs) {
      idx--
    }
    val snap = StateSnapshot(appState, bgMode, screenState, multicastHeld, elapsedMs, uptimeMs, ++stateSeq)
    pendingStates.add(idx, snap)
    if (pendingStates.size > STATE_QUEUE_CAP) {
      stateOverflowed = true
      pendingStates.removeAt(0)
    }
  }

  private fun insertPowerSortedLocked(powerSource: String, elapsedMs: Long) {
    // Preserve every timestamped power transition; causal deduplication runs at T_cut drain time.
    var idx = pendingPower.size
    while (idx > 0 && pendingPower[idx - 1].elapsedMs > elapsedMs) {
      idx--
    }
    pendingPower.add(idx, PowerSnapshot(powerSource, elapsedMs, ++powerSeq))
    if (pendingPower.size > STATE_QUEUE_CAP) {
      powerOverflowed = true
      pendingPower.removeAt(0)
    }
  }

  override fun collectResourceSnapshot(epoch: Long): String {
    requestedEpoch = epoch
    if (epoch <= 0L) {
      synchronized(eventLock) {
        activeEventEpoch = 0L
        pendingStates.clear()
        stateOverflowed = false
        pendingPower.clear()
        powerOverflowed = false
        pendingBatteryObs.clear()
        batteryObsOverflowed = false
      }
      synchronized(probeLock) {
        resetLocked()
        return ""
      }
    }
    synchronized(probeLock) {
      if (requestedEpoch <= 0L || requestedEpoch != epoch) {
        return ""
      }
      if (suspendedEpoch != 0L && epoch == suspendedEpoch) {
        return ""
      }
      if (epoch != lastEpoch || !hasBaseline) {
        suspendedEpoch = 0L
        lastEpoch = epoch
        primeLocked()
        return ""
      }
      var resetBatterySeed = false
      if (batteryReceiver == null && ensureBatteryReceiverLocked()) {
        batteryTracker.reset()
        resetBatterySeed = true
      }
      return buildEnvelopeLocked(expectedEpoch = epoch, resetBatterySeed = resetBatterySeed).first ?: ""
    }
  }

  /**
   * Flushes any queued or unnotified transition segments to Go using SubmitPowerWindow.
   */
  fun onStateTransition(eng: Engine?) {
    val envelope: String? = synchronized(probeLock) {
      if (lastEpoch <= 0L || !hasBaseline || requestedEpoch != lastEpoch || (suspendedEpoch != 0L && lastEpoch == suspendedEpoch)) {
        return
      }
      val nextAppState = currentAppState()
      val nextBgMode = currentBgMode()
      val nextScreenState = currentScreenState()
      val nextMulticastHeld = BackgroundSyncService.isMulticastLockHeld
      val hasQueuedOrUnnotified = synchronized(eventLock) {
        pendingStates.isNotEmpty() ||
          stateOverflowed ||
          pendingPower.isNotEmpty() ||
          powerOverflowed ||
          nextAppState != baseAppState ||
          nextBgMode != baseBgMode ||
          nextScreenState != baseScreenState ||
          nextMulticastHeld != baseMulticastHeld
      }
      if (!hasQueuedOrUnnotified) {
        return
      }
      val (json, resetBattery) = buildEnvelopeLocked(expectedEpoch = lastEpoch, resetBatterySeed = false)
      if (resetBattery && batteryReceiver != null && requestedEpoch == lastEpoch) {
        val (powerSource, batteryPct) = readBatterySnapshot(null)
        val lpm = isPowerSaveMode()
        val reseedMs = SystemClock.elapsedRealtime()
        basePowerSource = powerSource
        baseBatteryPct = batteryPct
        baseLowPowerMode = lpm
        synchronized(eventLock) {
          cachedPowerSource = powerSource
          cachedBatteryPct = batteryPct
          cachedLowPowerMode = lpm
        }
        batteryTracker.onObservation(
          powerSource = powerSource,
          batteryPct = batteryPct,
          lowPowerMode = lpm,
          appState = baseAppState,
          elapsedMs = reseedMs,
          active = true,
        )
      }
      json
    }
    if (envelope != null && eng != null) {
      try {
        eng.submitPowerWindow(envelope)
      } catch (_: Exception) {
      }
    }
  }

  fun resetOnSuspension() {
    synchronized(eventLock) {
      activeEventEpoch = 0L
      pendingStates.clear()
      stateOverflowed = false
      pendingPower.clear()
      powerOverflowed = false
      pendingBatteryObs.clear()
      batteryObsOverflowed = false
    }
    synchronized(probeLock) {
      if (lastEpoch > 0L) {
        suspendedEpoch = lastEpoch
      }
      hasBaseline = false
      basePowerSource = "unknown"
      baseBatteryPct = null
      baseLowPowerMode = false
      batteryTracker.reset()
      prevVoluntaryCtxSwitches = null
      prevTxBytes = null
      prevRxBytes = null
      unregisterBatteryReceiverLocked()
    }
  }

  private fun resetLocked() {
    lastEpoch = 0L
    suspendedEpoch = 0L
    hasBaseline = false
    basePowerSource = "unknown"
    baseBatteryPct = null
    baseLowPowerMode = false
    synchronized(eventLock) {
      activeEventEpoch = 0L
      pendingStates.clear()
      stateOverflowed = false
      pendingPower.clear()
      powerOverflowed = false
      pendingBatteryObs.clear()
      batteryObsOverflowed = false
    }
    batteryTracker.reset()
    prevVoluntaryCtxSwitches = null
    prevTxBytes = null
    prevRxBytes = null
    unregisterBatteryReceiverLocked()
  }

  private fun primeLocked() {
    prevUptimeMs = SystemClock.uptimeMillis()
    prevElapsedMs = SystemClock.elapsedRealtime()
    prevCPUMs = Process.getElapsedCpuTime()
    prevVoluntaryCtxSwitches = readProcSelfStatusField("voluntary_ctxt_switches")
    val uid = Process.myUid()
    val tx = TrafficStats.getUidTxBytes(uid)
    val rx = TrafficStats.getUidRxBytes(uid)
    prevTxBytes = if (tx >= 0L && tx != TrafficStats.UNSUPPORTED.toLong()) tx else null
    prevRxBytes = if (rx >= 0L && rx != TrafficStats.UNSUPPORTED.toLong()) rx else null
    val pm = appContext.getSystemService(Context.POWER_SERVICE) as? PowerManager
    val interactive = pm?.isInteractive != false
    val lowPowerMode = pm?.isPowerSaveMode == true
    val screenState = if (interactive) "on" else "off"
    baseAppState = resolveAppState(screenState)
    baseBgMode = currentBgMode()
    baseScreenState = screenState
    baseMulticastHeld = BackgroundSyncService.isMulticastLockHeld
    hasBaseline = true
    batteryTracker.reset()
    val (powerSource, batteryPct) = readBatterySnapshot(null)
    basePowerSource = powerSource
    baseBatteryPct = batteryPct
    baseLowPowerMode = lowPowerMode
    val receiverOk = ensureBatteryReceiverLocked()
    synchronized(eventLock) {
      pendingStates.clear()
      stateOverflowed = false
      pendingPower.clear()
      powerOverflowed = false
      pendingBatteryObs.clear()
      batteryObsOverflowed = false
      cachedScreenInteractive = interactive
      cachedPowerSource = powerSource
      cachedBatteryPct = batteryPct
      cachedLowPowerMode = lowPowerMode
      activeEventEpoch = lastEpoch
    }
    if (receiverOk) {
      batteryTracker.onObservation(
        powerSource = powerSource,
        batteryPct = batteryPct,
        lowPowerMode = lowPowerMode,
        appState = baseAppState,
        elapsedMs = prevElapsedMs,
        active = true,
      )
    }
  }

  private fun enqueueBatteryObsLocked(event: BatteryObsEvent) {
    var idx = pendingBatteryObs.size
    while (idx > 0 && pendingBatteryObs[idx - 1].elapsedMs > event.elapsedMs) {
      idx--
    }
    pendingBatteryObs.add(idx, event)
    if (pendingBatteryObs.size > STATE_QUEUE_CAP) {
      batteryObsOverflowed = true
      pendingBatteryObs.removeAt(0)
    }
  }

  private fun ensureBatteryReceiverLocked(): Boolean {
    if (batteryReceiver != null) return true
    val receiver = object : BroadcastReceiver() {
      override fun onReceive(context: Context?, intent: Intent?) {
        val action = intent?.action ?: return
        val elapsedMs = SystemClock.elapsedRealtime()
        when (action) {
          Intent.ACTION_BATTERY_CHANGED -> {
            val (powerSource, batteryPct) = parseBatteryIntent(intent)
            synchronized(eventLock) {
              cachedPowerSource = powerSource
              cachedBatteryPct = batteryPct
              val ep = activeEventEpoch
              if (ep <= 0L) {
                return
              }
              insertPowerSortedLocked(powerSource, elapsedMs)
              val appStateNow = resolveAppState(if (cachedScreenInteractive) "on" else "off")
              val bSeq = ++batteryObsSeq
              enqueueBatteryObsLocked(
                BatteryObsEvent(
                  epoch = ep,
                  powerSource = powerSource,
                  batteryPct = batteryPct,
                  lowPowerMode = cachedLowPowerMode,
                  appState = appStateNow,
                  elapsedMs = elapsedMs,
                  active = true,
                  seq = bSeq,
                  isLpmBroadcast = false,
                ),
              )
            }
          }
          PowerManager.ACTION_POWER_SAVE_MODE_CHANGED -> {
            synchronized(eventLock) {
              val ep = activeEventEpoch
              if (ep <= 0L) {
                return
              }
              val appStateNow = resolveAppState(if (cachedScreenInteractive) "on" else "off")
              val bSeq = ++batteryObsSeq
              enqueueBatteryObsLocked(
                BatteryObsEvent(
                  epoch = ep,
                  powerSource = cachedPowerSource,
                  batteryPct = cachedBatteryPct,
                  lowPowerMode = cachedLowPowerMode,
                  appState = appStateNow,
                  elapsedMs = elapsedMs,
                  active = true,
                  seq = bSeq,
                  isLpmBroadcast = true,
                ),
              )
            }
          }
        }
      }
    }
    val filter = IntentFilter().apply {
      addAction(Intent.ACTION_BATTERY_CHANGED)
      addAction(PowerManager.ACTION_POWER_SAVE_MODE_CHANGED)
    }
    return try {
      if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
        appContext.registerReceiver(receiver, filter, Context.RECEIVER_NOT_EXPORTED)
      } else {
        @Suppress("UnspecifiedRegisterReceiverFlag")
        appContext.registerReceiver(receiver, filter)
      }
      batteryReceiver = receiver
      true
    } catch (_: Exception) {
      batteryReceiver = null
      false
    }
  }

  private fun unregisterBatteryReceiverLocked() {
    val receiver = batteryReceiver ?: return
    batteryReceiver = null
    try {
      appContext.unregisterReceiver(receiver)
    } catch (_: Exception) {
    }
  }

  private fun spillLimitMs(prevDurMs: Long): Long =
    if (prevDurMs <= 0L) MIN_SPILL_MS else (prevDurMs / 10L).coerceIn(MIN_SPILL_MS, MAX_SPILL_MS)

  private fun buildEnvelopeLocked(expectedEpoch: Long, resetBatterySeed: Boolean): Pair<String?, Boolean> {
    val startElapsedMs = prevElapsedMs
    val nowUptimeMs = SystemClock.uptimeMillis()
    val nowElapsedMs = SystemClock.elapsedRealtime()
    val nowCPUMs = Process.getElapsedCpuTime()

    // Capture sequence watermarks and execute instant queries bracketed against T_cut.
    // An un-timestamped query that stalls past T_cut or returns after post-cutoff events have
    // arrived has NO cutoff proof and must never backdate post-cutoff values into T_cut.
    val preProbeStateSeq: Long
    val preProbePowerSeq: Long
    val preProbeBatteryObsSeq: Long
    synchronized(eventLock) {
      preProbeStateSeq = stateSeq
      preProbePowerSeq = powerSeq
      preProbeBatteryObsSeq = batteryObsSeq
    }

    val curCsw = readProcSelfStatusField("voluntary_ctxt_switches")
    val uid = Process.myUid()
    val curTx = TrafficStats.getUidTxBytes(uid)
    val curRx = TrafficStats.getUidRxBytes(uid)
    val (queryPowerSource, queryBatteryPct) = readBatterySnapshot(null)
    val afterBatteryElapsedMs = SystemClock.elapsedRealtime()
    val pm = appContext.getSystemService(Context.POWER_SERVICE) as? PowerManager
    val queryLowPowerMode = pm?.isPowerSaveMode == true
    val afterLpmElapsedMs = SystemClock.elapsedRealtime()
    val queryInteractive = pm?.isInteractive != false
    val queryScreenState = if (queryInteractive) "on" else "off"
    val queryAppState = resolveAppState(queryScreenState)
    val queryBgMode = currentBgMode()
    val queryMcastHeld = BackgroundSyncService.isMulticastLockHeld
    val afterInstantQueriesElapsedMs = SystemClock.elapsedRealtime()
    val queryThermalStatus = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q && pm != null) {
      pm.currentThermalStatus
    } else {
      null
    }
    val afterThermalElapsedMs = SystemClock.elapsedRealtime()

    val stateQueryProvenAtCut = afterInstantQueriesElapsedMs <= nowElapsedMs
    val batteryQueryProvenAtCut = afterBatteryElapsedMs <= nowElapsedMs
    val lpmQueryProvenAtCut = afterLpmElapsedMs <= nowElapsedMs
    val thermalCutoffProven = afterThermalElapsedMs <= nowElapsedMs
    synchronized(eventLock) {
      if (stateQueryProvenAtCut && stateSeq == preProbeStateSeq && pendingStates.none { it.elapsedMs > nowElapsedMs }) {
        cachedScreenInteractive = queryInteractive
      }
      if (batteryQueryProvenAtCut && powerSeq == preProbePowerSeq && batteryObsSeq == preProbeBatteryObsSeq &&
        pendingBatteryObs.none { !it.isLpmBroadcast && it.elapsedMs > nowElapsedMs }
      ) {
        cachedPowerSource = queryPowerSource
        cachedBatteryPct = queryBatteryPct
      }
      if (lpmQueryProvenAtCut && batteryObsSeq == preProbeBatteryObsSeq && pendingBatteryObs.none { it.elapsedMs > nowElapsedMs }) {
        cachedLowPowerMode = queryLowPowerMode
      }
    }

    val memBytes = readMemoryFootprintBytes()

    // Discard probe work cleanly if telemetry was disabled or suspended mid-probe.
    if (requestedEpoch <= 0L || requestedEpoch != expectedEpoch || !hasBaseline) {
      return null to true
    }

    val uptimeDeltaMs = nowUptimeMs - prevUptimeMs
    val elapsedDeltaMs = nowElapsedMs - startElapsedMs

    // Advance CPU/time baselines unconditionally so discarded mixed windows re-anchor cleanly (C1).
    val cpuStatus: String
    val cpuDeltaSec: Double
    if (nowCPUMs < prevCPUMs) {
      cpuStatus = "error"
      cpuDeltaSec = 0.0
    } else {
      cpuStatus = "ok"
      cpuDeltaSec = (nowCPUMs - prevCPUMs).toDouble() / 1000.0
    }
    prevUptimeMs = nowUptimeMs
    prevElapsedMs = nowElapsedMs
    prevCPUMs = nowCPUMs

    // Context switches & UID network deltas also advance cleanly so discarded windows never leak into next window.
    var cswStatus = "error"
    var cswDelta = 0L
    if (curCsw != null && prevVoluntaryCtxSwitches != null) {
      if (curCsw >= prevVoluntaryCtxSwitches!!) {
        cswStatus = "ok"
        cswDelta = curCsw - prevVoluntaryCtxSwitches!!
      }
    }
    prevVoluntaryCtxSwitches = curCsw

    val unsupported = TrafficStats.UNSUPPORTED.toLong()
    var netStatus = "ok"
    var txDelta: Long? = null
    var rxDelta: Long? = null
    if (curTx == unsupported || curRx == unsupported || curTx < 0L || curRx < 0L) {
      netStatus = "unsupported"
      prevTxBytes = null
      prevRxBytes = null
    } else {
      val pTx = prevTxBytes
      val pRx = prevRxBytes
      if (pTx != null && pRx != null && curTx >= pTx && curRx >= pRx) {
        txDelta = curTx - pTx
        rxDelta = curRx - pRx
      } else if (pTx != null || pRx != null) {
        netStatus = "error"
      }
      prevTxBytes = curTx
      prevRxBytes = curRx
    }

    // Drain only timestamped events captured at or before T_cut (nowElapsedMs) under eventLock.
    val events = mutableListOf<StateSnapshot>()
    val powerEvents = mutableListOf<PowerSnapshot>()
    val batteryObsEvents = mutableListOf<BatteryObsEvent>()
    val overflow: Boolean
    val pOverflow: Boolean
    val bOverflow: Boolean
    var drainedStateSeq = preProbeStateSeq
    var drainedPowerSeq = preProbePowerSeq
    var drainedBatteryObsSeq = preProbeBatteryObsSeq

    synchronized(eventLock) {
      overflow = stateOverflowed
      stateOverflowed = false
      while (pendingStates.isNotEmpty() && pendingStates.first().elapsedMs <= nowElapsedMs) {
        val ev = pendingStates.removeAt(0)
        events.add(ev)
        if (ev.seq > drainedStateSeq) {
          drainedStateSeq = ev.seq
        }
      }

      pOverflow = powerOverflowed
      powerOverflowed = false
      while (pendingPower.isNotEmpty() && pendingPower.first().elapsedMs <= nowElapsedMs) {
        val pev = pendingPower.removeAt(0)
        powerEvents.add(pev)
        if (pev.seq > drainedPowerSeq) {
          drainedPowerSeq = pev.seq
        }
      }

      bOverflow = batteryObsOverflowed
      batteryObsOverflowed = false
      while (pendingBatteryObs.isNotEmpty() && pendingBatteryObs.first().elapsedMs <= nowElapsedMs) {
        val bev = pendingBatteryObs.removeAt(0)
        batteryObsEvents.add(bev)
        if (bev.seq > drainedBatteryObsSeq) {
          drainedBatteryObsSeq = bev.seq
        }
      }
    }

    val hadContinuousBatteryCoverage = batteryReceiver != null && !resetBatterySeed && !bOverflow
    val settledStateTail = events.lastOrNull()
    val curAppState: String
    val curBgMode: String?
    val curScreenState: String
    val curMcastHeld: Boolean
    if (drainedStateSeq > preProbeStateSeq && settledStateTail != null && settledStateTail.elapsedMs == nowElapsedMs) {
      curAppState = settledStateTail.appState
      curBgMode = settledStateTail.bgMode
      curScreenState = settledStateTail.screenState
      curMcastHeld = settledStateTail.multicastHeld
    } else if (stateQueryProvenAtCut) {
      curAppState = queryAppState
      curBgMode = queryBgMode
      curScreenState = queryScreenState
      curMcastHeld = queryMcastHeld
    } else {
      curAppState = settledStateTail?.appState ?: baseAppState
      curBgMode = settledStateTail?.bgMode ?: baseBgMode
      curScreenState = settledStateTail?.screenState ?: baseScreenState
      curMcastHeld = settledStateTail?.multicastHeld ?: baseMulticastHeld
    }

    val settledPowerTail = powerEvents.lastOrNull()
    val powerSource = when {
      drainedPowerSeq > preProbePowerSeq && settledPowerTail != null && settledPowerTail.elapsedMs == nowElapsedMs -> settledPowerTail.powerSource
      batteryQueryProvenAtCut -> queryPowerSource
      hadContinuousBatteryCoverage -> settledPowerTail?.powerSource ?: basePowerSource
      else -> settledPowerTail?.powerSource ?: basePowerSource
    }
    val lastInWindowBatObs = if (!bOverflow) {
      batteryObsEvents
        .lastOrNull { !it.isLpmBroadcast && it.epoch == expectedEpoch }
    } else {
      null
    }
    val batteryCutoffProven: Boolean
    val batteryPct: Double?
    when {
      lastInWindowBatObs != null && lastInWindowBatObs.seq > preProbeBatteryObsSeq && lastInWindowBatObs.elapsedMs == nowElapsedMs -> {
        batteryCutoffProven = true
        batteryPct = lastInWindowBatObs.batteryPct
      }
      batteryQueryProvenAtCut -> {
        batteryCutoffProven = true
        batteryPct = queryBatteryPct
      }
      lastInWindowBatObs != null -> {
        batteryCutoffProven = true
        batteryPct = lastInWindowBatObs.batteryPct
      }
      hadContinuousBatteryCoverage -> {
        batteryCutoffProven = true
        batteryPct = baseBatteryPct
      }
      else -> {
        batteryCutoffProven = false
        batteryPct = null
      }
    }
    val lowPowerMode = when {
      lpmQueryProvenAtCut -> queryLowPowerMode
      hadContinuousBatteryCoverage -> {
        if (batteryObsEvents.any { it.epoch == expectedEpoch && it.isLpmBroadcast }) !baseLowPowerMode else baseLowPowerMode
      }
      else -> baseLowPowerMode
    }

    // Carry any unproven blocked query results strictly into the next window (> T_cut) when no
    // broadcast already queued a matching post-cutoff event.
    synchronized(eventLock) {
      if (requestedEpoch == expectedEpoch && activeEventEpoch == expectedEpoch) {
        if (!stateQueryProvenAtCut && !overflow &&
          (queryAppState != curAppState || queryBgMode != curBgMode || queryScreenState != curScreenState || queryMcastHeld != curMcastHeld) &&
          pendingStates.none {
            it.appState == queryAppState && it.bgMode == queryBgMode && it.screenState == queryScreenState && it.multicastHeld == queryMcastHeld
          }
        ) {
          val qStateMs = maxOf(afterInstantQueriesElapsedMs, nowElapsedMs + 1L)
          insertStateSortedLocked(queryAppState, queryBgMode, queryScreenState, queryMcastHeld, qStateMs, nowUptimeMs)
          if (pendingStates.none { it.elapsedMs > qStateMs }) {
            cachedScreenInteractive = queryInteractive
          }
        }
        if ((!batteryQueryProvenAtCut || !lpmQueryProvenAtCut) && !pOverflow && !bOverflow &&
          (queryPowerSource != "unknown" || queryBatteryPct != null)
        ) {
          val qBatMs = maxOf(afterBatteryElapsedMs, nowElapsedMs + 1L)
          val qObsMs = if (!batteryQueryProvenAtCut) qBatMs else maxOf(afterLpmElapsedMs, nowElapsedMs + 1L)
          if (!batteryQueryProvenAtCut && pendingPower.none { it.powerSource == queryPowerSource } && queryPowerSource != powerSource) {
            insertPowerSortedLocked(queryPowerSource, qBatMs)
          }
          val effCarrierSource = if (!batteryQueryProvenAtCut) queryPowerSource else powerSource
          val effCarrierPct = if (!batteryQueryProvenAtCut) queryBatteryPct else batteryPct
          val needCarryBat = !batteryQueryProvenAtCut &&
            (queryPowerSource != powerSource || queryBatteryPct != batteryPct) &&
            pendingBatteryObs.none { !it.isLpmBroadcast && it.powerSource == queryPowerSource && it.batteryPct == queryBatteryPct }
          val needCarryLpm = !lpmQueryProvenAtCut &&
            queryLowPowerMode != lowPowerMode &&
            pendingBatteryObs.none { it.lowPowerMode == queryLowPowerMode }
          if (needCarryBat || needCarryLpm) {
            enqueueBatteryObsLocked(
              BatteryObsEvent(
                epoch = expectedEpoch,
                powerSource = effCarrierSource,
                batteryPct = effCarrierPct,
                lowPowerMode = queryLowPowerMode,
                appState = curAppState,
                elapsedMs = qObsMs,
                active = true,
                seq = ++batteryObsSeq,
                isLpmBroadcast = !needCarryBat && needCarryLpm,
              ),
            )
            if (pendingBatteryObs.none { it.elapsedMs > qObsMs }) {
              if (!batteryQueryProvenAtCut) {
                cachedPowerSource = queryPowerSource
                cachedBatteryPct = queryBatteryPct
              }
              if (!lpmQueryProvenAtCut) {
                cachedLowPowerMode = queryLowPowerMode
              }
            }
          }
        }
      }
    }

    // Replay queued broadcast battery observations strictly up to nowElapsedMs before evaluation.
    if (bOverflow || !batteryCutoffProven) {
      batteryTracker.reset()
    } else {
      val lpmBroadcastCount = batteryObsEvents.count { it.epoch == expectedEpoch && it.isLpmBroadcast }
      val lpmToggledBack = lpmBroadcastCount >= 2 ||
        (lpmBroadcastCount == 1 && lowPowerMode == (batteryTracker.segment?.lowPowerMode ?: lowPowerMode))
      var lpmFlipApplied = false
      for (bev in batteryObsEvents) {
        if (bev.epoch == expectedEpoch) {
          val effLpm = when {
            !bev.isLpmBroadcast -> if (lpmFlipApplied) lowPowerMode else bev.lowPowerMode
            lpmToggledBack -> {
              lpmFlipApplied = !lpmFlipApplied
              !(batteryTracker.segment?.lowPowerMode ?: lowPowerMode)
            }
            else -> {
              lpmFlipApplied = true
              lowPowerMode
            }
          }
          batteryTracker.onObservation(
            powerSource = bev.powerSource,
            batteryPct = bev.batteryPct,
            lowPowerMode = effLpm,
            appState = bev.appState,
            elapsedMs = bev.elapsedMs,
            active = bev.active,
          )
        }
      }
    }

    // Check whether an unnotified host state/mode/power change happened between events and T_cut.
    val tailApp = settledStateTail?.appState ?: baseAppState
    val tailBg = settledStateTail?.bgMode ?: baseBgMode
    val tailScr = settledStateTail?.screenState ?: baseScreenState
    val tailMcast = settledStateTail?.multicastHeld ?: baseMulticastHeld
    val hasUnnotifiedApp = curAppState != tailApp
    val hasUnnotifiedBg = curBgMode != tailBg || curScreenState != tailScr || curMcastHeld != tailMcast
    val tailPower = settledPowerTail?.powerSource ?: basePowerSource
    val hasUnnotifiedPower = powerSource != tailPower || (!batteryCutoffProven && !hadContinuousBatteryCoverage)

    // 1. Evaluate cumulative-counter purity over (app_state, power_source), orthogonal to bg_mode switches.
    val appTransitions = mutableListOf<StateSnapshot>()
    var trackApp = baseAppState
    for (ev in events) {
      if (ev.appState != trackApp) {
        appTransitions.add(ev)
        trackApp = ev.appState
      }
    }
    val powerTransitions = mutableListOf<PowerSnapshot>()
    var trackPower = basePowerSource
    for (pev in powerEvents) {
      if (pev.powerSource != trackPower) {
        powerTransitions.add(pev)
        trackPower = pev.powerSource
      }
    }
    var eventOrderBroken = false
    var checkStateT = startElapsedMs
    for (ev in events) {
      if (ev.elapsedMs < checkStateT || (ev.elapsedMs - checkStateT) > MAX_OBSERVATION_GAP_MS) {
        eventOrderBroken = true
        break
      }
      checkStateT = ev.elapsedMs
    }
    if (!eventOrderBroken) {
      var checkPowerT = startElapsedMs
      for (pev in powerEvents) {
        if (pev.elapsedMs < checkPowerT || (pev.elapsedMs - checkPowerT) > MAX_OBSERVATION_GAP_MS) {
          eventOrderBroken = true
          break
        }
        checkPowerT = pev.elapsedMs
      }
    }
    if (!eventOrderBroken) {
      var checkBatT = startElapsedMs
      for (bev in batteryObsEvents) {
        if (bev.elapsedMs < checkBatT) {
          eventOrderBroken = true
          break
        }
        checkBatT = bev.elapsedMs
      }
    }

    val totalCounterTransitions = appTransitions.size + powerTransitions.size
    val (countersPure, counterAppState, counterPowerSource) = when {
      overflow || pOverflow || bOverflow || eventOrderBroken || hasUnnotifiedApp || hasUnnotifiedPower || totalCounterTransitions >= 2 ->
        Triple(false, curAppState, powerSource)
      totalCounterTransitions == 0 ->
        Triple(true, curAppState, powerSource)
      else -> {
        val evElapsed = appTransitions.firstOrNull()?.elapsedMs ?: powerTransitions[0].elapsedMs
        val prevDurMs = evElapsed - startElapsedMs
        val spillMs = nowElapsedMs - evElapsed
        if (prevDurMs > 0L && spillMs >= 0L && spillMs <= spillLimitMs(prevDurMs)) {
          Triple(true, baseAppState, basePowerSource)
        } else {
          Triple(false, curAppState, powerSource)
        }
      }
    }

    val sleepJumped: Boolean
    val windowSeconds: Double
    if (counterAppState == "background_screen_off") {
      // Standby CPU deep sleep causes elapsedDelta > uptimeDelta; only flag gaps/rollbacks.
      sleepJumped = elapsedDeltaMs <= 0L || uptimeDeltaMs < 0L || elapsedDeltaMs > MAX_OBSERVATION_GAP_MS
      windowSeconds = maxOf(0.0, elapsedDeltaMs.toDouble() / 1000.0)
    } else {
      sleepJumped = uptimeDeltaMs <= 0L ||
        elapsedDeltaMs < 0L ||
        (elapsedDeltaMs - uptimeDeltaMs) > 10_000L ||
        elapsedDeltaMs > MAX_OBSERVATION_GAP_MS
      windowSeconds = maxOf(0.0, uptimeDeltaMs.toDouble() / 1000.0)
    }

    // 2. Piecewise wall-clock residency segmentation for bg_mode.time.
    val modeSlices = linkedMapOf<ModeKey, Double>()
    val bgResidencyValid = !overflow && !eventOrderBroken && !hasUnnotifiedBg && !hasUnnotifiedApp && !sleepJumped && elapsedDeltaMs > 0L
    if (bgResidencyValid) {
      var segT = startElapsedMs
      var segApp = baseAppState
      var segBg = baseBgMode
      var segScr = baseScreenState
      var segMcast = baseMulticastHeld
      var monotonicOk = true

      for (ev in events) {
        if (ev.elapsedMs < segT || (ev.elapsedMs - segT) > MAX_OBSERVATION_GAP_MS) {
          monotonicOk = false
          break
        }
        val dtSec = (ev.elapsedMs - segT).toDouble() / 1000.0
        if (dtSec > 0.0 && segBg != null && segApp != "suspended") {
          val k = ModeKey(segBg, segScr, segMcast)
          modeSlices[k] = (modeSlices[k] ?: 0.0) + dtSec
        }
        segT = ev.elapsedMs
        segApp = ev.appState
        segBg = ev.bgMode
        segScr = ev.screenState
        segMcast = ev.multicastHeld
      }
      if (monotonicOk && nowElapsedMs >= segT && (nowElapsedMs - segT) <= MAX_OBSERVATION_GAP_MS) {
        val dtSec = (nowElapsedMs - segT).toDouble() / 1000.0
        if (dtSec > 0.0 && segBg != null && segApp != "suspended") {
          val k = ModeKey(segBg, segScr, segMcast)
          modeSlices[k] = (modeSlices[k] ?: 0.0) + dtSec
        }
      } else {
        modeSlices.clear()
      }
    }

    val hasBatteryInterruptingTransition = appTransitions.isNotEmpty() || powerTransitions.isNotEmpty() || hasUnnotifiedApp || hasUnnotifiedPower
    val resetAfterBattery = !countersPure || hasBatteryInterruptingTransition || bOverflow || eventOrderBroken || !batteryCutoffProven

    val batteryEval: BatteryDischargeTracker.Result = when {
      batteryReceiver == null -> {
        batteryTracker.reset()
        val status = if (powerSource == "unknown" || batteryPct == null || batteryPct < 0.0 || batteryPct > 100.0) {
          "unsupported"
        } else {
          "error"
        }
        BatteryDischargeTracker.Result(drainRate = null, probeStatus = status)
      }
      resetBatterySeed -> {
        batteryTracker.reset()
        if (batteryCutoffProven) {
          batteryTracker.onObservation(
            powerSource = powerSource,
            batteryPct = batteryPct,
            lowPowerMode = lowPowerMode,
            appState = curAppState,
            elapsedMs = nowElapsedMs,
            active = true,
          )
        } else if (queryPowerSource != "unknown" || queryBatteryPct != null) {
          batteryTracker.onObservation(
            powerSource = queryPowerSource,
            batteryPct = queryBatteryPct,
            lowPowerMode = queryLowPowerMode,
            appState = curAppState,
            elapsedMs = maxOf(afterBatteryElapsedMs, nowElapsedMs + 1L),
            active = true,
          )
        }
        BatteryDischargeTracker.Result(drainRate = null, probeStatus = "ok")
      }
      !batteryCutoffProven || eventOrderBroken -> {
        batteryTracker.reset()
        BatteryDischargeTracker.Result(drainRate = null, probeStatus = "ok")
      }
      else -> {
        batteryTracker.evaluateSample(
          powerSource = counterPowerSource,
          batteryPct = batteryPct,
          lowPowerMode = lowPowerMode,
          appState = counterAppState,
          elapsedMs = nowElapsedMs,
          sleepJumped = sleepJumped || !countersPure || bOverflow || eventOrderBroken,
          resetAfter = resetAfterBattery,
        )
      }
    }

    // Re-anchor baseline state to cutoff-proven settled snapshot.
    baseAppState = curAppState
    baseBgMode = curBgMode
    baseScreenState = curScreenState
    baseMulticastHeld = curMcastHeld
    basePowerSource = powerSource
    baseBatteryPct = batteryPct
    baseLowPowerMode = lowPowerMode

    if (elapsedDeltaMs <= 0L && !sleepJumped && modeSlices.isEmpty()) {
      return null to resetAfterBattery
    }

    val emitCounters = countersPure && (windowSeconds > 0.0 || sleepJumped)
    val elapsedSec = maxOf(0.0, elapsedDeltaMs.toDouble() / 1000.0)
    val hasPrimaryThermalOrBattery =
      (emitCounters && !sleepJumped && elapsedSec > 0.0) ||
        batteryEval.drainRate != null ||
        batteryEval.probeStatus == "unsupported" ||
        batteryEval.probeStatus == "error"

    if (!emitCounters && modeSlices.isEmpty() && !hasPrimaryThermalOrBattery) {
      return null to resetAfterBattery
    }

    val envelopeParts = mutableListOf<String>()
    if (emitCounters) {
      val resource = JSONObject()
        .put("process_role", "mobile_app")
        .put("app_state", counterAppState)
        .put("power_source", counterPowerSource)
        .put("window_seconds", windowSeconds)
        .put("cpu_seconds", maxOf(0.0, cpuDeltaSec))
        .put("cpu_probe_status", cpuStatus)
        .put("sleep_jumped", sleepJumped)
        .put("context_switches", maxOf(0L, cswDelta))
        .put("context_switches_probe_status", cswStatus)
      if (memBytes > 0L) {
        resource.put("memory_probe_status", "ok")
        resource.put("memory_footprint_bytes", memBytes)
      } else {
        resource.put("memory_probe_status", "error")
      }
      envelopeParts.add("\"resource\":$resource")
    }

    val power = JSONObject()
      .put("process_role", "mobile_app")
      .put("app_state", counterAppState)
      .put("power_source", counterPowerSource)
    var hasPower = false

    if (emitCounters) {
      power.put("network_probe_status", netStatus)
      if (txDelta != null) power.put("tx_bytes", txDelta)
      if (rxDelta != null) power.put("rx_bytes", rxDelta)
      hasPower = true

      if (!sleepJumped && elapsedSec > 0.0) {
        if (queryThermalStatus != null) {
          if (thermalCutoffProven) {
            val level = when (queryThermalStatus) {
              PowerManager.THERMAL_STATUS_NONE -> "nominal"
              PowerManager.THERMAL_STATUS_LIGHT, PowerManager.THERMAL_STATUS_MODERATE -> "fair"
              PowerManager.THERMAL_STATUS_SEVERE -> "serious"
              PowerManager.THERMAL_STATUS_CRITICAL,
              PowerManager.THERMAL_STATUS_EMERGENCY,
              PowerManager.THERMAL_STATUS_SHUTDOWN,
              -> "critical"
              else -> "nominal"
            }
            power.put("thermal_level", level)
            power.put("thermal_seconds", elapsedSec)
            power.put("thermal_probe_status", "ok")
          }
          // Omit thermal_level/seconds/probe_status if currentThermalStatus crossed T_cut
          // so post-cutoff readings neither attribute to the closed window nor emit synthetic failures.
        } else {
          power.put("thermal_probe_status", "unsupported")
        }
      }
    }

    if (batteryEval.drainRate != null) {
      power.put("battery_drain_rate", batteryEval.drainRate)
      power.put("low_power_mode", lowPowerMode)
      power.put("battery_probe_status", "ok")
      hasPower = true
    } else if (batteryEval.probeStatus == "unsupported" || batteryEval.probeStatus == "error") {
      power.put("battery_probe_status", batteryEval.probeStatus)
      hasPower = true
    }

    val extraPowerSamples = mutableListOf<JSONObject>()
    val sliceIter = modeSlices.entries.iterator()
    if (sliceIter.hasNext()) {
      val first = sliceIter.next()
      power.put("bg_mode", first.key.mode)
      power.put("screen_state", first.key.screenState)
      power.put("multicast_lock", first.key.multicastLock)
      power.put("bg_mode_seconds", first.value)
      hasPower = true
      while (sliceIter.hasNext()) {
        val next = sliceIter.next()
        extraPowerSamples.add(
          JSONObject()
            .put("process_role", "mobile_app")
            .put("app_state", if (next.key.screenState == "off") "background_screen_off" else "background")
            .put("power_source", powerSource)
            .put("bg_mode", next.key.mode)
            .put("screen_state", next.key.screenState)
            .put("multicast_lock", next.key.multicastLock)
            .put("bg_mode_seconds", next.value),
        )
      }
    }

    if (hasPower) {
      envelopeParts.add("\"power\":$power")
    }
    if (extraPowerSamples.isNotEmpty()) {
      val arr = extraPowerSamples.joinToString(",", "[", "]") { it.toString() }
      envelopeParts.add("\"power_samples\":$arr")
    }

    val body = if (envelopeParts.isEmpty()) null else envelopeParts.joinToString(",", "{", "}")
    return body to resetAfterBattery
  }

  private fun readMemoryFootprintBytes(): Long {
    try {
      val mi = Debug.MemoryInfo()
      Debug.getMemoryInfo(mi)
      if (mi.totalPss > 0) {
        return mi.totalPss.toLong() * 1024L
      }
    } catch (_: Exception) {
    }
    val rssKiB = readProcSelfStatusField("VmRSS")
    if (rssKiB != null && rssKiB > 0L) {
      return rssKiB * 1024L
    }
    return 0L
  }

  private fun isPowerSaveMode(): Boolean {
    val pm = appContext.getSystemService(Context.POWER_SERVICE) as? PowerManager
    return pm?.isPowerSaveMode == true
  }

  private fun resolveAppState(screenState: String): String {
    if (UnicEngineHost.isEngineSuspended) return "suspended"
    if (UnicEngineHost.isForeground) return "foreground"
    return if (screenState == "on") "background" else "background_screen_off"
  }

  private fun currentScreenState(): String {
    val pm = appContext.getSystemService(Context.POWER_SERVICE) as? PowerManager
    return if (pm?.isInteractive != false) "on" else "off"
  }

  private fun currentAppState(): String = resolveAppState(currentScreenState())

  private fun currentBgMode(): String? = UnicEngineHost.activeBackgroundMode()

  private fun parseBatteryIntent(intent: Intent): Pair<String, Double?> {
    val plugged = intent.getIntExtra(BatteryManager.EXTRA_PLUGGED, -1)
    val powerSource = when {
      plugged == 0 -> "battery"
      plugged > 0 -> "plugged"
      else -> "unknown"
    }
    val level = intent.getIntExtra(BatteryManager.EXTRA_LEVEL, -1)
    val scale = intent.getIntExtra(BatteryManager.EXTRA_SCALE, -1)
    val pct = if (level >= 0 && scale > 0) (level.toDouble() * 100.0) / scale.toDouble() else null
    return Pair(powerSource, pct)
  }

  private fun readBatterySnapshot(directIntent: Intent?): Pair<String, Double?> {
    val intent = directIntent ?: try {
      appContext.registerReceiver(null, IntentFilter(Intent.ACTION_BATTERY_CHANGED))
    } catch (_: Exception) {
      null
    } ?: return Pair("unknown", null)

    return parseBatteryIntent(intent)
  }

  private fun readProcSelfStatusField(key: String): Long? {
    return try {
      val prefix = "$key:"
      File("/proc/self/status").bufferedReader().useLines { lines ->
        for (raw in lines) {
          val line = raw.trim()
          if (!line.startsWith(prefix)) continue
          val valuePart = line.removePrefix(prefix).trim().split(Regex("\\s+")).firstOrNull()
          return@useLines valuePart?.toLongOrNull()
        }
        null
      }
    } catch (_: Exception) {
      null
    }
  }
}
