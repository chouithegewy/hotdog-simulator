// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The replays list: the Esc menu's REPLAYS (menu.rs), over the menu, which stays open beneath it (single player stays
//! paused, the game hears nothing; Esc closes both). It lists this machine's own recordings (single player's: record.rs)
//! and, in a browser on the game's server, the server's (every match it played); WATCH opens one in the viewer
//! (store.rs `watch`), SAVE (in a browser) downloads it. The switches at its top turn single player's recording on or
//! off, and whether this player's own view of an online match is sent to the server for that match's recording
//! (pov.rs: SHARE MY VIEW IN MATCH RECORDINGS).
use super::store::{self, Entry};
use bevy::prelude::*;
use bevy::ui::FocusPolicy;

const WHITE: Color = Color::srgb(0.96, 0.97, 0.93);
const DIM: Color = Color::srgb(0.62, 0.67, 0.66);
const FAINT: Color = Color::srgb(0.40, 0.44, 0.44);
const ACCENT: Color = Color::srgb(1.0, 0.80, 0.30);
const ON_ACCENT: Color = Color::srgb(0.10, 0.08, 0.03);
/// The most rows shown of each source.
const ROWS: usize = 9;

#[derive(Resource, Default)]
pub struct List {
    pub open: bool,
    root: Option<Entity>,
    built: String,
    pub local: Vec<Entry>,
    pub server: Option<Vec<Entry>>,
}
#[derive(Component, Clone, Debug, PartialEq)]
enum Hit {
    Close,
    Record,
    Share,
    Watch(String),
    Save(String),
}

/// Opens the list (the menu's REPLAYS), reading what there is again.
pub fn open(world: &mut World) {
    store::refresh();
    let mut list = world.resource_mut::<List>();
    list.open = true;
    list.built.clear();
    list.local = store::list_local();
    list.server = store::list_server();
}
/// `m:ss`.
fn length(ms: u32) -> String {
    let s = ms / 1000;
    format!("{}:{:02}", s / 60, s % 60)
}
/// What a row says of a replay: when, where, what, how long, and who won.
pub fn describe(e: &Entry) -> (String, String) {
    let (y, m, d, h, mi, _) = store::civil(e.started);
    let mode = match e.mode.as_str() {
        "forts" => "Forts",
        "ffa" => "Free For All",
        "cards" => "Fully Loaded",
        "kungfu" => "Kung Fu",
        _ => "Siege",
    };
    let first = format!("{y}-{m:02}-{d:02} {h:02}:{mi:02}  {}  {mode}  {}", e.map, length(e.duration_ms));
    let mut second = e.players.join(", ");
    if let Some(w) = &e.winner {
        second = format!("{w} won  /  {second}");
    }
    if !e.complete {
        second = format!("(cut short)  {second}");
    }
    let clean = |s: String| s.chars().map(|c| if c.is_ascii() && !c.is_ascii_control() { c.to_ascii_uppercase() } else { '?' }).collect::<String>();
    (clean(first), clean(second))
}

