import Foundation

/// The `plugins` answer (plans/plugins-and-marketplace.md 7.7): the profile's Nuvio repositories.
struct PluginsAnswer: Decodable, Hashable, Sendable {
    /// Plugins run in this build and for this profile (not a kids profile).
    let available: Bool
    let repos: [PluginRepoInfo]
}

struct PluginRepoInfo: Decodable, Hashable, Sendable, Identifiable {
    let url: String
    let host: String
    /// Null before the manifest was first read.
    let name: String?
    let description: String
    let enabled: Bool
    let defaultKey: String?
    let problem: String?
    let providers: [PluginInfo]

    var id: String { url }
    var shownName: String { name ?? host }
}

struct PluginInfo: Decodable, Hashable, Sendable, Identifiable {
    let id: String
    let name: String
    let version: String
    let description: String
    let languages: [String]
    let types: [String]
    let enabled: Bool
}

/// What a pasted link would add, for the consent sheet.
struct PluginRepoPreview: Decodable, Hashable, Sendable {
    let url: String
    let host: String
    let name: String
    let description: String
    let providers: [String]
}

/// The plugin words, the same as apps/shared/plugin-words.json (PluginsTests holds them equal).
enum PluginWords {
    static let group = "Plugins"
    static let section = "From plugins"
    static let looking = "Plugins are still looking."
    static let choose = "Choose a version to play."
    static let row = "{source}, plays from {host}"
    static let title = "Plugins"
    static let lede = "Nuvio plugin repositories. Their providers find direct links when you open Versions, and their rows play only when you choose them."
    static let fieldLabel = "Repository link"
    static let add = "Add"
    static let checking = "Reading the repository"
    static let adding = "Adding"
    static let added = "Added {name}"
    static let couldNotAdd = "Could not add: {reason}"
    static let sheetTitle = "Add {name}?"
    static let sheetFrom = "From {host}."
    static let sheetAdds = "Adds {what}."
    static let sheetNote = "Plugins are made by other people. They run on this device when you open Versions, get the name and year of what you open, and can contact any public website. Their links play only when you choose them. They cannot read your files, accounts or keys."
    static let providers = "Providers"
    static let on = "On"
    static let off = "Off"
    static let remove = "Remove"
    static let removeConfirm = "Press again to remove"
    static let empty = "No repositories yet. Paste a Nuvio repository link above."
    static let unread = "Could not read this repository: {problem}"
    static let loading = "Reading repositories"
    static let kids = "Kids profiles never run plugins."
    static let notAvailable = "Plugins are not available in this version."
    static let summaryNone = "Add Nuvio repositories for direct links."

    /// Every word by its key in the shared file.
    static let all: [String: String] = [
        "group": group, "section": section, "looking": looking, "choose": choose, "row": row, "title": title, "lede": lede,
        "fieldLabel": fieldLabel, "add": add, "checking": checking, "adding": adding, "added": added, "couldNotAdd": couldNotAdd,
        "sheetTitle": sheetTitle, "sheetFrom": sheetFrom, "sheetAdds": sheetAdds, "sheetNote": sheetNote, "providers": providers,
        "on": on, "off": off, "remove": remove, "removeConfirm": removeConfirm, "empty": empty, "unread": unread,
        "loading": loading, "kids": kids, "notAvailable": notAvailable, "summaryNone": summaryNone,
    ]

    /// `template` with each `{name}` replaced by its value.
    static func fill(_ template: String, _ values: [String: String]) -> String {
        values.reduce(template) { $0.replacingOccurrences(of: "{\($1.key)}", with: $1.value) }
    }

    /// "Dizibox, plays from cdn.example.com": the row's quality is its badge.
    static func rowLine(_ row: VersionRow) -> String {
        fill(Self.row, ["source": row.sourceName ?? "", "host": row.host ?? ""])
    }

    /// "Adds 8 providers: A, B, C, D, E and 3 more."
    static func adds(_ names: [String]) -> String {
        let n = names.count
        let shown = Array(names.prefix(5))
        let rest = n - shown.count
        let list: String
        if rest > 0 {
            list = "\(shown.joined(separator: ", ")) and \(rest) more"
        } else if shown.count < 2 {
            list = shown.joined()
        } else {
            list = "\(shown.dropLast().joined(separator: ", ")) and \(shown[shown.count - 1])"
        }
        return fill(sheetAdds, ["what": "\(n) \(n == 1 ? "provider" : "providers"): \(list)"])
    }

    /// The Settings row's line: repositories, and the providers on in those that are on.
    static func summary(_ repos: [PluginRepoInfo]) -> String {
        if repos.isEmpty { return summaryNone }
        let on = repos.filter(\.enabled).reduce(0) { $0 + $1.providers.filter(\.enabled).count }
        let n = repos.count
        return "\(n) \(n == 1 ? "repository" : "repositories"), \(on) \(on == 1 ? "provider" : "providers") on"
    }
}
