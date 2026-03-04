//! On-disk persistence format and version handling.

use std::collections::HashMap;
use std::fmt;

use bevy::prelude::*;
use serde::Deserialize;
use serde::Serialize;

use crate::state::PRIMARY_WINDOW_KEY;
use crate::types::WindowState;

/// Current persisted state format version.
pub const CURRENT_STATE_VERSION: u8 = 2;

/// Typed identifier for persisted window state.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Reflect)]
pub enum WindowKey {
    Primary,
    Managed(String),
}

impl WindowKey {
    /// Convert a legacy string key into a typed key.
    #[must_use]
    pub fn from_legacy_key(key: String) -> Self {
        if key == PRIMARY_WINDOW_KEY {
            Self::Primary
        } else {
            Self::Managed(key)
        }
    }
}

impl fmt::Display for WindowKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Primary => write!(f, "{PRIMARY_WINDOW_KEY}"),
            Self::Managed(name) => write!(f, "{name}"),
        }
    }
}

/// One persisted key/state pair in v2.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedEntry {
    pub key:   WindowKey,
    pub state: WindowState,
}

/// Versioned persisted state format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedStateV2 {
    pub version: u8,
    pub entries: Vec<PersistedEntry>,
}

/// Decode persisted state text into typed runtime state.
///
/// Supports:
/// - v2 format (`PersistedStateV2`)
/// - legacy v1 map format (`HashMap<String, WindowState>`)
/// - legacy single-window format (`WindowState`)
pub fn decode(contents: &str) -> Option<HashMap<WindowKey, WindowState>> {
    if let Ok(persisted) = ron::from_str::<PersistedStateV2>(contents) {
        return decode_v2(persisted);
    }

    if let Ok(legacy) = ron::from_str::<HashMap<String, WindowState>>(contents) {
        debug!("[decode] Migrated legacy HashMap<String, WindowState> format to v2");
        return Some(
            legacy
                .into_iter()
                .map(|(key, state)| (WindowKey::from_legacy_key(key), state))
                .collect(),
        );
    }

    if let Ok(state) = ron::from_str::<WindowState>(contents) {
        debug!("[decode] Migrated legacy single-window format to v2");
        return Some(HashMap::from([(WindowKey::Primary, state)]));
    }

    None
}

fn decode_v2(persisted: PersistedStateV2) -> Option<HashMap<WindowKey, WindowState>> {
    if persisted.version != CURRENT_STATE_VERSION {
        warn!(
            "[decode] Unsupported persisted state version {} (expected {})",
            persisted.version, CURRENT_STATE_VERSION
        );
        return None;
    }

    let mut states = HashMap::with_capacity(persisted.entries.len());
    for entry in persisted.entries {
        if states.insert(entry.key.clone(), entry.state).is_some() {
            warn!(
                "[decode] Invalid persisted state: duplicate key \"{}\"",
                entry.key
            );
            return None;
        }
    }

    Some(states)
}

/// Header comment prepended to the RON file to document the coordinate contract.
const RON_HEADER: &str = "\
// All spatial values are in physical pixels (not logical).
// Position: window content area top-left in physical monitor coordinates.
// Width/Height: content area size in physical pixels (excludes decoration).
// Monitor scale factor is NOT stored — it is looked up at runtime.
";

