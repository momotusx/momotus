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
    let menu = try #require(picker.range(of: "struct ProfileMenu<Face: View>")).lowerBound
    let choose = try body(of: "private func choose(_ profile: ProfileInfo, _ snapshot: ProfilesSnapshot)", in: String(picker[menu...]))
    #expect(choose.contains("ProfileRules.choice(profile, in: snapshot)"), "the menu left the shared rule")
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
    for account in ["AccountWords", "signInAnother", "requestScan", "Sign in"] {
        #expect(!menu.contains(account), "the profile menu holds an account action: \(account)")
    }
    #expect(menu.contains("Button(\"Settings\", glyph: \"settings\", action: onSettings)"))
}
