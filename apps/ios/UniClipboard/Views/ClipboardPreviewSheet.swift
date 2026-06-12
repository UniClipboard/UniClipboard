import SwiftUI
import UIKit

/// Detail preview for one history row. Surfaced by tapping a row on the
/// Home list — gives the user a way to inspect the full contents (long
/// text body, full-resolution image, file metadata) without committing
/// to a "复制到本机" round-trip that would route through UIPasteboard
/// and burn an "Allow Paste" prompt.
///
/// Loading discipline:
/// - Inline text (no `hasData`) is shown immediately from `entry.text`.
/// - Long-text overflow (§3.4: `text` is `Kind.text` AND `hasData=true`)
///   and image payloads are fetched lazily on appear via §2.11.
/// - File / Group entries don't pull bytes — there's no meaningful
///   preview surface for an arbitrary binary, and §4.3 group rendering
///   is out of scope. The metadata card and quick-save action are the
///   whole UX.
struct ClipboardPreviewSheet: View {
    let item: ClipboardHistoryItem
    @Bindable var vm: AppViewModel

    @Environment(\.dismiss) private var dismiss

    /// Lazy-loaded payload bytes for image / long-text rows. `.idle` for
    /// rows that never need bytes (inline text, file, group); `.loading`
    /// while the §2.11 round-trip is in flight; `.loaded` / `.failed`
    /// after.
    @State private var payload: PayloadState = .idle
    @State private var urlMeta: URLCardMetadata?

    /// Cached decoded long-text body. Decoding `Data` → `String` once on
    /// load() avoids re-running UTF-8 decode on every body re-render —
    /// SwiftUI invalidates this view often (vm @Observable churn), and
    /// re-decoding multi-MB payloads on the main thread is what makes
    /// long-text preview feel frozen.
    @State private var decodedText: String?

    /// Hard cap on how many characters we actually hand to UITextView.
    /// Above this we render a head slice plus a notice — TextKit's
    /// layout cost grows with string length, and beyond ~200k chars the
    /// initial layout pass starts to drop frames on the main thread.
    private static let maxDisplayChars = 200_000

    private enum PayloadState {
        case idle
        case loading
        case loaded(Data)
        case failed(SyncError)
    }

    private var needsRemotePayload: Bool {
        guard item.entry.hasData else { return false }
        switch item.entry.type {
        case .text, .image:  return true
        case .file, .group:  return false
        }
    }

    /// Whether `vm.applyAttachment` / `vm.reapplyText` can target this
    /// row. Text is always copyable; image needs the bytes to be on the
    /// server addressable via §2.11 (hash present). File / Group can't
    /// land on UIPasteboard meaningfully so the action is hidden.
    private var canCopyToDevice: Bool {
        switch item.entry.type {
        case .text:           return true
        case .image:          return item.entry.hasData && (item.entry.hash?.isEmpty == false)
        case .file, .group:   return false
        }
    }

