import CryptoKit
import Foundation
import Testing

// Accounts phase 4 (plans/accounts.md 3.1, 3.2, 4.1 to 4.3, 5): the shared
// words and vectors, the rules the pages read, and the core's answers.

private let shared = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../../shared")

private func json(_ name: String) throws -> [String: Any] {
    let data = try Data(contentsOf: shared.appending(path: name))
    return try #require(try JSONSerialization.jsonObject(with: data) as? [String: Any])
}

private func member(_ id: String, kit: Bool = false, name: String? = nil, this: Bool = false, passkey: Bool = false,
                    form: String? = nil, lastUsedBy: String? = nil, lastUsedMs: Int64? = nil) -> MemberInfo {
    MemberInfo(id: id, kit: kit, name: name, sponsor: nil, addedMs: 0, thisDevice: this, passkey: passkey, form: form,
               lastUsedBy: lastUsedBy, lastUsedMs: lastUsedMs)
}

private func announcement(_ kind: String, member: String? = "m", by: String? = "b", name: String? = nil, kit: Bool = false, passkey: Bool = false) -> Announcement {
    Announcement(id: "1.\(member ?? "")", kind: kind, member: member, kit: kit, passkey: passkey, by: by, name: name, count: nil)
}

@Test func theAccountWordsReaderHoldsEveryKeyOfTheSharedFile() throws {
    let file = try json("account-words.json")
    // A key the file adds or drops fails here first: every platform reads them all.
    #expect(Set(file.keys).subtracting(["about"]) == AccountWords.keys)
    let words = AccountWords.shared
    #expect(words.continueWith == file["continueWith"] as? String)
    #expect(words.accountGoneActions == ["Sign in", "Remove from this device", "OK"])
    #expect(words.providerAccount["apple"] == "Apple Account")
    #expect(words.reportLostRemove.count == 2)
    // The Account page's words, shared with the web: read from the file, never written here.
    for (key, value) in [("signedInWith", words.signedInWith), ("linkAnother", words.linkAnother), ("unlink", words.unlink),
                         ("linkedOn", words.linkedOn), ("savePasskeyHere", words.savePasskeyHere), ("unlinkLast", words.unlinkLast),
                         ("removePasskeyAsk", words.removePasskeyAsk), ("switchProfile", words.switchProfile), ("wipeFailed", words.wipeFailed)] {
        #expect(value == file[key] as? String, "\(key)")
    }
    #expect(AccountRules.providerName("google") == "Google")
    #expect(AccountRules.providerName("mock") == "mock")
}

@Test func thePasskeyInputAndSaltAreTheVectorsFiles() throws {
    let vectors = try json("passkey-vectors.json")
    let input = try #require(vectors["prfInput"] as? [String: String])
    #expect(PasskeyContract.prfInputText == input["text"])
    #expect(PasskeyContract.prfInput.map { String(format: "%02x", $0) }.joined() == input["hex"])
    #expect(PasskeyContract.base64url(PasskeyContract.prfInput) == input["base64url"])
    #expect(PasskeyContract.prfSalt.map { String(format: "%02x", $0) }.joined() == input["salt"])
    // savePasskey and recover take the PRF output unpadded; the account id comes back as bytes.
    for kit in try #require(vectors["passkeyKit"] as? [[String: String]]) {
        let prf = try #require(kit["prf"])
        let bytes = try #require(PasskeyContract.userID(prf))
        #expect(bytes.count == 32)
        #expect(PasskeyContract.base64url(bytes) == prf)
    }
    #expect(PasskeyContract.challenge().count == 32)
    #expect(PasskeyContract.challenge() != PasskeyContract.challenge())
}

@Test func providersAreWordedAndAppleComesFirst() {
    #expect(AccountRules.orderedProviders(["google", "facebook", "apple"]) == ["apple", "google", "facebook"])
    #expect(AccountRules.orderedProviders(["mock", "facebook", "google"]) == ["google", "facebook", "mock"])
    #expect(AccountRules.orderedProviders([]).isEmpty)
    #expect(AccountRules.providerName("google") == "Google")
    #expect(AccountRules.providerName("mock") == "mock")
    #expect(AccountRules.providerAccount("facebook") == "Facebook account")
    #expect(AccountRules.providerAccount("mock") == "mock sign-in")
    #expect(AccountRules.fill(AccountRules.words.continueWith, ["provider": "Apple"]) == "Continue with Apple")
}

