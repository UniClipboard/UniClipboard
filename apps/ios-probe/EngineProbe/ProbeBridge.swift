import Foundation

struct ProbeBridge: Sendable {
    func command(_ payload: [String: Any]) -> [String: Any] {
        guard JSONSerialization.isValidJSONObject(payload),
              let data = try? JSONSerialization.data(withJSONObject: payload),
              let input = String(data: data, encoding: .utf8)
        else {
            return ["ok": false, "kind": "invalid_command"]
        }

        return input.withCString { pointer in
            guard let responsePointer = uc_ios_probe_command(pointer) else {
                return ["ok": false, "kind": "empty_response"]
            }
            defer { uc_ios_probe_string_free(responsePointer) }
            let response = String(cString: responsePointer)
            guard let responseData = response.data(using: .utf8),
                  let object = try? JSONSerialization.jsonObject(with: responseData),
                  let dictionary = object as? [String: Any]
            else {
                return ["ok": false, "kind": "invalid_response"]
            }
            return dictionary
        }
    }
}
