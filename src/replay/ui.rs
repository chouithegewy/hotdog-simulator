// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The viewer's screen, in the head-up display's look (a dark translucent panel, white text, the mustard accent,
//! sized from the window's height; ASCII only):
//!
//! * **Bottom**: play or pause, the four speeds, the time and the duration, the timeline (click or drag it to seek;
//!   kills and knockouts marked in red, the ends of rounds in gold), the view and whose it is.
//! * **Top left**: what is being watched (the map, the mode, when), and a word about the file when there is one
//!   (another build of the game, a recording cut short).
//! * **The stats** (Tab): the scoreboard at the clock and each player's line (kitchen_sim::replay `Line`).
//! * **The help** (?): every key of the viewer, as bound.
//! * **Overlays**: names over everyone, at any distance (T); damage numbers rising from whoever is hit (Y); tracers that
//!   linger along each shot (R); the map from above, top right (M).
//!
//! Hiding everything (U) hides the game's own display too, for clean footage.
use super::{Mode, SPEEDS, Viewer};
use crate::arena::Game;
use crate::controls::{Action, Bindings, Group};
use bevy::prelude::*;
use bevy::ui::{FocusPolicy, RelativeCursorPosition};
use kitchen_sim::replay::Mark;

const WHITE: Color = Color::srgb(0.96, 0.97, 0.93);
const DIM: Color = Color::srgb(0.62, 0.67, 0.66);
const FAINT: Color = Color::srgb(0.40, 0.44, 0.44);
const ACCENT: Color = Color::srgb(1.0, 0.80, 0.30);
const ON_ACCENT: Color = Color::srgb(0.10, 0.08, 0.03);
const RED: Color = Color::srgb(1.0, 0.36, 0.30);
const PANEL: Color = Color::srgba(0.035, 0.042, 0.052, 0.86);
const WELL: Color = Color::srgba(1., 1., 1., 0.07);
const EDGE: Color = Color::srgba(1., 1., 1., 0.10);
/// Seconds a damage number rises for, and a tracer lingers.
const NUMBER_TIME: f64 = 1.2;
const TRACER_TIME: f64 = 2.5;
/// The stats' columns and their widths (at a 720-pixel window).
const STATS_HEADS: [&str; 11] = ["PLAYER", "K", "D", "SCORE", "HP", "WEAPON", "AMMO", "ACCURACY", "DEALT", "TAKEN", "STREAK"];
const STATS_WIDTHS: [f32; 11] = [120., 30., 30., 50., 46., 150., 64., 104., 52., 52., 64.];
/// The columns a narrow window keeps: the player to the ammunition, or on a phone held upright the player, the kills,
/// the deaths, the score and the health.
const STATS_NARROW: usize = 7;
const STATS_FEWEST: usize = 5;
/// The least scale the stats are drawn at on a narrow window (a phone's screen scale is 0.6, too small to read there).
const STATS_LEGIBLE: f32 = 0.9;

/// Where the stats card begins, from the middle of the window (its panel's place): centred with every column; with
/// fewer (a narrow window, a phone held upright) at the left, 8 px in, since a phone held upright draws a little
/// wider than it shows and the right edge is the one cut off.
pub fn card_left(columns: usize, width: f32, window: f32) -> Val {
    if columns < STATS_HEADS.len() { Val::Px((8. - window / 2.).round()) } else { Val::Px((-width / 2.).round()) }
}
/// The stats in a window `window` wide at `scale`: how many columns, and the scale they are drawn at. Every column at
/// the screen's scale where they fit; on a narrower window as many of the first columns as fit at a legible size, with
/// a margin (a phone held upright draws a little more than it shows at the sides), and at the fewest made smaller to
/// fit if need be.
pub fn stats_fit(scale: f32, window: f32) -> (usize, f32) {
    let room = (window - 16.).max(100.);
    let width = |n: usize| STATS_WIDTHS[..n].iter().sum::<f32>() + 36.;
    if width(STATS_HEADS.len()) * scale <= room {
        return (STATS_HEADS.len(), scale);
    }
    let legible = scale.max(STATS_LEGIBLE);
    for n in [STATS_NARROW, STATS_FEWEST] {
        if width(n) * legible <= room * 0.92 {
            return (n, legible);
        }
    }
    (STATS_FEWEST, (room * 0.92 / width(STATS_FEWEST)).max(scale * 0.5))
}

/// What a click on the viewer does.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub enum Hit {
    Play,
    Speed(usize),
    Timeline,
    View,
    Stats,
    Help,
    Previous,
    Next,
}
#[derive(Component)]
struct Root;
#[derive(Component)]
enum Live {
    Play,
    Speed(usize),
    Clock,
    Fill,
    Knob,
    View,
    Who,
    Swatch,
    Stats,
    Help,
    Bar,
    Top,
    Map,
    Note,
}
#[derive(Component)]
struct Tag;
#[derive(Component)]
struct Rising;
#[derive(Component)]
struct TracerMesh;
#[derive(Component)]
struct Dot;

/// What was built, and what it was built for: the screen's scale and width, and the replay as it stood (loaded, failed,
/// and whether the page's touch controls have the bar instead: page.rs). The title, the note, the timeline's markers
/// and the map's place are built from the replay, so the screen is built again when its file comes (in a browser some
/// frames after the first, when the screen already says it is loading) or cannot be played.
#[derive(Default)]
pub struct Built {
    root: Option<Entity>,
    scale: f32,
    width: f32,
    state: (bool, bool, bool),
    stats: String,
    hidden: Vec<Entity>,
    dragging: bool,
    tracers: Vec<Entity>,
    meshes: Option<(Handle<Mesh>, Vec<Handle<StandardMaterial>>)>,
    map: String,
}

