import Foundation
import Testing

// The profile in Settings' place (plans/nav-profile.md). The menu and the bars are not laid out in a hosted view (a
// Menu, a toolbar, the tvOS bar), so these pin the lines that wire them to the shared rules.

private func source(_ path: String) throws -> String {
    let app = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee")
    return try String(contentsOf: app.appending(path: path), encoding: .utf8)
}

@Test func thePickerAndTheMenuChooseByTheSameRule() throws {
    let picker = try source("Profiles/ProfilePicker.swift")
    let pick = try body(of: "private func pick(_ profile: ProfileInfo, _ snapshot: ProfilesSnapshot)", in: picker)
    #expect(pick.contains("ProfileRules.choice(profile, in: snapshot)"), "the picker's tile left the shared rule")
    // The bar's menu and the Mac's panel choose through one function on the session.
    let shared = try #require(picker.range(of: "extension ProfileSession {")).lowerBound
    let choose = try body(of: "func choose(_ profile: ProfileInfo, _ snapshot: ProfilesSnapshot, onPin: (ProfileInfo) -> Void)", in: String(picker[shared...]))
    #expect(choose.contains("ProfileRules.choice(profile, in: snapshot)"), "the menu left the shared rule")
    let menu = try body(of: "struct ProfileMenu<Face: View>: View", in: picker)
    #expect(menu.contains("profiles.choose(profile, snapshot, onPin: onPin)"), "the menu chooses by its own rule")
    let panel = try source("Profiles/ProfilePanel.swift")
    #expect(panel.contains("profiles.choose(profile, snapshot, onPin: onPin)"), "the panel chooses by its own rule")
    #expect(panel.contains("profiles.chooseIncognito(snapshot, onPin: onPin)"), "the panel's Incognito row left the shared rule")
}

@Test func theMacAvatarOpensThePanelAndOnlyIOSKeepsTheMenu() throws {
    // plans/profile-panel.md: the Mac's avatar toggles the panel; iOS keeps its Menu, tvOS its dialog.
    let menu = try body(of: "struct ProfileMenu<Face: View>: View", in: source("Profiles/ProfilePicker.swift"))
    let mac = try #require(menu.range(of: "#elseif os(macOS)\n            // No menu"))
    let rest = try #require(menu.range(of: "#else", range: mac.upperBound..<menu.endIndex))
    let macBranch = String(menu[mac.upperBound..<rest.lowerBound])
    #expect(macBranch.contains("Button { panel.wrappedValue.toggle() } label: { face(me) }"))
    #expect(!macBranch.contains("Menu {"), "the Mac's avatar builds a menu again")
    #expect(menu[rest.upperBound...].contains("Menu {"), "iOS lost its menu")
    #expect(menu.contains(".confirmationDialog(\"Profiles\", isPresented: $open"), "tvOS lost its dialog")
    // The shell: the panel over the places, closed by a place, Search, the player and the picker's sheet.
    let root = try source("App/RootView.swift")
    #expect(root.contains(".overlay { ProfilePanelLayer(open: $showPanel, onPin: pin, onManage: manage, onSettings: { select(.settings, link: true) }) }"))
    #expect(root.contains(".onChange(of: immersion.active) { _, on in if on { showPanel = false } }"))
    let select = try body(of: "private func select(_ asked: AppTab, link: Bool = false)", in: root)
    #expect(select.contains("showPanel = false"), "a place leaves the panel open")
    let pin = try body(of: "private func pin(_ profile: ProfileInfo)", in: root)
    #expect(pin.contains("showPanel = false"), "the keypad's sheet opens under the panel")
    #expect(root.contains("TopTools(find: find, shown: !immersion.active, covered: showPanel)"), "the toolbar stays in VoiceOver's reach under the panel")
    // Escape closes it wherever focus is.
    let panel = try source("Profiles/ProfilePanel.swift")
    #expect(panel.contains(".keyboardShortcut(.cancelAction)"))
}

@Test func tvFocusForSettingsGoesToTheProfileEverywhereTheBarTakesIt() throws {
    let root = try source("App/RootView.swift")
    #expect(root.contains("case .toBar: return { if let link = tab.barFocus { navFocus = link } else { avatarFocus = true } }"))
    #expect(root.contains(".defaultFocus(focused, selection.barFocus, priority: .userInitiated)"))
    #expect(root.contains(".defaultFocus(avatar, selection.barFocus == nil, priority: .userInitiated)"))
    #expect(root.contains("if let link = now.barFocus { focused.wrappedValue = link } else { avatar.wrappedValue = true }"))
    #expect(root.contains("ProfileMenu(onSettings: settings, onPin: pin, focus: avatar)"), "the avatar's focus left the button")
}

@Test func theMenuHoldsTheProfilesThenSettingsAndNothingAccountLike() throws {
    // The owner's rule (plans/owner-picks.md, 2026-10-29): account actions, Sign in another device among them, stay
    // inside Settings.
    let picker = try source("Profiles/ProfilePicker.swift")
    let start = try #require(picker.range(of: "struct ProfileMenu<Face: View>")).lowerBound
    let end = try #require(picker.range(of: "struct BarAvatar")).lowerBound
    let menu = String(picker[start..<end])
    let panel = try source("Profiles/ProfilePanel.swift")
    for account in ["AccountWords", "signInAnother", "requestScan", "Sign in"] {
        #expect(!menu.contains(account), "the profile menu holds an account action: \(account)")
        #expect(!panel.contains(account), "the profile panel holds an account action: \(account)")
    }
    #expect(menu.contains("Button(\"Settings\", glyph: \"settings\", action: onSettings)"))
}

@Test func theBarsAvatarIsAFillWithNoRestingEdge() throws {
    // DECISION: fills, not edges. The Mac ornament's, the iPhone bar's and the TV bar's avatar keep only the lit ring.
    let picker = try source("Profiles/ProfilePicker.swift")
    let avatar = try body(of: "var body: some View", in: try body(of: "struct BarAvatar: View", in: picker))
    #expect(avatar.contains(".overlay { if lit { Circle().strokeBorder(.white.opacity(0.92), lineWidth: 2) } }"))
    #expect(avatar.components(separatedBy: "strokeBorder").count - 1 == 1, "the avatar has an edge outside the lit ring")
}
