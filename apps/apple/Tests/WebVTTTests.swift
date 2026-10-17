import Testing

@Test func parsesCuesWithAndWithoutHoursAndDropsTags() {
    let cues = WebVTT.parse("""
        WEBVTT

        1
        00:00:01.000 --> 00:00:02.500 align:center
        <i>Hello</i> &amp; welcome

        02:03.250 --> 02:05.000
        Two
        lines
        """)
    #expect(cues == [
        WebVTT.Cue(start: 1, end: 2.5, text: "Hello & welcome"),
        WebVTT.Cue(start: 123.25, end: 125, text: "Two\nlines"),
    ])
}

@Test func findsTheCueShowingAtATime() {
    let cues = [WebVTT.Cue(start: 1, end: 2, text: "a"), WebVTT.Cue(start: 1.5, end: 3, text: "b")]
    #expect(WebVTT.text(at: 0.5, in: cues) == nil)
    #expect(WebVTT.text(at: 1.7, in: cues) == "a\nb")
    #expect(WebVTT.text(at: 2, in: cues) == "b")
    #expect(WebVTT.text(at: 3, in: cues) == nil)
}

@Test func skipsBlocksWithoutTiming() {
    #expect(WebVTT.parse("WEBVTT\n\nNOTE a comment\n\nSTYLE\n::cue {}").isEmpty)
}
