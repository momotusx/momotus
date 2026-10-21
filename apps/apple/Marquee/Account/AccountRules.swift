import Foundation

/// The account's rules in words and choices (plans/accounts.md 3.2, 4.1 to
/// 4.3, 5), pure so the tests read them without a core.
enum AccountRules {
    static var words: AccountWords { .shared }

    // MARK: Words

    /// A template's `{key}` places, filled.
    static func fill(_ template: String, _ values: [String: String]) -> String {
        values.reduce(template) { text, pair in text.replacingOccurrences(of: "{\(pair.key)}", with: pair.value) }
    }

    /// The medium date in the device's locale, as the words' `{date}`.
    static func date(_ ms: Int64, locale: Locale = .current) -> String {
        Date(timeIntervalSince1970: TimeInterval(ms) / 1000).formatted(Date.FormatStyle(date: .abbreviated, time: .omitted).locale(locale))
    }

    // MARK: Providers

    /// The relay's provider ids worded: Apple, Google and Facebook by name,
    /// any other (an OpenID entry) as the sync server gives it.
    static func providerName(_ id: String) -> String {
        switch id {
        case "apple": "Apple"
        case "google": "Google"
        case "facebook": "Facebook"
        default: id
        }
    }

    /// "Apple Account", "Google account", "Facebook account", else "<id> sign-in".
    static func providerAccount(_ id: String) -> String {
        words.providerAccount[id] ?? fill(words.providerAccountOther, ["provider": providerName(id)])
    }

    /// The sign-in buttons in order: Apple first on Apple platforms, then
    /// Google, Facebook, then any other the relay lists, in its order.
    static func orderedProviders(_ ids: [String]) -> [String] {
        let known = ["apple", "google", "facebook"]
        return known.filter(ids.contains) + ids.filter { !known.contains($0) }
    }

    // MARK: Announcements

    /// The security notices: the owner profile's to dismiss, and never on a kids profile.
    static func isSecurityNotice(_ kind: String) -> Bool {
        ["device", "passkey", "kit", "dropped"].contains(kind)
    }

    /// A kids profile shows no security notice; every other profile shows them all.
    static func shows(_ announcement: Announcement, kids: Bool) -> Bool {
        !(kids && isSecurityNotice(announcement.kind))
    }

    /// OK on a security notice is the owner's only (the core answers
    /// `notOwner` otherwise); a removal shows OK on every profile.
    static func showsOK(_ announcement: Announcement, owner: Bool) -> Bool {
        owner || !isSecurityNotice(announcement.kind)
    }

    /// One announcement in words, each kind its own key (account-words.json "about").
    static func text(_ a: Announcement, members: [MemberInfo]) -> String {
        let name = a.name ?? ""
        switch a.kind {
        case "device":
            return a.name == nil ? words.announceDeviceUnnamed : fill(words.announceDevice, ["name": name])
        case "passkey":
            return fill(words.announcePasskey, ["device": byName(a.by, members)])
        case "kit":
            return fill(words.announceKit, ["device": byName(a.by, members)])
        case "removed":
            if a.by != nil, a.by == a.member {
                return a.name == nil ? words.announceLeftUnnamed : fill(words.announceLeft, ["name": name])
            }
            let device = byName(a.by, members)
            if a.passkey { return fill(words.announceRemovedPasskey, ["device": device]) }
            if a.kit { return fill(words.announceRemovedKit, ["device": device]) }
            return a.name == nil
                ? fill(words.announceRemovedUnnamed, ["device": device])
                : fill(words.announceRemoved, ["name": name, "device": device])
        default:
            return words.announceDropped
        }
    }

    /// The member that made the change, by its name now: unnamed or no longer in the account, in words.
    static func byName(_ id: String?, _ members: [MemberInfo]) -> String {
        guard let id, let member = members.first(where: { $0.id == id }) else { return words.announceGoneDevice }
        return member.name ?? words.announceUnnamedDevice
    }

    // MARK: Home's account card

    /// Home's one account card, by priority (plans/accounts.md 4.3).
    enum HomeCard: Equatable {
        case deletion(ScheduledDeletion)
        case gone
        case announcement(Announcement)
        case secondWayIn
    }

    static func homeCard(deletion: ScheduledDeletion?, gone: Bool, announcements: [Announcement], secondWayIn: Bool) -> HomeCard? {
        if let deletion { return .deletion(deletion) }
        if gone { return .gone }
        if let first = announcements.first { return .announcement(first) }
        return secondWayIn ? .secondWayIn : nil
    }

    /// "Your account will be deleted on <date>.", or the web page's ask naming its provider.
    static func deletionText(_ deletion: ScheduledDeletion) -> String {
        let date = date(deletion.dueMs)
        guard let provider = deletion.provider else { return fill(words.deletionScheduled, ["date": date]) }
        return fill(words.deletionScheduledBy, ["provider": providerName(provider), "date": date])
    }

