//! On-disk persistence format and version handling.
//!
//! # Versioning strategy
//!
//! Every RON state file carries a `version` field inside [`PersistedState`].
//! [`decode`] parses the file once, then dispatches to a version-specific
//! decoder based on that field. All previously shipped versions remain
//! supported so that users never lose their saved window positions.
//!
//! ## Adding a new version
//!
//! 1. Bump [`CURRENT_STATE_VERSION`].
//! 2. If the new version changes the shape of an entry, add new structs (e.g. `PersistedEntryV2`)
//!    and a conversion from the old entry type. If only semantics change, the existing structs can
//!    be reused.
//! 3. Add a `decode_v<N>` function that accepts a [`PersistedState`] and returns
//!    `Option<HashMap<WindowKey, WindowState>>`.
//! 4. Add an arm to the `match persisted.version` block inside [`decode`].
//! 5. Update [`encode`] to write the new format (only the latest version is ever written).
//! 6. Add a test that round-trips through the new version **and** a test that an older version file
//!    still decodes correctly.
//!
//! ## Supported formats (oldest first)
//!
//! | Format | Description |
//! |--------|-------------|
//! | Legacy single-window | Bare `WindowState` (no version field, pre-multi-window) |
//! | v1 | `PersistedState { version: 1, entries }` with `width`/`height` (physical) |
//! | v2 | `PersistedState { version: 2, entries }` with `logical_width`/`logical_height` + `monitor_scale` |

use std::collections::HashMap;
use std::fmt;

use bevy::prelude::*;
use serde::Deserialize;
use serde::Serialize;

use super::constants::CURRENT_STATE_VERSION;
use super::constants::PRIMARY_WINDOW_KEY;
use super::types::SavedWindowMode;
use super::types::WindowState;

/// Typed identifier for persisted window state.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Reflect)]
pub enum WindowKey {
    Primary,
    Managed(String),
}

impl fmt::Display for WindowKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Primary => write!(f, "{PRIMARY_WINDOW_KEY}"),
            Self::Managed(name) => write!(f, "{name}"),
        }
    }
}

/// One persisted key/state pair in v1.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct PersistedEntry {
    pub key:   WindowKey,
    pub state: WindowState,
}

/// Versioned persisted state format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct PersistedState {
    pub version: u8,
    pub entries: Vec<PersistedEntry>,
}

/// Minimal version probe — just extract the version number from any versioned format.
#[derive(Deserialize)]
struct VersionProbe {
    version: u8,
}

/// Decode persisted state text into typed runtime state.
///
/// Tries versioned formats first (dispatching by the `version` field),
/// then falls back to legacy unversioned formats. See the module-level
/// docs for the full list of supported formats.
pub(super) fn decode(contents: &str) -> Option<HashMap<WindowKey, WindowState>> {
    // Probe the version field without requiring any particular entry shape.
    if let Ok(probe) = ron::from_str::<VersionProbe>(contents) {
        return match probe.version {
            1 => decode_v1(contents),
            2 => decode_v2(contents),
            unsupported => {
                warn!(
                    "[decode] Unsupported persisted state version {unsupported} \
                     (latest supported: {CURRENT_STATE_VERSION})"
                );
                None
            },
        };
    }

    // Legacy unversioned format — bare `WindowState` from before multi-window
    // support. Cannot participate in the version match above because it has no
    // `version` field.
    decode_legacy_single_window(contents)
}

/// v1 window state layout (used `width`/`height` field names).
/// Used only for deserializing v1 and legacy files.
#[derive(Debug, Clone, Deserialize)]
struct WindowStateV1 {
    position:      Option<(i32, i32)>,
    width:         u32,
    height:        u32,
    monitor_index: usize,
    mode:          SavedWindowMode,
    #[serde(default)]
    app_name:      String,
}

impl WindowStateV1 {
    /// Convert to current `WindowState`, treating v1 values as logical (assumes scale 1.0).
    fn into_current(self) -> WindowState {
        WindowState {
            logical_position: self.position,
            logical_width:    self.width,
            logical_height:   self.height,
            monitor_scale:    1.0,
            monitor_index:    self.monitor_index,
            mode:             self.mode,
            app_name:         self.app_name,
        }
    }
}

/// v1 persisted entry (uses `WindowStateV1`).
#[derive(Debug, Clone, Deserialize)]
struct PersistedEntryV1 {
    key:   WindowKey,
    state: WindowStateV1,
}

/// v1 persisted state wrapper.
#[derive(Debug, Clone, Deserialize)]
struct PersistedStateV1 {
    #[allow(dead_code, reason = "field required for v1 deserialization")]
    version: u8,
    entries: Vec<PersistedEntryV1>,
}

fn decode_legacy_single_window(contents: &str) -> Option<HashMap<WindowKey, WindowState>> {
    let state = ron::from_str::<WindowStateV1>(contents).ok()?;
    debug!("[decode] Migrated legacy single-window format to v2");
    Some(HashMap::from([(WindowKey::Primary, state.into_current())]))
}