    /// Whether `vm.saveAttachment` can target this row — image / file
    /// with a hash. Live-latest (no hash) is the live card's territory,
    /// not the per-row history preview.
    private var canSaveToDocuments: Bool {
        guard item.entry.hasData,
              item.entry.type == .image || item.entry.type == .file,
              let hash = item.entry.hash, !hash.isEmpty
        else { return false }
        return true
    }

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    header
                    Divider()
                    content
                    metadataCard
                }
                .padding(.horizontal, 20)
                .padding(.vertical, 16)
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            .background(Color(.systemGroupedBackground))
            .safeAreaInset(edge: .bottom, spacing: 0) {
                if canCopyToDevice || canSaveToDocuments {
                    actionBar
                }
            }
            .navigationTitle("预览")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("关闭") { dismiss() }
                }
            }
        }
        .task {
            guard needsRemotePayload, case .idle = payload else { return }
            await load()
        }
        .task {
            guard item.entry.displayKind == .url, let url = item.entry.parsedURL else { return }
            urlMeta = await URLMetadataCache.shared.fetch(for: url)
        }
    }

    @ViewBuilder
    private var header: some View {
        HStack(alignment: .center, spacing: 12) {
            ClipboardKindBadge(kind: item.entry.displayKind, size: .medium, showsLabel: false)
            VStack(alignment: .leading, spacing: 2) {
                Text(headerTitle)
                    .font(.headline)
                    .foregroundStyle(.primary)
                    .lineLimit(1)
                    .truncationMode(.middle)
                HStack(spacing: 6) {
                    Image(systemName: item.direction == .pulled ? "arrow.down" : "arrow.up")
                        .font(.caption2.weight(.semibold))
                        .foregroundStyle(item.direction == .pulled ? Color.blue : Color.accentColor)
                    Text(item.direction == .pulled ? "来自服务器" : "由本机推送")
                    Text("·").foregroundStyle(.tertiary)
                    Text(item.timestamp.formatted(date: .abbreviated, time: .shortened))
                }
                .font(.caption)
                .foregroundStyle(.secondary)
            }
            Spacer(minLength: 0)
        }
    }

    private var headerTitle: String {
        switch item.entry.displayKind {
        case .text:
            return String(localized: "文本")
        case .url:
            return urlMeta?.title ?? item.entry.parsedURL?.host ?? item.entry.text
        case .image, .file, .group:
            if let dataName = item.entry.dataName,
               !dataName.isEmpty {
                return dataName
            }
            return item.entry.text
        }
    }

    @ViewBuilder
    private var content: some View {
        switch item.entry.displayKind {
        case .text:
            textContent
        case .url:
            urlContent
        case .image:
            imageContent
        case .file, .group:
            fileContent
        }
    }

    // MARK: text

    @ViewBuilder
    private var textContent: some View {
        if !item.entry.hasData {
            // Short text — `entry.text` is the full content per §3.1.
            textBubble(text: item.entry.text, dimmed: false)
        } else {
            // §3.4 long-text overflow — `entry.text` is just the first
            // 10240 chars; full body lives at `<dataName>`. Show whatever
            // we have, then swap to the full text once §2.11 returns.
            longTextContent
        }
    }

    /// Capped, selectable text block backed by UITextView. We bypass
    /// SwiftUI `Text` for long bodies because its layout + selection
    /// pipeline becomes noticeably janky beyond a few tens of thousands
    /// of characters; UITextView with TextKit handles it cleanly.
    @ViewBuilder
    private func textBubble(text: String, dimmed: Bool) -> some View {
        let (display, overflowChars) = capped(text)
        VStack(alignment: .leading, spacing: 8) {
            SelectableTextView(text: display, dimmed: dimmed)
                .frame(maxWidth: .infinity, alignment: .leading)
            if overflowChars > 0 {
                Text("内容过长,仅显示前 \(Self.maxDisplayChars) 字符(余 \(overflowChars) 字符未显示)。可保存到 Documents 后查看完整内容。")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .padding(14)
        .background(
            Color(.secondarySystemGroupedBackground),
            in: RoundedRectangle(cornerRadius: 12, style: .continuous)
        )
    }

    private func capped(_ text: String) -> (String, Int) {
        if text.count <= Self.maxDisplayChars { return (text, 0) }
        let head = text.prefix(Self.maxDisplayChars)
        return (String(head), text.count - Self.maxDisplayChars)
    }

    @ViewBuilder
    private var longTextContent: some View {
        switch payload {
        case .idle, .loading:
            VStack(alignment: .leading, spacing: 12) {
                SelectableTextView(text: cappedHead(item.entry.text), dimmed: true)
                    .frame(maxWidth: .infinity, alignment: .leading)
                HStack(spacing: 8) {
                    ProgressView().controlSize(.small)
                    Text("正在加载完整内容…")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
            .padding(14)
            .background(
                Color(.secondarySystemGroupedBackground),
                in: RoundedRectangle(cornerRadius: 12, style: .continuous)
            )

        case .loaded:
            // Use the cached decoded string; `String(decoding:as:)` on
            // multi-MB Data is too expensive to redo each body pass.
            textBubble(text: decodedText ?? item.entry.text, dimmed: false)

        case .failed(let err):
            VStack(alignment: .leading, spacing: 10) {
                SelectableTextView(text: cappedHead(item.entry.text), dimmed: false)
                    .frame(maxWidth: .infinity, alignment: .leading)
                errorBanner(err)
            }
            .padding(14)
            .background(
                Color(.secondarySystemGroupedBackground),
                in: RoundedRectangle(cornerRadius: 12, style: .continuous)
            )
        }
    }

    private func cappedHead(_ text: String) -> String {
        text.count <= Self.maxDisplayChars ? text : String(text.prefix(Self.maxDisplayChars))
    }

    // MARK: image

    @ViewBuilder
    private var imageContent: some View {
        switch payload {
        case .idle, .loading:
            VStack(spacing: 12) {
                ProgressView()
                Text("正在加载图片…")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            .frame(maxWidth: .infinity, minHeight: 220)
            .background(
                Color(.secondarySystemGroupedBackground),
                in: RoundedRectangle(cornerRadius: 12, style: .continuous)
            )

        case .loaded(let bytes):
            if let image = UIImage(data: bytes) {
                Image(uiImage: image)
                    .resizable()
                    .aspectRatio(contentMode: .fit)
                    .frame(maxWidth: .infinity)
                    .background(Color(.systemBackground))
                    .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
                    .overlay(
                        RoundedRectangle(cornerRadius: 12, style: .continuous)
                            .strokeBorder(Color(.separator).opacity(0.4), lineWidth: 0.5)
                    )
            } else {
                errorBanner(SyncError(kind: .decodingFailed, underlying: "UIImage failed to decode \(bytes.count) bytes"))
            }

        case .failed(let err):
            errorBanner(err)
                .frame(maxWidth: .infinity, minHeight: 160, alignment: .center)
                .background(
                    Color(.secondarySystemGroupedBackground),
                    in: RoundedRectangle(cornerRadius: 12, style: .continuous)
                )
        }
    }

    // MARK: url

    @ViewBuilder
    private var urlContent: some View {
        VStack(alignment: .leading, spacing: 12) {
            if let ogImage = urlMeta?.ogImage {
                Image(uiImage: ogImage)
                    .resizable()
                    .aspectRatio(contentMode: .fit)
                    .frame(maxWidth: .infinity)
                    .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
                    .overlay(
                        RoundedRectangle(cornerRadius: 12, style: .continuous)
                            .strokeBorder(Color(.separator).opacity(0.4), lineWidth: 0.5)
                    )
            } else if urlMeta == nil {
                VStack(spacing: 12) {
                    ProgressView()
                    Text("正在获取链接预览…")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                .frame(maxWidth: .infinity, minHeight: 120)
                .background(
                    Color(.secondarySystemGroupedBackground),
                    in: RoundedRectangle(cornerRadius: 12, style: .continuous)
                )
            }

            if let title = urlMeta?.title, !title.isEmpty {
                Text(title)
                    .font(.headline)
            }

            VStack(alignment: .leading) {
                Text(item.entry.text)
                    .font(.callout)
                    .foregroundStyle(.blue)
                    .textSelection(.enabled)
            }
            .padding(14)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(
                Color(.secondarySystemGroupedBackground),
                in: RoundedRectangle(cornerRadius: 12, style: .continuous)
            )

            if let url = item.entry.parsedURL {
                Button {
                    UIApplication.shared.open(url)
                } label: {
                    HStack(spacing: 6) {
                        Image(systemName: "safari")
                        Text("在浏览器中打开")
                    }
                    .font(.subheadline.weight(.medium))
                    .foregroundStyle(.blue)
                }
            }
        }
    }

    // MARK: file / group

    @ViewBuilder
    private var fileContent: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 12) {
                Image(systemName: item.entry.displayKind.symbolName)
                    .font(.system(size: 28))
                    .foregroundStyle(.white)
                    .frame(width: 52, height: 52)
                    .background(item.entry.displayKind.tint.gradient,
                                in: RoundedRectangle(cornerRadius: 12, style: .continuous))
                VStack(alignment: .leading, spacing: 4) {
                    Text(item.entry.dataName ?? item.entry.text)
                        .font(.callout.weight(.semibold))
                        .lineLimit(2)
                        .truncationMode(.middle)
                    if let size = item.entry.size {
                        Text(formatFileSize(size))
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }
                Spacer(minLength: 0)
            }
            Text("文件内容无法在此预览。可保存到 Documents 后用文件 App 查看。")
                .font(.footnote)
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(14)
        .background(
            Color(.secondarySystemGroupedBackground),
            in: RoundedRectangle(cornerRadius: 12, style: .continuous)
        )
    }

    // MARK: metadata card

    @ViewBuilder
    private var metadataCard: some View {
        VStack(alignment: .leading, spacing: 0) {
            metadataRow(label: "类型", value: typeLabel)
            if let dataName = item.entry.dataName, !dataName.isEmpty {
                Divider()
                metadataRow(label: "名称", value: dataName, mono: true)
            }
            if let size = item.entry.size {
                Divider()
                metadataRow(label: "大小", value: sizeLabel(size))
            }
            if let hash = item.entry.hash, !hash.isEmpty {
                Divider()
                metadataRow(label: "校验值", value: shortHash(hash), mono: true)
            }
        }
        .background(
            Color(.secondarySystemGroupedBackground),
            in: RoundedRectangle(cornerRadius: 12, style: .continuous)
        )
    }

    @ViewBuilder
    private func metadataRow(label: LocalizedStringKey, value: String, mono: Bool = false) -> some View {
        HStack(alignment: .firstTextBaseline) {
            Text(label)
                .font(.footnote)
                .foregroundStyle(.secondary)
            Spacer(minLength: 12)
            Text(value)
                .font(mono ? .footnote.monospaced() : .footnote)
                .foregroundStyle(.primary)
                .lineLimit(1)
                .truncationMode(.middle)
                .textSelection(.enabled)
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 10)
    }

    // MARK: action bar

    @ViewBuilder
    private var actionBar: some View {
        HStack(spacing: 10) {
            if canCopyToDevice {
                Button {
                    Task { await copy() }
                } label: {
                    Label("复制到本机", systemImage: "doc.on.clipboard")
                        .font(.subheadline.weight(.semibold))
                        .foregroundStyle(Color(.systemBackground))
                        .frame(maxWidth: .infinity)
                        .frame(height: 44)
                }
                .buttonStyle(.borderedProminent)
                .disabled(vm.isApplying)
            }
            if canSaveToDocuments {
                Button {
                    Task { await save() }
                } label: {
                    Label("保存", systemImage: "square.and.arrow.down")
                        .font(.subheadline.weight(.semibold))
                        .frame(maxWidth: .infinity)
                        .frame(height: 44)
                }
                .buttonStyle(.bordered)
                .disabled(vm.isSaving)
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
        .background(.regularMaterial)
        .overlay(alignment: .top) { Divider() }
    }

    // MARK: helpers

    private func load() async {
        payload = .loading
        decodedText = nil
        do {
            let bytes = try await vm.fetchPreviewBytes(for: item)
            // Decode text payloads once, off the render path. Image
            // payloads stay as Data — UIImage(data:) does its own thing.
            if item.entry.type == .text {
                let decoded = await Task.detached(priority: .userInitiated) {
                    String(decoding: bytes, as: UTF8.self)
                }.value
                decodedText = decoded
            }
            payload = .loaded(bytes)
        } catch let e as SyncError {
            payload = .failed(e)
        } catch {
            payload = .failed(SyncError(kind: .networkUnreachable, underlying: "\(error)"))
        }
    }

    private func copy() async {
        if item.entry.type == .text {
            vm.reapplyHistoryItem(item)
            Task { await vm.pushHistoryEntryToServer(item) }
            dismiss()
            return
        }
        await vm.applyAttachment(for: item)
        if vm.applyError == nil {
            vm.reapplyHistoryItem(item)
            Task { await vm.pushHistoryEntryToServer(item) }
            dismiss()
        }
    }

    private func save() async {
        await vm.saveAttachment(for: item)
        if vm.saveError == nil { dismiss() }
    }

    @ViewBuilder
    private func errorBanner(_ err: SyncError) -> some View {
        HStack(alignment: .top, spacing: 8) {
            Image(systemName: "exclamationmark.triangle.fill")
                .font(.footnote)
                .foregroundStyle(.orange)
            VStack(alignment: .leading, spacing: 2) {
                Text("加载失败")
                    .font(.footnote.weight(.semibold))
                Text(errorMessage(err))
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Spacer(minLength: 0)
            Button("重试") { Task { await load() } }
                .font(.caption.weight(.semibold))
        }
    }

    private var typeLabel: String {
        item.entry.displayKind.localizedLabelString
    }

    private func sizeLabel(_ size: Int) -> String {
        switch item.entry.displayKind {
        case .text, .url:
            return String(localized: "\(size) 字")
        case .image, .file, .group:
            return formatFileSize(size)
        }
    }

    private func formatFileSize(_ size: Int) -> String {
        let f = ByteCountFormatter()
        f.countStyle = .file
        return f.string(fromByteCount: Int64(size))
    }

    private func shortHash(_ hash: String) -> String {
        guard hash.count > 16 else { return hash }
        let prefix = hash.prefix(8)
        let suffix = hash.suffix(8)
        return "\(prefix)…\(suffix)"
    }

    private func errorMessage(_ err: SyncError) -> String {
        switch err.kind {
        case .authFailed:                return String(localized: "认证失败 — 请检查用户名和密码")
        case .connectTimeout:            return String(localized: "连接超时 — 请检查服务器地址")
        case .receiveTimeout:            return String(localized: "接收超时 — 请稍后重试")
        case .networkUnreachable:        return String(localized: "无法连接 — 请检查网络和 URL")
        case .invalidURL:                return String(localized: "服务器地址无效")
        case .decodingFailed:            return String(localized: "服务器返回的数据无法解析")
        case .protocolError(let code):   return String(localized: "服务器返回 HTTP \(code)")
        case .serverError(let code):     return String(localized: "服务器错误 \(code)")
        case .notFound:                  return String(localized: "服务器尚未发布剪贴板")
        case .hashMismatch:              return String(localized: "内容校验失败 — 文件可能损坏")
        case .cancelled:                 return String(localized: "请求已取消")
        }
    }
}

/// Selectable, non-editable text backed by `UITextView`. SwiftUI's
/// native `Text` with `.textSelection(.enabled)` becomes janky past a
/// few tens of thousands of characters; UITextView with TextKit handles
/// hundreds of thousands smoothly. `isScrollEnabled = false` so it
/// participates in the outer `ScrollView` instead of nesting its own
/// scroll region.
///
/// **TextKit 1, not 2** (`usingTextLayoutManager: false`). TextKit 2's
/// selection geometry mis-handles a long unbreakable token (e.g. an
/// `https://…` URL) that character-wraps mid-token: the trailing grab
/// handle won't extend the selection onto the wrapped second visual line,
/// so users can't select the URL's tail (reported 2026-05). Plain prose,
/// which wraps at spaces, is unaffected — which is exactly why the bug
/// reads as "only happens with links". TextKit 2's payoff is viewport-
/// based lazy layout, and `isScrollEnabled = false` forces full layout of
/// the whole (capped) string regardless, so we give up nothing by using
/// the mature TextKit 1 selection path here.
private struct SelectableTextView: UIViewRepresentable {
    let text: String
    let dimmed: Bool

    func makeUIView(context: Context) -> UITextView {
        let tv = UITextView(usingTextLayoutManager: false)
        tv.isEditable = false
        tv.isSelectable = true
        tv.isScrollEnabled = false
        tv.backgroundColor = .clear
        tv.textContainer.lineFragmentPadding = 0
        tv.textContainerInset = .zero
        tv.adjustsFontForContentSizeCategory = true
        tv.font = UIFont.preferredFont(forTextStyle: .body)
        tv.dataDetectorTypes = []
        // TextKit 1's UITextView reports no usable multi-line
        // intrinsicContentSize inside a SwiftUI representable, so the block
        // collapses to a single visible line (TextKit 2 happened to self-
        // expand — but its selection geometry is what we're fixing). Let
        // SwiftUI own the width and resist vertical squeeze; `sizeThatFits`
        // supplies the wrapped height below.
        tv.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        tv.setContentCompressionResistancePriority(.required, for: .vertical)
        return tv
    }

    func updateUIView(_ tv: UITextView, context: Context) {
        if tv.text != text { tv.text = text }
        tv.textColor = dimmed ? .secondaryLabel : .label
    }

    /// Drive the representable's height from the wrapped layout at SwiftUI's
    /// proposed width. Without this, TextKit 1's `intrinsicContentSize`
    /// collapses the multi-line body to one visible line.
    func sizeThatFits(_ proposal: ProposedViewSize, uiView: UITextView, context: Context) -> CGSize? {
        guard let width = proposal.width, width > 0, width.isFinite else { return nil }
        let fit = uiView.sizeThatFits(CGSize(width: width, height: .greatestFiniteMagnitude))
        return CGSize(width: width, height: ceil(fit.height))
    }
}

#Preview("文本预览") {
    let vm = AppViewModel.preview()
    return Color.clear.sheet(isPresented: .constant(true)) {
        ClipboardPreviewSheet(
            item: ClipboardHistoryItem(
                entry: Clipboard.fromText("这是一段被复制到剪贴板的文本内容,可以多行,\n并支持选中复制。"),
                timestamp: .now,
                direction: .pulled
            ),
            vm: vm
        )
    }
}

#Preview("文件预览") {
    let vm = AppViewModel.preview()
    return Color.clear.sheet(isPresented: .constant(true)) {
        ClipboardPreviewSheet(
            item: ClipboardHistoryItem(
                entry: Clipboard(
                    type: .file,
                    hash: "ABCDEFGH12345678ABCDEFGH12345678ABCDEFGH12345678ABCDEFGH12345678",
                    text: "report.pdf",
                    hasData: true,
                    dataName: "report.pdf",
                    size: 245_000
                ),
                timestamp: .now.addingTimeInterval(-3600),
                direction: .pushed
            ),
            vm: vm
        )
    }
}