@Test func eachAnnouncementTakesItsOwnKey() {
    let members = [member("b", name: "Robin's iPhone"), member("u")]
    #expect(AccountRules.text(announcement("device", name: "Living room"), members: members) == "A new device joined: Living room.")
    #expect(AccountRules.text(announcement("device"), members: members) == "A new device joined.")
    #expect(AccountRules.text(announcement("passkey", kit: true, passkey: true), members: members) == "A passkey was added on Robin's iPhone.")
    #expect(AccountRules.text(announcement("kit", kit: true), members: members) == "A recovery kit was made on Robin's iPhone.")
    // Left by itself.
    #expect(AccountRules.text(announcement("removed", member: "m", by: "m", name: "Old Mac"), members: members) == "Old Mac signed out of your account.")
    #expect(AccountRules.text(announcement("removed", member: "m", by: "m"), members: members) == "A device signed out of your account.")
    // Removed by another member: passkey, kit, then a device by name or not.
    #expect(AccountRules.text(announcement("removed", kit: true, passkey: true), members: members) == "A passkey was removed from your account on Robin's iPhone.")
    #expect(AccountRules.text(announcement("removed", kit: true), members: members) == "A recovery kit was removed from your account on Robin's iPhone.")
    #expect(AccountRules.text(announcement("removed", name: "TV"), members: members) == "TV was removed from your account on Robin's iPhone.")
    #expect(AccountRules.text(announcement("removed"), members: members) == "A device was removed from your account on Robin's iPhone.")
    // The member that did it: unnamed, or gone from the account.
    #expect(AccountRules.text(announcement("kit", by: "u", kit: true), members: members) == "A recovery kit was made on a device with no name.")
    #expect(AccountRules.text(announcement("kit", by: "gone", kit: true), members: members) == "A recovery kit was made on a device no longer in your account.")
    #expect(AccountRules.text(announcement("dropped", member: nil, by: nil), members: members) == "Older changes were not kept. Check the Account page.")
}

@Test func securityNoticesAreTheOwnersToDismissAndHiddenFromKids() {
    for kind in ["device", "passkey", "kit", "dropped"] {
        let a = announcement(kind)
        #expect(AccountRules.showsOK(a, owner: true))
        #expect(!AccountRules.showsOK(a, owner: false), "\(kind): another profile shows it without OK")
        #expect(!AccountRules.shows(a, kids: true), "\(kind): a kids profile does not show it")
        #expect(AccountRules.shows(a, kids: false))
    }
    let removed = announcement("removed")
    #expect(AccountRules.showsOK(removed, owner: false))
    #expect(AccountRules.shows(removed, kids: true))
}

@Test func homesAccountCardGoesByPriority() {
    let deletion = ScheduledDeletion(dueMs: 1_800_000_000_000, provider: "google")
    let notice = announcement("device", name: "TV")
    #expect(AccountRules.homeCard(deletion: deletion, gone: true, announcements: [notice], secondWayIn: true, owner: true, kids: false) == .deletion(deletion))
    #expect(AccountRules.homeCard(deletion: nil, gone: true, announcements: [notice], secondWayIn: true, owner: true, kids: false) == .gone)
    #expect(AccountRules.homeCard(deletion: nil, gone: false, announcements: [notice], secondWayIn: true, owner: true, kids: false) == .announcement(notice))
    #expect(AccountRules.homeCard(deletion: nil, gone: false, announcements: [], secondWayIn: true, owner: true, kids: false) == .secondWayIn)
    #expect(AccountRules.homeCard(deletion: nil, gone: false, announcements: [], secondWayIn: false, owner: true, kids: false) == nil)
    // A kids profile sees no account card; the nudge leads to the owner's rows, so the owner only.
    #expect(AccountRules.homeCard(deletion: deletion, gone: true, announcements: [notice], secondWayIn: true, owner: false, kids: true) == nil)
    #expect(AccountRules.homeCard(deletion: nil, gone: false, announcements: [], secondWayIn: true, owner: false, kids: false) == nil)
    #expect(AccountRules.homeCard(deletion: deletion, gone: false, announcements: [], secondWayIn: true, owner: false, kids: false) == .deletion(deletion))
    // The web page's deletion names its provider; a Start over names none.
    #expect(AccountRules.deletionText(deletion).hasPrefix("Someone signed in with your Google account and asked to delete your Marquee account on "))
    #expect(AccountRules.deletionText(ScheduledDeletion(dueMs: 0, provider: nil)).hasPrefix("Your account will be deleted on "))
}

