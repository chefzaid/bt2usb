//! Pure helpers for what the UI task draws.
//!
//! [`next_scan_dots`] animates the Scanning screen, and [`device_list_window`]
//! chooses which rows of a device list fit the four-row OLED. List-selection
//! movement is not here: it lives in the UI reducer, `ui_logic::on_button`,
//! where it is tested against the screen state machine.

/// Advance the scanning "spinner" dot count, cycling 0 -> 1 -> 2 -> 3 -> 0.
///
/// The display renders `dots % 4` as "", ".", "..", "...".
pub fn next_scan_dots(dots: u8) -> u8 {
    dots.wrapping_add(1) % 4
}

/// Visible list range, keeping the highlighted device on the four-row OLED.
pub fn device_list_window(
    device_count: usize,
    selected: usize,
    rows: usize,
) -> core::ops::Range<usize> {
    if device_count == 0 || rows == 0 {
        return 0..0;
    }
    let selected = selected.min(device_count - 1);
    let start = selected.saturating_sub(rows - 1);
    start..start.saturating_add(rows).min(device_count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_scrolls_to_show_every_discovered_device() {
        for selected in 0..8 {
            let window = device_list_window(8, selected, 4);
            assert!(window.contains(&selected));
            assert!(window.len() <= 4);
            assert!(window.end <= 8);
        }
        assert_eq!(device_list_window(8, 7, 4), 4..8);
    }

    #[test]
    fn list_window_handles_empty_and_stale_selection() {
        assert_eq!(device_list_window(0, 0, 4), 0..0);
        assert_eq!(device_list_window(8, 4, 0), 0..0);
        assert_eq!(device_list_window(2, usize::MAX, 4), 0..2);
    }

    #[test]
    fn spinner_recovers_from_out_of_range_state() {
        assert_eq!(next_scan_dots(u8::MAX), 0);
    }
}