/// The width of the bar at the bottom (at a 720-pixel window).
const WIDTH: f32 = 640.;
/// The screen's size from the window: its height over 720, or less when the window is narrow for that (the stats are
/// 800 wide), so that everything fits.
pub fn scale_of(world: &mut World) -> f32 {
    world.query::<&Window>().iter(world).next().map_or(1., |w| (w.height() / 720.).min(w.width() / 1280.).clamp(0.6, 3.))
}
/// The window's width (logical pixels; 1280 with none).
fn width_of(world: &mut World) -> f32 {
    world.query::<&Window>().iter(world).next().map_or(1280., |w| w.width())
}
/// The bar's width at `scale` in a window `width` wide: WIDTH, or less where the window is narrower (a phone held
/// upright), with a margin either side.
pub fn bar_width(scale: f32, width: f32) -> f32 {
    WIDTH.min(((width - 24.) / scale.max(0.1)).max(200.))
}
/// `m:ss`, or `h:mm:ss`.
pub fn time(ms: f64) -> String {
    let s = (ms / 1000.).max(0.) as u64;
    if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
}
/// A speed as the buttons say it.
pub fn speed_name(i: usize) -> &'static str {
    ["0.25x", "0.5x", "1x", "2x"][i.min(3)]
}
/// The colour a player's mark is in, as its name tag's (tags.rs): its team's in Forts, the ring its player chose, or its
/// place's.
fn tint(p: &kitchen_sim::feast::Player, forts: bool, color: &str) -> Color {
    let [r, g, b] = crate::tags::colour(p, forts, crate::player_model::tag_rgb(color));
    Color::srgb(r, g, b)
}
fn ascii(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii() && !c.is_ascii_control() { c.to_ascii_uppercase() } else { '?' }).collect()
}

/// Whether the pointer is over one of the viewer's buttons or its bar (a drag there does not turn the view).
pub fn pointer_over(world: &mut World) -> bool {
    world.query_filtered::<&Interaction, Or<(With<Hit>, With<Root>)>>().iter(world).any(|i| *i != Interaction::None)
}

/// Every frame after the camera: the screen built once and brought up to date, and its clicks heard.
pub fn draw(world: &mut World, mut built: Local<Built>) {
    let scale = scale_of(world);
    let width = width_of(world);
    let Some(mut v) = world.remove_resource::<Viewer>() else { return };
    let state = (v.replay.is_some(), v.failed.is_some(), super::page::controls());
    if built.root.is_none_or(|r| world.get_entity(r).is_err()) || (built.scale - scale).abs() > 0.01 || (built.width - width).abs() > 0.5 || built.state != state {
        if let Some(root) = built.root.take()
            && let Ok(e) = world.get_entity_mut(root)
        {
            e.despawn();
        }
        built.root = Some(build(world, &v, scale, width, state.2));
        (built.scale, built.width, built.state) = (scale, width, state);
        built.stats.clear();
        built.map.clear();
    }
    clicks(world, &mut v, &mut built);
    refresh(world, &v, &mut built, scale);
    tracers(world, &v, &mut built);
    hide(world, &v, &mut built);
    world.insert_resource(v);
}