@Test func aSecondWayInIsAKitAnOpenedPasskeyOrAnotherPhone() {
    let me = member("me", this: true, form: "phone")
    #expect(!AccountRules.needsSecondWayIn([]), "nothing said before the list is read")
    #expect(AccountRules.needsSecondWayIn([me]))
    // A passkey saved but never used to open the account is not proven yet.
    #expect(AccountRules.needsSecondWayIn([me, member("p", kit: true, passkey: true)]))
    #expect(!AccountRules.needsSecondWayIn([me, member("p", kit: true, passkey: true, lastUsedBy: "x", lastUsedMs: 1)]))
    #expect(!AccountRules.needsSecondWayIn([me, member("k", kit: true)]), "a recovery kit")
    #expect(!AccountRules.needsSecondWayIn([me, member("ipad", form: "tablet")]))
    // A TV, a Mac or the web approves nothing: not a way in.
    #expect(AccountRules.needsSecondWayIn([me, member("tv", form: "tv"), member("mac", form: "computer")]))
}

@Test func eachSignInStateSaysItsWords() throws {
    #expect(AccountRules.trouble("cancelled") == nil, "a closed sheet says nothing")
    let unreachable = try #require(AccountRules.trouble("unreachable"))
    #expect(unreachable.text == "Marquee can't reach its sync service right now.")
    #expect(unreachable.actions == ["Try again", "Not now"])
    #expect(AccountRules.trouble("kitUnknown")?.actions == ["Use a recovery kit", "I can't use any of these"])
    #expect(AccountRules.trouble("signupLimit", provider: "apple")?.text == "This Apple Account has made several Marquee accounts lately. Try again later or use another way.")
    #expect(AccountRules.trouble("linkedElsewhere", provider: "google")?.text == "This Google account already signs in to another Marquee account.")
    #expect(AccountRules.trouble("linkedElsewhere", provider: "google")?.actions == ["Use it for this account instead", "Cancel"])
    #expect(AccountRules.trouble("ticketExpired")?.actions == ["Continue"])
    #expect(AccountRules.trouble("notOwner")?.text == "Only the main profile can do that.")
}

@Test func passkeyLabelsAndLastUsedSayTheDevice() {
    #expect(AccountRules.passkeyLabel(device: "Robin's iPhone") == "Passkey, saved on Robin's iPhone")
    let phone = member("phone", name: "Pixel")
    let kit = member("k", kit: true, passkey: true, lastUsedBy: "phone", lastUsedMs: 1_800_000_000_000)
    let line = AccountRules.lastUsed(kit, members: [phone, kit])
    #expect(line?.hasPrefix("Last used: Pixel, ") == true)
    #expect(AccountRules.lastUsed(member("k2", kit: true), members: []) == nil)
}

@Test func onlyTheLinkHostsPairPathOpensTheScanner() throws {
    let link = try #require(URL(string: "https://link.example/pair#v=3&s=x"))
    #expect(AccountRules.opensScanner(link, linkHost: "link.example"))
    #expect(!AccountRules.opensScanner(link, linkHost: "other.example"))
    #expect(!AccountRules.opensScanner(link, linkHost: nil))
    #expect(!AccountRules.opensScanner(try #require(URL(string: "https://link.example/delete")), linkHost: "link.example"))
    // The auth session's return: only marquee://auth, its fragment as it came.
    #expect(AccountRules.returnedFragment(try #require(URL(string: "marquee://auth#flow=f&code=c&relay=127.0.0.1"))) == "flow=f&code=c&relay=127.0.0.1")
    #expect(AccountRules.returnedFragment(try #require(URL(string: "marquee://pair#flow=f"))) == nil)
}

@Test func reportLostNamesTheCount() {
    #expect(AccountRules.reportLostButton(1) == "Remove 1 device")
    #expect(AccountRules.reportLostButton(4) == "Remove 4 devices")
}

