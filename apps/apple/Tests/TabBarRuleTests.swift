import SwiftUI
import Testing

// The iPhone bar drops its words below 360 points or past a 1.3 font scale, as Android does.

@Test func theBarKeepsItsWordsAtTheDefaultSizeOnAPhone() {
    #expect(!tabBarIconsOnly(width: 402, type: .large))
    #expect(!tabBarIconsOnly(width: 360, type: .large))
}

@Test func theBarShowsIconsOnlyBelow360Points() {
    #expect(tabBarIconsOnly(width: 359, type: .large))
    #expect(tabBarIconsOnly(width: 320, type: .xSmall))
}

@Test func theBarShowsIconsOnlyPastAFontScaleOf1Point3() {
    // xxLarge is 21 over 17 (1.24), xxxLarge is 23 over 17 (1.35).
    #expect(!tabBarIconsOnly(width: 402, type: .xxLarge))
    #expect(tabBarIconsOnly(width: 402, type: .xxxLarge))
    #expect(tabBarIconsOnly(width: 402, type: .accessibility1))
    #expect(tabBarIconsOnly(width: 1024, type: .accessibility5))
}

@Test func goBackNeedsAPushedPageAndNoPlayerOverThePlaces() {
    #expect(canGoBack(depth: 1, playerUp: false))
    #expect(!canGoBack(depth: 0, playerUp: false))
    #expect(!canGoBack(depth: 2, playerUp: true))
}
