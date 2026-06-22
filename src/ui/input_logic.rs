// Note: list-selection movement (previously `select_prev`/`select_next`) now
// lives inside the UI transition reducer `ui::ui_logic::on_button`, where it is
// tested directly against the screen state machine.

/// Advance the scanning "spinner" dot count, cycling 0 -> 1 -> 2 -> 3 -> 0.
///
/// The display renders `dots % 4` as "", ".", "..", "...".
pub fn next_scan_dots(dots: u8) -> u8 {
    (dots + 1) % 4
}
