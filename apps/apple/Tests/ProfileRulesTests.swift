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
    #expect(!ProfileRules.isDirty(name: "Ada", stored: "Ada", adding: false, touched: false))
    #expect(!ProfileRules.isDirty(name: " Ada ", stored: "Ada", adding: false, touched: false))
    #expect(ProfileRules.isDirty(name: "Ada L", stored: "Ada", adding: false, touched: false))
    #expect(ProfileRules.isDirty(name: "Ada", stored: "Ada", adding: false, touched: true))
    // Adding: nothing typed and nothing picked is nothing to lose.
    #expect(!ProfileRules.isDirty(name: "", stored: nil, adding: true, touched: false))
    #expect(ProfileRules.isDirty(name: "Kid", stored: nil, adding: true, touched: false))
    // Right after a Save the snapshot may still hold the old name: the saved one is the baseline.
    #expect(!ProfileRules.isDirty(name: "Ada L", stored: "Ada", saved: "Ada L", adding: false, touched: false))
    #expect(ProfileRules.isDirty(name: "Ada Lo", stored: "Ada", saved: "Ada L", adding: false, touched: false))
    // A profile removed on another device has nothing left to save, whatever the fields hold.
    #expect(!ProfileRules.isDirty(name: "Ada", stored: nil, adding: false, touched: true))
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
    let start = try #require(editor.range(of: "struct EditProfile"))
    let page = String(editor[start.lowerBound...])
    // A tint reaches only what it wraps: it follows both dialogs (`pinDialog` wraps `leaveGuard`) in the body.
    let bodyText = try body(of: "var body: some View", in: page)
    let dialogs = try #require(bodyText.range(of: "pinDialog("), "the PIN dialog left the body")
    let tint = try #require(bodyText.range(of: ".tint(Theme.Palette.key)"), "the editor lost its key tint")
    #expect(dialogs.lowerBound < tint.lowerBound, "the tint must wrap the dialogs, not sit inside them")
    // Not on the line that builds them either: that would tint only the page inside.
    let built = try #require(bodyText.split(separator: "\n").first { $0.contains("pinDialog(") })
    #expect(!built.contains(".tint("), "the tint sits inside the dialogs again")
    #expect(!page.replacingOccurrences(of: bodyText, with: "").contains(".tint("), "a tint inside the dialogs again")
}

@Test func theDirtyEditorInASheetKeepsAVisibleBackOnTheCancelPlacement() throws {
    // A sheet's toolbar draws no .navigation item and the system back is hidden while dirty: the Back would be gone.
    // Read from the sources: a toolbar is not laid out in a hosted view.
    let file = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee/Profiles/ManageProfiles.swift")
    let editor = try String(contentsOf: file, encoding: .utf8)
    let placement = try body(of: "private var leaveBackPlacement: ToolbarItemPlacement", in: editor)
    #expect(placement.contains("sheetBack.isInSheet ? .cancellationAction : .navigation"), "a sheet's editor has no visible Back")
    let back = try body(of: "private var leaveBack: some ToolbarContent", in: editor)
    #expect(back.contains("ToolbarItem(placement: leaveBackPlacement)"), "the Back ignores the sheet placement")
}

@Test func theSheetsBackNamesItself() throws {
    // Read from the sources: a toolbar is not laid out in a hosted view.
    let file = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee/Profiles/ManageProfiles.swift")
    let editor = try String(contentsOf: file, encoding: .utf8)
    // In a sheet nothing else names the Back: chevron and word (DECISION's back link).
    let label = try body(of: "private var leaveBackLabel: some View", in: editor)
    let sheet = try #require(label.range(of: "if sheetBack.isInSheet {"))
    let word = try #require(label.range(of: ".labelStyle(.titleAndIcon)"), "the sheet's Back lost its word")
    let bare = try #require(label.range(of: ".labelStyle(.iconOnly)"))
    #expect(sheet.lowerBound < word.lowerBound && word.lowerBound < bare.lowerBound, "the word belongs to the sheet case")
}

