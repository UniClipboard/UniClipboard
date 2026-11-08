import Foundation

/// JSON values retain their types; a selection never turns a number into text.
public indirect enum JSONValue: Codable, Equatable {
    case object([String: JSONValue]), array([JSONValue]), string(String), number(Double), bool(Bool), null

    public init(from decoder: Decoder) throws {
        let c = try decoder.singleValueContainer()
        if c.decodeNil() { self = .null }
        else if let v = try? c.decode(Bool.self) { self = .bool(v) }
        else if let v = try? c.decode(String.self) { self = .string(v) }
        else if let v = try? c.decode(Double.self) { self = .number(v) }
        else if let v = try? c.decode([String: JSONValue].self) { self = .object(v) }
        else { self = .array(try c.decode([JSONValue].self)) }
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.singleValueContainer()
        switch self {
        case .object(let v): try c.encode(v)
        case .array(let v): try c.encode(v)
        case .string(let v): try c.encode(v)
        case .number(let v): try c.encode(v)
        case .bool(let v): try c.encode(v)
        case .null: try c.encodeNil()
        }
    }

    public var text: String {
        switch self {
        case .string(let v): return v
        case .number(let v): return v.rounded() == v ? String(format: "%.0f", v) : String(v)
        case .bool(let v): return v ? "true" : "false"
        default: return ""
        }
    }

    public subscript(key: String) -> JSONValue? {
        if case .object(let values) = self { return values[key] }
        return nil
    }
}

public struct SettingsIssue: Error, Equatable, LocalizedError {
    public let code: String
    public let path: String
    public init(_ code: String, _ path: String = "") { self.code = code; self.path = path }
    public var errorDescription: String? { "settings: \(code) at \(path)" }
}

public struct UIHints: Decodable {
    public let widget: String?
    public let order: Int?
    public let group: String?
    public let optionLabels: [String]?
    enum CodingKeys: String, CodingKey { case widget, order, group; case optionLabels = "option_labels" }
}

/// The supported profile is intentionally small. Unsupported schema features
/// prevent editing rather than silently producing a misleading form.
public final class SettingsNode: Decodable {
    public let type: String
    public let title: String?
    public let description: String?
    public let properties: [String: SettingsNode]?
    public let required: [String]?
    public let additionalProperties: Bool?
    public let items: SettingsNode?
    public let choices: [JSONValue]?
    public let defaultValue: JSONValue?
    public let minimum: Double?
    public let maximum: Double?
    public let minLength: Int?
    public let maxLength: Int?
    public let minItems: Int?
    public let maxItems: Int?
    public let writeOnly: Bool?
    public let ui: UIHints?

    enum CodingKeys: String, CodingKey {
        case type, title, description, properties, required, additionalProperties, items
        case minimum, maximum, minLength, maxLength, minItems, maxItems, writeOnly
        case choices = "enum", defaultValue = "default", ui = "x-unic-ui"
    }

    public var orderedKeys: [String] {
        (properties ?? [:]).keys.sorted {
            let a = properties?[$0]?.ui?.order ?? 0
            let b = properties?[$1]?.ui?.order ?? 0
            return a == b ? $0 < $1 : a < b
        }
    }

    public var widget: String {
        if writeOnly == true { return "password" }
        if choices != nil { return "select" }
        if let explicit = ui?.widget, !explicit.isEmpty { return explicit }
        switch type {
        case "string": return "input"
        case "integer", "number": return "inputnumber"
        case "boolean": return "switch"
        case "object": return "group"
        case "array": return "list"
        default: return ""
        }
    }

    public func title(for key: String) -> String { title ?? key }

    /// Defaults for a newly added list item or explicitly added optional field.
    public var newValue: JSONValue {
        if let defaultValue { return insertingDefaults(defaultValue) }
        if let choice = choices?.first { return choice }
        switch type {
        case "object": return insertingDefaults(.object([:]))
        case "array": return .array([])
        case "boolean": return .bool(false)
        case "integer": return .number(ceil(max(minimum ?? 0, -9_007_199_254_740_991)))
        case "number": return .number(minimum ?? 0)
        default: return .string("")
        }
    }

    public func insertingDefaults(_ value: JSONValue) -> JSONValue {
        if case .object(var values) = value, type == "object" {
            for key in (properties ?? [:]).keys.sorted() {
                guard let child = properties?[key] else { continue }
                if let present = values[key] { values[key] = child.insertingDefaults(present) }
                else if let fallback = child.defaultValue { values[key] = child.insertingDefaults(fallback) }
            }
            return .object(values)
        }
        if case .array(let values) = value, type == "array", let items {
            return .array(values.map { items.insertingDefaults($0) })
        }
        return value
    }

