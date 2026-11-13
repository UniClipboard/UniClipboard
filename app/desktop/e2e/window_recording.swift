// Native acceptance helper: capture only the owned window using ScreenCaptureKit.
import AppKit
import ScreenCaptureKit
import AVFoundation
import CoreMedia

final class Recorder: NSObject, SCStreamOutput {
    let queue = DispatchQueue(label: "unic.e2e.video")
    let targetPID: pid_t
    let path: String
    var stream: SCStream!
    var writer: AVAssetWriter!
    var input: AVAssetWriterInput!
    var first: CMTime?
    var timestamps: [Double] = []
    var failure: String?
    var width = 0
    var height = 0
    init(pid: pid_t, path: String) { targetPID = pid; self.path = path }
    func emit(_ value: [String: Any]) {
        let data = try! JSONSerialization.data(withJSONObject: value)
        FileHandle.standardOutput.write(data)
        FileHandle.standardOutput.write(Data([10]))
    }
    @MainActor func start() async throws {
        let content = try await SCShareableContent.excludingDesktopWindows(true, onScreenWindowsOnly: false)
        guard let window = content.windows.first(where: { $0.owningApplication?.processID == targetPID && $0.frame.width > 500 && $0.frame.height > 300 }) else {
            throw NSError(domain: "UnicRecording", code: 1, userInfo: [NSLocalizedDescriptionKey: "owned desktop window not found"])
        }
        let filter = SCContentFilter(desktopIndependentWindow: window)
        let configuration = SCStreamConfiguration()
        width = Int(window.frame.width) / 2 * 2
        height = Int(window.frame.height) / 2 * 2
        configuration.width = width
        configuration.height = height
        configuration.minimumFrameInterval = CMTime(value: 1, timescale: 30)
        configuration.queueDepth = 6
        configuration.showsCursor = false
        configuration.capturesAudio = false
        configuration.pixelFormat = kCVPixelFormatType_32BGRA
        writer = try AVAssetWriter(outputURL: URL(fileURLWithPath: path), fileType: .mp4)
        input = AVAssetWriterInput(mediaType: .video, outputSettings: [
            AVVideoCodecKey: AVVideoCodecType.h264,
            AVVideoWidthKey: width, AVVideoHeightKey: height,
            AVVideoCompressionPropertiesKey: [AVVideoAverageBitRateKey: 4_000_000]
        ])
        input.expectsMediaDataInRealTime = true
        writer.add(input)
        guard writer.startWriting() else { throw writer.error! }
        stream = SCStream(filter: filter, configuration: configuration, delegate: nil)
        try stream.addStreamOutput(self, type: .screen, sampleHandlerQueue: queue)
        try await stream.startCapture()
    }
    func stream(_ stream: SCStream, didOutputSampleBuffer sample: CMSampleBuffer, of type: SCStreamOutputType) {
        guard type == .screen, sample.isValid,
              let attachments = CMSampleBufferGetSampleAttachmentsArray(sample, createIfNecessary: false) as? [[SCStreamFrameInfo: Any]],
              let status = attachments.first?[.status] as? Int,
              status == SCFrameStatus.complete.rawValue else { return }
        let time = CMSampleBufferGetPresentationTimeStamp(sample)
        let initial = first == nil
        if initial { first = time; writer.startSession(atSourceTime: time) }
        guard input.isReadyForMoreMediaData else { return }
        if input.append(sample) {
            timestamps.append(CMTimeGetSeconds(CMTimeSubtract(time, first!)))
            if initial { emit(["ready": true, "window_pid": Int(targetPID), "width": width, "height": height]) }
        } else { failure = writer.error?.localizedDescription ?? "video frame append failed" }
    }
    func stop() async {
        do { try await stream.stopCapture() } catch { failure = error.localizedDescription }
        await withCheckedContinuation { (continuation: CheckedContinuation<Void, Never>) in
            queue.async {
                self.input.markAsFinished()
                self.writer.finishWriting { continuation.resume() }
            }
        }
        if writer.status != .completed || timestamps.count < 2 || failure != nil {
            emit(["error": failure ?? writer.error?.localizedDescription ?? "recording has too few frames"])
            exit(1)
        }
        emit(["source": "ScreenCaptureKit desktopIndependentWindow", "window_pid": Int(targetPID),
              "width": width, "height": height, "frames": timestamps.map { ["at_seconds": $0] },
              "duration_seconds": timestamps.last!])
        exit(0)
    }
}
_ = NSApplication.shared
let recorder = Recorder(pid: pid_t(CommandLine.arguments[1])!, path: CommandLine.arguments[2])
Task {
    do {
        try await recorder.start()
        DispatchQueue.global().async {
            _ = readLine()
            Task { await recorder.stop() }
        }
    } catch {
        recorder.emit(["error": error.localizedDescription])
        exit(1)
    }
}
RunLoop.main.run()