@Test func profileAvatarsAreCircles() throws {
    // DECISION (Profiles): circles. Read from the sources: they are views.
    let dir = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee/Profiles")
    for file in ["PinKeypad.swift", "ProfilePicker.swift", "ManageProfiles.swift"] {
        let source = try String(contentsOf: dir.appending(path: file), encoding: .utf8)
        #expect(!source.contains("cornerRadius: size * 0.22") && !source.contains("cornerRadius: tile * 0.22"), "a rounded-square avatar is back in \(file)")
    }
    let keypad = try String(contentsOf: dir.appending(path: "PinKeypad.swift"), encoding: .utf8)
    let avatar = try body(of: "struct ProfileAvatar: View", in: keypad)
    let shown = try body(of: "var body: some View", in: avatar)
    #expect(shown.contains("Circle()\n            .fill(lit ? Theme.Palette.primary : Self.color(avatar.color))"), "the avatar is not a circle")
    // The picture choices follow it: ring and focus shape.
    let editor = try String(contentsOf: dir.appending(path: "ManageProfiles.swift"), encoding: .utf8)
    let choice = try body(of: "private func choice(selected: Bool", in: editor)
    #expect(choice.contains(".overlay(Circle().strokeBorder(") && choice.contains("FocusPlainStyle(shape: Circle())"), "the picture choices are not round")
    #expect(!choice.contains("RoundedRectangle"), "a rounded square is back around a picture choice")
    // The Add profile circle is a fill, with no edge (DECISION: fills, not edges).
    let picker = try String(contentsOf: dir.appending(path: "ProfilePicker.swift"), encoding: .utf8)
    let add = try #require(picker.range(of: "Circle()\n                            .fill(Theme.Palette.raised)"), "the Add profile circle changed")
    #expect(!picker[add.lowerBound...].prefix(200).contains("strokeBorder(Theme.Palette.control)"), "the Add profile circle has an edge again")
}


@Test func aPickedProfileDoesWhatItsTileDoes() {
    let main = profile("main", owner: true)
    let kid = profile("kid")
    let locked = profile("locked", pin: true)
    let syncing = profile("syncing", syncing: true)
    let open = snapshot([main, kid, locked, syncing])
    #expect(ProfileRules.choice(main, in: open) == .close)
    #expect(ProfileRules.choice(kid, in: open) == .switchTo)
    #expect(ProfileRules.choice(locked, in: open) == .pin)
    #expect(ProfileRules.choice(syncing, in: open) == .none)
    // A locked session: its own profile asks for its PIN too, or switches when it has none.
    let mainPin = profile("main", pin: true, owner: true)
    #expect(ProfileRules.choice(mainPin, in: snapshot([mainPin, kid], locked: true)) == .pin)
    #expect(ProfileRules.choice(main, in: snapshot([main, kid], locked: true)) == .switchTo)
}

@Test func theKeypadsDeleteKeyOnlyDeletes() throws {
    // The web's rule: the key on screen deletes a digit and never leaves; the keyboard's Delete leaves once empty.
    #expect(KeypadDelete.of(digits: "12", onScreen: true) == .removeLast)
    #expect(KeypadDelete.of(digits: "", onScreen: true) == .nothing)
    #expect(KeypadDelete.of(digits: "1", onScreen: false) == .removeLast)
    #expect(KeypadDelete.of(digits: "", onScreen: false) == .leave)
    // The keypad's key and the keyboard's Delete each say which they are.
    let keypad = try String(contentsOf: URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee/Profiles/PinKeypad.swift"), encoding: .utf8)
    #expect(keypad.contains("{ name == \"delete\" ? delete(onScreen: true) : type(Character(name)) }"))
    #expect(keypad.contains(".onKeyPress(.delete, phases: .down) { _ in delete(onScreen: false); return .handled }"))
    #expect(try body(of: "private func delete(onScreen: Bool)", in: keypad).contains("KeypadDelete.of(digits: digits, onScreen: onScreen)"))
}