    public func validate(_ value: JSONValue, path: String = "") throws {
        switch (type, value) {
        case ("object", .object(let values)):
            let declared = properties ?? [:]
            if values.keys.contains(where: { declared[$0] == nil }) { throw SettingsIssue("unknown_property", path) }
            for key in declared.keys.sorted() {
                if let present = values[key] { try declared[key]?.validate(present, path: pointer(path, key)) }
                else if required?.contains(key) == true { throw SettingsIssue("required", pointer(path, key)) }
            }
        case ("array", .array(let values)):
            if values.count < (minItems ?? 0) || values.count > (maxItems ?? Int.max) {
                throw SettingsIssue("items", path)
            }
            for (i, item) in values.enumerated() { try items?.validate(item, path: pointer(path, String(i))) }
        case ("string", .string(let text)):
            let count = text.unicodeScalars.count
            if count < (minLength ?? 0) || count > (maxLength ?? Int.max) { throw SettingsIssue("length", path) }
        case ("boolean", .bool): break
        case ("integer", .number(let n)), ("number", .number(let n)):
            if !n.isFinite || abs(n) > 9_007_199_254_740_991 { throw SettingsIssue("range", path) }
            if type == "integer" && n.rounded(.towardZero) != n { throw SettingsIssue("type", path) }
            if n < (minimum ?? -Double.infinity) || n > (maximum ?? Double.infinity) {
                throw SettingsIssue("range", path)
            }
        default: throw SettingsIssue("type", path)
        }
        if let choices, !choices.contains(value) { throw SettingsIssue("enum", path) }
    }

    fileprivate func containsSecret(_ value: JSONValue) -> Bool {
        if writeOnly == true { return true }
        if case .object(let values) = value {
            return values.contains { key, value in properties?[key]?.containsSecret(value) == true }
        }
        if case .array(let values) = value { return values.contains { items?.containsSecret($0) == true } }
        return false
    }

    fileprivate func check(path: String = "", depth: Int = 0, count: inout Int) throws {
        count += 1
        if depth > 16 || count > 512 { throw SettingsIssue("limit_exceeded", path) }
        if type != "object" && (properties != nil || required != nil || additionalProperties != nil) ||
            type != "array" && (items != nil || minItems != nil || maxItems != nil) ||
            type != "string" && (minLength != nil || maxLength != nil || writeOnly == true) ||
            type != "integer" && type != "number" && (minimum != nil || maximum != nil) {
            throw SettingsIssue("invalid_schema", path)
        }
        switch type {
        case "object":
            if additionalProperties != false || choices != nil { throw SettingsIssue("invalid_schema", path) }
            let names = required ?? []
            if Set(names).count != names.count || names.contains(where: { properties?[$0] == nil }) {
                throw SettingsIssue("invalid_schema", path)
            }
            for key in (properties ?? [:]).keys.sorted() {
                if key.isEmpty || key.utf8.count > 128 { throw SettingsIssue("invalid_schema", path) }
                try properties?[key]?.check(path: pointer(path, key), depth: depth + 1, count: &count)
            }
        case "array":
            guard let items, items.type != "array", items.type != "object", choices == nil else {
                throw SettingsIssue("unsupported_schema", path)
            }
            try items.check(path: path, depth: depth + 1, count: &count)
        case "string", "boolean", "integer", "number": break
        default: throw SettingsIssue("unsupported_schema", path)
        }
        for (min, max) in [(minLength, maxLength), (minItems, maxItems)] {
            if (min ?? 0) < 0 || (max ?? 0) < 0 || (min != nil && max != nil && min! > max!) {
                throw SettingsIssue("invalid_schema", path)
            }
        }
        for n in [minimum, maximum].compactMap({ $0 }) {
            if !n.isFinite || abs(n) > 9_007_199_254_740_991 { throw SettingsIssue("invalid_schema", path) }
        }
        if let minimum, let maximum, minimum > maximum { throw SettingsIssue("invalid_schema", path) }
        if writeOnly == true && (defaultValue != nil || choices != nil) { throw SettingsIssue("invalid_schema", path) }
        if let choices {
            if choices.isEmpty || choices.count > 128 { throw SettingsIssue("invalid_schema", path) }
            for (index, choice) in choices.enumerated() {
                // validate checks enum too, but this choice belongs to it.
                try validate(choice, path: path)
                if choices.prefix(index).contains(choice) { throw SettingsIssue("invalid_schema", path) }
            }
        }
        if let ui {
            let w = ui.widget ?? ""
            let valid = w.isEmpty ||
                ["input", "textarea"].contains(w) && type == "string" ||
                w == "password" && writeOnly == true ||
                w == "inputnumber" && ["integer", "number"].contains(type) ||
                w == "switch" && type == "boolean" ||
                w == "select" && choices != nil ||
                w == "group" && type == "object" ||
                w == "list" && type == "array"
            if !valid || writeOnly == true && !["", "password"].contains(w) ||
                choices != nil && !["", "select"].contains(w) ||
                !(ui.optionLabels ?? []).isEmpty && ui.optionLabels?.count != choices?.count {
                throw SettingsIssue("invalid_schema", path)
            }
        }
        if let defaultValue {
            do { try validate(insertingDefaults(defaultValue), path: path) }
            catch { throw SettingsIssue("invalid_schema", path) }
            if containsSecret(defaultValue) { throw SettingsIssue("invalid_schema", path) }
        }
    }
}

