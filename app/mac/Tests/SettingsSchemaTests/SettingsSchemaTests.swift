import XCTest
@testable import SettingsSchema

final class SettingsSchemaTests: XCTestCase {
    struct Vector: Decodable {
        let name: String
        let input: JSONValue
        let output: JSONValue?
        let code: String?
        let path: String?
    }

    func testSharedVectors() throws {
        let document = try SettingsDocument.demo()
        let cases = try JSONDecoder().decode([Vector].self, from: SettingsDocument.resource("validation-cases"))
        XCTAssertGreaterThan(cases.count, 15)
        for test in cases {
            do {
                let output = try document.normalize(test.input)
                XCTAssertNil(test.code, test.name)
                XCTAssertEqual(output, test.output, test.name)
            } catch let issue as SettingsIssue {
                XCTAssertEqual(issue.code, test.code, test.name)
                XCTAssertEqual(issue.path, test.path, test.name)
            }
        }
    }

    func testTypedControlMappingAndOrder() throws {
        let root = try SettingsDocument.demo().root
        XCTAssertEqual(root.orderedKeys, ["prefix", "max_length", "mode", "enabled", "notes", "token", "tags", "retry"])
        for (key, widget) in ["prefix":"input", "max_length":"inputnumber", "mode":"select",
                              "enabled":"switch", "notes":"textarea", "token":"password", "tags":"list", "retry":"group"] {
            XCTAssertEqual(root.properties?[key]?.widget, widget)
        }
        XCTAssertEqual(root.properties?["mode"]?.ui?.optionLabels, ["Plain text", "Markdown"])
        XCTAssertNil(try SettingsDocument.demo().normalize(.object([:]))["token"])
    }

    func testRejectsUnsupportedSchema() throws {
        let base = """
        {"$schema":"https://json-schema.org/draft/2020-12/schema","x-unic-settings-version":1,
        "type":"object","additionalProperties":false,"properties":{"x":FIELD}}
        """
        for field in [
            #"{"type":"string","pattern":"x"}"#,
            #"{"type":"string","x-unic-ui":{"widget":"inputnumber"}}"#,
            #"{"type":"string","writeOnly":true,"default":"SECRET"}"#,
            #"{"type":"string","enum":["a"],"default":"b"}"#,
            #"{"type":"integer","enum":[1,1.0]}"#,
            #"{"type":"string","minLength":2,"maxLength":1}"#,
            #"{"type":"object","additionalProperties":true}"#,
            #"{"type":"array","items":{"type":"object","additionalProperties":false}}"#,
            #"{"type":"string","minimum":null}"#,
            #"{"type":"string","type":"boolean"}"#
        ] {
            XCTAssertThrowsError(try SettingsDocument(data: Data(base.replacingOccurrences(of: "FIELD", with: field).utf8)))
        }
    }

    func testStrictJSONAndPrivacy() throws {
        let d = try SettingsDocument.demo()
        for bad in [
            #"{"prefix":"a","prefix":"b"}"#,
            #"{"retry":{"count":1,"count":2}}"#,
            "{} {}",
            #"{"notes":NaN}"#,
            #"{"max_length":9007199254740993}"#,
            #"{"max_length":1.00000000000000000001}"#,
            "{\"notes\":" + String(repeating: "[", count: 33) + "0" + String(repeating: "]", count: 33) + "}",
            "{\"notes\":\"" + String(repeating: "a", count: 65536) + "\"}"
        ] {
            XCTAssertThrowsError(try d.normalize(data: Data(bad.utf8)))
        }
        do {
            _ = try d.normalize(data: Data(#"{"private-key":"private-value"}"#.utf8))
            XCTFail("accepted unknown key")
        } catch let e as SettingsIssue {
            XCTAssertEqual(e, SettingsIssue("unknown_property"))
            XCTAssertFalse(e.localizedDescription.contains("private"))
        }
    }

    func testNumericAndBooleanChoicesKeepTheirTypes() throws {
        let raw = """
        {"$schema":"https://json-schema.org/draft/2020-12/schema","x-unic-settings-version":1,
        "type":"object","additionalProperties":false,"properties":{
          "count":{"type":"integer","enum":[1,2],"default":1},
          "enabled":{"type":"boolean","enum":[true,false],"default":false}
        }}
        """
        let d = try SettingsDocument(data: Data(raw.utf8))
        let value = try d.normalize(.object([:]))
        XCTAssertEqual(value["count"], .number(1))
        XCTAssertEqual(value["enabled"], .bool(false))
        XCTAssertThrowsError(try d.normalize(.object(["count": .string("1")])))
    }

    func testPackagedApplicationResourcesAndMissingFile() throws {
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: folder) }
        let application = folder.appendingPathComponent("Preview.app")
        let contents = application.appendingPathComponent("Contents")
        let resources = contents.appendingPathComponent("Resources/SettingsSchema")
        try FileManager.default.createDirectory(at: resources, withIntermediateDirectories: true)
        let info = ["CFBundleIdentifier": "dev.unic.settings-test", "CFBundlePackageType": "APPL"]
        let plist = try PropertyListSerialization.data(fromPropertyList: info, format: .xml, options: 0)
        try plist.write(to: contents.appendingPathComponent("Info.plist"))
        let fixture = try SettingsDocument.resource("settings-demo.schema")
        try fixture.write(to: resources.appendingPathComponent("settings-demo.schema.json"))
        let bundle = try XCTUnwrap(Bundle(url: application))
        let actual = try SettingsDocument.resource("settings-demo.schema", in: bundle)
        XCTAssertEqual(actual, fixture)
        XCTAssertNoThrow(try SettingsDocument(data: actual))
        XCTAssertThrowsError(try SettingsDocument.resource("missing", in: bundle)) { error in
            XCTAssertEqual(error as? SettingsIssue, SettingsIssue("unavailable"))
        }
    }
}
