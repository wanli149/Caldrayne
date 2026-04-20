use serde::{Deserialize, Serialize};

pub const DEFAULT_LANGUAGE: &str = "zh-Hans";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct LanguageSettings {
    pub selected_language: String,
    #[serde(default = "default_true")]
    /// Controls whether the locale is sent to servers we connect (usually for
    /// localizing rules & motd messages)
    pub send_to_server: bool,
    pub use_english_fallback: bool,
}

impl Default for LanguageSettings {
    fn default() -> Self {
        Self {
            selected_language: DEFAULT_LANGUAGE.to_string(),
            send_to_server: true,
            use_english_fallback: true,
        }
    }
}

fn default_true() -> bool { true }