public struct SettingsDocument {
    public let root: SettingsNode
    public init(data: Data) throws {
        guard data.count <= 256 << 10 else { throw SettingsIssue("limit_exceeded") }
        let raw = try decode(data, maxDepth: 64)
        guard raw["$schema"] == .string("https://json-schema.org/draft/2020-12/schema"),
              raw["x-unic-settings-version"] == .number(1),
              raw["type"] == .string("object") else { throw SettingsIssue("unsupported_schema") }
        try Self.checkKeywords(raw, root: true)
        do { root = try JSONDecoder().decode(SettingsNode.self, from: data) }
        catch { throw SettingsIssue("invalid_schema") }
        var count = 0
        try root.check(count: &count)
    }

    public func normalize(_ value: JSONValue) throws -> JSONValue {
        let result = root.insertingDefaults(value)
        try root.validate(result)
        let data = try JSONEncoder().encode(result)
        if data.count > 64 << 10 { throw SettingsIssue("limit_exceeded") }
        return result
    }

    public func normalize(data: Data) throws -> JSONValue {
        if data.count > 64 << 10 { throw SettingsIssue("invalid_json") }
        return try normalize(decode(data))
    }

    public static func demo() throws -> SettingsDocument {
        try SettingsDocument(data: resource("settings-demo.schema"))
    }

    public static func resource(_ name: String, in bundle: Bundle = .main) throws -> Data {
        // Packaged apps use ordinary resources, while swift run/test uses the
        // SwiftPM resource bundle. Avoid Bundle.module's fatalError fallback
        // if an installed application's resources are missing.
        if bundle.bundleURL.pathExtension == "app" {
            guard let url = bundle.resourceURL?
                .appendingPathComponent("SettingsSchema")
                .appendingPathComponent(name + ".json"),
                  FileManager.default.fileExists(atPath: url.path) else { throw SettingsIssue("unavailable") }
            return try Data(contentsOf: url)
        }
        guard let url = Bundle.module.url(forResource: name, withExtension: "json") else {
            throw SettingsIssue("unavailable")
        }
        return try Data(contentsOf: url)
    }

    private static func checkKeywords(_ value: JSONValue, root: Bool = false, depth: Int = 0) throws {
        guard depth <= 16, case .object(let fields) = value else { throw SettingsIssue("invalid_schema") }
        var allowed: Set<String> = ["type", "title", "description", "properties", "required",
            "additionalProperties", "items", "enum", "default", "minimum", "maximum",
            "minLength", "maxLength", "minItems", "maxItems", "writeOnly", "x-unic-ui"]
        if root { allowed.formUnion(["$schema", "x-unic-settings-version"]) }
        if !Set(fields.keys).isSubset(of: allowed) { throw SettingsIssue("invalid_schema") }
        for (key, value) in fields where key != "default" {
            if value == .null { throw SettingsIssue("invalid_schema") }
        }
        if let ui = fields["x-unic-ui"] {
            guard case .object(let hints) = ui,
                  Set(hints.keys).isSubset(of: ["widget", "order", "group", "option_labels"]),
                  !hints.values.contains(.null) else { throw SettingsIssue("invalid_schema") }
        }
        if let properties = fields["properties"] {
            guard case .object(let children) = properties else { throw SettingsIssue("invalid_schema") }
            for child in children.values { try checkKeywords(child, depth: depth + 1) }
        }
        if let items = fields["items"] { try checkKeywords(items, depth: depth + 1) }
    }
}