/// A text's shadow, as the head-up display's.
fn shadow(scale: f32) -> TextShadow {
    TextShadow { offset: Vec2::splat((1.2 * scale).max(1.)), color: Color::srgba(0., 0., 0., 0.7) }
}
fn px(scale: f32, base: f32) -> Val {
    Val::Px((base * scale).round())
}
fn font(scale: f32, base: f32) -> TextFont {
    TextFont { font_size: FontSize::Px((base * scale).round()), ..default() }
}
fn text(world: &mut World, scale: f32, words: &str, size: f32, color: Color) -> Entity {
    world.spawn((Text::new(words), font(scale, size), TextColor(color), TextLayout::no_wrap())).id()
}
fn button(world: &mut World, scale: f32, hit: Hit, words: &str, width: f32) -> Entity {
    let label = text(world, scale, words, 13., WHITE);
    let e = world
        .spawn((
            hit,
            Button,
            Node {
                min_width: px(scale, width),
                height: px(scale, 30.),
                padding: UiRect::axes(px(scale, 9.), Val::Px(0.)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(px(scale, 1.)),
                border_radius: BorderRadius::all(px(scale, 6.)),
                ..default()
            },
            BackgroundColor(WELL),
            BorderColor::all(EDGE),
        ))
        .id();
    world.entity_mut(e).add_child(label);
    e
}

/// The screen, built once (and again when the window's size changes, the replay comes or cannot be played, or the
/// page's touch controls take over the bar: `paged`, when the bottom is the page's, page.rs).
fn build(world: &mut World, v: &Viewer, scale: f32, window: f32, paged: bool) -> Entity {
    let s = scale;
    let wide = bar_width(s, window);
    let root = world
        .spawn((Root, Name::new("Replay"), Node { position_type: PositionType::Absolute, left: Val::Px(0.), top: Val::Px(0.), width: Val::Percent(100.), height: Val::Percent(100.), ..default() }, FocusPolicy::Pass, GlobalZIndex(60)))
        .id();
    // Top left: what this is.
    let top = world
        .spawn((Live::Top, Node { position_type: PositionType::Absolute, left: px(s, 16.), top: px(s, 14.), flex_direction: FlexDirection::Column, row_gap: px(s, 4.), ..default() }))
        .id();
    let badge = world.spawn((Node { padding: UiRect::axes(px(s, 8.), px(s, 3.)), border_radius: BorderRadius::all(px(s, 4.)), align_self: AlignSelf::FlexStart, ..default() }, BackgroundColor(ACCENT))).id();
    let badge_text = text(world, s, "REPLAY", 12., ON_ACCENT);
    world.entity_mut(badge).add_child(badge_text);
    let title = text(world, s, &title(v), 13., WHITE);
    world.entity_mut(top).add_children(&[badge, title]);
    if let Some(w) = v.warning.as_ref().or(v.failed.as_ref()) {
        let note = world.spawn((Live::Note, Text::new(ascii(w)), font(s, 12.), TextColor(if v.failed.is_some() { RED } else { ACCENT }), Node { max_width: px(s, 560.), ..default() })).id();
        world.entity_mut(top).add_child(note);
    }
    // The bottom middle (clear of the head-up display's corners): whose view, then the bar: the time and the timeline over
    // the buttons.
    let bottom = world
        .spawn((
            Node { position_type: PositionType::Absolute, left: Val::Percent(50.), bottom: px(s, 12.), width: px(s, wide), margin: UiRect::left(px(s, -wide / 2.)), flex_direction: FlexDirection::Column, row_gap: px(s, 6.), ..default() },
            if paged { Visibility::Hidden } else { Visibility::Inherited },
        ))
        .id();
    let who_row = world
        .spawn((Node { align_items: AlignItems::Center, align_self: AlignSelf::Center, column_gap: px(s, 8.), padding: UiRect::all(px(s, 4.)), border_radius: BorderRadius::all(px(s, 8.)), ..default() }, BackgroundColor(PANEL)))
        .id();
    let swatch = world.spawn((Live::Swatch, Node { width: px(s, 10.), height: px(s, 10.), border_radius: BorderRadius::all(px(s, 5.)), ..default() }, BackgroundColor(WHITE))).id();
    let who = world.spawn((Live::Who, Text::new(""), font(s, 14.), TextColor(WHITE), TextLayout::no_wrap())).id();
    let previous = button(world, s, Hit::Previous, "<", 28.);
    let next = button(world, s, Hit::Next, ">", 28.);
    world.entity_mut(who_row).add_children(&[previous, swatch, who, next]);
    let bar = world
        .spawn((
            Live::Bar,
            Root,
            Interaction::default(),
            FocusPolicy::Block,
            Node { flex_direction: FlexDirection::Column, row_gap: px(s, 4.), padding: UiRect::axes(px(s, 10.), px(s, 7.)), border_radius: BorderRadius::all(px(s, 10.)), ..default() },
            BackgroundColor(PANEL),
        ))
        .id();
    let clock = world.spawn((Live::Clock, Text::new("0:00 / 0:00"), font(s, 12.), TextColor(WHITE), TextLayout::no_wrap(), Node { min_width: px(s, 84.), ..default() })).id();
    // The timeline: a track, its fill, the markers and the knob.
    let line = world.spawn((Hit::Timeline, Button, RelativeCursorPosition::default(), Node { flex_grow: 1., height: px(s, 22.), align_items: AlignItems::Center, ..default() })).id();
    let track = world.spawn((Node { width: Val::Percent(100.), height: px(s, 6.), border_radius: BorderRadius::all(px(s, 3.)), ..default() }, BackgroundColor(Color::srgba(1., 1., 1., 0.16)))).id();
    let fill = world.spawn((Live::Fill, Node { width: Val::Percent(0.), height: Val::Percent(100.), border_radius: BorderRadius::all(px(s, 3.)), ..default() }, BackgroundColor(ACCENT))).id();
    world.entity_mut(track).add_child(fill);
    world.entity_mut(line).add_child(track);
    let duration = v.duration().max(1.);
    for m in v.markers() {
        let (color, tall) = if m.kind == Mark::RoundEnd { (ACCENT, 16.) } else { (RED, 12.) };
        let mark = world
            .spawn((Node { position_type: PositionType::Absolute, left: Val::Percent((m.ms as f64 / duration * 100.) as f32), width: px(s, 2.), height: px(s, tall), margin: UiRect::left(px(s, -1.)), ..default() }, BackgroundColor(color)))
            .id();
        world.entity_mut(line).add_child(mark);
    }
    let knob = world
        .spawn((Live::Knob, Node { position_type: PositionType::Absolute, left: Val::Percent(0.), width: px(s, 12.), height: px(s, 12.), margin: UiRect::left(px(s, -6.)), border_radius: BorderRadius::all(px(s, 6.)), ..default() }, BackgroundColor(WHITE)))
        .id();
    world.entity_mut(line).add_child(knob);
    let top_row = world.spawn(Node { align_items: AlignItems::Center, column_gap: px(s, 10.), ..default() }).id();
    world.entity_mut(top_row).add_children(&[clock, line]);
    let play = button(world, s, Hit::Play, "PAUSE", 70.);
    world.entity_mut(play).insert(Live::Play);
    let mut kids = vec![play];
    for (i, _) in SPEEDS.iter().enumerate() {
        let b = button(world, s, Hit::Speed(i), speed_name(i), 44.);
        world.entity_mut(b).insert(Live::Speed(i));
        kids.push(b);
    }
    let gap = world.spawn(Node { flex_grow: 1., ..default() }).id();
    let view = button(world, s, Hit::View, "FIRST PERSON", if wide < WIDTH { 96. } else { 116. });
    world.entity_mut(view).insert(Live::View);
    let stats = button(world, s, Hit::Stats, "STATS", 56.);
    let help = button(world, s, Hit::Help, "?", 28.);
    kids.extend([gap, view, stats, help]);
    let buttons = world.spawn(Node { align_items: AlignItems::Center, column_gap: px(s, 6.), flex_wrap: FlexWrap::Wrap, row_gap: px(s, 4.), ..default() }).id();
    world.entity_mut(buttons).add_children(&kids);
    world.entity_mut(bar).add_children(&[top_row, buttons]);
    world.entity_mut(bottom).add_children(&[who_row, bar]);
    // The stats and the help, over the middle; the map, top right.
    let stats = world
        .spawn((Live::Stats, Node { position_type: PositionType::Absolute, top: px(s, 70.), left: Val::Percent(50.), ..default() }, Visibility::Hidden, GlobalZIndex(90)))
        .id();
    let help = help_panel(world, s);
    let map = world
        .spawn((
            Live::Map,
            Node { position_type: PositionType::Absolute, left: px(s, 16.), top: px(s, if v.warning.is_some() || v.failed.is_some() { 96. } else { 76. }), width: px(s, 200.), height: px(s, 80.), border: UiRect::all(px(s, 1.)), border_radius: BorderRadius::all(px(s, 6.)), ..default() },
            BackgroundColor(Color::srgba(0.03, 0.04, 0.05, 0.72)),
            BorderColor::all(EDGE),
        ))
        .id();
    world.entity_mut(root).add_children(&[top, bottom, stats, help, map]);
    root
}
/// What the replay's mode is called: the rules' mode where they name one (a game in a mode other than its map's), or
/// else the header's.
fn mode_name(mode: &str, rules: &str) -> &'static str {
    let named = kitchen_sim::rules::Rules::parse(rules).mode().map(|m| m.name());
    match named.unwrap_or(mode) {
        "forts" => "FORTS",
        "ffa" => "FREE FOR ALL",
        "cards" => "FULLY LOADED",
        "kungfu" => "KUNG FU",
        _ => "SIEGE",
    }
}
/// The help: every key of the viewer as bound, and the mouse.
fn help_panel(world: &mut World, s: f32) -> Entity {
    let bindings = world.get_resource::<Bindings>().cloned().unwrap_or_default();
    let panel = world
        .spawn((
            Live::Help,
            GlobalZIndex(91),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(50.),
                top: Val::Percent(50.),
                margin: UiRect { left: px(s, -330.), top: px(s, -250.), ..default() },
                width: px(s, 660.),
                padding: UiRect::all(px(s, 18.)),
                flex_direction: FlexDirection::Column,
                row_gap: px(s, 10.),
                border_radius: BorderRadius::all(px(s, 10.)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.035, 0.042, 0.052, 0.95)),
            Visibility::Hidden,
        ))
        .id();
    let head = text(world, s, "REPLAY CONTROLS", 18., ACCENT);
    let columns = world.spawn(Node { column_gap: px(s, 28.), ..default() }).id();
    for group in [Group::Replay, Group::Camera] {
        let col = world.spawn(Node { flex_direction: FlexDirection::Column, row_gap: px(s, 3.), flex_grow: 1., ..default() }).id();
        for action in Action::ALL.into_iter().filter(|a| a.group() == group) {
            let keys: Vec<&str> = bindings.get(action).into_iter().flatten().map(|i| i.label()).collect();
            let row = world.spawn(Node { justify_content: JustifyContent::SpaceBetween, column_gap: px(s, 12.), ..default() }).id();
            let name = text(world, s, action.name(), 11., DIM);
            let key = text(world, s, &if keys.is_empty() { "-".to_string() } else { keys.join("  OR  ") }, 11., WHITE);
            world.entity_mut(row).add_children(&[name, key]);
            world.entity_mut(col).add_child(row);
        }
        world.entity_mut(columns).add_child(col);
    }
    let mouse = text(world, s, "DRAG (RIGHT BUTTON, OR LEFT OFF THE BAR): LOOK   WHEEL: FLYING SPEED / FOLLOW DISTANCE   CLICK OR DRAG THE BAR: SEEK", 10., FAINT);
    let more = text(world, s, "ESC: MENU (REPLAYS, CONTROLS TO REBIND THESE)   RED MARKS: KILLS AND KNOCKOUTS   GOLD: ENDS OF ROUNDS", 10., FAINT);
    world.entity_mut(panel).add_children(&[head, columns, mouse, more]);
    panel
}

