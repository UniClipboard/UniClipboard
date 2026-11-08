import SwiftUI
import SettingsSchema

/// A form renderer shared by all plugin definitions in this shell. Only the
/// preview entry point below selects a fixture; rendering has no plugin IDs.
struct SchemaSettingsForm: View {
    let schema: SettingsNode
    @Binding var value: JSONValue

    var body: some View {
        let groups = orderedGroups
        ForEach(groups, id: \.self) { group in
            Section {
                ForEach(schema.orderedKeys.filter { schema.properties?[$0]?.ui?.group ?? "" == group }, id: \.self) { key in
                    if let child = schema.properties?[key] {
                        SchemaSettingField(
                            schema: child,
                            name: child.title(for: key),
                            required: schema.required?.contains(key) == true,
                            value: binding(key)
                        )
                    }
                }
            } header: {
                if !group.isEmpty { Text(group) }
            }
        }
    }

    private var orderedGroups: [String] {
        var result: [String] = []
        for key in schema.orderedKeys {
            let group = schema.properties?[key]?.ui?.group ?? ""
            if !result.contains(group) { result.append(group) }
        }
        return result
    }

    private func binding(_ key: String) -> Binding<JSONValue?> {
        Binding(
            get: { value[key] },
            set: { new in
                guard case .object(var values) = value else { return }
                values[key] = new
                value = .object(values)
            }
        )
    }
}

private struct SchemaSettingField: View {
    let schema: SettingsNode
    let name: String
    let required: Bool
    @Binding var value: JSONValue?
    @State private var numericText: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 5) {
            if value == nil {
                HStack {
                    Text(name)
                    Spacer()
                    Button("Set…") { value = schema.newValue }
                        .accessibilityLabel("Set \(name)")
                }
            } else {
                control
                if !required {
                    Button("Remove value") { value = nil }
                        .font(.caption)
                        .accessibilityLabel("Remove \(name)")
                }
            }
            if let description = schema.description {
                Text(description).font(.caption).foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            if let issue {
                Text(message(issue)).font(.caption).foregroundStyle(.red)
                    .accessibilityLabel("\(name): \(message(issue))")
            }
        }
        .padding(.vertical, 2)
        .onChange(of: value) { _, next in
            // An external reset must replace the edit buffer; an ordinary
            // keystroke may keep it (e.g. the incomplete decimal "1.").
            if let numericText, next != parsedNumber(numericText) && next != .string(numericText) {
                self.numericText = nil
            }
        }
    }

    @ViewBuilder private var control: some View {
        switch schema.widget {
        case "input":
            TextField(name, text: textBinding).textFieldStyle(.roundedBorder)
        case "password":
            SecureField(name, text: textBinding).textFieldStyle(.roundedBorder)
        case "textarea":
            Text(name)
            TextEditor(text: textBinding)
                .font(.body)
                .frame(minHeight: 65, maxHeight: 100)
                .overlay(RoundedRectangle(cornerRadius: 4).stroke(.separator))
                .accessibilityLabel(name)
        case "inputnumber":
            HStack {
                TextField(name, text: numberBinding)
                    .textFieldStyle(.roundedBorder)
                    .accessibilityHint(schema.type == "integer" ? "Whole number" : "Number")
                Stepper(name, onIncrement: { step(1) }, onDecrement: { step(-1) })
                    .labelsHidden().fixedSize()
                    .disabled(number == nil)
                    .accessibilityLabel("Adjust \(name)")
            }
        case "switch":
            Toggle(name, isOn: Binding(
                get: { value == .bool(true) },
                set: { value = .bool($0) }
            ))
        case "select":
            Picker(name, selection: Binding(
                get: { schema.choices?.firstIndex(of: value ?? .null) ?? -1 },
                set: { index in
                    if let choices = schema.choices, choices.indices.contains(index) { value = choices[index] }
                }
            )) {
                if schema.choices?.contains(value ?? .null) != true { Text("Choose…").tag(-1) }
                ForEach(Array((schema.choices ?? []).enumerated()), id: \.offset) { index, choice in
                    Text(choiceLabel(index, choice)).tag(index)
                }
            }
        case "group":
            DisclosureGroup(name) {
                // AnyView breaks the recursive generic View type while the
                // bounded schema keeps runtime recursion finite.
                AnyView(SchemaSettingsForm(schema: schema, value: nonoptional))
            }
        case "list":
            VStack(alignment: .leading, spacing: 8) {
                Text(name)
                ForEach(Array(array.indices), id: \.self) { index in
                    HStack(alignment: .top) {
                        if let item = schema.items {
                            SchemaSettingField(
                                schema: item, name: "\(name) \(index + 1)", required: true,
                                value: itemBinding(index)
                            )
                        }
                        Button(role: .destructive) {
                            var values = array
                            guard values.indices.contains(index) else { return }
                            values.remove(at: index)
                            value = .array(values)
                        } label: { Image(systemName: "minus.circle") }
                            .buttonStyle(.borderless)
                            .accessibilityLabel("Remove \(name) \(index + 1)")
                    }
                }
                Button("Add item", systemImage: "plus") {
                    guard let item = schema.items else { return }
                    value = .array(array + [item.newValue])
                }
                .disabled(array.count >= (schema.maxItems ?? Int.max))
                .accessibilityLabel("Add \(name)")
            }
        default:
            Label("This setting is unavailable.", systemImage: "exclamationmark.triangle")
        }
    }

    private var textBinding: Binding<String> {
        Binding(get: { value?.text ?? "" }, set: { value = .string($0) })
    }

    private var numberBinding: Binding<String> {
        Binding(
            get: {
                if let numericText { return numericText }
                if let number { return number.formatted(.number.grouping(.never).precision(.significantDigits(1...17))) }
                return value?.text ?? ""
            },
            set: { text in
                // Keep invalid/incomplete edits as text so Save is disabled;
                // never submit the previous valid number by accident.
                numericText = text
                value = parsedNumber(text) ?? .string(text)
            }
        )
    }

    private func parsedNumber(_ text: String) -> JSONValue? {
        let style = FloatingPointFormatStyle<Double>.number.grouping(.never).precision(.significantDigits(1...17))
        guard let n = try? Double(text, format: style, lenient: false), n.isFinite else { return nil }
        return .number(n)
    }

    private func choiceLabel(_ index: Int, _ value: JSONValue) -> String {
        if let labels = schema.ui?.optionLabels, labels.indices.contains(index) { return labels[index] }
        return value.text
    }

    private var number: Double? {
        if case .number(let n) = value { return n }
        return nil
    }

    private func step(_ delta: Double) {
        guard let n = number else { return }
        numericText = nil
        let next = n + delta
        value = .number(min(max(next, schema.minimum ?? -9_007_199_254_740_991), schema.maximum ?? 9_007_199_254_740_991))
    }

    private var array: [JSONValue] {
        if case .array(let values) = value { return values }
        return []
    }

    private var nonoptional: Binding<JSONValue> {
        Binding(get: { value ?? schema.newValue }, set: { value = $0 })
    }

    private func itemBinding(_ index: Int) -> Binding<JSONValue?> {
        Binding(
            get: { array.indices.contains(index) ? array[index] : nil },
            set: { new in
                var values = array
                if let new, values.indices.contains(index) { values[index] = new; value = .array(values) }
            }
        )
    }

    private var issue: SettingsIssue? {
        guard let value else { return required ? SettingsIssue("required") : nil }
        do { try schema.validate(value); return nil }
        catch { return error as? SettingsIssue }
    }

    private func message(_ issue: SettingsIssue) -> String {
        switch issue.code {
        case "required": return "A value is required."
        case "type": return schema.type == "integer" ? "Enter a whole number." : "Enter a valid \(schema.type) value."
        case "range":
            if let min = schema.minimum, let max = schema.maximum {
                return "Enter a number from \(JSONValue.number(min).text) to \(JSONValue.number(max).text)."
            }
            return "Enter a number within the allowed range."
        case "length": return "Check the text length."
        case "items": return "Check the number of items."
        case "enum": return "Choose one of the available options."
        default: return "Check this setting."
        }
    }
}

