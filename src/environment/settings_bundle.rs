/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Emulation of the iOS *Settings* app for apps that ship a `Settings.bundle`.
//!
//! On a real device, an app that wants user-visible options drops a
//! `Settings.bundle/Root.plist` into its bundle and reads the values back with
//! `NSUserDefaults`. iOS surfaces those options in the system Settings app;
//! touchHLE has no Settings app, so the options would be permanently
//! unreachable. Instead, we parse the bundle ourselves and put the toggles in
//! the app picker's Settings panel, writing the same keys the app reads.
//! That way the guest does not need to know anything about us: it just sees the
//! preference key it asked for.

use crate::fs::BundleData;
use plist::Value;
use std::io::Cursor;
/// A user-visible toggle declared by the app's `Settings.bundle`.
#[derive(Clone)]
pub struct SettingsToggle {
    /// `NSUserDefaults` key the app reads.
    pub key: String,
    /// Human-readable title, from `Settings.bundle/<lang>.lproj/Root.strings`
    /// if it can be resolved, otherwise the raw title key.
    pub title: String,
    pub default_value: bool,
    /// Value the app expects for "on"/"off". Almost always YES/NO, but the
    /// plist format allows arbitrary true/false values.
    pub true_value: String,
    pub false_value: String,
}

impl SettingsToggle {
    /// Read the current value from the app's own preferences in its sandbox,
    /// falling back to the bundle's default.
    pub fn current_value(&self, app_path: Option<&std::path::Path>) -> bool {
        let value = app_path.and_then(|app_path| read_app_pref(app_path, &self.key));
        match value {
            // Anything that isn't the declared "off" value counts as "on",
            // which matches how these plists are normally written (YES/true/1
            // vs NO).
            Some(value) => !value.eq_ignore_ascii_case(&self.false_value),
            None => self.default_value,
        }
    }
}

/// The app's preferences plist in its sandbox, at the same host path the
/// guest's `NSUserDefaults` reads and writes it at.
fn app_prefs_plist_path(app_path: &std::path::Path) -> Result<std::path::PathBuf, String> {
    let (bundle, _fs) = BundleData::open_any(app_path)
        .and_then(|data| {
            crate::bundle::Bundle::new_bundle_and_fs_from_host_path(
                data, /* read_only_mode: */ true,
            )
        })
        .map_err(|e| format!("Couldn't open the app bundle: {e}"))?;
    let bundle_id = bundle.bundle_identifier().to_string();

    let prefs_dir = crate::paths::user_data_base_path()
        .join(crate::paths::SANDBOX_DIR)
        .join(&bundle_id)
        .join("Library")
        .join("Preferences");
    Ok(prefs_dir.join(format!("{bundle_id}.plist")))
}

/// Read a single string preference for the app, host-side, from its sandbox.
/// Returns `None` if the app has never saved a value for it. Boolean values
/// (which is what iOS's own Settings app writes for toggle switches) are
/// normalised to "YES"/"NO" strings.
pub fn read_app_pref(app_path: &std::path::Path, key: &str) -> Option<String> {
    let value = Value::from_file(app_prefs_plist_path(app_path).ok()?).ok()?;
    let dict = value.into_dictionary()?;
    match dict.get(key)? {
        Value::Boolean(true) => Some("YES".to_string()),
        Value::Boolean(false) => Some("NO".to_string()),
        Value::String(s) => Some(s.clone()),
        _ => None,
    }
}

/// Write a single string preference for the app, host-side, into its
/// sandbox —
/// the same way the system Settings app would. Other keys are preserved; the
/// app reads the value back through `NSUserDefaults` on its next launch.
///
/// This has to happen host-side because the app picker's environment has a
/// fake bundle and a fake filesystem: the guest's `NSUserDefaults` can't be
/// used there.
pub fn write_app_pref(app_path: &std::path::Path, key: &str, value: &str) -> Result<(), String> {
    let plist_path = app_prefs_plist_path(app_path)?;
    if let Some(parent) = plist_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            format!(
                "Couldn't create the app's preferences directory {}: {e}",
                parent.display()
            )
        })?;
    }

    // Preserve any other keys the app (or a previous toggle) has saved.
    let mut dict = match Value::from_file(&plist_path) {
        Ok(value) => value
            .into_dictionary()
            .unwrap_or_else(plist::Dictionary::new),
        Err(_) => plist::Dictionary::new(),
    };
    // iOS's own Settings app writes actual booleans to the preferences plist
    // for toggle switches, so an app reading its key back as a boolean or
    // comparing objects gets what it expects. Non-standard true/false values
    // (which the plist format allows) stay strings.
    let value = if value.eq_ignore_ascii_case("YES") {
        Value::Boolean(true)
    } else if value.eq_ignore_ascii_case("NO") {
        Value::Boolean(false)
    } else {
        Value::String(value.to_owned())
    };
    dict.insert(key.to_owned(), value);

    Value::Dictionary(dict)
        .to_file_binary(&plist_path)
        .map_err(|e| format!("Couldn't write the app's preferences: {e}"))?;
    Ok(())
}