/// The viewer's buttons and its timeline.
fn clicks(world: &mut World, v: &mut Viewer, built: &mut Built) {
    let pressed: Vec<Hit> = world.query::<(&Hit, &Interaction)>().iter(world).filter(|(_, i)| **i == Interaction::Pressed).map(|(h, _)| *h).collect();
    let mouse = world.resource::<ButtonInput<MouseButton>>().clone();
    let menu = world.get_resource::<crate::menu::Menu>().is_some_and(|m| m.open);
    // (A finger is the left button: Bevy's UI takes a touch as a press, the bar as well. On a phone the page's own
    // controls usually have the bar instead: page.rs.)
    let (touched, touching) = world.get_resource::<Touches>().map_or((false, false), |t| (t.any_just_pressed(), t.iter().next().is_some()));
    let just = (mouse.just_pressed(MouseButton::Left) || touched) && !menu;
    // A press on the timeline seeks, and so does dragging along it until the button (or the finger) is let go.
    if just && pressed.contains(&Hit::Timeline) {
        built.dragging = true;
    }
    if !(mouse.pressed(MouseButton::Left) || touching) || menu {
        built.dragging = false;
    }
    if built.dragging {
        let x = world.query::<(&Hit, &RelativeCursorPosition)>().iter(world).find(|(h, _)| **h == Hit::Timeline).and_then(|(_, p)| p.normalized).map(|p| (p.x + 0.5).clamp(0., 1.));
        if let Some(x) = x {
            let to = x as f64 * v.duration();
            if v.seek.is_none_or(|s| (s - to).abs() > 1.) && (v.clock - to).abs() > 30. {
                v.seek = Some(to);
            }
        }
    }
    if !just {
        return;
    }
    for hit in pressed {
        match hit {
            Hit::Play => super::toggle_play(world, v),
            Hit::Speed(i) => v.speed = i,
            Hit::View => {
                let next = v.mode.next();
                super::set_mode(world, v, next);
            }
            Hit::Stats => v.show.stats = !v.show.stats,
            Hit::Help => v.show.help = !v.show.help,
            Hit::Previous | Hit::Next => v.step_player(if hit == Hit::Next { 1 } else { -1 }),
            Hit::Timeline => {}
        }
    }
}