/// Encode typed runtime state into persisted v2 text.
pub fn encode(states: &HashMap<WindowKey, WindowState>) -> Result<String, ron::Error> {
    let mut entries: Vec<PersistedEntry> = states
        .iter()
        .map(|(key, state)| PersistedEntry {
            key:   key.clone(),
            state: state.clone(),
        })
        .collect();
    entries.sort_by(|a, b| a.key.cmp(&b.key));

    let persisted = PersistedStateV2 {
        version: CURRENT_STATE_VERSION,
        entries,
    };
    let ron_body = ron::ser::to_string_pretty(&persisted, ron::ser::PrettyConfig::default())?;
    Ok(format!("{RON_HEADER}{ron_body}"))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use bevy::prelude::*;

    use super::CURRENT_STATE_VERSION;
    use super::PersistedEntry;
    use super::PersistedStateV2;
    use super::WindowKey;
    use crate::state_format::decode;
    use crate::state_format::encode;
    use crate::types::SavedWindowMode;
    use crate::types::WindowState;

    fn sample_state() -> WindowState {
        WindowState {
            position:      Some((10, 20)),
            width:         800,
            height:        600,
            monitor_index: 1,
            mode:          SavedWindowMode::Windowed,
            app_name:      "test-app".to_string(),
        }
    }

    #[test]
    fn decode_v2_distinguishes_primary_and_managed_primary() {
        let persisted = PersistedStateV2 {
            version: CURRENT_STATE_VERSION,
            entries: vec![
                PersistedEntry {
                    key:   WindowKey::Primary,
                    state: sample_state(),
                },
                PersistedEntry {
                    key:   WindowKey::Managed("primary".to_string()),
                    state: WindowState {
                        position: Some((30, 40)),
                        ..sample_state()
                    },
                },
            ],
        };
        let contents =
            match ron::ser::to_string_pretty(&persisted, ron::ser::PrettyConfig::default()) {
                Ok(contents) => contents,
                Err(error) => panic!("failed to serialize test state: {error}"),
            };

        let decoded = decode(&contents);
        assert!(decoded.is_some(), "expected v2 decode to succeed");
        let decoded = decoded.unwrap_or_default();
        assert!(decoded.contains_key(&WindowKey::Primary));
        assert!(decoded.contains_key(&WindowKey::Managed("primary".to_string())));
        assert_eq!(decoded.len(), 2);
    }

    #[test]
    fn decode_legacy_v1_converts_primary_and_managed() {
        let legacy = HashMap::from([
            ("primary".to_string(), sample_state()),
            ("inspector".to_string(), sample_state()),
        ]);
        let contents = match ron::ser::to_string_pretty(&legacy, ron::ser::PrettyConfig::default())
        {
            Ok(contents) => contents,
            Err(error) => panic!("failed to serialize legacy test state: {error}"),
        };

        let decoded = decode(&contents);
        assert!(decoded.is_some(), "expected legacy decode to succeed");
        let decoded = decoded.unwrap_or_default();
        assert!(decoded.contains_key(&WindowKey::Primary));
        assert!(decoded.contains_key(&WindowKey::Managed("inspector".to_string())));
        assert_eq!(decoded.len(), 2);
    }

    #[test]
    fn decode_v2_rejects_duplicate_keys() {
        let persisted = PersistedStateV2 {
            version: CURRENT_STATE_VERSION,
            entries: vec![
                PersistedEntry {
                    key:   WindowKey::Primary,
                    state: sample_state(),
                },
                PersistedEntry {
                    key:   WindowKey::Primary,
                    state: sample_state(),
                },
            ],
        };
        let contents =
            match ron::ser::to_string_pretty(&persisted, ron::ser::PrettyConfig::default()) {
                Ok(contents) => contents,
                Err(error) => panic!("failed to serialize duplicate-key test state: {error}"),
            };

        assert!(
            decode(&contents).is_none(),
            "duplicate keys should fail decode"
        );
    }

    #[test]
    fn encode_sets_version_2() {
        let states = HashMap::from([
            (WindowKey::Primary, sample_state()),
            (WindowKey::Managed("inspector".to_string()), sample_state()),
        ]);

        let encoded = match encode(&states) {
            Ok(encoded) => encoded,
            Err(error) => panic!("failed to encode state: {error}"),
        };
        let decoded = ron::from_str::<PersistedStateV2>(&encoded);
        assert!(decoded.is_ok(), "encoded text should parse as v2");
        let decoded = decoded.unwrap_or(PersistedStateV2 {
            version: 0,
            entries: Vec::new(),
        });
        assert_eq!(decoded.version, CURRENT_STATE_VERSION);
        assert_eq!(decoded.entries.len(), 2);
    }
}
