import Foundation
import Testing

// Error reports on iOS and the Mac (plans/error-reports.md 6.3, 10): the shared words, the device fields in the
// relay's charset, and MetricKit's diagnostics read from their JSON.

private let shared = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../../shared")

@Test func theReportWordsAreTheSharedFile() throws {
    let data = try Data(contentsOf: shared.appending(path: "report-words.json"))
    var file = try #require(try JSONSerialization.jsonObject(with: data) as? [String: String])
    file.removeValue(forKey: "about")
    #expect(file == ReportWords.table)
    #expect(ReportWords.privacyURL(server: "https://relay.example/")?.absoluteString == "https://relay.example/privacy")
    #expect(ReportWords.privacyURL(server: nil) == nil)
}

@Test func deviceFieldsKeepTheRelaysCharset() {
    #expect(ReportRules.plain("iPhone14,2", max: 60) == "iPhone14,2")
    #expect(ReportRules.plain("Robin’s iPhone", max: 60) == "Robin s iPhone")
    let device = ReportRules.device(app: "ios", debug: false, version: "0.14.2", core: "0.1.0", os: "iOS 18.1",
                                    model: "", form: "phone", coreBuild: "none")
    #expect(device["model"] as? String == "Apple")
    #expect(device["coreBuild"] as? String == "own")
    #expect(device["channel"] as? String == "release")
}

@Test func aCrashDiagnosticReadsAsACrashWithTheAttributedThreadsFrames() throws {
    let json = """
    {"diagnosticMetaData":{"exceptionType":1,"exceptionCode":0,"signal":11,"terminationReason":"Namespace SIGNAL, Code 11"},
     "callStackTree":{"callStackPerThread":true,"callStacks":[
       {"threadAttributed":false,"callStackRootFrames":[{"binaryName":"libsystem_kernel.dylib","offsetIntoBinaryTextSegment":10}]},
       {"threadAttributed":true,"callStackRootFrames":[{"binaryName":"Momotus","offsetIntoBinaryTextSegment":4096,
         "subFrames":[{"binaryName":"Momotus","offsetIntoBinaryTextSegment":255}]}]}]}}
    """
    let report = try #require(ReportRules.report(diagnostic: Data(json.utf8), hang: false, at: 5))
    #expect(report["kind"] as? String == "crash")
    #expect(report["message"] as? String == "crash, exception type 1, code 0, signal 11: Namespace SIGNAL, Code 11")
    #expect(report["stack"] as? [String] == ["Momotus 0x1000", "Momotus 0xff"])
}

@Test func aHangDiagnosticReadsAsAnAnr() throws {
    let json = #"{"diagnosticMetaData":{"hangDuration":"3.5 sec"},"callStackTree":{"callStacks":[]}}"#
    let report = try #require(ReportRules.report(diagnostic: Data(json.utf8), hang: true, at: 5))
    #expect(report["kind"] as? String == "anr")
    #expect(report["message"] as? String == "hang of 3.5 sec")
    #expect(ReportRules.report(diagnostic: Data("not json".utf8), hang: true, at: 5) == nil)
}