/// Whose view it is and how (`WATCHING NAME  2/5  /  RECORDED VIEW`), or nothing before the first frame.
pub fn watching(v: &Viewer) -> String {
    let Some(seat) = v.frame.as_ref().and_then(|f| f.players.iter().find(|p| p.player.id == v.watched)) else { return String::new() };
    let players = v.players();
    let place = players.iter().position(|id| *id == v.watched).map_or(0, |i| i + 1);
    let p = &seat.player;
    let state = if !p.alive { "  (DOWN)" } else { "" };
    let mode = if v.mode == Mode::Fly { "FREE CAMERA  /  LAST WATCHED " } else if v.mode == Mode::Follow { "FOLLOWING " } else { "WATCHING " };
    // In first person: the player's own view as its screen drew it, or one worked out from the recording.
    let how = match v.mode {
        Mode::First if v.recorded() => "  /  RECORDED VIEW",
        Mode::First => "  /  RECONSTRUCTED",
        _ => "",
    };
    format!("{mode}{}  {place}/{}{state}{how}", ascii(&p.name), players.len())
}
/// What the replay is (the map, the mode, when), as the top left says it.
pub fn title(v: &Viewer) -> String {
    match &v.replay {
        Some(r) => {
            let h = &r.header;
            let (y, m, d, hh, mm, _) = super::store::civil(h.started);
            ascii(&format!("{}  /  {}  /  {y}-{m:02}-{d:02} {hh:02}:{mm:02}", h.map_name, mode_name(&h.mode, &h.rules)))
        }
        None => v.failed.clone().map_or("LOADING THE REPLAY...".into(), |_| "THE REPLAY CANNOT BE PLAYED".into()),
    }
}

/// The screen brought up to date with the viewer.
fn refresh(world: &mut World, v: &Viewer, built: &mut Built, s: f32) {
    let duration = v.duration();
    let part = if duration > 0. { (v.clock / duration).clamp(0., 1.) as f32 * 100. } else { 0. };
    let watched = v.frame.as_ref().and_then(|f| f.players.iter().find(|p| p.player.id == v.watched));
    let forts = v.frame.as_ref().is_some_and(|f| f.forts.is_some());
    let who = watching(v);
    let colour = watched.map_or(WHITE, |seat| tint(&seat.player, forts, &seat.color));
    let mut live = world.query::<(&Live, Option<&mut Text>, Option<&mut Node>, Option<&mut BackgroundColor>, Option<&mut Visibility>)>();
    for (which, text, node, bg, vis) in live.iter_mut(world) {
        match which {
            Live::Clock => {
                if let Some(mut t) = text {
                    let words = format!("{} / {}", time(v.clock), time(duration));
                    if t.0 != words {
                        t.0 = words;
                    }
                }
            }
            Live::Fill => {
                if let Some(mut n) = node {
                    n.width = Val::Percent(part);
                }
            }
            Live::Knob => {
                if let Some(mut n) = node {
                    n.left = Val::Percent(part);
                }
            }
            Live::Who => {
                if let Some(mut t) = text
                    && t.0 != who
                {
                    t.0 = who.clone();
                }
            }
            Live::Swatch => {
                if let Some(mut bg) = bg {
                    bg.0 = colour;
                }
            }
            Live::Speed(i) => {
                if let Some(mut bg) = bg {
                    bg.0 = if *i == v.speed { Color::srgba(1.0, 0.80, 0.30, 0.32) } else { WELL };
                }
            }
            Live::Stats => {
                if let Some(mut vis) = vis {
                    vis.set_if_neq(if v.show.stats { Visibility::Inherited } else { Visibility::Hidden });
                }
            }
            Live::Help => {
                if let Some(mut vis) = vis {
                    vis.set_if_neq(if v.show.help { Visibility::Inherited } else { Visibility::Hidden });
                }
            }
            Live::Map => {
                if let Some(mut vis) = vis {
                    vis.set_if_neq(if v.show.minimap && v.replay.is_some() { Visibility::Inherited } else { Visibility::Hidden });
                }
            }
            Live::Bar | Live::Top | Live::Note | Live::Play | Live::View => {}
        }
    }
    // The labels of the play button and the view's.
    let play_words = if v.ended { "AGAIN" } else if v.paused { "PLAY" } else { "PAUSE" };
    let view_words = v.mode.name();
    let labels: Vec<(Entity, &str)> = world.query::<(Entity, &Live)>().iter(world).filter_map(|(e, l)| match l {
        Live::Play => Some((e, play_words)),
        Live::View => Some((e, view_words)),
        _ => None,
    }).collect();
    for (e, words) in labels {
        let child = world.get::<Children>(e).and_then(|c| c.first().copied());
        if let Some(child) = child
            && let Some(mut t) = world.get_mut::<Text>(child)
            && t.0 != words
        {
            t.0 = words.to_string();
        }
    }
    stats(world, v, built, s);
    minimap(world, v, built, s);
}