/// Every frame: the list built (again when what it shows changes), its clicks heard; it closes with the menu.
pub fn update(world: &mut World) {
    let menu = world.get_resource::<crate::menu::Menu>().is_some_and(|m| m.open);
    let Some(mut list) = world.remove_resource::<List>() else { return };
    if list.open && !menu {
        list.open = false;
    }
    if list.server.is_none() && list.open {
        list.server = store::list_server();
        #[cfg(target_arch = "wasm32")]
        {
            let local = store::list_local();
            if local != list.local {
                list.local = local;
            }
        }
    }
    let on = world.get_resource::<super::record::Recorder>().is_none_or(|r| r.on);
    let barred = world.get_resource::<crate::runtime::Options>().is_some_and(|o| !o.record);
    let share = Share { on: world.get_resource::<super::pov::Pov>().is_none_or(|p| p.share), barred: world.get_resource::<crate::runtime::Options>().is_some_and(|o| !o.pov) };
    let scale = world.query::<&Window>().iter(world).next().map_or(1., |w| (w.height() / 720.).min(w.width() / 860.).clamp(0.6, 3.));
    let sig = format!("{}{on}{barred}{share:?}{scale}{:?}{:?}", list.open, list.local, list.server);
    if sig != list.built {
        list.built = sig;
        if let Some(root) = list.root.take()
            && let Ok(e) = world.get_entity_mut(root)
        {
            e.despawn();
        }
        if list.open {
            list.root = Some(build(world, &list, on, barred, share, scale));
        }
    }
    // Clicks: a press of one of the list's buttons (read from the UI's interactions: the menu's gate keeps the mouse
    // itself from the game while it is open).
    let pressed: Vec<Hit> = world.query_filtered::<(&Hit, &Interaction), Changed<Interaction>>().iter(world).filter(|(_, i)| **i == Interaction::Pressed).map(|(h, _)| h.clone()).collect();
    for hit in pressed {
        match hit {
            Hit::Close => list.open = false,
            Hit::Record => {
                world.resource_scope(|world, mut r: Mut<super::record::Recorder>| {
                    let mut prefs = world.get_resource_mut::<crate::prefs::Prefs>();
                    r.set(prefs.as_deref_mut(), !on);
                });
            }
            Hit::Share => {
                world.resource_scope(|world, mut p: Mut<super::pov::Pov>| {
                    let mut prefs = world.get_resource_mut::<crate::prefs::Prefs>();
                    p.set(prefs.as_deref_mut(), !share.on);
                });
            }
            Hit::Watch(key) => store::watch(world, &key),
            Hit::Save(key) => store::download(&key),
        }
    }
    world.insert_resource(list);
}