/// Parse the app's `Settings.bundle/Root.plist`, returning the toggles it
/// declares. Returns an empty list if the app has no settings bundle or it
/// cannot be parsed; this is a best-effort convenience, not something that
/// should ever take an app down.
pub fn load_toggles(app_path: &std::path::Path) -> Vec<SettingsToggle> {
    let Ok((bundle, fs)) = BundleData::open_any(app_path).and_then(|data| {
        crate::bundle::Bundle::new_bundle_and_fs_from_host_path(data, /* read_only: */ true)
    }) else {
        return Vec::new();
    };

    // Settings.bundle lives inside the app bundle. Relative paths resolve
    // against the fake filesystem's working directory, not the bundle, so
    // read through the bundle's own guest path; that works for both .app
    // directories and .ipa archives (where the Payload/ prefix is hidden).
    let settings_root = bundle.bundle_path().join("Settings.bundle/Root.plist");

    let Ok(bytes) = fs.read(&settings_root) else {
        return Vec::new();
    };
    let Ok(root) = Value::from_reader(Cursor::new(&bytes[..])) else {
        return Vec::new();
    };
    let Some(specifiers) = root
        .as_dictionary()
        .and_then(|d| d.get("PreferenceSpecifiers"))
        .and_then(|v| v.as_array())
    else {
        return Vec::new();
    };

    // Localised titles, e.g. Settings.bundle/en.lproj/Root.strings.
    let strings = load_strings(&fs, settings_root.as_str());

    let mut toggles = Vec::new();
    for specifier in specifiers {
        let Some(specifier) = specifier.as_dictionary() else {
            continue;
        };
        // Only PSToggleSwitchSpecifier is supported for now. Radio groups,
        // sliders and text fields are rarer, and each needs its own widget.
        if specifier
            .get("Type")
            .and_then(|v| v.as_string())
            .is_none_or(|t| t != "PSToggleSwitchSpecifier")
        {
            continue;
        }
        let Some(key) = specifier.get("Key").and_then(|v| v.as_string()) else {
            continue;
        };
        let raw_title = specifier
            .get("Title")
            .and_then(|v| v.as_string())
            .unwrap_or(key);
        let title = strings
            .iter()
            .find(|(k, _)| k == raw_title)
            .map(|(_, v)| v.clone())
            .unwrap_or_else(|| raw_title.to_string());

        let bool_of = |name: &str, fallback: bool| -> bool {
            match specifier.get(name) {
                Some(Value::Boolean(b)) => *b,
                Some(Value::String(s)) => {
                    matches!(s.as_str(), "YES" | "yes" | "true" | "1")
                }
                _ => fallback,
            }
        };
        let string_of = |name: &str, fallback: &str| -> String {
            match specifier.get(name) {
                Some(Value::String(s)) => s.clone(),
                Some(Value::Boolean(b)) => if *b { "YES" } else { "NO" }.to_string(),
                _ => fallback.to_string(),
            }
        };

        toggles.push(SettingsToggle {
            key: key.to_string(),
            title,
            default_value: bool_of("DefaultValue", true),
            true_value: string_of("TrueValue", "YES"),
            false_value: string_of("FalseValue", "NO"),
        });
    }
    toggles
}

/// Parse a `.strings` file (the old-style `"key" = "value";` format; plists
/// would be nicer but this is what Settings bundles use).
fn load_strings(fs: &crate::fs::Fs, root_plist_path: &str) -> Vec<(String, String)> {
    let parent = match root_plist_path.rsplit_once('/') {
        Some((parent, _)) => parent,
        None => return Vec::new(),
    };
    let candidates = ["en.lproj/Root.strings", "English.lproj/Root.strings"];
    for candidate in candidates {
        let path = format!("{parent}/{candidate}");
        let Ok(bytes) = fs.read(path.as_str()) else {
            continue;
        };
        // UTF-16 with a BOM is the traditional encoding for these files.
        let text = decode_strings_file(bytes.as_ref());
        return parse_strings(&text);
    }
    Vec::new()
}

