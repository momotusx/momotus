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

@Test func resumeSaysWhatTheFileHasLeftFirst() {
    // The stored duration wins over the runtime, rounded up as Continue Watching words it.
    #expect(resumeText(resumeMs: 21 * 60_000, leftMs: 60_001, runtime: "57 min") == "2 min left")
    #expect(resumeText(resumeMs: 21 * 60_000, leftMs: 60_001, runtime: nil) == "2 min left")
    #expect(resumeText(resumeMs: 21 * 60_000, leftMs: 0, runtime: "57 min") == "36 min left")
}
