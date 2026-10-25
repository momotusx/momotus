import Foundation

/// The default network as `networkChanged` tells the core (plans/offline-downloads.md 4.13): `none` when the
/// path is not satisfied, `metered` from the platform's cost flag (`isExpensive`, never the interface, so a
/// phone's hotspot or a VPN over mobile data holds Wi-Fi only downloads), `lowData` from Low Data Mode
/// (`isConstrained`). A Mac, whose Wi-Fi only switch is hidden, sends both false.
struct NetworkReport: Equatable, Sendable {
    let network: String
    let metered: Bool
    let lowData: Bool

    static func of(
        satisfied: Bool, wifi: Bool, ethernet: Bool, cellular: Bool, expensive: Bool, constrained: Bool, desktop: Bool
    ) -> NetworkReport {
        guard satisfied else { return NetworkReport(network: "none", metered: false, lowData: false) }
        let kind = StreamChoice.network(wifi: wifi, ethernet: ethernet, cellular: cellular)
        return NetworkReport(network: kind, metered: !desktop && expensive, lowData: !desktop && constrained)
    }

    /// The `lifecycle networkChanged` fields.
    var fields: [String: any Sendable] { ["network": network, "metered": metered, "lowData": lowData] }
}
