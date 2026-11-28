import Darwin
import Foundation
import UIKit
import UnicCore

/// Collects iOS main-app process CPU, physical memory footprint, thermal residency,
/// and cached power_source labels without blocking synchronously on the UI main thread
/// inside Go metrics callbacks. Per Decision A1, system_battery.drain_rate is not
/// emitted on iOS (`battery_probe_status="unsupported"`).
final class ResourcePowerCollector: NSObject, MobileHostResourceProviderProtocol, @unchecked Sendable {
  private let lock = NSLock()
  private var lastEpoch: Int64 = 0
  private var hasBaseline = false
  private var prevUptimeSec: Double = 0
  private var prevContSec: Double = 0
  private var prevCPUTimeSec: Double = 0

  private var currentAppState: String = "foreground"
  private var cachedBatteryState: UIDevice.BatteryState = .unknown
  private var cachedThermalState: ProcessInfo.ThermalState = ProcessInfo.processInfo.thermalState
  private var batteryMonitoringRequested = false
  private var observers: [NSObjectProtocol] = []

  private static let timebase: mach_timebase_info_data_t = {
    var info = mach_timebase_info_data_t()
    mach_timebase_info(&info)
    if info.denom == 0 {
      info.numer = 1
      info.denom = 1
    }
    return info
  }()

  override init() {
    super.init()
    let center = NotificationCenter.default
    observers.append(
      center.addObserver(forName: UIDevice.batteryStateDidChangeNotification, object: nil, queue: nil) { [weak self] _ in
        if Thread.isMainThread {
          self?.refreshDeviceBatteryOnMain()
        } else {
          // Off-main delivery cannot trust delayed main-queue reads across plug/unplug bursts;
          // invalidate cached power_source to unknown until main thread refreshes cleanly.
          self?.invalidateCachedBatteryState()
          DispatchQueue.main.async { self?.refreshDeviceBatteryOnMain() }
        }
      }
    )
    observers.append(
      center.addObserver(forName: ProcessInfo.thermalStateDidChangeNotification, object: nil, queue: nil) { [weak self] _ in
        guard let self else { return }
        self.lock.lock()
        self.cachedThermalState = ProcessInfo.processInfo.thermalState
        self.lock.unlock()
      }
    )
  }

  deinit {
    let center = NotificationCenter.default
    for obs in observers {
      center.removeObserver(obs)
    }
    lock.lock()
    let wasMonitoring = batteryMonitoringRequested
    batteryMonitoringRequested = false
    lock.unlock()
    if wasMonitoring {
      DispatchQueue.main.async {
        UIDevice.current.isBatteryMonitoringEnabled = false
      }
    }
  }

  private func invalidateCachedBatteryState() {
    lock.lock()
    cachedBatteryState = .unknown
    lock.unlock()
  }

  private func setBatteryMonitoringLocked(_ enabled: Bool) {
    guard batteryMonitoringRequested != enabled else { return }
    batteryMonitoringRequested = enabled
    if !enabled {
      cachedBatteryState = .unknown
    }
    DispatchQueue.main.async { [weak self] in
      UIDevice.current.isBatteryMonitoringEnabled = enabled
      if enabled {
        self?.refreshDeviceBatteryOnMain()
      }
    }
  }

  private func refreshDeviceBatteryOnMain() {
    let enabled = UIDevice.current.isBatteryMonitoringEnabled
    let state = enabled ? UIDevice.current.batteryState : .unknown
    lock.lock()
    cachedBatteryState = state
    lock.unlock()
  }

  /// Called on foreground/background/suspend lifecycle transitions to settle
  /// the ending state's CPU/thermal window with its prior app_state label.
  func notifyLifecycleTransition(appState: String, engine: MobileEngine?) {
    var payloadJSON: String?
    lock.lock()
    if lastEpoch > 0 && hasBaseline {
      payloadJSON = buildSampleLocked()
    } else {
      hasBaseline = false
    }
    currentAppState = appState
    lock.unlock()

    if let json = payloadJSON, let eng = engine {
      try? eng.submitPowerWindow(json)
    }
  }

  func resetOnSuspension() {
    lock.lock()
    hasBaseline = false
    setBatteryMonitoringLocked(false)
    lock.unlock()
  }

  func collectResourceSnapshot(_ epoch: Int64) -> String {
    lock.lock()
    defer { lock.unlock() }

    if epoch <= 0 {
      lastEpoch = 0
      hasBaseline = false
      setBatteryMonitoringLocked(false)
      return ""
    }
    if epoch != lastEpoch || !hasBaseline {
      lastEpoch = epoch
      primeLocked()
      return ""
    }
    return buildSampleLocked() ?? ""
  }

