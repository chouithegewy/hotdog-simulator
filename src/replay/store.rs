// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Where replays come from and where single player's go.
//!
//! * **Natively** a replay is a file: `--replay <file>` opens one, and single player's recordings are written into
//!   `replays/` beside the settings file (prefs.rs: `~/.config/hotdog-simulator/replays/`; `--replays-dir <dir>` or
//!   `HOTDOG_REPLAYS_DIR` names another folder, and `--no-record` or `HOTDOG_NO_RECORD` records nothing: tests,
//!   screenshots and scratch runs use them), the newest KEEP_LOCAL of its own recordings kept (only those: a file
//!   put there by hand is never deleted). Watching one from the list starts the game again on it.
//! * **In a browser** `?replay=` names one: an id of the server's (`/replays/<id>`), an address, or `local:<name>` for
//!   one of this browser's own. Single player's recordings are kept in the page's IndexedDB (the newest KEEP_BROWSER),
//!   written when a round ends and every so often while it lasts, and each can be downloaded as a file. Watching one
//!   loads the page again with `?replay=`.
#[cfg(not(target_arch = "wasm32"))]
use kitchen_sim::replay::{self, Listing};
use serde::{Deserialize, Serialize};

/// What a recording made here is written into: natively a file, in a browser its bytes (kept in IndexedDB).
#[cfg(not(target_arch = "wasm32"))]
pub type Out = std::io::BufWriter<std::fs::File>;
#[cfg(target_arch = "wasm32")]
pub type Out = Vec<u8>;
/// What this build writes in a replay's header, and compares a replay's with.
pub const BUILD: &str = concat!("hotdog-simulator ", env!("CARGO_PKG_VERSION"));
/// How many of single player's own recordings are kept.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub const KEEP_LOCAL: usize = 20;
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub const KEEP_BROWSER: usize = 6;

/// A replay as a list shows it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Entry {
    /// What opens it: a file's path, `local:<name>` (this browser's), or the server's id.
    pub key: String,
    /// From the server (not this machine's own).
    pub server: bool,
    pub map: String,
    pub mode: String,
    pub started: u64,
    pub duration_ms: u32,
    pub winner: Option<String>,
    pub players: Vec<String>,
    pub complete: bool,
}
#[cfg(not(target_arch = "wasm32"))]
impl Entry {
    /// What a listing says of a file.
    pub fn of(key: String, listing: &Listing) -> Entry {
        let h = &listing.header;
        Entry {
            key,
            server: false,
            map: h.map_name.clone(),
            mode: h.mode.clone(),
            started: h.started,
            duration_ms: listing.summary.as_ref().map_or(listing.duration_ms, |s| s.duration_ms),
            winner: listing.summary.as_ref().and_then(|s| s.winner.clone()),
            players: h.roster.iter().map(|m| m.name.clone()).collect(),
            complete: !listing.truncated,
        }
    }
}

/// Unix seconds now.
pub fn now() -> u64 {
    #[cfg(target_arch = "wasm32")]
    return (web::replay_now() / 1000.) as u64;
    #[cfg(not(target_arch = "wasm32"))]
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}
/// The calendar date and time (UTC) of Unix second `secs`: year, month, day, hour, minute, second.
pub fn civil(secs: u64) -> (i64, u32, u32, u32, u32, u32) {
    let days = (secs / 86400) as i64;
    let rem = secs % 86400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + (m <= 2) as i64, m, d, (rem / 3600) as u32, (rem % 3600 / 60) as u32, (rem % 60) as u32)
}
/// A name for a new recording: when (UTC) and on what map.
pub fn name(started: u64, map: &str) -> String {
    let (y, m, d, h, mi, s) = civil(started);
    let slug: String = map.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    let slug = slug.split('-').filter(|w| !w.is_empty()).collect::<Vec<_>>().join("-");
    format!("{y:04}{m:02}{d:02}-{h:02}{mi:02}{s:02}-{}", if slug.is_empty() { "map" } else { &slug })
}

// ----- Natively: files.

