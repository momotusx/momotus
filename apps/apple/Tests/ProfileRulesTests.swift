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

@Test func theEditorIsDirtyOnlyWithAChangeNotYetSaved() {
    // Editing: the stored name, however it is spaced, is no change; a different name or a picked picture is.
    #expect(!ProfileRules.isDirty(name: "Ada", stored: "Ada", touched: false))
    #expect(!ProfileRules.isDirty(name: " Ada ", stored: "Ada", touched: false))
    #expect(ProfileRules.isDirty(name: "Ada L", stored: "Ada", touched: false))
    #expect(ProfileRules.isDirty(name: "Ada", stored: "Ada", touched: true))
    // Adding: nothing typed and nothing picked is nothing to lose.
    #expect(!ProfileRules.isDirty(name: "", stored: nil, touched: false))
    #expect(ProfileRules.isDirty(name: "Kid", stored: nil, touched: false))
}

@Test func onlyAPinRemovalWithNoKeypadAsksFirst() {
    #expect(PinAction.removeOther.confirms)
    #expect(PinAction.removeUnreadable.confirms)
    // Set, change and the own PIN's removal run through the keypad already.
    #expect(![PinAction.set, .change, .remove].contains { $0.confirms })
}

@Test func theEditorHasNoColorRow() throws {
    // DECISION (Profiles): the picture is the one choice. Read from the sources: the editor is a view.
    let file = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee/Profiles/ManageProfiles.swift")
    let editor = try String(contentsOf: file, encoding: .utf8)
    #expect(!editor.contains("caps(\"Color\")"), "the Color row is back in the editor")
}

@Test func theEditorsConfirmationsTakeNoDestructiveRole() throws {
    // DECISION (Confirmation): Apple's dialogs carry the meaning in the word, with no red role.
    let file = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee/Profiles/ManageProfiles.swift")
    let editor = try String(contentsOf: file, encoding: .utf8)
    for line in editor.split(separator: "\n") where line.contains("Discard") || line.contains("removePin(") {
        #expect(!line.contains(".destructive"), "a dialog button took the destructive role: \(line)")
    }
    #expect(editor.contains("label($0.action, $0.target) + \"?\""), "the PIN dialog no longer names whose PIN")
}

@Test func leavingWithEditsOffersSaveDiscardAndKeepEditing() throws {
    // STATE-41. Read from the sources: a dialog is a view.
    let file = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee/Profiles/ManageProfiles.swift")
    let editor = try String(contentsOf: file, encoding: .utf8)
    for choice in ["Button(\"Save changes\") { save(thenLeave: true) }", "Button(\"Discard changes\")", "Button(\"Keep editing\", role: .cancel)"] {
        #expect(editor.contains(choice), "the leave ask lost \(choice)")
    }
}

@Test func returnNeverRemovesAPin() throws {
    let file = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee/Profiles/ManageProfiles.swift")
    let editor = try String(contentsOf: file, encoding: .utf8)
    let cancel = try #require(editor.range(of: "Button(\"Cancel\", role: .cancel) {}"))
    #expect(editor[cancel.upperBound...].prefix(200).contains(".keyboardShortcut(.defaultAction)"), "Cancel lost the default action")
}

@Test func theEditorsDialogsDrawInTheKeyBlue() throws {
    // DECISION (Confirmation): with no destructive role the buttons take the tint, which is the key blue here.
    let file = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee/Profiles/ManageProfiles.swift")
    let editor = try String(contentsOf: file, encoding: .utf8)
    let tint = try #require(editor.range(of: ".tint(Theme.Palette.key)"), "the editor lost its key tint")
    let dialog = try #require(editor.range(of: ".confirmationDialog("))
    #expect(tint.lowerBound < dialog.lowerBound, "the tint must wrap the dialogs")
}