/// The stats panel, built again a few times a second while it is shown.
fn stats(world: &mut World, v: &Viewer, built: &mut Built, s: f32) {
    let Some(panel) = world.query::<(Entity, &Live)>().iter(world).find(|(_, l)| matches!(l, Live::Stats)).map(|(e, _)| e) else { return };
    if !v.show.stats {
        return;
    }
    let Some(head) = v.now.as_ref() else { return };
    let forts = v.frame.as_ref().is_some_and(|f| f.forts.is_some());
    let colours: Vec<(u32, Color)> = v.frame.as_ref().map_or(vec![], |f| f.players.iter().map(|p| (p.player.id, tint(&p.player, forts, &p.color))).collect());
    let reserve = |id: u32, weapon: usize| -> Option<i32> {
        let g = world.get_resource::<Game>()?;
        g.mag.stocks.iter().find(|s| s.id == id).and_then(|s| s.reserve.get(weapon).copied())
    };
    let mut lines = head.tally.lines.clone();
    lines.sort_by(|a, b| b.score.cmp(&a.score).then(b.kills.cmp(&a.kills)).then(a.deaths.cmp(&b.deaths)).then(a.id.cmp(&b.id)));
    let rows: Vec<(bool, Color, Vec<String>)> = lines
        .iter()
        .map(|l| {
            let color = colours.iter().find(|c| c.0 == l.id).map_or(WHITE, |c| c.1);
            let weapon = if l.weapon == 0 { "-".to_string() } else { crate::arena::Weapon::index(l.weapon).name().to_string() };
            let mag = match (l.magazine, reserve(l.id, l.weapon)) {
                (m, _) if m < 0 => "-".to_string(),
                (m, Some(r)) => format!("{m} / {r}"),
                (m, None) => m.to_string(),
            };
            let acc = l.accuracy().map_or("-".into(), |a| format!("{:.0}%", a * 100.));
            let cells = vec![
                ascii(&l.name),
                l.kills.to_string(),
                l.deaths.to_string(),
                l.score.to_string(),
                if l.alive { format!("{:.0}", l.hp.max(0.)) } else { "DOWN".into() },
                weapon,
                mag,
                format!("{acc} ({}/{})", l.hits, l.shots + l.swings),
                format!("{:.0}", l.dealt as f32 / 100.),
                format!("{:.0}", l.taken as f32 / 100.),
                format!("{} ({})", l.streak, l.best),
            ];
            (l.id == v.watched, color, cells)
        })
        .collect();
    // A window too narrow for every column (a phone) has the first seven, as large as fit.
    let window = width_of(world);
    let (columns, s) = stats_fit(s, window);
    let sig = format!("{s}{columns}{:?}", rows.iter().map(|r| (r.0, &r.2)).collect::<Vec<_>>());
    if sig == built.stats {
        return;
    }
    built.stats = sig;
    let rows: Vec<(bool, Color, Vec<String>)> = rows.into_iter().map(|(lit, color, mut cells)| {
        cells.truncate(columns);
        (lit, color, cells)
    }).collect();
    let kids: Vec<Entity> = world.get::<Children>(panel).map(|c| c.iter().collect()).unwrap_or_default();
    for kid in kids {
        world.entity_mut(kid).despawn();
    }
    let width: f32 = STATS_WIDTHS[..columns].iter().sum::<f32>() + 36.;
    let card = world
        .spawn((
            Node { flex_direction: FlexDirection::Column, row_gap: px(s, 2.), padding: UiRect::all(px(s, 14.)), margin: UiRect::left(card_left(columns, s * width, window)), width: px(s, width), border_radius: BorderRadius::all(px(s, 10.)), ..default() },
            BackgroundColor(Color::srgba(0.035, 0.042, 0.052, 0.97)),
        ))
        .id();
    let row = |world: &mut World, cells: &[String], color: Color, size: f32, lit: bool, swatch: Option<Color>| -> Entity {
        let r = world.spawn((Node { align_items: AlignItems::Center, padding: UiRect::axes(px(s, 4.), px(s, 3.)), border_radius: BorderRadius::all(px(s, 4.)), ..default() }, BackgroundColor(if lit { Color::srgba(1.0, 0.80, 0.30, 0.18) } else { Color::NONE }))).id();
        for (i, cell) in cells.iter().enumerate() {
            let c = world.spawn(Node { width: px(s, STATS_WIDTHS[i]), align_items: AlignItems::Center, column_gap: px(s, 6.), ..default() }).id();
            if i == 0
                && let Some(sw) = swatch
            {
                let dot = world.spawn((Node { width: px(s, 8.), height: px(s, 8.), border_radius: BorderRadius::all(px(s, 4.)), ..default() }, BackgroundColor(sw))).id();
                world.entity_mut(c).add_child(dot);
            }
            let t = world.spawn((Text::new(cell.clone()), font(s, size), TextColor(color), TextLayout::no_wrap())).id();
            world.entity_mut(c).add_child(t);
            world.entity_mut(r).add_child(c);
        }
        r
    };
    let title = text(world, s, &format!("STATS AT {}", time(v.clock)), 15., ACCENT);
    let heads: Vec<String> = STATS_HEADS[..columns].iter().map(|h| h.to_string()).collect();
    let head_row = row(world, &heads, FAINT, 10., false, None);
    world.entity_mut(card).add_children(&[title, head_row]);
    for (lit, color, cells) in &rows {
        let r = row(world, cells, WHITE, 12., *lit, Some(*color));
        world.entity_mut(card).add_child(r);
    }
    let note = if columns < STATS_HEADS.len() { "WIDER SCREEN (TURN THE PHONE): ACCURACY, DAMAGE AND STREAKS" } else { "ACCURACY: HITS ON HOTDOGS FOR EVERY ROUND FIRED OR SWING. DAMAGE TO CATS IS NOT IN A RECORDING." };
    let note = text(world, s, note, 9., FAINT);
    world.entity_mut(card).add_child(note);
    world.entity_mut(panel).add_child(card);
}

