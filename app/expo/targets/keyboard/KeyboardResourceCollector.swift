import Darwin
import Foundation
import UnicCore

/// Extension-safe resource collector for the iOS Keyboard Extension (`ios_keyboard`).
/// Never accesses `UIApplication.shared` or `MXMetricManager`.
final class KeyboardResourceCollector: NSObject, MobileHostResourceProviderProtocol, @unchecked Sendable {
  private let lock = NSLock()
  private var lastEpoch: Int64 = 0
  private var hasBaseline = false
  private var prevUptimeSec: Double = 0
  private var prevContSec: Double = 0
  private var prevCPUTimeSec: Double = 0

  private static let timebase: mach_timebase_info_data_t = {
    var info = mach_timebase_info_data_t()
    mach_timebase_info(&info)
    if info.denom == 0 {
      info.numer = 1
      info.denom = 1
    }
    return info
  }()

  func resetOnSuspension() {
    lock.lock()
    hasBaseline = false
    lock.unlock()
  }

  func collectResourceSnapshot(_ epoch: Int64) -> String {
    lock.lock()
    defer { lock.unlock() }

    if epoch <= 0 {
      lastEpoch = 0
      hasBaseline = false
      return ""
    }
    if epoch != lastEpoch || !hasBaseline {
      lastEpoch = epoch
      prevUptimeSec = ProcessInfo.processInfo.systemUptime
      prevContSec = Self.continuousSeconds()
      prevCPUTimeSec = Self.processCPUSeconds() ?? 0
      hasBaseline = true
      return ""
    }

    let nowUptime = ProcessInfo.processInfo.systemUptime
    let nowCont = Self.continuousSeconds()
    let activeDelta = nowUptime - prevUptimeSec
    let contDelta = nowCont - prevContSec
    let sleepJumped = activeDelta <= 0 || contDelta < 0 || (contDelta - activeDelta) > 10.0 || contDelta > 180.0
    if activeDelta <= 0 && !sleepJumped {
      return ""
    }

    var cpuStatus = "ok"
    var cpuDelta = 0.0
    if let nowCPU = Self.processCPUSeconds() {
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

    var resource: [String: Any] = [
      "process_role": "ios_keyboard",
      "app_state": "foreground",
      "power_source": "unknown",
      "window_seconds": max(0.0, activeDelta),
      "cpu_seconds": max(0.0, cpuDelta),
      "cpu_probe_status": cpuStatus,
      "sleep_jumped": sleepJumped,
      "wakeups_probe_status": "unsupported",
    ]
    if let footprint = Self.physFootprintBytes(), footprint > 0 {
      resource["memory_footprint_bytes"] = footprint
      resource["memory_probe_status"] = "ok"
    } else {
      resource["memory_probe_status"] = "error"
    }

    let power: [String: Any] = [
      "process_role": "ios_keyboard",
      "app_state": "foreground",
      "power_source": "unknown",
      "battery_probe_status": "unsupported",
    ]
    let envelope: [String: Any] = [
      "resource": resource,
      "power": power,
    ]
    guard let data = try? JSONSerialization.data(withJSONObject: envelope),
          let json = String(data: data, encoding: .utf8) else {
      return ""
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
}
