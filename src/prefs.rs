// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The player's choices that are kept from one game to the next (whether the hints are on, hints.rs; a settings menu's
//! choices may join them), as text by key.
//!
//! * In a browser, in its local storage as `hotdog.<key>`, as the page keeps its own (lobby.js, touch.js).
//! * Natively, in one small JSON object in the user's configuration directory: `$XDG_CONFIG_HOME/hotdog-simulator/
//!   settings.json`, or `~/.config/hotdog-simulator/settings.json`, or on Windows `%APPDATA%\hotdog-simulator\
//!   settings.json`; `HOTDOG_SETTINGS` names another file. A file that cannot be read is no choices, and one that
//!   cannot be written is left as it was: nothing here ever stops the game.
//! * The tests keep theirs in memory (`Prefs::memory`), so that they never touch the player's own.
use bevy::prelude::*;
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;

#[derive(Resource)]
pub struct Prefs {
    store: Store,
}
enum Store {
    #[cfg(target_arch = "wasm32")]
    Browser,
    #[cfg(not(target_arch = "wasm32"))]
    File(Option<PathBuf>),
    Memory(serde_json::Map<String, serde_json::Value>),
}
impl Default for Prefs {
    fn default() -> Self {
        if cfg!(test) { Self::memory() } else { Self::system() }
    }
}
impl Prefs {
    /// Where this build keeps choices: the browser's storage, or the settings file.
    pub fn system() -> Self {
        #[cfg(target_arch = "wasm32")]
        return Self { store: Store::Browser };
        #[cfg(not(target_arch = "wasm32"))]
        Self { store: Store::File(settings_file()) }
    }
    /// Choices kept only while the game runs.
    pub fn memory() -> Self {
        Self { store: Store::Memory(Default::default()) }
    }
    /// Choices kept in the JSON file at `path`.
    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub fn file(path: PathBuf) -> Self {
        Self { store: Store::File(Some(path)) }
    }
    /// The choice kept under `key`, if any.
    pub fn get(&self, key: &str) -> Option<String> {
        match &self.store {
            #[cfg(target_arch = "wasm32")]
            Store::Browser => web::prefs_get(key),
            #[cfg(not(target_arch = "wasm32"))]
            Store::File(path) => read(path.as_ref()?).get(key)?.as_str().map(str::to_string),
            Store::Memory(map) => map.get(key)?.as_str().map(str::to_string),
        }
    }
    /// Keeps `value` under `key`.
    pub fn set(&mut self, key: &str, value: &str) {
        match &mut self.store {
            #[cfg(target_arch = "wasm32")]
            Store::Browser => web::prefs_set(key, value),
            #[cfg(not(target_arch = "wasm32"))]
            Store::File(path) => {
                let Some(path) = path else { return };
                let mut all = read(path);
                all.insert(key.into(), value.into());
                if let Some(dir) = path.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                if let Ok(text) = serde_json::to_string_pretty(&serde_json::Value::Object(all)) {
                    let _ = std::fs::write(path, text + "\n");
                }
            }
            Store::Memory(map) => {
                map.insert(key.into(), value.into());
            }
        }
    }
}

/// The settings file's choices (none when it is missing or not a JSON object).
#[cfg(not(target_arch = "wasm32"))]
fn read(path: &PathBuf) -> serde_json::Map<String, serde_json::Value> {
    std::fs::read_to_string(path).ok().and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok()).and_then(|v| v.as_object().cloned()).unwrap_or_default()
}
/// Where the native game keeps its settings.
#[cfg(not(target_arch = "wasm32"))]
pub fn settings_file() -> Option<PathBuf> {
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from);
    if let Some(file) = var("HOTDOG_SETTINGS") {
        return Some(file);
    }
    let dir = var("XDG_CONFIG_HOME").or_else(|| var("APPDATA")).or_else(|| var("HOME").map(|home| home.join(".config")))?;
    Some(dir.join("hotdog-simulator").join("settings.json"))
}

#[cfg(target_arch = "wasm32")]
mod web {
    pub fn prefs_get(key: &str) -> Option<String> {
        let value = prefs_load(key);
        (!value.is_empty()).then_some(value)
    }
    pub fn prefs_set(key: &str, value: &str) {
        prefs_store(key, value);
    }
    #[wasm_bindgen::prelude::wasm_bindgen(inline_js = "
export function prefs_load(key){try{return localStorage.getItem(`hotdog.${key}`)||''}catch{return ''}}
export function prefs_store(key,value){try{localStorage.setItem(`hotdog.${key}`,value)}catch{}}
")]
    extern "C" {
        fn prefs_load(key: &str) -> String;
        fn prefs_store(key: &str, value: &str);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choices_are_kept_by_key_in_memory_and_in_a_file() {
        let mut prefs = Prefs::memory();
        assert_eq!(prefs.get("hints"), None);
        prefs.set("hints", "on");
        assert_eq!(prefs.get("hints").as_deref(), Some("on"));
        // A file: kept across two stores of the same file, other keys left alone, and a broken file is no choices.
        // (Next to the test binary, in the build's own directory.)
        let path = std::env::current_exe().unwrap().with_file_name(format!("hotdog-prefs-test-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let mut a = Prefs::file(path.clone());
        assert_eq!(a.get("hints"), None);
        a.set("hints", "off");
        a.set("other", "1");
        let b = Prefs::file(path.clone());
        assert_eq!((b.get("hints").as_deref(), b.get("other").as_deref()), (Some("off"), Some("1")));
        std::fs::write(&path, "not json").unwrap();
        assert_eq!(Prefs::file(path.clone()).get("hints"), None);
        let _ = std::fs::remove_file(&path);
        // The tests never touch the player's own file.
        assert!(matches!(Prefs::default().store, Store::Memory(_)));
    }
}
