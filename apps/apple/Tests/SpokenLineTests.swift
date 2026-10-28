import Testing

// A spoken StateLine: said when it appears, said again only when its words change.

@Test func aLineThatAppearsIsSaidAndAChangedLineIsSaidAgain() {
    #expect(spokenLine(last: nil, current: "Search did not load.") == "Search did not load.")
    #expect(spokenLine(last: "No matches", current: "No matches for \"zz\"") == "No matches for \"zz\"")
}

@Test func aRedrawOfTheSameWordsSaysNothing() {
    #expect(spokenLine(last: "No matches", current: "No matches") == nil)
    #expect(spokenLine(last: nil, current: "") == nil)
}

@Test func aLineOutOfSightStaysSilentAndIsSaidOnceOnReturn() {
    // Under a pushed page, the player, or in a place kept behind the shown one.
    #expect(spokenLine(last: nil, current: "Could not load", audible: false) == nil)
    // Back in sight, what came in meanwhile is said; the same words are not said twice.
    #expect(spokenLine(last: nil, current: "Could not load", audible: true) == "Could not load")
    #expect(spokenLine(last: "Could not load", current: "Could not load", audible: true) == nil)
}
