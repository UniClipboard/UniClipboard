@preconcurrency import AVFoundation
@preconcurrency import ExpoModulesCore
import UIKit
import VisionKit

@MainActor
final class PairingQRScanner {
  private let promise: Promise
  private let cancelLabel: String
  private let hint: String
  private let onFinish: () -> Void
  private var activeController: AnyObject?
  private var completed = false

  init(
    cancelLabel: String,
    hint: String,
    promise: Promise,
    onFinish: @escaping () -> Void
  ) {
    self.cancelLabel = cancelLabel
    self.hint = hint
    self.promise = promise
    self.onFinish = onFinish
  }

  func start() {
    switch AVCaptureDevice.authorizationStatus(for: .video) {
    case .authorized:
      presentScanner()
    case .notDetermined:
      AVCaptureDevice.requestAccess(for: .video) { [weak self] granted in
        Task { @MainActor [weak self] in
          guard let self else { return }
          if granted {
            self.presentScanner()
          } else {
            self.reject(code: "permission_denied", message: "camera access denied")
          }
        }
      }
    case .denied, .restricted:
      reject(code: "permission_denied", message: "camera access denied")
    @unknown default:
      reject(code: "permission_denied", message: "camera access denied")
    }
  }

  private func presentScanner() {
    guard let presenter = Self.topPresenter() else {
      reject(code: "invalid_state", message: "no available presenter")
      return
    }

    if #available(iOS 16.0, *),
       DataScannerViewController.isSupported,
       DataScannerViewController.isAvailable {
      presentVisionKitScanner(from: presenter)
      return
    }

    if AVCaptureDevice.default(for: .video) != nil {
      presentAVCaptureScanner(from: presenter)
      return
    }

    reject(code: "unsupported", message: "QR scanning is unavailable on this device")
  }

  @available(iOS 16.0, *)
  private func presentVisionKitScanner(from presenter: UIViewController) {
    let scanner = DataScannerViewController(
      recognizedDataTypes: [.barcode(symbologies: [.qr])],
      qualityLevel: .balanced,
      recognizesMultipleItems: false,
      isHighFrameRateTrackingEnabled: false,
      isPinchToZoomEnabled: true,
      isGuidanceEnabled: true,
      isHighlightingEnabled: true
    )
    let coordinator = VisionKitScanCoordinator(
      scanner: scanner,
      cancelLabel: cancelLabel,
      hint: hint,
      onResult: { [weak self] value in
        self?.resolve(value)
      },
      onError: { [weak self] code, message in
        self?.reject(code: code, message: message)
      }
    )
    scanner.delegate = coordinator
    scanner.modalPresentationStyle = .fullScreen
    activeController = coordinator

    presenter.present(scanner, animated: true) {
      do {
        try scanner.startScanning()
        coordinator.addOverlay()
      } catch {
        coordinator.fail(code: "internal", message: error.localizedDescription)
      }
    }
  }

  private func presentAVCaptureScanner(from presenter: UIViewController) {
    let controller = AVCaptureQRScannerViewController(
      cancelLabel: cancelLabel,
      hint: hint,
      onResult: { [weak self] value in
        self?.resolve(value)
      },
      onError: { [weak self] code, message in
        self?.reject(code: code, message: message)
      }
    )
    controller.modalPresentationStyle = .fullScreen
    activeController = controller
    presenter.present(controller, animated: true)
  }

  private func resolve(_ value: String?) {
    guard !completed else { return }
    completed = true
    activeController = nil
    onFinish()
    promise.resolve(value)
  }

  private func reject(code: String, message: String) {
    guard !completed else { return }
    completed = true
    activeController = nil
    onFinish()
    promise.reject(Self.formatException(code: code, message: message))
  }

  private static func formatException(code: String, message: String) -> GenericException<String> {
    let payload: [String: String] = ["code": code, "message": message]
    let json = (try? JSONSerialization.data(withJSONObject: payload))
      .flatMap { String(data: $0, encoding: .utf8) }
      ?? "{\"code\":\"\(code)\",\"message\":\"operation failed\"}"
    return PairingQRScannerException("[unic:\(code)] \(json)")
  }

