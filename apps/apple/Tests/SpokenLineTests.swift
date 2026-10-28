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
