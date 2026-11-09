import Foundation
import Testing

// First run's order (plans/first-screens.md 2, 4.3, 9) against the shared vectors file, all three parts: what shows
// next, the screens each path shows before Home through the rule's own step sequence, and the setup's subtitle chips.
// Presses are counted on the device by MarqueeWalkUITests (10); here each press is what it tells the rule.

private func rows(_ part: String) throws -> [[String: Any]] {
    let url = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../../shared/fixtures/first-run-vectors.json")
    let vectors = try #require(try JSONSerialization.jsonObject(with: Data(contentsOf: url)) as? [String: Any])
    return try #require(vectors[part] as? [[String: Any]], "the vectors file has no \(part)")
}

@Test func everyEntryRowShowsTheStepTheVectorsName() throws {
    let entries = try rows("entry")
    #expect(entries.count >= 20)
    for row in entries {
        let after = try #require(FirstRunRules.After(rawValue: row["after"] as? String ?? ""), "\(row)")
        let entry = FirstRunRules.Entry(
            fresh: try #require(row["fresh"] as? Bool), kidsSession: try #require(row["kidsSession"] as? Bool),
            server: try #require(row["server"] as? Bool), inAccount: try #require(row["inAccount"] as? Bool),
            profileStepDone: try #require(row["profileStepDone"] as? Bool), profiles: try #require(row["profiles"] as? Int),
            pin: try #require(row["pin"] as? Bool), locked: try #require(row["locked"] as? Bool),
            after: after)
        #expect(FirstRunRules.next(entry).rawValue == row["step"] as? String, "\(row)")
    }
}

/// The screens a path shows on one form, from Welcome, each press told to the rule as what it means.
private func walk(_ path: String, presses: [String]) -> [String] {
    var entry = FirstRunRules.Entry(fresh: true, server: path != "noServer", inAccount: false, profileStepDone: false, after: .start)
    var screens = [FirstRunRules.next(entry)]
    var bring = false
    var joined = false
    // The other device approves (a QR, Use another device): no press here, the join's rule decides.
    func join() {
        joined = true
        entry.after = .joined
        entry.inAccount = true
        entry.fresh = false
        entry.profiles = path == "joinManyProfilesOrPin" ? 2 : 1
        screens.append(FirstRunRules.next(entry))
    }
    for press in presses {
        switch press {
        case "getStarted": entry.after = .getStarted
        case "alreadyUse": entry.after = .alreadyUse
        case "notNow": entry.after = .notNow
        case "continueWithGoogle":
            entry.inAccount = true
            entry.after = .created
        case "continue":
            entry.profileStepDone = true
            entry.after = .profileDone
        case "startWatching":
            screens.append(FirstRunRules.afterSetup(bring: bring))
            continue
        case "bringLibrary":
            bring = true
            continue
        case "useAnotherDevice": continue
        case "profileTile":
            if !joined { join() }
            screens.append(.home)
            continue
        default:
            Issue.record("a press the rule does not know: \(press)")
            continue
        }
        screens.append(FirstRunRules.next(entry))
    }
    if path.hasPrefix("join"), !joined { join() }
    return screens.filter { $0 != .home }.map(\.rawValue)
}

@Test func everyPathShowsTheVectorsScreensOnEveryForm() throws {
    let paths = try rows("paths")
    #expect(paths.count == 6)
    for row in paths {
        let path = try #require(row["path"] as? String)
        // The web's recovery kit is the web's own.
        for form in ["phone", "wide", "tv"] {
            guard let expected = row[form] as? [String: Any] else { continue }
            let presses = try #require(expected["presses"] as? [String])
            let screens = try #require(expected["screens"] as? [String])
            #expect(walk(path, presses: presses) == screens, "\(path) on \(form)")
            #expect(presses.count <= 5, "\(path) on \(form) takes \(presses.count) presses")
        }
    }
}

@Test func theSubtitleChipsAreTheVectorsChipsInOrder() throws {
    let chips = try rows("chips")
    #expect(chips.count == 5)
    for row in chips {
        let made = FirstRunRules.chips(device: row["device"] as? String, chosen: try #require(row["chosen"] as? [String]))
        #expect(made.chips == row["chips"] as? [String], "\(row)")
        #expect(made.selected == row["selected"] as? String, "\(row)")
    }
}

@Test func backFollowsThePlansTable() {
    #expect(FirstRunRules.back(from: .welcome, server: true, made: false) == nil, "Welcome leaves the app")
    #expect(FirstRunRules.back(from: .login, server: true, made: false) == .welcome)
    #expect(FirstRunRules.back(from: .profile, server: true, made: false) == .login, "Not now: back to Login")
    #expect(FirstRunRules.back(from: .profile, server: false, made: false) == .welcome, "no server: back to Welcome")
    #expect(FirstRunRules.back(from: .profile, server: true, made: true) == nil, "an account made here: the form is where a restart resumes")
    #expect(FirstRunRules.back(from: .setup, server: true, made: true) == .profile)
    #expect(FirstRunRules.back(from: .importing, server: true, made: false) == .setup)
    #expect(FirstRunRules.back(from: .who, server: true, made: false) == nil, "the start picker leaves the app")
}
