import Foundation

/// What the core's `zap` answers (plans/zap.md 1.1): a title, the episode for a series, and where to start in it.
struct ZapPick: Decodable, Hashable, Sendable {
    let kind: String
    let itemId: String
    let videoId: String
    let season: Int?
    let episode: Int?
    let name: String
    let poster: String?
    let episodeName: String?
    let startMs: Int64
}

/// Zap's rules (plans/zap.md 2.2, 2.3), the web's and Android's in the same words.
enum ZapRules {
    /// A zap that ends here has not played this many times in a row: "Nothing played." instead of another.
    static let maxTries = 3

    /// What started a zap: a press (Zap, Zap again, the dial) or a play that failed before its first frame.
    enum Ask { case press, auto }

    /// The tries the next zap carries: 0 from every press, one more for an auto zap; nil once an auto zap would
    /// reach `maxTries`, where the quiet "Nothing played." shows instead.
    static func nextTries(_ ask: Ask, after tries: Int) -> Int? {
        guard ask == .auto else { return 0 }
        return tries + 1 < maxTries ? tries + 1 : nil
    }

    /// An auto zap counts only while a play is shown: one per play, however often its failure reports while the next
    /// zap is asked. A press always counts.
    static func counts(_ ask: Ask, playing: Bool) -> Bool { ask == .press || playing }

    /// The player's target for a pick: the real name rides along (the core saves it with progress once the hold
    /// lifts), the start as `zapAt`.
    static func target(_ pick: ZapPick, tries: Int) -> PlayTargetInfo {
        var target = PlayTargetInfo(
            kind: pick.kind, itemId: pick.itemId, videoId: pick.videoId, title: pick.name, poster: pick.poster,
            resumeMs: nil, season: pick.season, episode: pick.episode)
        target.zapAt = pick.startMs
        target.zapTries = tries
        return target
    }

    /// What is this?'s names from the target alone, for a zap play with no pick kept: its name and episode, no
    /// episode name.
    static func pick(_ target: PlayTargetInfo) -> ZapPick {
        ZapPick(
            kind: target.kind, itemId: target.itemId, videoId: target.videoId, season: target.season, episode: target.episode,
            name: target.title, poster: target.poster, episodeName: nil, startMs: target.zapAt ?? 0)
    }

    /// Start from the beginning: a plain play of the same video at 0, so progress counts at once.
    static func plain(_ target: PlayTargetInfo) -> PlayTargetInfo {
        var plain = PlayTargetInfo(
            kind: target.kind, itemId: target.itemId, videoId: target.videoId, title: target.title, poster: target.poster,
            resumeMs: nil, season: target.season, episode: target.episode)
        (plain.runtime, plain.releaseInfo, plain.released, plain.episodesInSeason) =
            (target.runtime, target.releaseInfo, target.released, target.episodesInSeason)
        return plain
    }

    /// Start from the beginning takes a zap whose pick has landed: before it, the pick would bring zap mode back.
    static func startsOver(zap: Bool, waitsForPick: Bool) -> Bool { zap && !waitsForPick }

    /// The title's name shows: an ordinary play, or a zap after What is this?.
    static func nameShown(zapAt: Int64?, revealed: Bool) -> Bool { zapAt == nil || revealed }

    /// A zap play that counts as one failed try: before the first frame, a failure, no pick or a view that did not
    /// open. The core's 20 s offer of another version is no try: a torrent's first frame often comes at 16 to 26 s
    /// (Pixel walk 2026-11-18, where zapping on at the offer left three starts in a row for "Nothing played."), and
    /// the core moves between versions itself until its start budget ends in a failure. After the first frame
    /// nothing zaps by itself.
    static func failed(firstFrame: Bool, status: PlayerStatus) -> Bool {
        guard !firstFrame else { return false }
        switch status {
        case .failed, .noPick, .cannotOpen: return true
        case .waiting, .none: return false
        }
    }

    /// A zap's failure card after its first frame: Zap again, Try again and Back, whatever the card would offer
    /// otherwise. Never Choose another version or the offer: release names carry the title.
    static let failureActions: [FailureAction] = [.zapAgain, .retry, .back]

    /// What a zap that answered no pick says: the no-source line, or "Nothing came up."; nil for any other failure,
    /// which the shared error words say.
    static func missLine(code: String?) -> String? {
        switch code {
        case "noAddons": VersionWords.emptyLine("noSource")
        case "nothingCameUp": "Nothing came up."
        default: nil
        }
    }

    /// A miss offers Zap again, except with no source to zap from, where another press finds none either.
    static func missOffersAgain(code: String?) -> Bool { code != "noAddons" }

    static func word(_ dial: ZapDial) -> String {
        switch dial {
        case .same: "Same"
        case .near: "Near"
        case .far: "Far"
        }
    }

    /// The dial's spoken labels.
    static func spoken(_ dial: ZapDial) -> String {
        switch dial {
        case .same: "Same: what you watch"
        case .near: "Near: titles like yours"
        case .far: "Far: what you never open"
        }
    }

    /// What is this?'s second line for an episode, "S2:E5 · Episode name"; nil for a film.
    static func episodeLine(season: Int?, episode: Int?, name: String?) -> String? {
        guard let season, let episode else { return nil }
        let code = "S\(season):E\(episode)"
        guard let name, !name.isEmpty else { return code }
        return "\(code) · \(name)"
    }
}
