/// A chip's detail (the catalog's addon on Discover) is a step quieter than its label only on the
/// selected white pill. Unselected, the label is `muted` on a white 8 percent capsule (14 on a Mac
/// hover), and the detail at 70 percent of that read 4.49:1, so it stays `muted` there (7.46:1).
func chipDetailOpacity(selected: Bool) -> Double {
    selected ? 0.7 : 1
}
