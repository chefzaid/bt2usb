/// Decide whether screen should be on based on base power state and inactivity policy.
pub fn screen_should_be_on(
    base_display_on: bool,
    auto_off_enabled: bool,
    idle_secs: u64,
    auto_off_timeout_secs: u64,
) -> bool {
    if !base_display_on {
        return false;
    }

    if auto_off_enabled && idle_secs >= auto_off_timeout_secs {
        return false;
    }

    true
}