fn decode_v1(contents: &str) -> Option<HashMap<WindowKey, WindowState>> {
    let v1 = ron::from_str::<PersistedStateV1>(contents).ok()?;

    let mut states = HashMap::with_capacity(v1.entries.len());
    for entry in v1.entries {
        if states
            .insert(entry.key.clone(), entry.state.into_current())
            .is_some()
        {
            warn!(
                "[decode] Invalid persisted state: duplicate key \"{}\"",
                entry.key
            );
            return None;
        }
    }

    debug!("[decode] Migrated v1 state to v2");
    Some(states)
}

fn decode_v2(contents: &str) -> Option<HashMap<WindowKey, WindowState>> {
    let persisted = ron::from_str::<PersistedState>(contents).ok()?;
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
// All spatial values (position, size) are in logical pixels.
// monitor_scale: scale factor at save time (informational, not used during restore).
";

/// Encode typed runtime state into persisted v1 text.
pub(super) fn encode(states: &HashMap<WindowKey, WindowState>) -> Result<String, ron::Error> {
    let mut entries: Vec<PersistedEntry> = states
        .iter()
        .map(|(key, state)| PersistedEntry {
            key:   key.clone(),
            state: state.clone(),
        })
        .collect();
    entries.sort_by(|a, b| a.key.cmp(&b.key));

    let persisted = PersistedState {
        version: CURRENT_STATE_VERSION,
        entries,
    };
    let ron_body = ron::ser::to_string_pretty(&persisted, ron::ser::PrettyConfig::default())?;
    Ok(format!("{RON_HEADER}{ron_body}"))
}

#[cfg(test)]
#[allow(clippy::panic, reason = "tests should panic on unexpected values")]
mod tests {
    use std::collections::HashMap;

    use bevy::prelude::*;

    use super::CURRENT_STATE_VERSION;
    use super::PersistedEntry;
    use super::PersistedState;
    use super::WindowKey;
    use super::decode;
    use super::encode;
    use crate::types::SavedVideoMode;
    use crate::types::SavedWindowMode;
    use crate::types::WindowState;

    fn sample_state() -> WindowState {
        WindowState {
            logical_position: Some((10, 20)),
            logical_width:    800,
            logical_height:   600,
            monitor_scale:    1.0,
            monitor_index:    1,
            mode:             SavedWindowMode::Windowed,
            app_name:         "test-app".to_string(),
        }
    }

    #[test]
    fn decode_v2_distinguishes_primary_and_managed_primary() {
        let persisted = PersistedState {
            version: CURRENT_STATE_VERSION,
            entries: vec![
                PersistedEntry {
                    key:   WindowKey::Primary,
                    state: sample_state(),
                },
                PersistedEntry {
                    key:   WindowKey::Managed("primary".to_string()),
                    state: WindowState {
                        logical_position: Some((30, 40)),
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
    fn decode_legacy_single_window_migrates_to_v2() {
        // Legacy format uses `width`/`height` field names (pre-multi-window era)
        let legacy_ron = "\
(
    position: Some((10, 20)),
    width: 800,
    height: 600,
    monitor_index: 1,
    mode: Windowed,
    app_name: \"test-app\",
)";

        let decoded = decode(legacy_ron);
        assert!(
            decoded.is_some(),
            "expected legacy single-window decode to succeed"
        );
        let decoded = decoded.unwrap_or_default();
        assert!(decoded.contains_key(&WindowKey::Primary));
        assert_eq!(decoded.len(), 1);
        let state = &decoded[&WindowKey::Primary];
        assert_eq!(state.logical_position, Some((10, 20)));
        assert_eq!(state.logical_width, 800);
        assert_eq!(state.logical_height, 600);
        assert!((state.monitor_scale - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn decode_v1_migrates_to_v2() {
        let v1_ron = "\
(
    version: 1,
    entries: [
        (
            key: Primary,
            state: (
                position: Some((10, 20)),
                width: 800,
                height: 600,
                monitor_index: 1,
                mode: Windowed,
                app_name: \"test-app\",
            ),
        ),
    ],
)";

        let decoded = decode(v1_ron);
        assert!(decoded.is_some(), "expected v1 decode to succeed");
        let decoded = decoded.unwrap_or_default();
        let state = &decoded[&WindowKey::Primary];
        assert_eq!(state.logical_width, 800);
        assert_eq!(state.logical_height, 600);
        assert!((state.monitor_scale - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn decode_v2_rejects_duplicate_keys() {
        let persisted = PersistedState {
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

    /// Golden-file tests using exact RON strings from the pre-multi-window era
    /// (commit 516f5930, used through v0.18.2). These are byte-for-byte copies of
    /// files that the published crate wrote via `ron::ser::to_string_pretty` with
    /// `PrettyConfig::default()`. If a dependency bump or struct change silently
    /// breaks deserialization, these tests catch it.
    mod golden_legacy {
        use super::*;

        /// Bare `WindowState` — windowed mode, from `macos_0/same_monitor_restore.ron`.
        const WINDOWED: &str = "\
(
    position: Some((200, 200)),
    width: 1600,
    height: 1200,
    monitor_index: 0,
    mode: Windowed,
    app_name: \"restore_window\",
)";

        /// Bare `WindowState` — borderless fullscreen, from
        /// `macos_0/fullscreen_borderless_programmatic.ron`.
        const BORDERLESS_FULLSCREEN: &str = "\
(
    position: Some((0, 0)),
    width: 3456,
    height: 2234,
    monitor_index: 0,
    mode: BorderlessFullscreen,
    app_name: \"restore_window\",
)";

        /// Bare `WindowState` — exclusive fullscreen with explicit video mode,
        /// from `macos_0/fullscreen_exclusive.ron`.
        const EXCLUSIVE_FULLSCREEN: &str = "\
(
    position: Some((0, 0)),
    width: 1920,
    height: 1200,
    monitor_index: 0,
    mode: Fullscreen(
        video_mode: Some((
            physical_size: (1920, 1200),
            bit_depth: 32,
            refresh_rate_millihertz: 60000,
        )),
    ),
    app_name: \"restore_window\",
)";

        #[test]
        fn decode_golden_legacy_windowed() {
            let decoded = decode(WINDOWED);
            assert!(decoded.is_some(), "golden legacy windowed file must decode");
            let decoded = decoded.unwrap_or_default();
            assert_eq!(decoded.len(), 1);
            let state = &decoded[&WindowKey::Primary];
            assert_eq!(state.logical_position, Some((200, 200)));
            assert_eq!(state.logical_width, 1600);
            assert_eq!(state.logical_height, 1200);
            assert!((state.monitor_scale - 1.0).abs() < f64::EPSILON);
            assert_eq!(state.monitor_index, 0);
            assert_eq!(state.mode, SavedWindowMode::Windowed);
            assert_eq!(state.app_name, "restore_window");
        }

        #[test]
        fn decode_golden_legacy_borderless_fullscreen() {
            let decoded = decode(BORDERLESS_FULLSCREEN);
            assert!(
                decoded.is_some(),
                "golden legacy borderless fullscreen file must decode"
            );
            let decoded = decoded.unwrap_or_default();
            let state = &decoded[&WindowKey::Primary];
            assert_eq!(state.logical_position, Some((0, 0)));
            assert_eq!(state.logical_width, 3456);
            assert_eq!(state.logical_height, 2234);
            assert_eq!(state.mode, SavedWindowMode::BorderlessFullscreen);
        }

        #[test]
        fn decode_golden_legacy_exclusive_fullscreen() {
            let decoded = decode(EXCLUSIVE_FULLSCREEN);
            assert!(
                decoded.is_some(),
                "golden legacy exclusive fullscreen file must decode"
            );
            let decoded = decoded.unwrap_or_default();
            let state = &decoded[&WindowKey::Primary];
            assert_eq!(state.logical_position, Some((0, 0)));
            assert_eq!(state.logical_width, 1920);
            assert_eq!(state.logical_height, 1200);
            assert_eq!(
                state.mode,
                SavedWindowMode::Fullscreen {
                    video_mode: Some(SavedVideoMode {
                        physical_size:           UVec2::new(1920, 1200),
                        bit_depth:               32,
                        refresh_rate_millihertz: 60000,
                    }),
                }
            );
        }
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
        let decoded = ron::from_str::<PersistedState>(&encoded);
        assert!(decoded.is_ok(), "encoded text should parse as v2");
        let decoded = decoded.unwrap_or(PersistedState {
            version: 0,
            entries: Vec::new(),
        });
        assert_eq!(decoded.version, CURRENT_STATE_VERSION);
        assert_eq!(decoded.entries.len(), 2);
    }

    #[test]
    fn encode_then_decode_roundtrip() {
        let states = HashMap::from([
            (WindowKey::Primary, sample_state()),
            (
                WindowKey::Managed("inspector".to_string()),
                WindowState {
                    logical_position: Some((100, 200)),
                    logical_width:    1024,
                    logical_height:   768,
                    monitor_scale:    2.0,
                    monitor_index:    0,
                    mode:             SavedWindowMode::Windowed,
                    app_name:         "test-app".to_string(),
                },
            ),
        ]);

        let encoded = match encode(&states) {
            Ok(encoded) => encoded,
            Err(error) => panic!("failed to encode state: {error}"),
        };
        let decoded = decode(&encoded);
        assert!(decoded.is_some(), "roundtrip decode should succeed");
        let decoded = decoded.unwrap_or_default();
        assert_eq!(decoded.len(), 2);
        let primary = &decoded[&WindowKey::Primary];
        assert_eq!(primary.logical_width, 800);
        assert_eq!(primary.logical_height, 600);
        assert!((primary.monitor_scale - 1.0).abs() < f64::EPSILON);
        let inspector = &decoded[&WindowKey::Managed("inspector".to_string())];
        assert_eq!(inspector.logical_width, 1024);
        assert_eq!(inspector.logical_height, 768);
        assert!((inspector.monitor_scale - 2.0).abs() < f64::EPSILON);
    }
}
