// Reads the text in a window screenshot with the system's Vision OCR and prints it as JSON.
//
//   swift read_text.swift <image.png> <language,language,...>
//
// The languages are Vision recognition languages such as en-US, zh-Hans, zh-Hant, ja-JP, ru-RU
// or pt-BR. Accurate recognition with language correction, one string per recognized line.
import AppKit
import Foundation
import Vision

let arguments = CommandLine.arguments.dropFirst()
guard let path = arguments.first,
    let image = NSImage(contentsOfFile: path),
    let cgImage = image.cgImage(forProposedRect: nil, context: nil, hints: nil)
else {
    FileHandle.standardError.write("usage: read_text.swift <image.png> <languages>\n".data(using: .utf8)!)
    exit(2)
}
let languages = (arguments.dropFirst().first ?? "en-US").split(separator: ",").map(String.init)

let request = VNRecognizeTextRequest()
request.recognitionLevel = .accurate
request.usesLanguageCorrection = true
request.recognitionLanguages = languages
try VNImageRequestHandler(cgImage: cgImage).perform([request])
let lines = (request.results ?? []).compactMap { $0.topCandidates(1).first?.string }
let data = try JSONSerialization.data(withJSONObject: ["languages": languages, "lines": lines])
print(String(decoding: data, as: UTF8.self))
