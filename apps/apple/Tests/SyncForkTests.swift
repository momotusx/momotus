import Foundation
import Testing

// The compare page after a different device list (forkPreview), They differ,
// leaving here only, and the failure words of the new steps.

private typealias W = SyncWords

private func member(_ id: String, name: String? = nil, kit: Bool = false, sponsor: String? = nil, addedMs: Int64 = 0, this: Bool = false) -> MemberInfo {
    MemberInfo(id: id, kit: kit, name: name, sponsor: sponsor, addedMs: addedMs, thisDevice: this)
}

/// The server's side: this phone, a tablet that made kit k1, a TV that made k2
/// and k3 (k3 goes anyway), and k9, a kit only the server knows (it goes anyway).
private let preview = ForkPreview(
    mine: [member("me", name: "Phone", this: true), member("old", name: "Old laptop", sponsor: "me")],
    server: [
        member("me", name: "Phone", this: true), member("tab", name: "Tablet", sponsor: "me", addedMs: 1),
        member("k1", kit: true, sponsor: "tab", addedMs: 2), member("tv", name: "TV", sponsor: "me", addedMs: 3),
        member("k2", kit: true, sponsor: "tv", addedMs: 4), member("k3", kit: true, sponsor: "tv", addedMs: 5),
        member("k9", kit: true, addedMs: 6),
    ],
    drops: ["k3", "k9"], kits: [DeviceKits(device: "tab", kits: ["k1"]), DeviceKits(device: "tv", kits: ["k2", "k3"])],
    onServer: true, head: "abc123")

@Suite struct SyncForkTests {
    @Test func aPreviewDecodesAsTheCoreSendsIt() throws {
        let json = #"""
        {"mine": [{"id": "a", "kit": false, "name": "Phone", "sponsor": null, "addedMs": 1, "thisDevice": true}],
         "server": [{"id": "a", "kit": false, "name": "Phone", "sponsor": null, "addedMs": 1, "thisDevice": true},
                    {"id": "b", "kit": false, "name": "TV", "sponsor": "a", "addedMs": 2, "thisDevice": false}],
         "drops": [], "kits": [{"device": "b", "kits": ["k"]}], "onServer": false, "head": "f00d"}
        """#
        let decoded = try JSONDecoder().decode(ForkPreview.self, from: Data(json.utf8))
        #expect(decoded.server.count == 2 && !decoded.onServer && decoded.head == "f00d")
        #expect(decoded.kits == [DeviceKits(device: "b", kits: ["k"])])
    }

    @Test func theSidesAreWhatOnlyOneListHas() {
        let sides = SyncRules.forkSides(mine: preview.mine, server: preview.server)
        #expect(sides.mineOnly.map(\.id) == ["old"])
        #expect(sides.serverOnly.map(\.id) == ["tab", "k1", "tv", "k2", "k3", "k9"])
    }

    @Test func continuingDropsOnlyTicksAndNeverThisDevice() {
        #expect(SyncRules.forkDrop(server: preview.server, drops: preview.drops, ticked: []).isEmpty)
        #expect(SyncRules.forkDrop(server: preview.server, drops: preview.drops, ticked: ["tv", "me", "k9", "nobody"]) == ["tv"])
    }

    @Test func aTickedDevicesKitsAreNamedAndCounted() {
        // The TV's own kits that go with it: k2 (k3 goes anyway).
        #expect(SyncRules.forkKits(of: "tv", in: preview) == ["k2"])
        #expect(SyncRules.forkKits(of: "me", in: preview).isEmpty)
        let none = SyncRules.forkCounts(preview, ticked: [])
        #expect(none.devices == 0 && none.kits == 0)
        #expect(W.forkButton(devices: 0, kits: 0) == "Continue with the server's list")
        let tv = SyncRules.forkCounts(preview, ticked: ["tv"])
        #expect(tv.devices == 1 && tv.kits == 1)
        #expect(W.forkButton(devices: tv.devices, kits: tv.kits) == "Continue and remove 1 device and 1 recovery kit")
        // A kit ticked on its own and the kits of a ticked device count once each.
        let both = SyncRules.forkCounts(preview, ticked: ["tv", "tab", "k1"])
        #expect(both.devices == 2 && both.kits == 2)
        #expect(W.goesWith(1) == "Its recovery kit goes with it.")
        #expect(W.goesWith(2) == "Its 2 recovery kits go with it.")
    }

    @Test func eachServerRowSaysWhetherTheUserDecides() {
        let row = { (id: String, ticked: Set<String>) in SyncRules.forkRow(preview.server.first { $0.id == id }!, in: preview, ticked: ticked) }
        #expect(row("me", []) == .thisDevice)
        #expect(row("k9", []) == .goes)
        #expect(row("tv", []) == .tickable)
        #expect(row("k2", []) == .tickable)
        // Its maker ticked: the kit goes with it and has no box of its own.
        #expect(row("k2", ["tv"]) == .withTicked("TV"))
    }

    @Test func theNewStepsAreWordedForWhatDidNotHappen() {
        let words = { (code: String?, message: String, action: SyncAction) in SyncProblem(code: code, message: message, action: action).words }
        #expect(words("rosterChanged", "", .serverList) == W.forkChanged)
        #expect(words("wrongRecord", "", .leave) == W.wrongRecord)
        #expect(words(nil, "http status 401", .compare) == W.forkTokenLost)
        #expect(words(nil, "http status 401", .serverList).hasPrefix("Nothing changed."))
        #expect(words("unconfirmed", "", .differ).hasPrefix("The approval is still open"))
        #expect(words("unconfirmed", "", .remove).hasPrefix("Nothing changed"))
        #expect(words(nil, "network: x", .differ).hasPrefix("The approval is still open."))
        #expect(words(nil, "network: x", .approve).hasPrefix("Nothing was approved."))
        #expect(words(nil, "network: x", .kit).hasPrefix("No kit was made."))
        #expect(words(nil, "network: x", .leave).hasPrefix("This device still syncs."))
        #expect(words(nil, "network: x", .compare).hasPrefix("The lists could not be compared."))
        // The core's own sentence for a code of a device the account already has.
        #expect(words(nil, "pairing: this device is already in the account", .approve) == "This device is already in the account.")
    }

    @Test func leavingSaysWhatIsLost() {
        #expect(W.leaveHereAsk == "Stop syncing here only?")
        #expect(W.leaveHereText.contains("lists it until another device removes it"))
        #expect(W.leftHere.contains("still lists it"))
        #expect(W.differEnded.contains("a device that already did is removed"))
    }
}
