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

/// The repositories nuvioplugins.com lists, for Find addons (R8).
struct PluginDirectory: Decodable, Hashable, Sendable {
    let source: String
    let fromSnapshot: Bool
    let repos: [ListedRepo]
}

struct ListedRepo: Decodable, Hashable, Sendable, Identifiable {
    let url: String
    let host: String
    let name: String
    let description: String
    let providers: [String]
    let languages: [String]
    let types: [String]
    let installed: Bool

    var id: String { url }
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
    static let directoryTitle = "Nuvio plugins"
    static let directoryFrom = "From {source}"
    static let directorySearch = "Search plugins"
    static let installed = "Installed"
    static let directoryNoMatch = "No repository matches."
    static let forKinds = "for {kinds}"
    static let languages = "Languages: {languages}."

    /// Every word by its key in the shared file.
    static let all: [String: String] = [
        "group": group, "section": section, "looking": looking, "choose": choose, "row": row, "title": title, "lede": lede,
        "fieldLabel": fieldLabel, "add": add, "checking": checking, "adding": adding, "added": added, "couldNotAdd": couldNotAdd,
        "sheetTitle": sheetTitle, "sheetFrom": sheetFrom, "sheetAdds": sheetAdds, "sheetNote": sheetNote, "providers": providers,
        "on": on, "off": off, "remove": remove, "removeConfirm": removeConfirm, "empty": empty, "unread": unread,
        "loading": loading, "kids": kids, "notAvailable": notAvailable, "summaryNone": summaryNone,
        "directoryTitle": directoryTitle, "directoryFrom": directoryFrom, "directorySearch": directorySearch,
        "installed": installed, "directoryNoMatch": directoryNoMatch, "forKinds": forKinds, "languages": languages,
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

    /// "8 providers for films and series. Languages: tr, en." for a listed repository's card.
    static func directoryLine(_ r: ListedRepo) -> String {
        let n = r.providers.count
        let kinds = r.types.map { $0 == "movie" ? "films" : $0 }
        let forKinds = kinds.isEmpty ? "" : " " + fill(Self.forKinds, ["kinds": kinds.joined(separator: " and ")])
        let head = "\(n) \(n == 1 ? "provider" : "providers")\(forKinds)."
        return r.languages.isEmpty ? head : "\(head) \(fill(Self.languages, ["languages": r.languages.joined(separator: ", ")]))"
    }

    /// Whether every word of `query` is in the repository's name, its providers' names or its
    /// languages, case and accents aside ("turk" finds "Türk").
    static func matches(_ r: ListedRepo, _ query: String) -> Bool {
        let fold = { (s: String) in s.folding(options: [.caseInsensitive, .diacriticInsensitive], locale: nil) }
        let text = fold(([r.name] + r.providers + r.languages).joined(separator: " "))
        return fold(query).split(whereSeparator: \.isWhitespace).allSatisfy { text.contains($0) }
    }

    /// The Settings row's line: repositories, and the providers on in those that are on.
    static func summary(_ repos: [PluginRepoInfo]) -> String {
        if repos.isEmpty { return summaryNone }
        let on = repos.filter(\.enabled).reduce(0) { $0 + $1.providers.filter(\.enabled).count }
        let n = repos.count
        return "\(n) \(n == 1 ? "repository" : "repositories"), \(on) \(on == 1 ? "provider" : "providers") on"
    }
}
