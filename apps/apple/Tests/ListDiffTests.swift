import Foundation
import Testing

/// Builds diffs from JSON so the tests also cover decoding of every op.
private func diffs(_ json: String) throws -> [ListDiff<Int>] {
    try JSONDecoder().decode([ListDiff<Int>].self, from: Data(json.utf8))
}

@Test func matchesTheCoreAndWebSemanticsForEveryOp() throws {
    // The same sequence as apps/web/src/core/diff.test.ts.
    let out = applyDiffs([], try diffs("""
        [{"op":"pushBack","value":1},{"op":"pushBack","value":2},
         {"op":"insert","index":1,"value":9},{"op":"set","index":0,"value":5},
         {"op":"remove","index":2},{"op":"pushFront","value":7},
         {"op":"append","values":[8,8]},{"op":"popBack"},{"op":"popFront"},
         {"op":"truncate","length":2}]
        """))
    #expect(out == [5, 9])
    #expect(applyDiffs(out, try diffs(#"[{"op":"reset","values":[1]},{"op":"clear"}]"#)) == [])
}

@Test func replaysTheRustMirrorTest() throws {
    // crates/mq-runtime/src/live.rs: push_back, push_back, insert, set, remove.
    let out = applyDiffs([], try diffs("""
        [{"op":"pushBack","value":1},{"op":"pushBack","value":2},{"op":"insert","index":1,"value":9},
         {"op":"set","index":0,"value":5},{"op":"remove","index":2}]
        """))
    #expect(out == [5, 9])
}

@Test func popsOnAnEmptyListDoNothing() throws {
    #expect(applyDiffs([], try diffs(#"[{"op":"popFront"},{"op":"popBack"}]"#)) == [])
}

@Test func outOfRangeIndexesClampOrAreIgnoredLikeTheWebClient() throws {
    #expect(applyDiffs([1], try diffs(#"[{"op":"insert","index":5,"value":2}]"#)) == [1, 2])
    #expect(applyDiffs([1], try diffs(#"[{"op":"set","index":3,"value":2}]"#)) == [1])
    #expect(applyDiffs([1], try diffs(#"[{"op":"remove","index":3}]"#)) == [1])
    #expect(applyDiffs([1, 2], try diffs(#"[{"op":"truncate","length":5}]"#)) == [1, 2])
}

@Test func rejectsAnUnknownOp() {
    #expect(throws: DecodingError.self) { try diffs(#"[{"op":"shuffle"}]"#) }
}