/// The map from above: the counter's blocks, every hotdog (the watched one ringed) and the cats; and in the free camera
/// where it is.
fn minimap(world: &mut World, v: &Viewer, built: &mut Built, s: f32) {
    let Some(panel) = world.query::<(Entity, &Live)>().iter(world).find(|(_, l)| matches!(l, Live::Map)).map(|(e, _)| e) else { return };
    if !v.show.minimap {
        return;
    }
    let Some(g) = world.get_resource::<Game>() else { return };
    let [x0, z0, x1, z1] = g.feast.level.map.bounds.map(|b| b as f32);
    let (w, h) = ((x1 - x0).max(0.1), (z1 - z0).max(0.1));
    let width = 200.;
    let height = (width * h / w).clamp(50., 200.);
    let blocks: Vec<[f32; 4]> = g.feast.level.map.blocks.iter().map(|b| b.rect.map(|x| x as f32)).collect();
    let forts = v.frame.as_ref().is_some_and(|f| f.forts.is_some());
    let hz = g.online.hz;
    let mut dots: Vec<(Vec3, Color, bool, bool)> = vec![];
    if let Some(f) = &v.frame {
        for seat in &f.players {
            let p = &seat.player;
            let at = g.online.timeline.player(p.id, hz).map_or(Vec3::from_array(p.p), |s| s.p);
            let color = if p.alive { tint(p, forts, &seat.color) } else { Color::srgba(0.6, 0.6, 0.6, 0.6) };
            dots.push((at, color, p.id == v.watched, false));
        }
        for e in &f.enemies {
            dots.push((Vec3::from_array(e.p), Color::srgb(0.55, 0.85, 0.35), false, true));
        }
    }
    if v.mode == Mode::Fly {
        dots.push((v.fly.at, Color::srgb(0.5, 0.8, 1.), false, true));
    }
    let map = g.feast.level.map.name.clone();
    if built.map != map {
        built.map = map;
        let kids: Vec<Entity> = world.get::<Children>(panel).map(|c| c.iter().collect()).unwrap_or_default();
        for kid in kids {
            world.entity_mut(kid).despawn();
        }
        if let Some(mut n) = world.get_mut::<Node>(panel) {
            (n.width, n.height) = (px(s, width), px(s, height));
        }
        for [a, b, c, d] in blocks {
            let r = world
                .spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Percent((a - x0) / w * 100.),
                        top: Val::Percent((b - z0) / h * 100.),
                        width: Val::Percent((c - a) / w * 100.),
                        height: Val::Percent((d - b) / h * 100.),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(1., 1., 1., 0.13)),
                ))
                .id();
            world.entity_mut(panel).add_child(r);
        }
    }
    // The dots: as many as are needed, placed each frame.
    let mut have: Vec<Entity> = world.query_filtered::<Entity, With<Dot>>().iter(world).collect();
    while have.len() < dots.len() {
        let d = world.spawn((Dot, Node { position_type: PositionType::Absolute, ..default() }, BackgroundColor(WHITE), BorderColor::all(Color::NONE))).id();
        world.entity_mut(panel).add_child(d);
        have.push(d);
    }
    for (i, e) in have.into_iter().enumerate() {
        let Some(&(at, color, ringed, small)) = dots.get(i) else {
            world.entity_mut(e).insert(Visibility::Hidden);
            continue;
        };
        let size = if small { 5. } else if ringed { 11. } else { 8. };
        world.entity_mut(e).insert((
            Visibility::Inherited,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(((at.x - x0) / w * 100.).clamp(0., 100.)),
                top: Val::Percent(((at.z - z0) / h * 100.).clamp(0., 100.)),
                width: px(s, size),
                height: px(s, size),
                margin: UiRect { left: px(s, -size / 2.), top: px(s, -size / 2.), ..default() },
                border: UiRect::all(px(s, if ringed { 2. } else { 0. })),
                border_radius: BorderRadius::all(px(s, size / 2.)),
                ..default()
            },
            BackgroundColor(color),
            BorderColor::all(if ringed { WHITE } else { Color::NONE }),
        ));
    }
}

/// The shots' tracers: a thin bar along each, in its condiment's colour, thinning away as it lingers.
fn tracers(world: &mut World, v: &Viewer, built: &mut Built) {
    let shown: Vec<&super::Tracer> = if v.show.tracers { v.tracers.iter().collect() } else { vec![] };
    if built.meshes.is_none() && !shown.is_empty() {
        let mesh = world.resource_mut::<Assets<Mesh>>().add(Cuboid::new(1., 1., 1.));
        let colours = [[1., 1., 1.], [0.98, 0.82, 0.15], [0.92, 0.18, 0.12], [1., 0.45, 0.1], [0.45, 0.85, 0.25], [0.75, 0.9, 0.95], [0.85, 0.55, 0.2], [0.98, 0.97, 0.88]];
        let mut materials = world.resource_mut::<Assets<StandardMaterial>>();
        let mats = colours.iter().map(|[r, g, b]| materials.add(StandardMaterial { base_color: Color::srgba(*r, *g, *b, 0.55), emissive: LinearRgba::rgb(*r * 2., *g * 2., *b * 2.), unlit: true, alpha_mode: AlphaMode::Blend, ..default() })).collect();
        built.meshes = Some((mesh, mats));
    }
    while built.tracers.len() < shown.len() {
        let Some((mesh, mats)) = built.meshes.clone() else { break };
        let e = world.spawn((TracerMesh, Mesh3d(mesh), MeshMaterial3d(mats[0].clone()), Transform::default(), Visibility::Hidden, bevy::light::NotShadowCaster)).id();
        built.tracers.push(e);
    }
    let clock = v.clock;
    for (i, &e) in built.tracers.iter().enumerate() {
        let Some(t) = shown.get(i) else {
            if world.get::<Visibility>(e) != Some(&Visibility::Hidden) {
                world.entity_mut(e).insert(Visibility::Hidden);
            }
            continue;
        };
        let age = ((clock - t.ms) / 1000. / TRACER_TIME).clamp(0., 1.) as f32;
        let length = t.a.distance(t.b);
        if length < 1e-4 {
            world.entity_mut(e).insert(Visibility::Hidden);
            continue;
        }
        let thick = 0.004 * (1. - age).max(0.05);
        let tr = Transform::from_translation((t.a + t.b) / 2.).looking_at(t.b, if (t.b - t.a).normalize().y.abs() > 0.99 { Vec3::X } else { Vec3::Y }).with_scale(Vec3::new(thick, thick, length));
        let mat = built.meshes.as_ref().map(|(_, m)| m[t.weapon.min(m.len() - 1)].clone());
        let mut entity = world.entity_mut(e);
        entity.insert((tr, Visibility::Visible));
        if let Some(mat) = mat {
            entity.insert(MeshMaterial3d(mat));
        }
    }
}