  private func primeLocked() {
    prevUptimeSec = ProcessInfo.processInfo.systemUptime
    prevContSec = Self.continuousSeconds()
    prevCPUTimeSec = Self.processCPUSeconds() ?? 0
    hasBaseline = true
    setBatteryMonitoringLocked(true)
  }

  private func buildSampleLocked() -> String? {
    let nowUptime = ProcessInfo.processInfo.systemUptime
    let nowCont = Self.continuousSeconds()
    let nowCPU = Self.processCPUSeconds()
    let physFootprint = Self.physFootprintBytes()

    let activeDelta = nowUptime - prevUptimeSec
    let contDelta = nowCont - prevContSec
    let sleepJumped = activeDelta <= 0 || contDelta < 0 || (contDelta - activeDelta) > 10.0 || contDelta > 180.0

    var cpuStatus = "ok"
    var cpuDelta = 0.0
    if let nowCPU {
      if nowCPU < prevCPUTimeSec {
        cpuStatus = "error"
      } else {
        cpuDelta = nowCPU - prevCPUTimeSec
      }
      prevCPUTimeSec = nowCPU
    } else {
      cpuStatus = "error"
    }

    prevUptimeSec = nowUptime
    prevContSec = nowCont

    if activeDelta <= 0 && !sleepJumped {
      return nil
    }

    let powerSource = Self.mapBatteryState(cachedBatteryState)

    var resource: [String: Any] = [
      "process_role": "mobile_app",
      "app_state": currentAppState,
      "power_source": powerSource,
      "window_seconds": max(0.0, activeDelta),
      "cpu_seconds": max(0.0, cpuDelta),
      "cpu_probe_status": cpuStatus,
      "sleep_jumped": sleepJumped,
      "wakeups_probe_status": "unsupported",
    ]
    if let physFootprint, physFootprint > 0 {
      resource["memory_footprint_bytes"] = physFootprint
      resource["memory_probe_status"] = "ok"
    } else {
      resource["memory_probe_status"] = "error"
    }

    var power: [String: Any] = [
      "process_role": "mobile_app",
      "app_state": currentAppState,
      "power_source": powerSource,
      "network_probe_status": "unsupported",
      "battery_probe_status": "unsupported",
    ]
    if !sleepJumped && contDelta > 0 {
      power["thermal_level"] = Self.mapThermalState(cachedThermalState)
      power["thermal_seconds"] = contDelta
      power["thermal_probe_status"] = "ok"
    }

    let envelope: [String: Any] = [
      "resource": resource,
      "power": power,
    ]
    guard let data = try? JSONSerialization.data(withJSONObject: envelope),
          let json = String(data: data, encoding: .utf8) else {
      return nil
    }
    return json
  }

  private static func continuousSeconds() -> Double {
    let ticks = mach_continuous_time()
    let numer = UInt64(timebase.numer)
    let denom = UInt64(timebase.denom)
    let ns = (ticks / denom) * numer + ((ticks % denom) * numer) / denom
    return Double(ns) / 1_000_000_000.0
  }

  private static func processCPUSeconds() -> Double? {
    var ru = rusage()
    guard getrusage(RUSAGE_SELF, &ru) == 0 else { return nil }
    let user = Double(ru.ru_utime.tv_sec) + Double(ru.ru_utime.tv_usec) / 1_000_000.0
    let sys = Double(ru.ru_stime.tv_sec) + Double(ru.ru_stime.tv_usec) / 1_000_000.0
    return user + sys
  }

  private static func physFootprintBytes() -> UInt64? {
    var info = task_vm_info_data_t()
    var count = mach_msg_type_number_t(MemoryLayout<task_vm_info_data_t>.size / MemoryLayout<integer_t>.size)
    let kr = withUnsafeMutablePointer(to: &info) { ptr in
      ptr.withMemoryRebound(to: integer_t.self, capacity: Int(count)) { intPtr in
        task_info(mach_task_self_, task_flavor_t(TASK_VM_INFO), intPtr, &count)
      }
    }
    guard kr == KERN_SUCCESS else { return nil }
    return UInt64(info.phys_footprint)
  }

  private static func mapBatteryState(_ state: UIDevice.BatteryState) -> String {
    switch state {
    case .unplugged:
      return "battery"
    case .charging, .full:
      return "plugged"
    case .unknown:
      return "unknown"
    @unknown default:
      return "unknown"
    }
  }

  private static func mapThermalState(_ state: ProcessInfo.ThermalState) -> String {
    switch state {
    case .nominal:
      return "nominal"
    case .fair:
      return "fair"
    case .serious:
      return "serious"
    case .critical:
      return "critical"
    @unknown default:
      return "nominal"
    }
  }
}
