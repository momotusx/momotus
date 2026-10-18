/// Find addons copy, the same as apps/shared/find-addons-words.json (the web
/// and Android read that file; FindAddonsTests checks this one against it).
enum FindAddonsWords {
    static let eyebrow = "Addons"
    static let title = "Find addons"
    static let lede = "Most of these add subtitles and catalogs. Sources usually come as a link from the addon's own website."
    static let community = "Community addons are made by other people. They see what you open in them, and their catalogs show on Home on all your devices."
    static let configureHelp = "Set it up on its website, copy the link it gives you, then paste it here."
    static let configureTv = "Set it up on its website, then add the link in Marquee on your phone or computer. With sync on, it follows you here."
    static let pasteLink = "Paste a link"
    static let listLabel = "List"
    static let official = "Official"
    static let communityList = "Community"
    static let showLabel = "Show"
    static let all = "All"
    static let sources = "Sources"
    static let subtitles = "Subtitles"
    static let catalogs = "Catalogs"
    static let loading = "Asking Cinemeta"
    static let failed = "Cinemeta did not answer."
    static let tryAgain = "Try again"
    static let refresh = "Refresh"
    static let empty = "Nothing to show from Cinemeta here."
    static let filterEmpty = "No {kind} addons in this list."
    static let kindSources = "source"
    static let kindSubtitles = "subtitle"
    static let kindCatalogs = "catalog"
    static let installing = "Installing"
    static let turningOn = "Turning on"
    static let installed = "Installed"
    static let off = "Off"
    static let setUp = "Set up"
    static let setUpOff = "Set up, off"
    static let install = "Install"
    static let configure = "Configure"
    static let turnOn = "Turn on"
    static let fromHost = "from {host}"

    /// Every word by its key in the shared file, for the parity test.
    static let table: [String: String] = [
        "eyebrow": eyebrow, "title": title, "lede": lede, "community": community,
        "configureHelp": configureHelp, "configureTv": configureTv, "pasteLink": pasteLink,
        "listLabel": listLabel, "official": official, "communityList": communityList, "showLabel": showLabel,
        "all": all, "sources": sources, "subtitles": subtitles, "catalogs": catalogs,
        "loading": loading, "failed": failed, "tryAgain": tryAgain, "refresh": refresh, "empty": empty,
        "filterEmpty": filterEmpty, "kindSources": kindSources, "kindSubtitles": kindSubtitles, "kindCatalogs": kindCatalogs,
        "installing": installing, "turningOn": turningOn, "installed": installed, "off": off,
        "setUp": setUp, "setUpOff": setUpOff, "install": install, "configure": configure,
        "turnOn": turnOn, "fromHost": fromHost,
    ]
}
