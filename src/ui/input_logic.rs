/// Move selection cursor one item up.
pub fn select_prev(selected: usize) -> usize {
    selected.saturating_sub(1)
}

/// Move selection cursor one item down if another item exists.
pub fn select_next(selected: usize, item_count: usize) -> usize {
    if selected + 1 < item_count {
        selected + 1
    } else {
        selected
    }
}

/// Advance the scanning "spinner" dot count, cycling 0 -> 1 -> 2 -> 3 -> 0.
///
/// The display renders `dots % 4` as "", ".", "..", "...".
pub fn next_scan_dots(dots: u8) -> u8 {
    (dots + 1) % 4
}