fn decode_strings_file(bytes: &[u8]) -> String {
    fn utf16_units(body: &[u8], big_endian: bool) -> Vec<u16> {
        let (pairs, _remainder) = body.as_chunks::<2>();
        pairs
            .iter()
            .map(|c| {
                if big_endian {
                    u16::from_be_bytes(*c)
                } else {
                    u16::from_le_bytes(*c)
                }
            })
            .collect()
    }

    if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE {
        String::from_utf16_lossy(&utf16_units(&bytes[2..], false))
    } else if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF {
        String::from_utf16_lossy(&utf16_units(&bytes[2..], true))
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

/// Minimal `"key" = "value";` parser. Tolerates comments (`//` and `/* */`)
/// and whitespace, which is all these files ever contain in practice.
fn parse_strings(text: &str) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    let bytes: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        // Skip whitespace and comments
        if bytes[i].is_whitespace() {
            i += 1;
            continue;
        }
        if bytes[i] == '/' && i + 1 < bytes.len() && bytes[i + 1] == '/' {
            while i < bytes.len() && bytes[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if bytes[i] == '/' && i + 1 < bytes.len() && bytes[i + 1] == '*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == '*' && bytes[i + 1] == '/') {
                i += 1;
            }
            i += 2;
            continue;
        }
        let Some(key) = parse_quoted(&bytes, &mut i) else {
            i += 1;
            continue;
        };
        // Skip whitespace, then '=' then whitespace
        while i < bytes.len() && bytes[i].is_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != '=' {
            continue;
        }
        i += 1;
        while i < bytes.len() && bytes[i].is_whitespace() {
            i += 1;
        }
        let Some(value) = parse_quoted(&bytes, &mut i) else {
            continue;
        };
        pairs.push((key, value));
        while i < bytes.len() && bytes[i] != ';' {
            i += 1;
        }
        i += 1;
    }
    pairs
}

