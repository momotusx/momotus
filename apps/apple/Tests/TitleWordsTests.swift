import Testing

// Resume says the same fact as the episode list: the time left.

@Test func resumeSaysTheTimeLeftWhenTheRuntimeIsKnown() {
    #expect(resumeText(resumeMs: 21 * 60_000, runtime: "57 min") == "36 min left")
    #expect(resumeText(resumeMs: 70 * 60_000, runtime: "2h 9min") == "59 min left")
    // Never "0 min left": past the stated runtime it still says a minute.
    #expect(resumeText(resumeMs: 58 * 60_000, runtime: "57 min") == "1 min left")
}

@Test func resumeSaysThePositionWithoutARuntime() {
    #expect(resumeText(resumeMs: 21 * 60_000, runtime: nil) == "from 21:00")
    #expect(resumeText(resumeMs: 75 * 60_000 + 3_000, runtime: "unknown") == "from 1:15:03")
}