private func pointer(_ path: String, _ key: String) -> String {
    path + "/" + key.replacingOccurrences(of: "~", with: "~0").replacingOccurrences(of: "/", with: "~1")
}

private func decode(_ data: Data, maxDepth: Int = 32) throws -> JSONValue {
    do {
        var scanner = JSONScanner(data, maxDepth: maxDepth)
        try scanner.check()
        return try JSONDecoder().decode(JSONValue.self, from: data)
    } catch { throw SettingsIssue("invalid_json") }
}

/// Preflight duplicate keys and depth, which JSONDecoder otherwise accepts.
/// JSONDecoder performs the final JSON grammar and UTF-8 checks.
private struct JSONScanner {
    let bytes: [UInt8]
    let maxDepth: Int
    var index = 0
    init(_ data: Data, maxDepth: Int) { bytes = Array(data); self.maxDepth = maxDepth }
    mutating func check() throws {
        try value(depth: 0)
        whitespace()
        if index != bytes.count { throw SettingsIssue("invalid_json") }
    }
    mutating func whitespace() {
        while index < bytes.count && [9, 10, 13, 32].contains(bytes[index]) { index += 1 }
    }
    mutating func consume(_ byte: UInt8) throws {
        whitespace()
        guard index < bytes.count, bytes[index] == byte else { throw SettingsIssue("invalid_json") }
        index += 1
    }
    mutating func string() throws -> String {
        whitespace()
        let start = index
        try consume(34)
        while index < bytes.count {
            let c = bytes[index]; index += 1
            if c == 92 { index += 1 }
            else if c == 34 { return try JSONDecoder().decode(String.self, from: Data(bytes[start..<index])) }
        }
        throw SettingsIssue("invalid_json")
    }
    mutating func value(depth: Int) throws {
        whitespace()
        guard depth <= maxDepth, index < bytes.count else { throw SettingsIssue("invalid_json") }
        switch bytes[index] {
        case 123:
            index += 1; whitespace()
            var seen = Set<String>()
            if index < bytes.count && bytes[index] == 125 { index += 1; return }
            while true {
                let key = try string()
                if !seen.insert(key).inserted { throw SettingsIssue("invalid_json") }
                try consume(58)
                try value(depth: depth + 1)
                whitespace()
                if index < bytes.count && bytes[index] == 125 { index += 1; return }
                try consume(44)
            }
        case 91:
            index += 1; whitespace()
            if index < bytes.count && bytes[index] == 93 { index += 1; return }
            while true {
                try value(depth: depth + 1)
                whitespace()
                if index < bytes.count && bytes[index] == 93 { index += 1; return }
                try consume(44)
            }
        case 34: _ = try string()
        default:
            let start = index
            while index < bytes.count && ![9, 10, 13, 32, 44, 93, 125].contains(bytes[index]) { index += 1 }
            if index == start { throw SettingsIssue("invalid_json") }
            if bytes[start] == 45 || (48...57).contains(bytes[start]) {
                let raw = String(decoding: bytes[start..<index], as: UTF8.self)
                guard let n = Double(raw), n.isFinite,
                      decimalKey(raw) == decimalKey(String(n)) else { throw SettingsIssue("invalid_json") }
            }
        }
    }
}

private func decimalKey(_ raw: String) -> String {
    var text = raw.lowercased()
    let sign = text.hasPrefix("-") ? "-" : ""
    if !sign.isEmpty { text.removeFirst() }
    var exponent = 0
    if let e = text.firstIndex(of: "e") {
        guard let number = Int(text[text.index(after: e)...]), (-10000...10000).contains(number) else { return "invalid" }
        exponent = number
        text = String(text[..<e])
    }
    if let dot = text.firstIndex(of: ".") {
        exponent -= text.distance(from: text.index(after: dot), to: text.endIndex)
        text.remove(at: dot)
    }
    if !text.utf8.allSatisfy({ (48...57).contains($0) }) { return "invalid" }
    text = String(text.drop(while: { $0 == "0" }))
    if text.isEmpty { return "0" }
    while text.last == "0" { exponent += 1; text.removeLast() }
    return sign + text + "e" + String(exponent)
}