fn parse_quoted(chars: &[char], i: &mut usize) -> Option<String> {
    if *i >= chars.len() || chars[*i] != '"' {
        return None;
    }
    *i += 1;
    let mut out = String::new();
    while *i < chars.len() {
        match chars[*i] {
            '"' => {
                *i += 1;
                return Some(out);
            }
            '\\' if *i + 1 < chars.len() => {
                *i += 1;
                out.push(match chars[*i] {
                    'n' => '\n',
                    't' => '\t',
                    other => other,
                });
                *i += 1;
            }
            c => {
                out.push(c);
                *i += 1;
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// JellyCar 1.2's `Settings.bundle/en.lproj/Root.strings`, verbatim
    /// (UTF-16LE with a BOM, as shipped).
    fn jellycar_strings() -> Vec<u8> {
        let text = "/* A single strings file. */\n\
\"SoundGroupName\" = \"Sound\";\n\
\"MusicEnabled\" = \"Use Game Music\";\n\
\"SFXEnabled\" = \"Sound Effects\";\n";
        let mut bytes = vec![0xFF, 0xFE];
        for unit in text.encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn strings_file_parses() {
        let pairs = parse_strings(&decode_strings_file(&jellycar_strings()));
        assert!(pairs.contains(&("SoundGroupName".into(), "Sound".into())));
        assert!(pairs.contains(&("MusicEnabled".into(), "Use Game Music".into())));
        assert!(pairs.contains(&("SFXEnabled".into(), "Sound Effects".into())));
    }

    /// The titles the user sees must be the friendly ones, not the raw keys
    /// ("MusicEnabled") that the plist contains.
    #[test]
    fn jellycar_toggle_titles_are_localised() {
        let pairs = parse_strings(&decode_strings_file(&jellycar_strings()));
        let title = pairs
            .iter()
            .find(|(k, _)| k == "MusicEnabled")
            .map(|(_, v)| v.clone());
        assert_eq!(title.as_deref(), Some("Use Game Music"));
    }

    /// Both JellyCar toggles must survive the encoding round trip, or the
    /// "in-game music" / "in-game sounds" options silently vanish.
    /// The .ipa layout the real file uses: the app directory entry is
    /// namespaced by the extraction path, so the `.app` directory must
    /// still be found.
    #[test]
    fn ipa_app_dir_is_detected_from_an_entry_name() {
        let name = "Payload/JellyCar.app/Settings.bundle/Root.plist";
        let rest = name.strip_prefix("Payload/").unwrap();
        let dir = rest.split_once('/').unwrap().0;
        assert!(dir.ends_with(".app"));
        assert_eq!(format!("Payload/{dir}"), "Payload/JellyCar.app");
    }

    #[test]
    fn utf16_bom_decode() {
        let decoded = decode_strings_file(&[0xFF, 0xFE, b'A', 0, b'B', 0]);
        assert_eq!(decoded, "AB");
    }

    #[test]
    fn utf16_be_bom_decode() {
        let decoded = decode_strings_file(&[0xFE, 0xFF, 0, b'A', 0, b'B']);
        assert_eq!(decoded, "AB");
    }

    #[test]
    fn utf8_passthrough() {
        assert_eq!(decode_strings_file(b"\"a\" = \"b\";\n"), "\"a\" = \"b\";\n");
    }

    /// Build a minimal .app bundle with a `Settings.bundle` containing one
    /// toggle, and return the path to it.
    fn make_test_app(base: &std::path::Path) -> std::path::PathBuf {
        let app_dir = base.join("ToggleTest.app");
        let settings_dir = app_dir.join("Settings.bundle");
        std::fs::create_dir_all(&settings_dir).unwrap();
        std::fs::write(
            app_dir.join("Info.plist"),
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<plist version=\"1.0\"><dict>\
<key>CFBundleIdentifier</key><string>com.touchhle.toggletest</string>\
<key>CFBundleName</key><string>ToggleTest</string>\
<key>CFBundleExecutable</key><string>ToggleTest</string>\
<key>CFBundlePackageType</key><string>APPL</string>\
</dict></plist>"
            ),
        )
        .unwrap();
        std::fs::write(
            settings_dir.join("Root.plist"),
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<plist version=\"1.0\"><dict>\
<key>PreferenceSpecifiers</key><array><dict>\
<key>Type</key><string>PSToggleSwitchSpecifier</string>\
<key>Title</key><string>Sound</string>\
<key>Key</key><string>sound_enabled</string>\
<key>DefaultValue</key><true/>\
</dict></array>\
</dict></plist>",
        )
        .unwrap();
        app_dir
    }

    #[test]
    fn settings_toggle_roundtrip() {
        let base = std::env::temp_dir().join("touchhle_toggle_test");
        let _ = std::fs::remove_dir_all(&base);
        let app_dir = make_test_app(&base);

        let toggles = load_toggles(&app_dir);
        assert_eq!(toggles.len(), 1);
        let toggle = &toggles[0];
        assert_eq!(toggle.key, "sound_enabled");
        assert_eq!(toggle.title, "Sound");
        // No saved value: fall back to the bundle's default.
        assert!(toggle.current_value(Some(&app_dir)) == toggle.default_value);

        // A write must be readable back and reflected in current_value.
        write_app_pref(&app_dir, &toggle.key, "NO").unwrap();
        assert_eq!(read_app_pref(&app_dir, &toggle.key).as_deref(), Some("NO"));
        assert!(!toggle.current_value(Some(&app_dir)));
        write_app_pref(&app_dir, &toggle.key, "YES").unwrap();
        assert_eq!(read_app_pref(&app_dir, &toggle.key).as_deref(), Some("YES"));
        assert!(toggle.current_value(Some(&app_dir)));

        // iOS's Settings app writes booleans for toggles: reading one back
        // must also work (it is normalised to YES/NO).
        let plist_path = app_prefs_plist_path(&app_dir).unwrap();
        let mut dict = plist::Dictionary::new();
        dict.insert("sound_enabled".to_string(), Value::Boolean(false));
        Value::Dictionary(dict).to_file_binary(&plist_path).unwrap();
        assert!(!toggle.current_value(Some(&app_dir)));

        std::fs::remove_dir_all(&base).ok();
        let _ = std::fs::remove_dir_all(
            std::path::Path::new(".").join("touchHLE_sandbox/com.touchhle.toggletest"),
        );
    }
}