/// Where `--replays-dir` (or `HOTDOG_REPLAYS_DIR`) puts single player's recordings, set when the game starts
/// (runtime.rs `Options::replays_dir`).
#[cfg(not(target_arch = "wasm32"))]
static DIR: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
#[cfg(not(target_arch = "wasm32"))]
pub fn configure(dir: &str) {
    if !dir.is_empty() {
        let _ = DIR.set(dir.into());
    }
}
#[cfg(target_arch = "wasm32")]
pub fn configure(_dir: &str) {}
/// The folder single player's recordings go to (and the list reads): `--replays-dir`, or `replays/` beside the
/// settings file.
#[cfg(not(target_arch = "wasm32"))]
pub fn local_dir() -> Option<std::path::PathBuf> {
    // (The tests keep theirs beside the test binary, in the build's own directory: never the player's own folder.)
    if cfg!(test) {
        return Some(std::env::current_exe().ok()?.with_file_name(format!("replays-test-{}", std::process::id())));
    }
    if let Some(dir) = DIR.get() {
        return Some(dir.clone());
    }
    Some(crate::prefs::settings_file()?.parent()?.join("replays"))
}
/// Whether a recording's key or name is one of this client's own views of an online match (pov.rs): not a replay to
/// watch on its own (it holds no game), so the list leaves it out.
pub fn view_only(key: &str) -> bool {
    key.trim_end_matches(".replay").ends_with("-view")
}
/// The replays in the local folder, newest first.
#[cfg(not(target_arch = "wasm32"))]
pub fn list_local() -> Vec<Entry> {
    let Some(dir) = local_dir() else { return vec![] };
    let Ok(read) = std::fs::read_dir(&dir) else { return vec![] };
    let mut out: Vec<Entry> = read
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == replay::EXTENSION) && !view_only(&p.to_string_lossy()))
        .filter_map(|p| {
            let mut file = std::fs::File::open(&p).ok()?;
            let listing = replay::scan(&mut file).ok()?;
            Some(Entry::of(p.to_string_lossy().into(), &listing))
        })
        .collect();
    out.sort_by(|a, b| b.started.cmp(&a.started).then_with(|| b.key.cmp(&a.key)));
    out
}
/// Deletes the oldest of single player's own recordings in the local folder (`*-solo.replay`, as record.rs names them)
/// beyond KEEP_LOCAL (not `keep`, the one being written). Nothing else in the folder is touched.
#[cfg(not(target_arch = "wasm32"))]
pub fn prune(keep: Option<&std::path::Path>) {
    prune_own("-solo", keep);
}
/// The same for this client's own views of online matches (`*-view.replay`, pov.rs): the newest KEEP_LOCAL kept.
#[cfg(not(target_arch = "wasm32"))]
pub fn prune_views(keep: Option<&std::path::Path>) {
    prune_own("-view", keep);
}
#[cfg(not(target_arch = "wasm32"))]
fn prune_own(ending: &str, keep: Option<&std::path::Path>) {
    let Some(dir) = local_dir() else { return };
    let Ok(read) = std::fs::read_dir(&dir) else { return };
    let mut files: Vec<(std::path::PathBuf, std::time::SystemTime)> = read
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == replay::EXTENSION) && p.file_stem().is_some_and(|s| s.to_string_lossy().ends_with(ending)) && Some(p.as_path()) != keep)
        .map(|p| {
            let at = std::fs::metadata(&p).and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
            (p, at)
        })
        .collect();
    let room = KEEP_LOCAL.saturating_sub(keep.is_some() as usize);
    if files.len() <= room {
        return;
    }
    files.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
    for (path, _) in &files[..files.len() - room] {
        let _ = std::fs::remove_file(path);
    }
}

// ----- Loading the replay to watch.

/// Starts loading what `?replay=` or `--replay` names. Natively it is read at once (`poll` has it).
pub fn fetch(source: &str) -> Loading {
    #[cfg(target_arch = "wasm32")]
    {
        web::replay_fetch(source);
        Loading::Waiting
    }
    #[cfg(not(target_arch = "wasm32"))]
    match std::fs::read(source) {
        Ok(bytes) => Loading::Done(Ok(bytes)),
        Err(error) => Loading::Done(Err(format!("{source} could not be read: {error}"))),
    }
}
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub enum Loading {
    Waiting,
    Done(Result<Vec<u8>, String>),
}
/// What a load has come to: Waiting until it has, then the bytes or why not.
pub fn poll(loading: &mut Loading) -> Option<Result<Vec<u8>, String>> {
    match loading {
        Loading::Done(_) => match std::mem::replace(loading, Loading::Waiting) {
            Loading::Done(done) => Some(done),
            Loading::Waiting => None,
        },
        Loading::Waiting => {
            #[cfg(target_arch = "wasm32")]
            {
                let failed = web::replay_failed();
                if !failed.is_empty() {
                    return Some(Err(failed));
                }
                let bytes = web::replay_fetched();
                if !bytes.is_empty() {
                    return Some(Ok(bytes));
                }
            }
            None
        }
    }
}

// ----- Watching one from a list, keeping and downloading single player's.