  private static func topPresenter() -> UIViewController? {
    let scenes = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }
    let activeWindows = scenes
      .filter { $0.activationState == .foregroundActive }
      .flatMap(\.windows)
    let rootViewController = (activeWindows.first(where: \.isKeyWindow)
      ?? scenes.flatMap(\.windows).first(where: \.isKeyWindow))?
      .rootViewController
    guard let rootViewController else { return nil }
    return visibleChildViewController(from: rootViewController)
  }

  private static func visibleChildViewController(from rootViewController: UIViewController) -> UIViewController {
    var topViewController = rootViewController
    while let presented = topViewController.presentedViewController {
      topViewController = presented
    }
    if let navigationController = topViewController as? UINavigationController,
       let visible = navigationController.visibleViewController {
      return visibleChildViewController(from: visible)
    }
    if let tabController = topViewController as? UITabBarController,
       let selected = tabController.selectedViewController {
      return visibleChildViewController(from: selected)
    }
    for child in topViewController.children.reversed()
      where child.viewIfLoaded?.window != nil {
      let visible = visibleChildViewController(from: child)
      if visible.presentedViewController != nil || visible !== child || child.children.isEmpty {
        return visible
      }
    }
    return topViewController
  }

  fileprivate static func installOverlay(
    in container: UIView,
    buttonContainer: UIView,
    cancelLabel: String,
    hint: String,
    target: AnyObject,
    cancelAction: Selector
  ) {
    let cancelButton = UIButton(type: .system)
    var config = UIButton.Configuration.filled()
    config.title = cancelLabel
    config.baseBackgroundColor = UIColor.black.withAlphaComponent(0.48)
    config.baseForegroundColor = .white
    config.cornerStyle = .capsule
    config.contentInsets = NSDirectionalEdgeInsets(top: 8, leading: 16, bottom: 8, trailing: 16)
    config.titleTextAttributesTransformer = UIConfigurationTextAttributesTransformer { incoming in
      var outgoing = incoming
      outgoing.font = .systemFont(ofSize: 17, weight: .semibold)
      return outgoing
    }
    cancelButton.configuration = config
    cancelButton.addTarget(target, action: cancelAction, for: .touchUpInside)
    cancelButton.translatesAutoresizingMaskIntoConstraints = false
    cancelButton.accessibilityLabel = cancelLabel
    buttonContainer.addSubview(cancelButton)

    let reticle = UIView()
    reticle.isUserInteractionEnabled = false
    reticle.layer.borderColor = UIColor.white.cgColor
    reticle.layer.borderWidth = 3
    reticle.layer.cornerRadius = 18
    reticle.translatesAutoresizingMaskIntoConstraints = false
    container.addSubview(reticle)

    let hintLabel = UILabel()
    hintLabel.text = hint
    hintLabel.textColor = .white
    hintLabel.font = .systemFont(ofSize: 15, weight: .medium)
    hintLabel.textAlignment = .center
    hintLabel.numberOfLines = 2
    hintLabel.translatesAutoresizingMaskIntoConstraints = false
    container.addSubview(hintLabel)

    NSLayoutConstraint.activate([
      cancelButton.topAnchor.constraint(
        equalTo: buttonContainer.safeAreaLayoutGuide.topAnchor,
        constant: 12
      ),
      cancelButton.leadingAnchor.constraint(
        equalTo: buttonContainer.safeAreaLayoutGuide.leadingAnchor,
        constant: 20
      ),
      cancelButton.heightAnchor.constraint(greaterThanOrEqualToConstant: 36),

      reticle.centerXAnchor.constraint(equalTo: container.centerXAnchor),
      reticle.centerYAnchor.constraint(equalTo: container.centerYAnchor, constant: -30),
      reticle.widthAnchor.constraint(equalToConstant: 240),
      reticle.heightAnchor.constraint(equalToConstant: 240),

      hintLabel.centerXAnchor.constraint(equalTo: container.centerXAnchor),
      hintLabel.leadingAnchor.constraint(
        greaterThanOrEqualTo: container.leadingAnchor,
        constant: 24
      ),
      hintLabel.trailingAnchor.constraint(
        lessThanOrEqualTo: container.trailingAnchor,
        constant: -24
      ),
      hintLabel.topAnchor.constraint(equalTo: reticle.bottomAnchor, constant: 24),
    ])
  }
}

private final class PairingQRScannerException: GenericException<String> {
  override var reason: String { param }
}

@available(iOS 16.0, *)
@MainActor
private final class VisionKitScanCoordinator: NSObject, DataScannerViewControllerDelegate {
  private let scanner: DataScannerViewController
  private let cancelLabel: String
  private let hint: String
  private let onResult: (String?) -> Void
  private let onError: (String, String) -> Void
  private var completed = false

  init(
    scanner: DataScannerViewController,
    cancelLabel: String,
    hint: String,
    onResult: @escaping (String?) -> Void,
    onError: @escaping (String, String) -> Void
  ) {
    self.scanner = scanner
    self.cancelLabel = cancelLabel
    self.hint = hint
    self.onResult = onResult
    self.onError = onError
  }

  func addOverlay() {
    PairingQRScanner.installOverlay(
      in: scanner.overlayContainerView,
      buttonContainer: scanner.view,
      cancelLabel: cancelLabel,
      hint: hint,
      target: self,
      cancelAction: #selector(cancel)
    )
  }

  @objc private func cancel() {
    finish(with: nil)
  }

  func fail(code: String, message: String) {
    guard !completed else { return }
    completed = true
    scanner.stopScanning()
    if scanner.presentingViewController != nil {
      scanner.dismiss(animated: true) {
        self.onError(code, message)
      }
    } else {
      onError(code, message)
    }
  }

  private func finish(with value: String?) {
    guard !completed else { return }
    completed = true
    scanner.stopScanning()
    if scanner.presentingViewController != nil {
      scanner.dismiss(animated: true) {
        self.onResult(value)
      }
    } else {
      onResult(value)
    }
  }

