//! Physical keyboard identifiers shared by the compiler, browser, and native runtime.
//! The append-only catalog lives in `runtime/crumb/keys.def`.

/// One supported physical key and its platform mappings.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KeyInfo {
    pub key: Key,
    pub name: &'static str,
    pub browser_code: &'static str,
    /// `None` for keys without a documented macOS virtual key code.
    pub macos_code: Option<u16>,
    /// A sided macOS modifier mask, or zero for a regular key.
    pub modifier_mask: u32,
}

include!(concat!(env!("OUT_DIR"), "/keyboard.rs"));

impl Key {
    pub fn from_id(id: u8) -> Option<Self> {
        KEYS.get(usize::from(id)).map(|info| info.key)
    }

    pub fn from_browser_code(code: &str) -> Option<Self> {
        KEYS.iter()
            .find(|info| info.browser_code == code)
            .map(|info| info.key)
    }
}