/// Opens a replay in the viewer: natively the game starts again on it (and this one ends), in a browser the page loads
/// with `?replay=`.
pub fn watch(world: &mut bevy::prelude::World, key: &str) {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = world;
        web::replay_open(key);
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let Ok(exe) = std::env::current_exe() else { return };
        match std::process::Command::new(exe).arg("--replay").arg(key).spawn() {
            Ok(_) => {
                world.write_message(bevy::app::AppExit::Success);
            }
            Err(error) => bevy::log::warn!("The replay viewer could not be started: {error}"),
        }
    }
}
/// Keeps single player's recording `name` (bytes so far) in this browser, with what a list shows of it. (This client's
/// own views of online matches, `*-view`, are kept the newest KEEP_BROWSER of their own.)
#[cfg(target_arch = "wasm32")]
pub fn keep(name: &str, entry: &Entry, bytes: &[u8]) {
    web::replay_store(name, &serde_json::to_string(entry).unwrap_or_default(), bytes, KEEP_BROWSER as u32);
}
/// This browser's own recordings, newest first (as last read from its storage: `refresh` reads it again).
#[cfg(target_arch = "wasm32")]
pub fn list_local() -> Vec<Entry> {
    let mut out: Vec<Entry> = serde_json::from_str::<Vec<Entry>>(&web::replay_local()).unwrap_or_default().into_iter().filter(|e| !view_only(&e.key)).collect();
    out.sort_by(|a, b| b.started.cmp(&a.started));
    out
}
/// Asks again what this browser keeps and what the server has.
pub fn refresh() {
    #[cfg(target_arch = "wasm32")]
    web::replay_refresh();
}
/// The server's replays, once they have come (None while they are on their way, or with no server).
pub fn list_server() -> Option<Vec<Entry>> {
    #[cfg(target_arch = "wasm32")]
    {
        #[derive(Deserialize)]
        struct Served {
            id: String,
            started: u64,
            mode: String,
            map: String,
            players: Vec<String>,
            duration: f64,
            winner: Option<String>,
            complete: bool,
        }
        let text = web::replay_server();
        if text.is_empty() {
            return None;
        }
        let served: Vec<Served> = serde_json::from_str::<serde_json::Value>(&text).ok().and_then(|v| serde_json::from_value(v["replays"].clone()).ok()).unwrap_or_default();
        Some(
            served
                .into_iter()
                .map(|s| Entry { key: s.id, server: true, map: s.map, mode: s.mode, started: s.started, duration_ms: (s.duration * 1000.) as u32, winner: s.winner, players: s.players, complete: s.complete })
                .collect(),
        )
    }
    #[cfg(not(target_arch = "wasm32"))]
    None
}
/// Offers a replay as a file to save: this browser's own (`local:<name>`) or the server's.
pub fn download(key: &str) {
    #[cfg(target_arch = "wasm32")]
    web::replay_download(key);
    #[cfg(not(target_arch = "wasm32"))]
    let _ = key;
}
/// While a replay is watched in a browser, the keys it uses are kept from the page (Tab would move the focus, / find
/// text, Space and the arrows scroll).
pub fn hold_keys() {
    #[cfg(target_arch = "wasm32")]
    web::replay_keys();
}

