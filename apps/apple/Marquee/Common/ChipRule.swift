/// The segmented control's fills (design/DECISION.md, Selected): an unselected chip sits on the
/// track grey (118, 118, 128) at 24 percent, 32 under the pointer, with its words at `muted`; the
/// selected one on `selected.fill` (#636366) with white words. The detail beside the label keeps
/// the label's color: at 70 percent of it, it fell under 4.5:1 on the grey.
enum ChipFill {
    static let track: (r: Double, g: Double, b: Double) = (118, 118, 128)
    static let rest = 0.24
    static let hover = 0.32
}
