import Foundation
import Testing

// Profile rules, as the web's lib/profiles.ts and Android's ProfileCopy have them.

private func profile(_ id: String, pin: Bool = false, owner: Bool = false, syncing: Bool = false) -> ProfileInfo {
    ProfileInfo(id: id, name: id.capitalized, avatar: "", position: 0, hasPin: pin, isOwner: owner, syncing: syncing)
}

private func snapshot(_ profiles: [ProfileInfo], locked: Bool = false) -> ProfilesSnapshot {
    ProfilesSnapshot(profiles: profiles, active: profiles[0].id, locked: locked, max: 6)
}

@Test func avatarsParseWithFallbacks() {
    #expect(ProfileRules.parseAvatar("cat:sky") == .init(glyph: "cat", color: "sky"))
    // Never chosen: the letter on ember.
    #expect(ProfileRules.parseAvatar("") == .init(glyph: "letter", color: "ember"))
    // Chosen but unknown: the letter, on raised.
    #expect(ProfileRules.parseAvatar(":") == .init(glyph: "letter", color: nil))
    #expect(ProfileRules.parseAvatar("unicorn:sky") == .init(glyph: "letter", color: "sky"))
    #expect(ProfileRules.parseAvatar("cat:neon") == .init(glyph: "cat", color: nil))
    // Anything after a second colon is ignored.
    #expect(ProfileRules.parseAvatar("dog:moss:sparkles") == .init(glyph: "dog", color: "moss"))
    #expect(ProfileRules.nextAvatar([""]) == "letter:amber")
    #expect(ProfileRules.nextAvatar(["letter:ember", "cat:amber"]) == "letter:moss")
    #expect(ProfileRules.initial("  élodie") == "É")
    #expect(ProfileRules.initial("") == "?")
}

@Test func thePickerStartsWhenLockedOrShared() {
    #expect(!ProfileRules.startsOnPicker(snapshot([profile("main", owner: true)])))
    #expect(ProfileRules.startsOnPicker(snapshot([profile("main", owner: true), profile("kid")])))
    // Locked wins over the one-profile shortcut.
    #expect(ProfileRules.startsOnPicker(snapshot([profile("main", pin: true, owner: true)], locked: true)))
}

@Test func theWaitReadsInMinutesAndCountsDown() {
    #expect(ProfileRules.waitText(61_000) == "1:01")
    #expect(ProfileRules.waitText(299_001) == "5:00")
    #expect(ProfileRules.waitText(0) == "0:00")
    #expect(ProfileRules.waitSentence(56_000) == "Too many tries. Try again in 1 minute.")
    #expect(ProfileRules.waitSentence(299_000) == "Too many tries. Try again in 5 minutes.")
    #expect(KeypadResult.of(code: "locked", retryAfterMs: 60_000, message: "") == .wait(60_000))
    #expect(KeypadResult.of(code: "wrongPin", retryAfterMs: nil, message: "wrong pin") == .wrong)
    #expect(KeypadResult.of(code: "weakPin", retryAfterMs: nil, message: "") == .problem("Pick a less obvious PIN: no repeats, runs or 1234."))
}

@Test func theCoreAllowsTheseTilesAndPinActions() {
    let owner = profile("main", owner: true)
    let kid = profile("kid", pin: true)
    #expect(ProfileRules.spoken(kid, active: "kid") == "Kid, current profile, locked with a PIN")
    #expect(ProfileRules.spoken(profile("kid", syncing: true), active: "main") == "Kid, still syncing")
    #expect(PinAction.allowed(for: owner, by: owner) == [.set])
    #expect(PinAction.allowed(for: kid, by: kid) == [.change, .remove])
    #expect(PinAction.allowed(for: kid, by: owner) == [.removeOther])
    #expect(PinAction.allowed(for: profile("kid"), by: owner).isEmpty)
    #expect(PinAction.allowed(for: owner, by: profile("kid")).isEmpty)
}

@Test func settingAPinAsksTwiceAndAMismatchStartsOver() {
    var flow = PinFlow(.set)
    #expect(flow.prompt == "Choose a PIN")
    #expect(flow.enter("2580") == nil)
    #expect(flow.prompt == "Enter it again")
    #expect(flow.enter("2581") == nil)
    #expect(flow.note == "The PINs did not match. Choose one again.")
    #expect(flow.prompt == "Choose a PIN")
    #expect(flow.enter("1357") == nil)
    #expect(flow.enter("1357") == .init(pin: "1357", currentPin: nil))
}

@Test func changingAndRemovingAskTheCurrentPinFirst() {
    var change = PinFlow(.change)
    #expect(change.prompt == "Enter your current PIN")
    #expect(change.enter("1357") == nil)
    #expect(change.prompt == "Enter the new PIN")
    #expect(change.enter("2468") == nil)
    #expect(change.enter("2468") == .init(pin: "2468", currentPin: "1357"))
    change.wrongCurrent()
    #expect(change.prompt == "Enter your current PIN")
    var remove = PinFlow(.remove)
    #expect(remove.enter("1357") == .init(pin: nil, currentPin: "1357"))
}

@Test func aPinANewerVersionStoredCanOnlyBeRemoved() {
    let kid = profile("kid", pin: true)
    #expect(PinAction.allowed(for: kid, by: kid, unreadable: true) == [.removeUnreadable])
    // It wins over Set, which the snapshot alone would offer.
    let main = profile("main", owner: true)
    #expect(PinAction.allowed(for: main, by: main, unreadable: true) == [.removeUnreadable])
    #expect(PinAction.allowed(for: main, by: main) == [.set])
}

@Test func aPinAnswerNeverTouchesANewerFlow() {
    let sent = PinFlow(.change)
    var newer = PinFlow(.set)
    _ = newer.enter("2719")
    // The flow that asked left and a newer one is on screen: it stays as it is.
    #expect(PinFlow.settle(newer, sent: sent, ok: true, code: nil) == newer)
    #expect(PinFlow.settle(newer, sent: sent, ok: false, code: "wrongPin") == newer)
    #expect(PinFlow.settle(nil, sent: sent, ok: true, code: nil) == nil)
    // Its own flow closes when it went through, and steps back when refused.
    var mine = PinFlow(.change)
    _ = mine.enter("1357")
    #expect(PinFlow.settle(mine, sent: mine, ok: true, code: nil) == nil)
    #expect(PinFlow.settle(mine, sent: mine, ok: false, code: "wrongPin")?.prompt == "Enter your current PIN")
    #expect(PinFlow.settle(mine, sent: mine, ok: false, code: "weakPin")?.prompt == "Enter the new PIN")
}