#[cfg(target_arch = "wasm32")]
mod web {
    #[wasm_bindgen::prelude::wasm_bindgen(inline_js = "
const DB='hotdog-replays',STORE='replays';let got=null,failed='',local='[]',server='';
function open(){return new Promise((ok,no)=>{const r=indexedDB.open(DB,1);r.onupgradeneeded=()=>r.result.createObjectStore(STORE);r.onsuccess=()=>ok(r.result);r.onerror=()=>no(r.error)})}
function each(db,mode,work){return new Promise((ok,no)=>{const t=db.transaction(STORE,mode),s=t.objectStore(STORE);const out=work(s);t.oncomplete=()=>ok(out&&out.result);t.onerror=()=>no(t.error)})}
export function replay_now(){return Date.now()}
export function replay_fetch(source){got=null;failed='';
 if(source.startsWith('local:')){const key=source.slice(6);open().then(db=>each(db,'readonly',s=>s.get(key))).then(v=>{if(!v)throw new Error('This browser has no replay '+key);got=new Uint8Array(v.bytes)}).catch(e=>{failed=String(e&&e.message||e)});return}
 const url=/^[A-Za-z0-9_-]+$/.test(source)?'replays/'+source:source;
 fetch(url).then(r=>{if(!r.ok)throw new Error(url+': HTTP '+r.status);return r.arrayBuffer()}).then(b=>{got=new Uint8Array(b)}).catch(e=>{failed=String(e&&e.message||e)})}
export function replay_fetched(){if(!got)return new Uint8Array(0);const b=got;got=null;return b}
export function replay_failed(){return failed}
export function replay_refresh(){
 open().then(db=>each(db,'readonly',s=>s.getAll())).then(all=>{local=JSON.stringify((all||[]).map(v=>v.entry))}).catch(()=>{});
 const none=JSON.stringify({replays:[]});fetch('replays').then(r=>r.ok?r.text():none).then(t=>{server=t}).catch(()=>{server=none})}
export function replay_local(){return local}
export function replay_server(){return server}
export function replay_store(name,entry,bytes,keep){const e=JSON.parse(entry);e.key='local:'+name;const copy=bytes.slice().buffer;
 open().then(db=>each(db,'readwrite',s=>s.put({entry:e,bytes:copy},name)).then(()=>each(db,'readonly',s=>s.getAll())).then(all=>{
  const kind=k=>String(k).endsWith('-view');const old=(all||[]).map(v=>v.entry).filter(o=>kind(o.key)===kind(e.key)).sort((a,b)=>b.started-a.started).slice(keep);
  return each(db,'readwrite',s=>{for(const o of old)s.delete(String(o.key).slice(6))})})).then(()=>replay_refresh()).catch(e=>console.warn('A replay could not be kept',e))}
export function replay_download(key){const save=(bytes,name)=>{const a=document.createElement('a');a.href=URL.createObjectURL(new Blob([bytes],{type:'application/octet-stream'}));a.download=name;document.body.append(a);a.click();a.remove();setTimeout(()=>URL.revokeObjectURL(a.href),10000)};
 if(key.startsWith('local:')){const name=key.slice(6);open().then(db=>each(db,'readonly',s=>s.get(name))).then(v=>{if(v)save(v.bytes,name+'.replay')}).catch(()=>{});return}
 const a=document.createElement('a');a.href='replays/'+key;a.download=key+'.replay';document.body.append(a);a.click();a.remove()}
export function replay_open(key){const q=new URLSearchParams(location.search);for(const k of ['mp','server','demo','sandbox','replay'])q.delete(k);q.set('replay',key);location.href=location.pathname+'?'+q.toString()}
let held=false;export function replay_keys(){if(held)return;held=true;window.addEventListener('keydown',e=>{if(['Tab','Slash','Space','ArrowUp','ArrowDown','ArrowLeft','ArrowRight','F1','PageUp','PageDown'].includes(e.code)&&!(e.target instanceof HTMLInputElement))e.preventDefault()},true)}
")]
    extern "C" {
        pub fn replay_now() -> f64;
        pub fn replay_fetch(source: &str);
        pub fn replay_fetched() -> Vec<u8>;
        pub fn replay_failed() -> String;
        pub fn replay_refresh();
        pub fn replay_local() -> String;
        pub fn replay_server() -> String;
        pub fn replay_store(name: &str, entry: &str, bytes: &[u8], keep: u32);
        pub fn replay_download(key: &str);
        pub fn replay_open(key: &str);
        pub fn replay_keys();
    }
}

/// The tests share the one test folder: one at a time.
#[cfg(test)]
pub(crate) static FOLDER: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    /// The folder keeps the newest KEEP_LOCAL of single player's own recordings, deletes the oldest of them first, and
    /// never touches anything else in it; and the tests' folder is beside the test binary, never the player's own.
    #[test]
    fn the_local_folder_keeps_the_newest_of_its_own_recordings_and_nothing_else_is_touched() {
        let _one = super::FOLDER.lock().unwrap_or_else(|e| e.into_inner());
        let dir = super::local_dir().unwrap();
        assert!(dir.starts_with(std::env::current_exe().unwrap().parent().unwrap()), "{}", dir.display());
        let home = std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_default();
        assert!(home.as_os_str().is_empty() || !dir.starts_with(home.join(".config")));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for i in 0..25u64 {
            let path = dir.join(format!("2026010{}-{i:06}-map-solo.replay", i / 10));
            std::fs::write(&path, b"x").unwrap();
            std::fs::File::options().write(true).open(&path).unwrap().set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_800_000_000 + i * 60)).unwrap();
        }
        std::fs::write(dir.join("downloaded-from-a-server.replay"), b"x").unwrap();
        std::fs::write(dir.join("notes.txt"), b"x").unwrap();
        super::prune(None);
        let mut left: Vec<String> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into()).collect();
        left.sort();
        assert_eq!(left.len(), super::KEEP_LOCAL + 2, "{left:?}");
        assert!(left.contains(&"downloaded-from-a-server.replay".to_string()) && left.contains(&"notes.txt".to_string()));
        assert!(!left.iter().any(|n| n.contains("000000-") || n.contains("000004-")), "the oldest went first: {left:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }
    #[test]
    fn a_recordings_name_says_when_and_where() {
        assert_eq!(super::name(1_763_472_896, "Counter Siege!"), "20251118-133456-counter-siege");
        assert_eq!(super::name(0, "?"), "19700101-000000-map");
    }
}