/// Everything on the screen hidden (the viewer's own, the game's display, the name tags), or shown again.
fn hide(world: &mut World, v: &Viewer, built: &mut Built) {
    let menu = world.get_resource::<crate::menu::Menu>().is_some_and(|m| m.open);
    let hidden = !v.show.ui && !menu;
    if hidden {
        let roots: Vec<(Entity, Visibility)> = world
            .query_filtered::<(Entity, &Visibility, Option<&GlobalZIndex>), (With<Node>, Without<ChildOf>)>()
            .iter(world)
            .filter(|(_, _, z)| z.is_none_or(|z| z.0 < 100))
            .map(|(e, vis, _)| (e, *vis))
            .collect();
        for (e, vis) in roots {
            if vis != Visibility::Hidden {
                world.entity_mut(e).insert(Visibility::Hidden);
                if !built.hidden.contains(&e) {
                    built.hidden.push(e);
                }
            }
        }
    } else {
        for e in built.hidden.drain(..) {
            if let Ok(mut e) = world.get_entity_mut(e) {
                e.insert(Visibility::Inherited);
            }
        }
    }
}

/// After the transforms: the names over everyone and the damage numbers, where the camera sees them.
pub fn place(world: &mut World, mut pool: Local<Vec<Entity>>, mut numbers: Local<Vec<Entity>>) {
    let Some(v) = world.remove_resource::<Viewer>() else { return };
    let scale = scale_of(world);
    let camera = world.query_filtered::<(&Camera, &GlobalTransform), With<crate::maya::HotdogCamera>>().iter(world).find(|(c, _)| c.is_active).map(|(c, t)| (c.clone(), *t));
    let on = v.show.ui && v.replay.is_some();
    let (hz, me, first) = world.get_resource::<Game>().map_or((60., 0, false), |g| (g.online.hz, g.network_id, !g.demo));
    let project = |at: Vec3| -> Option<Vec2> {
        let (cam, view) = camera.as_ref()?;
        let p = cam.world_to_viewport(view, at).ok()?;
        ((at - view.translation()).dot(*view.forward()) > 0.03).then_some(p)
    };
    // Names.
    let mut tags: Vec<(String, Color, Vec2)> = vec![];
    if on && v.show.tags
        && let (Some(f), Some(g)) = (&v.frame, world.get_resource::<Game>())
    {
        let forts = f.forts.is_some();
        for seat in &f.players {
            let p = &seat.player;
            if (first && p.id == me) || seat.waiting {
                continue;
            }
            let at = g.online.timeline.player(p.id, hz).map_or(Vec3::from_array(p.p), |s| s.p) + Vec3::Y * 0.3;
            if let Some(spot) = project(at) {
                let words = if p.alive { format!("{}  {:.0}", ascii(&p.name), p.hp.max(0.)) } else { format!("{}  DOWN", ascii(&p.name)) };
                tags.push((words, tint(p, forts, &seat.color), spot));
            }
        }
    }
    while pool.len() < tags.len() {
        let e = world.spawn((Tag, Text::new(""), font(scale, 11.), TextColor(WHITE), TextLayout::no_wrap(), Node { position_type: PositionType::Absolute, ..default() }, GlobalZIndex(55), shadow(scale))).id();
        pool.push(e);
    }
    for (i, &e) in pool.iter().enumerate() {
        let Ok(mut entity) = world.get_entity_mut(e) else { continue };
        match tags.get(i) {
            Some((words, color, spot)) => {
                let width = words.len() as f32 * 6.6 * scale;
                entity.insert((Text::new(words.clone()), TextColor(*color), Visibility::Inherited, Node { position_type: PositionType::Absolute, left: Val::Px(spot.x - width / 2.), top: Val::Px(spot.y - 16. * scale), ..default() }));
            }
            None => {
                entity.insert(Visibility::Hidden);
            }
        }
    }
    // Damage numbers, rising and fading.
    let mut shown: Vec<(String, f32, Vec2)> = vec![];
    if on && v.show.numbers {
        for n in &v.numbers {
            let age = ((v.clock - n.ms) / 1000. / NUMBER_TIME).clamp(0., 1.) as f32;
            if first && n.to == me {
                continue;
            }
            if let Some(spot) = project(n.at + Vec3::Y * (0.2 + 0.12 * age)) {
                shown.push((format!("-{:.0}", n.amount.max(1.)), 1. - age, spot));
            }
        }
    }
    while numbers.len() < shown.len() {
        let e = world.spawn((Rising, Text::new(""), font(scale, 15.), TextColor(RED), TextLayout::no_wrap(), Node { position_type: PositionType::Absolute, ..default() }, GlobalZIndex(56), shadow(scale))).id();
        numbers.push(e);
    }
    for (i, &e) in numbers.iter().enumerate() {
        let Ok(mut entity) = world.get_entity_mut(e) else { continue };
        match shown.get(i) {
            Some((words, alpha, spot)) => {
                entity.insert((Text::new(words.clone()), TextColor(Color::srgba(1.0, 0.42, 0.32, *alpha)), Visibility::Inherited, Node { position_type: PositionType::Absolute, left: Val::Px(spot.x - 10. * scale), top: Val::Px(spot.y - 20. * scale), ..default() }));
            }
            None => {
                entity.insert(Visibility::Hidden);
            }
        }
    }
    world.insert_resource(v);
}