    /// Until a second way in exists (4.1.7): a recovery kit, a passkey that
    /// opened the account on a device, or another phone or tablet, which are
    /// the devices that approve. Before the list is read, nothing is said.
    static func needsSecondWayIn(_ members: [MemberInfo]) -> Bool {
        guard !members.isEmpty else { return false }
        let recoveryKit = members.contains { $0.kit && !$0.passkey }
        let openedPasskey = members.contains { $0.kit && $0.passkey && $0.lastUsedBy != nil }
        let approver = members.contains { !$0.kit && !$0.thisDevice && ($0.form == "phone" || $0.form == "tablet") }
        return !(recoveryKit || openedPasskey || approver)
    }

    // MARK: Sign-in states (plans/accounts.md 5)

    /// What a failed sign-in says, and its buttons in order. A cancelled
    /// provider sheet says nothing: back on the sign-in screen.
    struct Trouble: Equatable {
        let text: String
        let actions: [String]
    }

    static func trouble(_ code: String, provider: String? = nil) -> Trouble? {
        let account = providerAccount(provider ?? "")
        switch code {
        case "cancelled": return nil
        case "unreachable": return Trouble(text: words.unreachable, actions: words.unreachableActions)
        case "noRedirect": return Trouble(text: words.noRedirect, actions: words.noRedirectActions)
        case "passkeyCancelled": return Trouble(text: words.passkeyCancelled, actions: words.passkeyCancelledActions)
        case "kitUnknown": return Trouble(text: words.kitUnknownPasskey, actions: words.kitUnknownPasskeyActions)
        case "ticketExpired": return Trouble(text: words.ticketExpired, actions: words.ticketExpiredActions)
        case "signupLimit": return Trouble(text: fill(words.signupLimit, ["providerAccount": account]), actions: words.signupLimitActions)
        case "linkedElsewhere": return Trouble(text: fill(words.linkedElsewhere, ["providerAccount": account]), actions: words.linkedElsewhereActions)
        case "busy": return Trouble(text: words.busy, actions: [])
        case "otherRelay": return Trouble(text: words.otherRelay, actions: [])
        case "notOwner": return Trouble(text: words.notOwner, actions: [])
        case "kitHere": return Trouble(text: words.kitHere, actions: [])
        case "kitDropped": return Trouble(text: words.kitDropped, actions: [])
        case "tooManyKits": return Trouble(text: words.tooManyKits, actions: [])
        case "kitTaken": return Trouble(text: words.kitTaken, actions: [])
        case "plainLink": return Trouble(text: words.plainLink, actions: [])
        case "debridConnected": return Trouble(text: words.debridConnected, actions: [])
        case "traktConnected": return Trouble(text: words.traktConnected, actions: [])
        default: return nil
        }
    }

    // MARK: Passkeys and links

    /// The label `savePasskey` writes, the same on every platform: no date.
    static func passkeyLabel(device: String) -> String {
        fill(words.passkeyLabel, ["device": device])
    }

    /// A kit's row line: "Last used: <device>, <date>", that device's name when it is still a member.
    static func lastUsed(_ kit: MemberInfo, members: [MemberInfo]) -> String? {
        guard let by = kit.lastUsedBy, let ms = kit.lastUsedMs else { return nil }
        return fill(words.lastUsed, ["device": byName(by, members), "date": date(ms)])
    }

    /// The universal link a phone camera opens: `https://<link host>/pair`.
    /// It only opens the scanner; its fragment is never read (D6).
    static func opensScanner(_ url: URL, linkHost: String?) -> Bool {
        guard let linkHost, !linkHost.isEmpty else { return false }
        return url.scheme == "https" && url.host() == linkHost && url.path() == "/pair"
    }

    /// A brokered sign-in's return (`marquee://auth#...`): its fragment, for
    /// the `signIn` view's `returned` command. Only the auth session's own
    /// callback is passed here, never a link opened from elsewhere.
    static func returnedFragment(_ url: URL) -> String? {
        guard url.scheme == "marquee", url.host() == "auth" else { return nil }
        return url.fragment(percentEncoded: true)
    }

    /// Report lost's button names the count: "Remove 1 device", "Remove 4 devices".
    static func reportLostButton(_ count: Int) -> String {
        fill(count == 1 ? words.reportLostRemove[0] : words.reportLostRemove[1], ["n": "\(count)"])
    }

    // MARK: Sign out (D13)

    /// "Remove your profiles and library from this device?": on by default,
    /// always on for a TV, and offered on the owner profile only.
    static func removesLibrary(asked: Bool, tv: Bool) -> Bool {
        tv || asked
    }
}
