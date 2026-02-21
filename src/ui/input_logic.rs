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
