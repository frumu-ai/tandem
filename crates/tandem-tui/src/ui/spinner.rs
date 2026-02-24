pub const SPINNER_FRAMES: [&str; 10] = ["|", "/", "-", "\\", "|", "/", "-", "\\", ">", "<"];

pub fn frame_for_tick(tick: usize) -> &'static str {
    SPINNER_FRAMES[tick % SPINNER_FRAMES.len()]
}