@Test func aCodeForAnotherSyncServiceIsSaidSo() {
    let v3 = "https://link.example/pair#v=3&s=https%3A%2F%2Frelay.example&p=slot&k=AAAA&g=BBBB"
    let v2 = "marquee:pair?v=2&s=https%3A%2F%2Fother.example%3A8787&p=slot&k=AAAA&g=BBBB"
    #expect(AccountRules.codeServer(v3) == "https://relay.example")
    #expect(AccountRules.codeServer(v2) == "https://other.example:8787")
    #expect(AccountRules.codeServer("https://link.example/pair#v=3&p=slot") == nil)
    #expect(!AccountRules.isOtherRelayCode(v3, joined: "https://relay.example"))
    #expect(AccountRules.isOtherRelayCode(v2, joined: "https://relay.example"))
    // Nothing to compare with: the core decides.
    #expect(!AccountRules.isOtherRelayCode(v2, joined: nil))
}

@Test func aTVAlwaysRemovesItsLibraryOnSignOut() {
    #expect(AccountRules.removesLibrary(asked: false, tv: true))
    #expect(AccountRules.removesLibrary(asked: true, tv: false))
    #expect(!AccountRules.removesLibrary(asked: false, tv: false))
}

@Test func theCoresAccountAnswersDecode() throws {
    let base = #"{"pushed":0,"stale":0,"refused":0,"waiting":0,"pulled":0,"applied":0,"unreadable":0,"newer":0,"parked":0,"oldEpochWrites":0,"clockGapMs":0,"rotationWaitMs":0,"blocked":null,"fingerprint":"F","record":null,"again":false"#
    let extras = #","deletion":{"dueMs":5,"provider":"google"},"announcements":[{"id":"7.m","kind":"passkey","member":"m","kit":true,"passkey":true,"by":"b","name":"Passkey, saved on X","count":null}]"#
    let decoded = try JSONDecoder().decode(SyncReport.self, from: Data((base + extras + "}").utf8))
    #expect(decoded.deletion == ScheduledDeletion(dueMs: 5, provider: "google"))
    #expect(decoded.announcements.map(\.id) == ["7.m"])
    #expect(decoded.without(["7.m"]).announcements.isEmpty)
    // An older core: no deletion, no announcements.
    let plain = try JSONDecoder().decode(SyncReport.self, from: Data((base + "}").utf8))
    #expect(plain.deletion == nil && plain.announcements.isEmpty)

    let member = try JSONDecoder().decode(MemberInfo.self, from: Data(#"{"id":"k","kit":true,"name":"Passkey, saved on X","sponsor":"a","addedMs":1,"thisDevice":false,"passkey":true,"form":null,"viaPasskey":false,"lastUsedBy":"p","lastUsedMs":9}"#.utf8))
    #expect(member.passkey && member.lastUsedBy == "p" && member.lastUsedMs == 9)
    let older = try JSONDecoder().decode(MemberInfo.self, from: Data(#"{"id":"d","kit":false,"name":null,"sponsor":null,"addedMs":1,"thisDevice":true}"#.utf8))
    #expect(!older.passkey && older.form == nil)

    let signIn: [(String, SignInSnapshot)] = [
        (#"{"phase":"browser","url":"https://r/x","expiresMs":3}"#, .browser(url: "https://r/x", expiresMs: 3)),
        (#"{"phase":"found","ticket":"t","account":"a"}"#, .found(ticket: "t", account: "a")),
        (#"{"phase":"new","ticket":"t"}"#, .new(ticket: "t")),
        (#"{"phase":"receipt","receipt":"r","ticket":"t","account":null}"#, .receipt(receipt: "r", ticket: "t", account: nil)),
        (#"{"phase":"failed","code":"cancelled"}"#, .failed(code: "cancelled")),
        (#"{"phase":"later"}"#, .waiting),
    ]
    for (text, expected) in signIn {
        #expect(try JSONDecoder().decode(SignInSnapshot.self, from: Data(text.utf8)) == expected)
    }
    let preview = try JSONDecoder().decode(LostPreview.self, from: Data(#"{"devices":["a"],"passkeyJoined":["a"],"kits":[],"deviceKits":[]}"#.utf8))
    #expect(preview.passkeyJoined == ["a"])
}

// Sign out with the wipe (D13), steps 2 and 3 on a real core: each profile's
// Trakt record goes first; the core holds the removal (`wipePending`) from
// before the leave until the wipe, so a stopped run is finished after a restart.
@Suite(.serialized) @MainActor struct AccountWipeTests {
    /// A core with the app's model and its profiles view.
    private func device() async throws -> (SyncModel, ProfileSession, RealCore) {
        let core = RealCore()
        #expect(await core.ready())
        let sync = SyncModel(core: core.client, records: FakeKeychain().record,
                             defaults: try #require(UserDefaults(suiteName: "mq-test-\(UUID().uuidString)")))
        let session = ProfileSession(core: core.client, sync: sync)
        #expect(await eventually { session.snapshot != nil })
        return (sync, session, core)
    }

    @Test func removeHereSignsEachProfileOutOfTraktThenWipes() async throws {
        let (sync, session, core) = try await device()
        try await session.perform("createProfile", ["name": "Kid", "avatar": ProfileRules.formatAvatar(glyph: "cat", color: "teal")])
        #expect(await eventually { session.snapshot?.profiles.count == 2 })
        let profiles = try #require(session.snapshot?.profiles.map(\.id))
        var gone: [String] = []
        let wipe = AccountWipe(core: core.client, sync: sync, wipePending: { session.snapshot?.wipePending },
                               traktGone: { gone.append($0) }, debridAccounts: { [] }, disconnect: { _ in })
        // Remove from this device asks the OS prompt first, as a sign-out does.
        var asked: [String] = []
        sync.ownerPrompt = { asked.append($0); return true }

        try await wipe.removeHere(profiles: profiles, disconnectDebrid: false)
        #expect(asked == [SyncWords.authRemoveHere])
        #expect(gone == profiles)
        #expect(wipe.problem == nil)
        #expect(await eventually { session.snapshot?.profiles.map(\.id) == ["default"] && session.snapshot?.wipePending == nil })
    }

    @Test func aStoppedRemovalIsHeldByTheCoreAndAResumeFinishesIt() async throws {
        let (sync, session, core) = try await device()
        sync.ownerPrompt = { _ in true }
        var fails = true
        var disconnected: [String] = []
        let account = DebridAccount(service: "realdebrid", generation: "g1", state: "ok")
        func model() -> AccountWipe {
            AccountWipe(core: core.client, sync: sync, wipePending: { session.snapshot?.wipePending }, traktGone: { _ in },
                        debridAccounts: { [account] }, disconnect: {
                            if fails { throw CoreError("offline", code: "unreachable") }
                            disconnected.append($0.generation)
                        })
        }
        let first = model()
        // Nothing pending: a resume does nothing.
        await first.resume(profiles: ["default"])
        #expect(disconnected.isEmpty && !first.pending)

        await #expect(throws: CoreError.self) { try await first.removeHere(profiles: ["default"], disconnectDebrid: true, askedAgain: true) }
        #expect(first.problem != nil)
        // The core holds the removal, with its debrid choice.
        #expect(await eventually { session.snapshot?.wipePending == WipePending(debrid: true) })
        // A restart: a new model finishes it from the core's flag.
        fails = false
        let second = model()
        await second.resume(profiles: ["default"])
        #expect(disconnected == ["g1"] && second.problem == nil)
        #expect(await eventually { session.snapshot?.wipePending == nil })
    }

    @Test func aDeclinedPromptRemovesNothing() async throws {
        let (sync, session, core) = try await device()
        var gone: [String] = []
        let wipe = AccountWipe(core: core.client, sync: sync, wipePending: { session.snapshot?.wipePending },
                               traktGone: { gone.append($0) }, debridAccounts: { [] }, disconnect: { _ in })
        sync.ownerPrompt = { _ in false }
        // With a lock the prompt says no; without one the page asks a second time first.
        await #expect { try await wipe.removeHere(profiles: ["default"], disconnectDebrid: true) } throws: {
            ["unconfirmed", "noLockAsk"].contains(($0 as? CoreError)?.code)
        }
        #expect(gone.isEmpty && !wipe.pending)
        withExtendedLifetime(core) {}
    }
}
