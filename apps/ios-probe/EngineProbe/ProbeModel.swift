import Foundation
import Observation
import SwiftUI
import UIKit

@MainActor
@Observable
final class ProbeModel {
    var passphrase = "ios-probe-passphrase"
    var invitationCode = ""
    var textPayload = "iOS probe text"
    var query = ""
    var status = "Waiting"
    var transcript: [String] = []
    var isBusy = false

    private let bridge = ProbeBridge()
    private var started = false
    private var suspended = false
    private var automatedActionRan = false

    func startIfNeeded() async {
        guard !started else { return }
        let fileManager = FileManager.default
        guard let support = fileManager.urls(
            for: .applicationSupportDirectory,
            in: .userDomainMask
        ).first else {
            record(["ok": false, "kind": "directory_unavailable"])
            return
        }
        let cache = fileManager.urls(for: .cachesDirectory, in: .userDomainMask).first
            ?? support.appending(path: "cache")
        let result = await run([
            "command": "start",
            "private_data": support.path,
            "cache": cache.path,
            "temporary": fileManager.temporaryDirectory.path,
            "app_version": Bundle.main.object(
                forInfoDictionaryKey: "CFBundleShortVersionString"
            ) as? String ?? "0.1.0",
        ])
        started = result["ok"] as? Bool == true
    }

    func createSpace() async {
        _ = await run([
            "command": "create_space",
            "device_name": "iOS Engine Probe",
            "passphrase": passphrase,
        ])
    }

    func joinSpace() async {
        _ = await run([
            "command": "join_space",
            "invitation_code": invitationCode,
            "device_name": "iOS Engine Probe",
            "passphrase": passphrase,
        ])
    }

    func unlock() async -> Bool {
        let result = await run([
            "command": "unlock_space",
            "passphrase": passphrase,
        ])
        return result["ok"] as? Bool == true
    }

    func issueInvitation() async {
        let result = await run(["command": "issue_invitation"])
        if let code = result["invitation_code"] as? String {
            invitationCode = code
        }
    }

    func listDevices() async {
        _ = await run(["command": "list_devices"])
    }

    func sendText() async {
        _ = await run([
            "command": "send_text",
            "text": textPayload,
        ])
    }

    func sendImage() async {
        _ = await run([
            "command": "send_image",
            "bytes_base64": Self.samplePNG,
            "mime_type": "image/png",
        ])
    }

    func sendFile() async {
        let file = FileManager.default.temporaryDirectory
            .appending(path: "ios-probe-document.txt")
        do {
            try Data("iOS file payload".utf8).write(to: file, options: .atomic)
        } catch {
            record(["ok": false, "kind": "sample_file_failed"])
            return
        }
        _ = await run([
            "command": "send_file",
            "path": file.path,
            "display_name": "ios-probe-document.txt",
            "mime_type": "text/plain",
        ])
    }

    @discardableResult
    func queryHistory() async -> [String: Any] {
        await run([
            "command": "query_history",
            "query": query.isEmpty ? NSNull() : query,
            "limit": 50,
        ])
    }

    func verifyTextReceived(expectedBytes: Data) async {
        await verifyLatestPayload(
            contentType: "text",
            expectedBytes: expectedBytes,
            resultKind: "text_received"
        )
    }

    func verifyImageReceived() async {
        await verifyLatestPayload(
            contentType: "image",
            expectedBytes: nil,
            resultKind: "image_received"
        )
    }

    func verifyFileReceived(expectedBytes: Data) async {
        await verifyLatestPayload(
            contentType: "file",
            expectedBytes: expectedBytes,
            resultKind: "file_received"
        )
    }

    func eventSummary() async {
        _ = await run(["command": "event_summary"])
    }

    func suspendAndResume() async {
        let suspendedResult = await run(["command": "suspend"])
        guard suspendedResult["ok"] as? Bool == true else { return }
        suspended = true
        try? await Task.sleep(for: .seconds(1))
        let resumed = await run(["command": "resume"])
        suspended = resumed["ok"] as? Bool != true
    }

