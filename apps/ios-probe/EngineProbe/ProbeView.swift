import SwiftUI

struct ProbeView: View {
    @Bindable var model: ProbeModel

    var body: some View {
        NavigationStack {
            Form {
                Section("Status") {
                    LabeledContent("Engine", value: model.status)
                    if model.isBusy {
                        ProgressView()
                    }
                }

                Section("Space") {
                    SecureField("Passphrase", text: $model.passphrase)
                    TextField("Invitation", text: $model.invitationCode, axis: .vertical)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                    commandButton("Create", systemImage: "plus.circle") {
                        await model.createSpace()
                    }
                    commandButton("Join", systemImage: "link") {
                        await model.joinSpace()
                    }
                    commandButton("Unlock", systemImage: "lock.open") {
                        _ = await model.unlock()
                    }
                    commandButton("Invitation", systemImage: "qrcode") {
                        await model.issueInvitation()
                    }
                    commandButton("Devices", systemImage: "iphone.and.arrow.forward") {
                        await model.listDevices()
                    }
                }

                Section("Transfer") {
                    TextField("Text", text: $model.textPayload, axis: .vertical)
                    commandButton("Send text", systemImage: "text.bubble") {
                        await model.sendText()
                    }
                    commandButton("Send image", systemImage: "photo") {
                        await model.sendImage()
                    }
                    commandButton("Send file", systemImage: "doc") {
                        await model.sendFile()
                    }
                }

                Section("Verify") {
                    TextField("Search", text: $model.query)
                    commandButton("Query", systemImage: "magnifyingglass") {
                        _ = await model.queryHistory()
                    }
                    commandButton("Suspend and resume", systemImage: "pause.circle") {
                        await model.suspendAndResume()
                    }
                    commandButton("Events", systemImage: "waveform.path.ecg") {
                        await model.eventSummary()
                    }
                }

                Section("Evidence") {
                    ForEach(Array(model.transcript.enumerated()), id: \.offset) { _, line in
                        Text(line)
                            .font(.caption.monospaced())
                            .textSelection(.enabled)
                    }
                }
            }
            .navigationTitle("Engine Probe")
        }
    }

    private func commandButton(
        _ title: String,
        systemImage: String,
        action: @escaping () async -> Void
    ) -> some View {
        Button {
            Task { await action() }
        } label: {
            Label(title, systemImage: systemImage)
        }
        .disabled(model.isBusy)
    }
}