/// The SHARE MY VIEW switch: as kept, and whether this run keeps no view at all (`--no-pov`).
#[derive(Clone, Copy, Debug)]
struct Share {
    on: bool,
    barred: bool,
}
fn build(world: &mut World, list: &List, on: bool, barred: bool, share: Share, s: f32) -> Entity {
    let px = |v: f32| Val::Px((v * s).round());
    let font = |v: f32| TextFont { font_size: FontSize::Px((v * s).round()), ..default() };
    let text = |world: &mut World, words: &str, size: f32, color: Color| world.spawn((Text::new(words), font(size), TextColor(color), TextLayout::no_wrap())).id();
    let root = world
        .spawn((
            Name::new("Replays"),
            Node { position_type: PositionType::Absolute, left: Val::Px(0.), top: Val::Px(0.), width: Val::Percent(100.), height: Val::Percent(100.), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() },
            BackgroundColor(Color::srgba(0.008, 0.01, 0.014, 0.9)),
            FocusPolicy::Block,
            GlobalZIndex(150),
        ))
        .id();
    let panel = world
        .spawn((
            Node { width: px(760.), max_height: Val::Percent(92.), padding: UiRect::all(px(22.)), flex_direction: FlexDirection::Column, row_gap: px(6.), border: UiRect::all(px(1.)), border_radius: BorderRadius::all(px(12.)), overflow: Overflow::clip(), ..default() },
            BackgroundColor(Color::srgba(0.035, 0.042, 0.052, 0.985)),
            BorderColor::all(Color::srgba(1., 1., 1., 0.09)),
        ))
        .id();
    let button = |world: &mut World, hit: Hit, words: &str, lit: bool| -> Entity {
        let label = world.spawn((Text::new(words), font(12.), TextColor(if lit { ON_ACCENT } else { WHITE }), TextLayout::no_wrap())).id();
        let b = world
            .spawn((
                hit,
                Button,
                Node { height: px(28.), padding: UiRect::axes(px(12.), Val::Px(0.)), align_items: AlignItems::Center, border: UiRect::all(px(1.)), border_radius: BorderRadius::all(px(6.)), ..default() },
                BackgroundColor(if lit { ACCENT } else { Color::srgba(1., 1., 1., 0.05) }),
                BorderColor::all(Color::srgba(1., 1., 1., 0.12)),
            ))
            .id();
        world.entity_mut(b).add_child(label);
        b
    };
    let head = world.spawn(Node { align_items: AlignItems::Center, justify_content: JustifyContent::SpaceBetween, margin: UiRect::bottom(px(8.)), ..default() }).id();
    let title = text(world, "REPLAYS", 30., WHITE);
    let close = button(world, Hit::Close, "CLOSE", false);
    world.entity_mut(head).add_children(&[title, close]);
    let switch = world.spawn(Node { align_items: AlignItems::Center, column_gap: px(12.), margin: UiRect::bottom(px(10.)), ..default() }).id();
    let words = text(world, "RECORD REPLAYS", 13., DIM);
    let flip = button(world, Hit::Record, if on { "ON" } else { "OFF" }, on);
    let note = text(
        world,
        if barred {
            "OFF FOR THIS RUN (--no-record, HOTDOG_NO_RECORD OR ?norecord)"
        } else if cfg!(target_arch = "wasm32") {
            "SINGLE PLAYER, KEPT IN THIS BROWSER (THE NEWEST 6)"
        } else {
            "SINGLE PLAYER, KEPT ON THIS COMPUTER (THE NEWEST 20)"
        },
        10.,
        FAINT,
    );
    world.entity_mut(switch).add_children(&[words, flip, note]);
    let sharing = world.spawn(Node { align_items: AlignItems::Center, column_gap: px(12.), margin: UiRect::bottom(px(10.)), ..default() }).id();
    let words = text(world, "SHARE MY VIEW IN MATCH RECORDINGS", 13., DIM);
    let flip = button(world, Hit::Share, if share.on { "ON" } else { "OFF" }, share.on);
    let note = text(world, if share.barred { "NO VIEW KEPT FOR THIS RUN (--no-pov, HOTDOG_NO_POV OR ?nopov)" } else { "ONLINE: THE SERVER'S REPLAY SHOWS WHAT YOUR SCREEN SHOWED" }, 10., FAINT);
    world.entity_mut(sharing).add_children(&[words, flip, note]);
    world.entity_mut(panel).add_children(&[head, switch, sharing]);
    let section = |world: &mut World, heading: &str, entries: &[Entry], empty: &str| {
        let h = text(world, heading, 11., ACCENT);
        world.entity_mut(panel).add_child(h);
        if entries.is_empty() {
            let e = text(world, empty, 12., FAINT);
            world.entity_mut(panel).add_child(e);
        }
        for entry in entries.iter().take(ROWS) {
            let (first, second) = describe(entry);
            let row = world.spawn((Node { align_items: AlignItems::Center, column_gap: px(10.), padding: UiRect::axes(px(10.), px(5.)), border_radius: BorderRadius::all(px(6.)), ..default() }, BackgroundColor(Color::srgba(1., 1., 1., 0.035)))).id();
            let words = world.spawn(Node { flex_direction: FlexDirection::Column, flex_grow: 1., row_gap: px(2.), overflow: Overflow::clip(), ..default() }).id();
            let a = text(world, &first, 13., WHITE);
            let b = text(world, &second, 10., DIM);
            world.entity_mut(words).add_children(&[a, b]);
            let watch = button(world, Hit::Watch(entry.key.clone()), "WATCH", true);
            world.entity_mut(row).add_children(&[words, watch]);
            if cfg!(target_arch = "wasm32") {
                let save = button(world, Hit::Save(entry.key.clone()), "SAVE", false);
                world.entity_mut(row).add_child(save);
            }
            world.entity_mut(panel).add_child(row);
        }
    };
    section(world, "MY GAMES", &list.local, "NOTHING RECORDED YET: PLAY A ROUND OF SINGLE PLAYER");
    if cfg!(target_arch = "wasm32") {
        match &list.server {
            Some(server) => section(world, "THIS SERVER'S MATCHES", server, "NONE: THE SERVER HAS RECORDED NO MATCH (OR THIS PAGE HAS NO SERVER)"),
            None => section(world, "THIS SERVER'S MATCHES", &[], "ASKING THE SERVER..."),
        }
    }
    let foot = text(world, if cfg!(target_arch = "wasm32") { "A REPLAY OPENS IN THIS TAB   ESC: BACK TO THE GAME" } else { "WATCH STARTS THE GAME AGAIN ON THE REPLAY   ESC: BACK TO THE GAME" }, 10., FAINT);
    world.entity_mut(foot).insert(Node { margin: UiRect::top(px(12.)), ..default() });
    world.entity_mut(panel).add_child(foot);
    world.entity_mut(root).add_child(panel);
    root
}