    func runAutomatedAction() async {
        guard started, !automatedActionRan else { return }
        automatedActionRan = true
        let environment = ProcessInfo.processInfo.environment
        guard let action = environment["UC_PROBE_ACTION"], !action.isEmpty else { return }
        if let value = environment["UC_PROBE_PASSPHRASE"] { passphrase = value }
        if let value = environment["UC_PROBE_INVITATION"] { invitationCode = value }

        switch action {
        case "join":
            await joinSpace()
        case "unlock":
            _ = await unlock()
        case "send_text":
            guard await unlock() else { return }
            if let encoded = environment["UC_PROBE_TEXT_BASE64"],
               let data = Data(base64Encoded: encoded),
               let text = String(data: data, encoding: .utf8) {
                textPayload = text
            }
            await sendText()
        case "send_image":
            guard await unlock() else { return }
            await sendImage()
        case "send_file":
            guard await unlock() else { return }
            await sendFile()
        case "query":
            guard await unlock() else { return }
            if let encoded = environment["UC_PROBE_QUERY_BASE64"],
               let data = Data(base64Encoded: encoded),
               let text = String(data: data, encoding: .utf8) {
                query = text
            }
            await queryHistory()
        case "verify_text":
            guard await unlock() else { return }
            guard let encoded = environment["UC_PROBE_EXPECTED_BASE64"],
                  let expectedBytes = Data(base64Encoded: encoded) else {
                record(["ok": false, "kind": "missing_expected_payload"])
                return
            }
            await verifyTextReceived(expectedBytes: expectedBytes)
        case "verify_image":
            guard await unlock() else { return }
            await verifyImageReceived()
        case "verify_file":
            guard await unlock() else { return }
            guard let encoded = environment["UC_PROBE_EXPECTED_BASE64"],
                  let expectedBytes = Data(base64Encoded: encoded) else {
                record(["ok": false, "kind": "missing_expected_payload"])
                return
            }
            await verifyFileReceived(expectedBytes: expectedBytes)
        case "monitor":
            guard await unlock() else { return }
            _ = await queryHistory()
            try? await Task.sleep(for: .seconds(20))
            await eventSummary()
            _ = await queryHistory()
        case "suspend_resume":
            guard await unlock() else { return }
            await suspendAndResume()
        case "events":
            await eventSummary()
        default:
            record(["ok": false, "kind": "unknown_automated_action"])
        }
    }

    func handleScenePhase(_ phase: ScenePhase) async {
        guard started, !isBusy else { return }
        switch phase {
        case .background where !suspended:
            let result = await run(["command": "suspend"])
            suspended = result["ok"] as? Bool == true
        case .active where suspended:
            let result = await run(["command": "resume"])
            suspended = result["ok"] as? Bool != true
        default:
            break
        }
    }

    private func run(_ command: [String: Any]) async -> [String: Any] {
        isBusy = true
        defer { isBusy = false }
        let bridge = bridge
        let result = await Task.detached(priority: .userInitiated) {
            bridge.command(command)
        }.value
        record(result)
        return result
    }

    private func verifyLatestPayload(
        contentType: String,
        expectedBytes: Data?,
        resultKind: String
    ) async {
        let history = await run([
            "command": "query_history",
            "query": NSNull(),
            "limit": 50,
        ])
        guard history["ok"] as? Bool == true,
              let entryIDs = history["entry_ids"] as? [String],
              let contentTypes = history["content_types"] as? [String],
              let index = contentTypes.firstIndex(of: contentType),
              entryIDs.indices.contains(index) else {
            record(["ok": false, "kind": resultKind, "count": 0])
            return
        }

        let destination = FileManager.default.temporaryDirectory
            .appending(path: "received-\(UUID().uuidString)")
        let exported = await run([
            "command": "export_entry",
            "entry_id": entryIDs[index],
            "path": destination.path,
        ])
        guard exported["ok"] as? Bool == true,
              let actualBytes = try? Data(contentsOf: destination) else {
            record(["ok": false, "kind": resultKind, "count": 0])
            return
        }

        let payloadMatches: Bool
        if let expectedBytes {
            payloadMatches = actualBytes == expectedBytes
        } else {
            payloadMatches = UIImage(data: actualBytes) != nil
        }
        record([
            "ok": payloadMatches,
            "kind": resultKind,
            "count": actualBytes.count,
        ])
    }

    private func record(_ result: [String: Any]) {
        let kind = result["kind"] as? String ?? "unknown"
        let ok = result["ok"] as? Bool == true
        status = ok ? "Passed: \(kind)" : "Failed: \(kind)"
        transcript.insert(evidenceLine(result), at: 0)
        if transcript.count > 20 { transcript.removeLast() }
        print("UC_PROBE_EVIDENCE \(evidenceLine(result))")
    }

    private func evidenceLine(_ result: [String: Any]) -> String {
        let allowed = [
            "ok", "kind", "code", "category", "retryable", "count", "online_count",
            "incoming_entries", "transfer_updates", "refresh_requests",
            "completed_operations", "fatal_errors", "last_state", "has_next",
        ]
        let evidence = result.filter { allowed.contains($0.key) }
        guard let data = try? JSONSerialization.data(
            withJSONObject: evidence,
            options: [.sortedKeys]
        ) else { return "{\"ok\":false,\"kind\":\"evidence_failed\"}" }
        return String(data: data, encoding: .utf8) ?? "{}"
    }

    private static let samplePNG =
        "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII="
}