/// Session-only preview; this callback can later be replaced by the daemon's
/// instance-settings API without changing the renderer.
struct PluginSettingsPreview: View {
    @Environment(\.dismiss) private var dismiss
    @Binding var saved: JSONValue?
    private let document: SettingsDocument?
    @State private var draft: JSONValue

    init(saved: Binding<JSONValue?>) {
        _saved = saved
        let loaded = try? SettingsDocument.demo()
        document = loaded
        _draft = State(initialValue: saved.wrappedValue ?? (try? loaded?.normalize(.object([:]))) ?? .object([:]))
    }

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                VStack(alignment: .leading, spacing: 4) {
                    Text("Plugin settings preview").font(.title3.weight(.semibold))
                    Text("Try these settings here. Changes last until this preview session ends.")
                        .font(.callout).foregroundStyle(.secondary)
                }
                Spacer()
            }
            .padding(20)
            Divider()
            if let document {
                Form { SchemaSettingsForm(schema: document.root, value: $draft) }
                    .formStyle(.grouped)
            } else {
                ContentUnavailableView("Settings unavailable", systemImage: "exclamationmark.triangle",
                                       description: Text("This settings definition could not be loaded."))
            }
            Divider()
            HStack {
                Button("Restore defaults") {
                    if let defaults = try? document?.normalize(.object([:])) { draft = defaults }
                }
                .disabled(document == nil)
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }.keyboardShortcut(.cancelAction)
                Button("Save preview") {
                    guard let result = try? document?.normalize(draft) else { return }
                    saved = result
                    dismiss()
                }
                .buttonStyle(.borderedProminent)
                .keyboardShortcut(.defaultAction)
                .disabled(!valid)
            }
            .padding(16)
        }
        .frame(width: 560, height: 620)
    }

    private var valid: Bool { (try? document?.normalize(draft)) != nil }
}
