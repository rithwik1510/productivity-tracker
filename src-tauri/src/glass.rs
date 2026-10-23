//! Whether Acrylic can show at all: with Windows "Transparency effects" off, DWM draws every
//! backdrop solid, so the UI must use its solid tokens.

const PERSONALIZE: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";

pub fn available() -> bool {
    // A missing key or value means the Windows default, which is on.
    windows_registry::CURRENT_USER
        .open(PERSONALIZE)
        .and_then(|key| key.get_u32("EnableTransparency"))
        != Ok(0)
}