  private func resolve(_ items: [RecognizedItem]) {
    for item in items {
      if case .barcode(let barcode) = item, let value = barcode.payloadStringValue {
        finish(with: value)
        return
      }
    }
  }

  func dataScanner(
    _ dataScanner: DataScannerViewController,
    didAdd addedItems: [RecognizedItem],
    allItems: [RecognizedItem]
  ) {
    resolve(addedItems)
  }

  func dataScanner(_ dataScanner: DataScannerViewController, didTapOn item: RecognizedItem) {
    resolve([item])
  }

  func dataScanner(
    _ dataScanner: DataScannerViewController,
    becameUnavailableWithError error: DataScannerViewController.ScanningUnavailable
  ) {
    fail(code: "unsupported", message: error.localizedDescription)
  }

  nonisolated func dataScannerDidCancel(_ dataScanner: DataScannerViewController) {
    Task { @MainActor in
      finish(with: nil)
    }
  }
}

@MainActor
private final class AVCaptureQRScannerViewController: UIViewController, AVCaptureMetadataOutputObjectsDelegate {
  private let cancelLabel: String
  private let hint: String
  private let onResult: (String?) -> Void
  private let onError: (String, String) -> Void
  private let session = AVCaptureSession()
  private let sessionQueue = DispatchQueue(label: "app.uniclipboard.qrscanner")
  private var previewLayer: AVCaptureVideoPreviewLayer?
  private var completed = false

  init(
    cancelLabel: String,
    hint: String,
    onResult: @escaping (String?) -> Void,
    onError: @escaping (String, String) -> Void
  ) {
    self.cancelLabel = cancelLabel
    self.hint = hint
    self.onResult = onResult
    self.onError = onError
    super.init(nibName: nil, bundle: nil)
  }

  @available(*, unavailable)
  required init?(coder: NSCoder) {
    fatalError("init(coder:) has not been implemented")
  }

  override func viewDidLoad() {
    super.viewDidLoad()
    view.backgroundColor = .black

    guard let videoDevice = AVCaptureDevice.default(for: .video),
          let videoInput = try? AVCaptureDeviceInput(device: videoDevice),
          session.canAddInput(videoInput) else {
      fail(code: "unsupported", message: "QR scanning is unavailable on this device")
      return
    }
    session.addInput(videoInput)

    let metadataOutput = AVCaptureMetadataOutput()
    guard session.canAddOutput(metadataOutput) else {
      fail(code: "unsupported", message: "QR scanning is unavailable on this device")
      return
    }
    session.addOutput(metadataOutput)
    metadataOutput.setMetadataObjectsDelegate(self, queue: .main)
    if metadataOutput.availableMetadataObjectTypes.contains(.qr) {
      metadataOutput.metadataObjectTypes = [.qr]
    } else {
      fail(code: "unsupported", message: "QR scanning is unavailable on this device")
      return
    }

    let preview = AVCaptureVideoPreviewLayer(session: session)
    preview.videoGravity = .resizeAspectFill
    preview.frame = view.bounds
    view.layer.addSublayer(preview)
    previewLayer = preview

    PairingQRScanner.installOverlay(
      in: view,
      buttonContainer: view,
      cancelLabel: cancelLabel,
      hint: hint,
      target: self,
      cancelAction: #selector(cancel)
    )

    let captureSession = session
    sessionQueue.async {
      captureSession.startRunning()
    }
  }

  override func viewDidLayoutSubviews() {
    super.viewDidLayoutSubviews()
    previewLayer?.frame = view.bounds
  }

  override func viewDidDisappear(_ animated: Bool) {
    super.viewDidDisappear(animated)
    stopSession()
    if isBeingDismissed && !completed {
      completed = true
      onResult(nil)
    }
  }

  @objc private func cancel() {
    finish(with: nil)
  }

  private func stopSession() {
    let captureSession = session
    sessionQueue.async {
      if captureSession.isRunning {
        captureSession.stopRunning()
      }
    }
  }

  private func finish(with value: String?) {
    guard !completed else { return }
    completed = true
    stopSession()
    if presentingViewController != nil {
      dismiss(animated: true) {
        self.onResult(value)
      }
    } else {
      onResult(value)
    }
  }

  private func fail(code: String, message: String) {
    guard !completed else { return }
    completed = true
    stopSession()
    if presentingViewController != nil {
      dismiss(animated: true) {
        self.onError(code, message)
      }
    } else {
      onError(code, message)
    }
  }

  nonisolated func metadataOutput(
    _ output: AVCaptureMetadataOutput,
    didOutput metadataObjects: [AVMetadataObject],
    from connection: AVCaptureConnection
  ) {
    guard let readable = metadataObjects.first as? AVMetadataMachineReadableCodeObject,
          readable.type == .qr,
          let value = readable.stringValue else {
      return
    }
    Task { @MainActor in
      self.finish(with: value)
    }
  }
}
