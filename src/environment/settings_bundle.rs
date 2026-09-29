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
//! the app picker's Quick options panel, writing the same keys the app reads.
//! That way the guest does not need to know anything about us: it just sees the
//! preference key it asked for.

use crate::fs::BundleData;
use crate::objc::{id, msg_class};
use crate::Environment;
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
    /// Read the current value from the guest's defaults, falling back to the
    /// bundle's default.
    pub fn current_value(&self, env: &mut Environment) -> bool {
        let user_defaults: id = msg_class![env; NSUserDefaults standardUserDefaults];
        let key: id = crate::frameworks::foundation::ns_string::from_rust_string(
            env,
            self.key.clone(),
        );
        let value: id = crate::objc::msg![env; user_defaults stringForKey:key];
        crate::objc::release(env, key);
        if value == crate::objc::nil {
            return self.default_value;
        }
        let value = crate::frameworks::foundation::ns_string::to_rust_string(env, value);
        // Anything that isn't the declared "off" value counts as "on", which
        // matches how these plists are normally written (YES/true/1 vs NO).
        !value.eq_ignore_ascii_case(&self.false_value)
    }

}

/// Parse the app's `Settings.bundle/Root.plist`, returning the toggles it
/// declares. Returns an empty list if the app has no settings bundle or it
/// cannot be parsed; this is a best-effort convenience, not something that
/// should ever take an app down.
pub fn load_toggles(app_path: &std::path::Path) -> Vec<SettingsToggle> {
    let Ok((_bundle, fs)) = BundleData::open_any(app_path).and_then(|data| {
        crate::bundle::Bundle::new_bundle_and_fs_from_host_path(data, /* read_only: */ true)
    }) else {
        return Vec::new();
    };

    // Settings.bundle is a directory inside the app bundle, not part of the
    // bundle's resource map, so it has to be read from the host filesystem.
    // For an .ipa it lives at Payload/<name>.app/Settings.bundle; touchHLE
    // reads the bundle itself by locating the single .app directory inside.
    let settings_root: String = match app_path.extension().and_then(|e| e.to_str()) {
        Some("ipa") => {
            let Some(app_dir) = single_app_dir_in_ipa(app_path) else {
                return Vec::new();
            };
            format!("{app_dir}/Settings.bundle/Root.plist")
        }
        _ => "Settings.bundle/Root.plist".to_string(),
    };

    let Ok(bytes) = fs.read(settings_root.as_str()) else {
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

/// Find the single `<name>.app` directory inside an .ipa (a zip). touchHLE
/// only ever looks at the first one, which is what we do here too.
fn single_app_dir_in_ipa(app_path: &std::path::Path) -> Option<String> {
    let file = std::fs::File::open(app_path).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;
    let mut app_dir = None;
    for i in 0..archive.len() {
        let Ok(name) = archive.by_index_raw(i).map(|f| f.name().to_string()) else {
            continue;
        };
        let Some(rest) = name.strip_prefix("Payload/") else {
            continue;
        };
        let Some((dir, _)) = rest.split_once('/') else {
            continue;
        };
        if dir.ends_with(".app") {
            app_dir = Some(format!("Payload/{dir}"));
            break;
        }
    }
    app_dir
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
}
