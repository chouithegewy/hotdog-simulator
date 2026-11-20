// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The multiplayer lobby's scene. Between rounds the players are hotdogs shut in the fridge. The
//! package lies on the shelf with five franks asleep in its tray, one for each of the lobby's five
//! seats (the hotdog slots 1 to 5, left to right): as a player takes a seat its frank wakes and gets
//! up out of the tray, and stands in a ring on the shelf in front of the package, under its name (a
//! crown over the leader's), the ring round its waist in the colour its player chose (in Forts, its
//! team's colour on its legs and the ring), turning to whoever is talking, its mouth moving while that
//! player speaks. When the player leaves, the frank lies back down in the tray. The local player looks
//! out from its own place and looks around with the mouse.
//!
//! The server owns the lobby (crates/server/src/game.rs, `linger`): its snapshots say `Lobby`
//! while people get ready, then `Opening` for the start sequence, which every client plays from
//! the server's clock: the fridge door swings open and the light comes in, then the package is on
//! the counter, its seal peels back, the players' hotdogs pop up beside it, and the round begins.
//! Somebody who arrived during a round waits here, door shut, until the next one.
//!
//! While this scene is shown (`Lobby::holding`) the arena sleeps: `arena::tick` does nothing and
//! the camera, the fridge door and its lamp are this file's. In a browser the page shows the lobby's
//! header, the ready queue and the chat (lobby.js); without one (the native client) a few lines of
//! text stand in for it, and Enter says "ready".
use crate::{arena, fridge, hotdog, maya, player_model};
use bevy::prelude::*;
use std::{collections::HashMap, f32::consts::{FRAC_PI_2, PI, TAU}};

/// The top of the glass shelf the package lies on, the middle of the ring the hotdogs stand in, and
/// the ring's radius. While the lobby lasts the package lies at the back of the shelf (`PACK`), which
/// leaves the front of it to stand on.
const SHELF: f32 = 0.9437;
const CENTRE: Vec3 = Vec3::new(0., SHELF, -0.225);
const RING: f32 = 0.11;
const PACK: Vec3 = Vec3::new(0., fridge::PACK_FLOOR, -0.52);
/// A hotdog's eyes above its feet, and its name above its feet.
const EYES: f32 = 0.150;
const LABEL: f32 = 0.198;
/// How far behind its own place a hotdog's view is taken from.
const STEP_BACK: f32 = 0.05;
/// How far the mouse turns the view: radians either side, and up and down.
const TURN: Vec2 = Vec2::new(1.9, 0.6);
/// The start sequence, in seconds when it lasts `SEQUENCE` (the server says how long it really lasts,
/// and everything stretches to fit): the door starts to move, how long it swings, when the lamp comes
/// on, when the view goes to the counter, when the seal starts to peel and for how long, and when the
/// first hotdog pops up.
const SEQUENCE: f32 = 9.;
const DOOR: (f32, f32) = (0.35, 2.9);
const LAMP: f32 = 0.5;
const CUT: f32 = 3.6;
const PEEL: (f32, f32) = (4.1, 2.4);
const POP: f32 = 6.3;
/// How bright the fridge lamps and the room's light are while the door is shut, as a part of full.
const DIM: f32 = 0.3;
/// The even, cold light there is to see by while the door is shut (the theme's own is off until it opens).
const FILL: f32 = 0.8;
/// The tray's five places, one for each seat, this far apart across it (the intro lays six franks 0.034 m apart).
const SLOT: f32 = 0.042;
/// How long a frank takes to get up out of the tray and over to its place in the ring.
const WAKE: f32 = 0.8;
/// A team's colour (forts.rs), worn on the legs and the badge in a Forts lobby.
fn team(side: u8) -> Color {
    let [r, g, b] = crate::forts::TEAM_RGB[side.min(1) as usize];
    Color::srgb(r, g, b)
}

#[derive(Clone, Copy, PartialEq, Default)]
enum Stage {
    #[default]
    Lobby,
    Opening,
    Round,
}
/// One player in the fridge, from the last snapshot.
#[derive(Clone)]
struct Member {
    id: u32,
    name: String,
    color: String,
    talk: f32,
    ready: bool,
    /// The lobby's leader, who chooses the next game (the server's `leader`, a hotdog id).
    leader: bool,
    /// Forts: the side it is on, or would start on (the server's `sides`).
    side: Option<u8>,
}
struct Figure {
    model: Entity,
    label: Entity,
    /// The leader's crown, over the name.
    crown: Entity,
    yaw: f32,
    /// When it woke and began to get up out of the tray.
    woke: f32,
    /// The parts that wear a team's colour in Forts, with the material each had, and the side they wear now.
    livery: Vec<(Entity, Handle<StandardMaterial>)>,
    dressed: Option<u8>,
}
#[derive(Clone, Copy)]
struct Set {
    strip: Entity,
    glow: Entity,
    veil: Entity,
    hud: Entity,
}
#[derive(Resource, Default)]
pub struct Lobby {
    /// The arena sleeps and the fridge is shown: the lobby, the start sequence, or waiting for the next round.
    pub holding: bool,
    online: bool,
    stage: Stage,
    waiting: bool,
    countdown: Option<f32>,
    /// Seconds into the start sequence at the last snapshot, how long it lasts, and when that snapshot came.
    opening: f32,
    opening_for: f32,
    stamp: f32,
    alert: String,
    me: u32,
    members: Vec<Member>,
    /// Enter was pressed: what to tell the server about being ready.
    wish: Option<bool>,
    figures: HashMap<u32, Figure>,
    look: Vec2,
    set: Option<Set>,
    shown: bool,
    /// What the next round plays, as the leader chose it (the server's `choice`), and who leads.
    choice: String,
    leader: String,
    /// The tray's five sleeping franks, one for each seat, left to right; and the materials of the teams' colours.
    franks: Vec<Entity>,
    teams: Option<[Handle<StandardMaterial>; 2]>,
}
impl Lobby {
    /// A client that connects to a server starts in the fridge and stays until the server says a round is on.
    pub fn new(options: &crate::runtime::Options) -> Self {
        let online = !options.connect.is_empty();
        Self { holding: online, online, opening_for: SEQUENCE, ..default() }
    }
    /// What Enter asked for since the last call, for `network::send` to send.
    pub fn take_wish(&mut self) -> Option<bool> {
        self.wish.take()
    }
    /// The colour the name of the player whose hotdog is `id` is written in on the tags over the heads (tags.rs, and at
    /// the card table cards.rs): the colour that player chose, which its ring wears (`player_model::tag_rgb`). None for
    /// a bot, and offline, where nobody chooses one.
    pub fn colour(&self, id: u32) -> Option<[f32; 3]> {
        self.members.iter().find(|m| m.id == id).and_then(|m| player_model::tag_rgb(&m.color))
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(inline_js = "
export function lobby_page(){return !!(window.hotdogLobby&&window.hotdogLobby.active)}
")]
extern "C" {
    /// Whether the page is showing the lobby itself (lobby.js).
    fn lobby_page() -> bool;
}
#[cfg(not(target_arch = "wasm32"))]
fn lobby_page() -> bool {
    false
}

fn smooth(t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    t * t * (3. - 2. * t)
}
/// Where the hotdog in slot `id` (1 to 5) stands on the shelf, and the yaw that faces the middle of the ring.
/// Slots that follow one another are two fifths of the way round apart, so the first few to arrive face each other.
fn place(id: u32) -> (Vec3, f32) {
    let angle = FRAC_PI_2 + (id.max(1) - 1) as f32 * TAU * 0.4;
    let at = CENTRE + Vec3::new(RING * angle.cos(), 0., RING * angle.sin());
    (at, facing(at, CENTRE))
}
/// The seat of the hotdog in slot `id`, 0 to 4: its frank in the tray, left to right.
fn seat(id: u32) -> usize {
    (id.max(1) as usize - 1) % 5
}
/// Where the frank of a seat lies across the tray, from the tray's middle.
fn slot(seat: usize) -> f32 {
    (seat as f32 - 2.) * SLOT
}
/// The yaw at which a hotdog standing at `from` (it faces -z) looks at `to`.
fn facing(from: Vec3, to: Vec3) -> f32 {
    (from.x - to.x).atan2(from.z - to.z)
}
/// The middle of where everyone but `id` stands, if anyone else is there.
fn company(members: &[Member], id: u32) -> Option<Vec3> {
    let others: Vec<Vec3> = members.iter().filter(|m| m.id != id).map(|m| place(m.id).0).collect();
    (!others.is_empty()).then(|| others.iter().sum::<Vec3>() / others.len() as f32)
}
/// The shortest way round from one yaw to another.
fn wrap(angle: f32) -> f32 {
    (angle + PI).rem_euclid(TAU) - PI
}

/// Reads a frame (before `arena::apply_frame` does): the stage, who is in the fridge, and whether this player is held
/// there.
pub fn observe(world: &mut World, frame: &kitchen_sim::net::Frame) {
    let me = world.resource::<arena::Game>().network_id;
    read(world, frame, me);
}
/// What a mode is called in the lobby's lines: the chosen mode's name, or nothing before one is chosen (the server's
/// `mode_name`, and the page's `MODE_NAME`).
fn mode_name(mode: &str) -> &'static str {
    match mode {
        "forts" => "Forts",
        "ffa" => "Free For All",
        "cards" => "Fully Loaded",
        "kungfu" => "Kung Fu",
        "titan" => "Titan",
        "" => "",
        _ => "Siege",
    }
}
/// The "Next:" line once the fridge has opened: the mode the round is played in (kitchen-sim `Feast::mode`: a Forts map
/// may be played as a siege, any map as Free For All), then its map.
fn playing_line(mode: &str, map: &str) -> String {
    format!("{} | {map}", mode_name(mode))
}
/// `observe` for the player whose hotdog is `me`.
fn read(world: &mut World, frame: &kitchen_sim::net::Frame, me: u32) {
    let now = world.resource::<Time<Real>>().elapsed_secs();
    let players = &frame.players;
    let waiting = players.iter().any(|p| p.player.id == me && p.waiting);
    let mut lobby = world.resource_mut::<Lobby>();
    lobby.stage = match frame.stage.as_str() {
        "Lobby" => Stage::Lobby,
        "Opening" => Stage::Opening,
        _ => Stage::Round,
    };
    (lobby.me, lobby.waiting) = (me, waiting);
    lobby.holding = lobby.stage != Stage::Round || waiting;
    lobby.countdown = frame.countdown;
    lobby.opening = frame.opening;
    lobby.opening_for = (if frame.opening_for > 0. { frame.opening_for } else { SEQUENCE }).max(0.1);
    lobby.stamp = now;
    lobby.alert = frame.alert.clone();
    // The next game as the leader chose it, and who leads.
    let choice = &frame.choice;
    let mode = mode_name(&choice.mode);
    lobby.choice = match (choice.random, choice.title.as_str(), choice.theme.as_str()) {
        _ if mode.is_empty() => String::new(),
        (true, ..) => format!("{mode} | Random map"),
        (false, title, theme) if !theme.is_empty() => format!("{mode} | {title} | {theme}"),
        (false, title, _) => format!("{mode} | {title}"),
    };
    let leader = frame.leader;
    // In the fridge: the people the next round waits for, or, for somebody waiting out a round, the others waiting.
    lobby.members = players
        .iter()
        .filter(|p| p.human && p.waiting == waiting)
        .map(|p| {
            let id = p.player.id;
            let (name, color) = match &p.profile {
                Some(profile) => (profile.name.clone(), profile.color.clone()),
                None => (p.player.name.clone(), p.color.clone()),
            };
            Member {
                id,
                name,
                color,
                talk: p.talk,
                ready: p.player.ready,
                leader: leader > 0 && id == leader,
                side: frame.sides.as_ref().and_then(|sides| sides.get(&id.to_string())).map(|side| (*side).min(1)),
            }
        })
        .collect();
    lobby.leader = lobby.members.iter().find(|m| m.leader).map(|m| m.name.clone()).unwrap_or_default();
}

#[derive(Component)]
struct LobbyPart;

fn build(world: &mut World) -> Set {
    let (cube, warm) = {
        let cube = world.resource_mut::<Assets<Mesh>>().add(Cuboid::default());
        let warm = world.resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial {
            base_color: Color::srgb(1., 0.84, 0.58),
            emissive: LinearRgba::rgb(46., 33., 17.),
            ..default()
        });
        (cube, warm)
    };
    // The strip of kitchen light at the door seal: down the side the door opens on, and along the top.
    let strip = world.spawn((LobbyPart, Name::new("Light at the fridge door seal"), Transform::default(), Visibility::Hidden)).id();
    for (at, size) in [
        (Vec3::new(0.3662, 1.03, -0.006), Vec3::new(0.0022, 1.58, 0.004)),
        (Vec3::new(0., 1.8255, -0.006), Vec3::new(0.73, 0.0022, 0.004)),
    ] {
        let bar = world
            .spawn((
                Mesh3d(cube.clone()),
                MeshMaterial3d(warm.clone()),
                Transform::from_translation(at).with_scale(size),
                bevy::light::NotShadowCaster,
            ))
            .id();
        world.entity_mut(strip).add_child(bar);
    }
    // What that strip lights of the door's edge. It hangs from the strip, so it is hidden with it.
    let glow = world
        .spawn((
            PointLight { intensity: 0., range: 0.6, radius: 0.01, color: Color::srgb(1., 0.8, 0.55), shadow_maps_enabled: false, ..default() },
            Transform::from_xyz(0.33, 1.06, -0.03),
            Visibility::default(),
        ))
        .id();
    world.entity_mut(strip).add_child(glow);
    // The light that floods in when the door opens, and out of which the counter appears.
    let veil = world
        .spawn((
            LobbyPart,
            Node { position_type: PositionType::Absolute, left: Val::Px(0.), top: Val::Px(0.), width: Val::Percent(100.), height: Val::Percent(100.), ..default() },
            BackgroundColor(Color::srgba(1., 0.97, 0.9, 0.)),
            ZIndex(-1),
            Visibility::Hidden,
        ))
        .id();
    let hud = world
        .spawn((
            LobbyPart,
            Text::new(""),
            TextFont { font_size: FontSize::Px(15.), ..default() },
            TextColor(Color::srgb(0.96, 0.95, 0.87)),
            Node { position_type: PositionType::Absolute, left: Val::Px(28.), top: Val::Px(24.), padding: UiRect::all(Val::Px(12.)), border_radius: BorderRadius::all(Val::Px(8.)), ..default() },
            BackgroundColor(Color::srgba(0.05, 0.105, 0.105, 0.82)),
            Visibility::Hidden,
        ))
        .id();
    Set { strip, glow, veil, hud }
}

/// The leader's mark over a name: a small gold crown of boxes, a band with three points (the default font has no crown).
fn crown(world: &mut World) -> Entity {
    let gold = Color::srgb(0.95, 0.77, 0.19);
    let root = world
        .spawn((
            LobbyPart,
            Node { position_type: PositionType::Absolute, width: Val::Px(18.), height: Val::Px(13.), flex_direction: FlexDirection::Column, ..default() },
            Visibility::Hidden,
        ))
        .id();
    let points = world
        .spawn(Node { width: Val::Percent(100.), height: Val::Px(8.), justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::FlexEnd, ..default() })
        .id();
    for height in [6., 8., 6.] {
        let point = world
            .spawn((Node { width: Val::Px(4.), height: Val::Px(height), border_radius: BorderRadius::top(Val::Px(2.)), ..default() }, BackgroundColor(gold)))
            .id();
        world.entity_mut(points).add_child(point);
    }
    let band = world
        .spawn((Node { width: Val::Percent(100.), height: Val::Px(5.), border_radius: BorderRadius::bottom(Val::Px(1.5)), ..default() }, BackgroundColor(gold)))
        .id();
    world.entity_mut(root).add_children(&[points, band]);
    root
}

/// The tray's franks as the lobby uses them: the five sleeping ones, laid out evenly across the tray left to
/// right, one for each seat. The intro's own frank (the self-aware one, eyes open) is not a seat and is not
/// shown. Nothing happens until the fridge has been built, or once it has been done.
fn tray(world: &mut World, lobby: &mut Lobby) {
    if !lobby.franks.is_empty() {
        return;
    }
    let mut sleepers = vec![];
    let mut spare = vec![];
    for (entity, mut life) in world.query::<(Entity, &mut hotdog::FrankLife)>().iter_mut(world) {
        if life.terrified() {
            spare.push(entity);
        } else {
            sleepers.push((life.rest_mut().translation.x, entity));
        }
    }
    sleepers.sort_by(|a, b| a.0.total_cmp(&b.0));
    spare.extend(sleepers.iter().skip(5).map(|(_, entity)| *entity));
    for (index, (_, entity)) in sleepers.iter().take(5).enumerate() {
        if let Some(mut life) = world.get_mut::<hotdog::FrankLife>(*entity) {
            life.rest_mut().translation.x = slot(index);
        }
        lobby.franks.push(*entity);
    }
    for entity in spare {
        world.entity_mut(entity).insert(Visibility::Hidden);
    }
}

/// Leaves the fridge: the players' hotdogs and their names go, and the door, the lamp and the camera
/// are the arena's again.
fn dismiss(world: &mut World, lobby: &mut Lobby) {
    for (_, figure) in lobby.figures.drain() {
        world.entity_mut(figure.model).despawn();
        world.entity_mut(figure.label).despawn();
        world.entity_mut(figure.crown).despawn();
    }
    if let Some(set) = &lobby.set {
        for entity in [set.strip, set.veil, set.hud] {
            world.entity_mut(entity).insert(Visibility::Hidden);
        }
    }
    // The package goes back on its shelf, sealed, for the next time.
    for (_, mut tr) in world.query::<(&maya::Part, &mut Transform)>().iter_mut(world) {
        *tr = Transform::from_xyz(0., fridge::PACK_FLOOR, fridge::PACK_Z);
    }
    lobby.shown = false;
}

/// Runs after everything else has had its say about the camera and the lights. Outside multiplayer it does nothing.
pub fn update(world: &mut World) {
    if !world.get_resource::<Lobby>().is_some_and(|lobby| lobby.online) {
        return;
    }
    world.resource_scope(|world, mut lobby: Mut<Lobby>| {
        if !lobby.holding {
            if lobby.shown {
                dismiss(world, &mut lobby);
            }
            return;
        }
        show(world, &mut lobby);
    });
}

fn show(world: &mut World, lobby: &mut Lobby) {
    let now = world.resource::<Time<Real>>().elapsed_secs();
    let dt = world.resource::<Time<Real>>().delta_secs().min(0.1);
    let Set { strip, glow, veil, hud } = *lobby.set.get_or_insert_with(|| build(world));
    lobby.shown = true;
    // Enter says "ready" (and "not ready"). In a browser the page has taken Enter for the chat and offers a button instead.
    let me = lobby.members.iter().find(|m| m.id == lobby.me).cloned();
    if lobby.stage == Stage::Lobby && !lobby.waiting && world.resource::<ButtonInput<KeyCode>>().just_pressed(KeyCode::Enter) {
        lobby.wish = Some(!me.as_ref().is_some_and(|m| m.ready));
    }
    // The start sequence's clock: the server's, carried on between snapshots, on the scale of SEQUENCE.
    let opening = lobby.stage == Stage::Opening && !lobby.waiting;
    let t = if opening { (lobby.opening + (now - lobby.stamp).clamp(0., 0.5)) * SEQUENCE / lobby.opening_for } else { -1. };
    let counter = opening && t >= CUT;
    let swing = smooth((t - DOOR.0) / DOOR.1);
    let lit = if t >= LAMP { 1. } else { DIM };

    // The door, the lamps and the room's light. The theme's own lights follow the fridge scene's clock, which is
    // set so that they come up with the lamp.
    for mut door in world.query_filtered::<&mut Transform, With<fridge::Door>>().iter_mut(world) {
        door.rotation = Quat::from_rotation_y(-swing * 108_f32.to_radians());
    }
    // (Shut in, the small lamp at the back is turned up to light the faces that are turned away from the big one.)
    for (mut lamp, place) in world.query_filtered::<(&mut PointLight, &Transform), With<fridge::FridgeLamp>>().iter_mut(world) {
        lamp.intensity = if place.translation.x > 0. { 145. } else if t >= LAMP { 8. } else { 90. } * lit;
    }
    world.resource_mut::<GlobalAmbientLight>().brightness = if t >= LAMP { 14.025 } else { 0.4 };
    let real = world.resource::<Time<Real>>().elapsed_secs_f64();
    let bulb = world.get_resource_mut::<fridge::FridgeScene>().map(|mut scene| {
        scene.restart(real - if t >= LAMP { 2.3 + (t - LAMP) as f64 } else { -60. });
        scene.bulb().clone()
    });
    if let Some(bulb) = bulb {
        if let Some(mut material) = world.resource_mut::<Assets<StandardMaterial>>().get_mut(&bulb) {
            let level = if t >= LAMP { 1. } else { 0.012 };
            material.emissive = LinearRgba::new(35. * level, 45. * level, 60. * level, 1.);
        }
    }
    world.entity_mut(strip).insert(if swing < 0.02 { Visibility::Visible } else { Visibility::Hidden });
    // (The theme scales the room's lamps in its own time, visuals::tune_room_lights; this one is set after it.)
    if let Some(mut light) = world.get_mut::<PointLight>(glow) {
        light.intensity = 2.2;
    }
    if let Some(mut tint) = world.get_mut::<BackgroundColor>(veil) {
        let white = if t < CUT { smooth((t - (CUT - 0.75)) / 0.7) } else { 1. - smooth((t - CUT) / 0.7) };
        tint.0 = Color::srgba(1., 0.97, 0.9, if opening { white } else { 0. });
    }
    world.entity_mut(veil).insert(if opening { Visibility::Visible } else { Visibility::Hidden });

    // The package: sealed on its shelf, or on the counter where the first hotdog starts, its seal peeling back.
    let start = world.resource::<arena::Game>().feast.level.start(0).0;
    let pack = if counter { Vec3::new(start.x, start.y + 0.009, start.z) } else { PACK };
    for (_, mut tr) in world.query::<(&maya::Part, &mut Transform)>().iter_mut(world) {
        *tr = Transform::from_translation(pack).with_rotation(Quat::from_rotation_y(if counter { PI } else { 0. }));
    }
    let peel = if counter { smooth((t - PEEL.0) / PEEL.1) } else { 0. };
    for (mut tr, mut seen) in world.query_filtered::<(&mut Transform, &mut Visibility), With<arena::PackageFilm>>().iter_mut(world) {
        tr.rotation = Quat::from_rotation_x(-peel * 2.2);
        tr.translation.y = 0.038 + peel * 0.09;
        tr.scale.z = 1. - peel * 0.7;
        *seen = Visibility::Inherited;
    }
    for mut seen in world.query_filtered::<&mut Visibility, With<arena::PackageLabel>>().iter_mut(world) {
        *seen = Visibility::Inherited;
    }
    // Five franks asleep in the tray, one for each seat; a seat somebody has taken has its frank up and about.
    tray(world, lobby);
    for (index, frank) in lobby.franks.clone().into_iter().enumerate() {
        let taken = lobby.members.iter().any(|m| seat(m.id) == index);
        world.entity_mut(frank).insert(if taken { Visibility::Hidden } else { Visibility::Inherited });
    }

    // One hotdog for each player in the fridge; the local player's own is not drawn until the counter.
    let members = lobby.members.clone();
    let page = lobby_page();
    let gone: Vec<u32> = lobby.figures.keys().filter(|id| !members.iter().any(|m| m.id == **id)).copied().collect();
    for id in gone {
        if let Some(figure) = lobby.figures.remove(&id) {
            world.entity_mut(figure.model).despawn();
            world.entity_mut(figure.label).despawn();
            world.entity_mut(figure.crown).despawn();
        }
    }
    let teams = lobby
        .teams
        .get_or_insert_with(|| {
            let mut materials = world.resource_mut::<Assets<StandardMaterial>>();
            [0, 1].map(|side| materials.add(StandardMaterial { base_color: team(side), perceptual_roughness: 0.55, ..default() }))
        })
        .clone();
    let models = world.resource::<player_model::PlayerModels>().clone();
    let loudest = members.iter().filter(|m| m.talk > 0.12).max_by(|a, b| a.talk.total_cmp(&b.talk)).map(|m| m.id);
    let row = members.len() as f32;
    let mut marks = vec![];
    for (index, member) in members.iter().enumerate() {
        let (home, inward) = place(member.id);
        let figure = lobby.figures.entry(member.id).or_insert_with(|| {
            let model = models.spawn(world, home, member.id);
            world.entity_mut(model).insert(LobbyPart);
            let label = world
                .spawn((
                    LobbyPart,
                    Text::new(""),
                    TextFont { font_size: FontSize::Px(15.), ..default() },
                    TextColor(Color::WHITE),
                    Node { position_type: PositionType::Absolute, padding: UiRect::axes(Val::Px(7.), Val::Px(2.)), border_radius: BorderRadius::all(Val::Px(4.)), ..default() },
                    BackgroundColor(Color::srgba(0.03, 0.07, 0.07, 0.78)),
                    Visibility::Hidden,
                ))
                .id();
            let crown = crown(world);
            // The parts that take a team's colour in Forts, and what they wear otherwise.
            let parts = world.get::<player_model::Livery>(model).map(|livery| livery.0.clone()).unwrap_or_default();
            let livery = parts.into_iter().filter_map(|part| world.get::<MeshMaterial3d<StandardMaterial>>(part).map(|m| (part, m.0.clone()))).collect();
            Figure { model, label, crown, yaw: inward, woke: now, livery, dressed: None }
        });
        // In the fridge it faces the middle of the ring, or whoever is talking, or the opening door; having just woken it
        // gets up out of its place in the tray and hops over to its place in the ring, growing to its full size on the way.
        // On the counter the players stand in a row before the package and throw their arms open as they pop up.
        let (at, aim, size) = if counter {
            let rise = smooth((t - POP - 0.14 * index as f32) / 0.45);
            let hop = (rise * PI).sin() * 0.03;
            (Vec3::new(pack.x + (index as f32 - (row - 1.) / 2.) * 0.085, start.y + hop, pack.z - 0.165), 0., rise)
        } else {
            let aim = if opening && swing > 0.02 {
                facing(home, Vec3::new(home.x * 0.4, SHELF, 0.4))
            } else {
                match loudest.filter(|id| *id != member.id) {
                    Some(id) => facing(home, place(id).0),
                    None => company(&members, member.id).map_or(inward, |others| facing(home, others)),
                }
            };
            let up = smooth((now - figure.woke) / WAKE);
            let bed = pack + Vec3::new(slot(seat(member.id)), 0., 0.);
            (bed.lerp(home, up) + Vec3::Y * (up * PI).sin() * 0.06, aim, 0.45 + 0.55 * up)
        };
        figure.yaw = if counter { aim } else { figure.yaw + wrap(aim - figure.yaw) * (1. - (-dt * 7.).exp()) };
        let own = member.id == lobby.me && !counter;
        let (model, label, crown, yaw) = (figure.model, figure.label, figure.crown, figure.yaw);
        world.entity_mut(model).insert((
            Transform::from_translation(at).with_rotation(Quat::from_rotation_y(yaw)).with_scale(Vec3::splat(size.max(0.001))),
            if own || size <= 0. { Visibility::Hidden } else { Visibility::Visible },
        ));
        // Forts: the team's colour on its legs and badge, and under its name.
        if figure.dressed != member.side {
            figure.dressed = member.side;
            for (part, own) in figure.livery.clone() {
                let material = member.side.map_or(own, |side| teams[side as usize].clone());
                if let Ok(mut part) = world.get_entity_mut(part) {
                    part.insert(MeshMaterial3d(material));
                }
            }
            if let Some(mut back) = world.get_mut::<BackgroundColor>(label) {
                back.0 = match member.side {
                    Some(side) => {
                        let [r, g, b] = crate::forts::TEAM_RGB[side.min(1) as usize];
                        Color::srgba(r * 0.55, g * 0.55, b * 0.55, 0.88)
                    }
                    None => Color::srgba(0.03, 0.07, 0.07, 0.78),
                };
            }
        }
        player_model::paint(world, model, &member.color);
        player_model::animate(world, model, now, player_model::Cues { alive: true, cheer: counter && size > 0.6, ..default() });
        player_model::speak(world, model, member.talk, now);
        marks.push((label, crown, at + Vec3::Y * LABEL * size, !own && size > 0.5, member.name.clone(), member.ready, member.leader));
    }

    // The camera: from this player's place in the ring, turned by the mouse; then on the counter, closing in on the package.
    let cursor = world.query::<&Window>().iter(world).next().and_then(|window| {
        window.cursor_position().map(|at| Vec2::new(at.x / window.width().max(1.) * 2. - 1., at.y / window.height().max(1.) * 2. - 1.))
    });
    // Touch controls (touch.rs): a drag on the kitchen moves the view as the mouse's place would. (Not while the menu
    // is open over it: menu.rs.)
    let cursor = cursor.or_else(|| crate::touch::cursor(world)).filter(|_| !world.get_resource::<crate::menu::Menu>().is_some_and(|m| m.open));
    if let Some(cursor) = cursor {
        let want = Vec2::new(-cursor.x * TURN.x, -cursor.y * TURN.y);
        lobby.look = lobby.look + (want - lobby.look) * (1. - (-dt * 9.).exp());
    }
    let (home, inward) = place(lobby.me);
    // It looks at the others (alone, at the franks asleep in the tray), and a little down, which lifts their faces clear of the
    // page's panel.
    let ahead = company(&members, lobby.me).map_or(facing(home, PACK), |others| facing(home, others));
    let _ = inward;
    let view = if counter {
        let close = smooth((t - CUT) / (SEQUENCE - CUT));
        Transform::from_translation(pack + Vec3::new(0.02, 0.23 - 0.05 * close, -0.70 + 0.12 * close)).looking_at(pack + Vec3::new(0., 0.065, -0.13), Vec3::Y)
    } else {
        // As the door opens the view comes round to it.
        let out = smooth((t - DOOR.0 - 0.3) / 1.6);
        let yaw = ahead + lobby.look.x * (1. - out) + wrap(facing(home, Vec3::new(0.05, SHELF, 0.5)) - ahead) * out;
        // The eye is a step behind the hotdog's own place, which brings its neighbours into the picture.
        Transform::from_translation(home + Vec3::Y * EYES + (home - CENTRE).normalize_or_zero() * STEP_BACK).with_rotation(Quat::from_rotation_y(yaw) * Quat::from_rotation_x((lobby.look.y - 0.17) * (1. - out) - 0.04))
    };
    let cameras: Vec<Entity> = world.query_filtered::<Entity, With<maya::HotdogCamera>>().iter(world).collect();
    let mut lens = None;
    for camera in cameras {
        world.entity_mut(camera).remove::<ChildOf>();
        if let Some(mut tr) = world.get_mut::<Transform>(camera) {
            *tr = view;
        }
        if let Some(mut projection) = world.get_mut::<Projection>(camera) {
            if let Projection::Perspective(ref mut p) = *projection {
                p.fov = if counter { 50_f32 } else { 78_f32 }.to_radians();
                p.near = 0.004;
            }
        }
        // Shut in, the theme has turned its light off (visuals::follow_intro); a little is left to see faces by.
        if t < LAMP {
            if let Some(mut ambient) = world.get_mut::<AmbientLight>(camera) {
                ambient.brightness = FILL;
            }
        }
        lens = world.get::<Camera>(camera).cloned();
    }
    // Names float over the heads, gold once their player is ready, with a crown over the leader's.
    for (label, crown, at, seen, name, ready, leader) in marks {
        let spot = lens.as_ref().and_then(|camera| camera.world_to_viewport(&GlobalTransform::from(view), at).ok()).filter(|_| seen && (at - view.translation).dot(*view.forward()) > 0.02);
        if let (Some(spot), Some(mut node)) = (spot, world.get_mut::<Node>(label)) {
            node.left = Val::Px(spot.x - 4. - name.chars().count() as f32 * 4.6);
            node.top = Val::Px(spot.y - 24.);
        }
        if let (Some(spot), Some(mut node)) = (spot, world.get_mut::<Node>(crown)) {
            node.left = Val::Px(spot.x + 3. - 9.);
            node.top = Val::Px(spot.y - 24. - 16.);
        }
        world.entity_mut(crown).insert(if spot.is_some() && leader { Visibility::Visible } else { Visibility::Hidden });
        if let Some(mut text) = world.get_mut::<Text>(label) {
            if text.0 != name {
                text.0 = name;
            }
        }
        if let Some(mut color) = world.get_mut::<TextColor>(label) {
            color.0 = if ready { Color::srgb(0.96, 0.78, 0.42) } else { Color::srgb(0.96, 0.95, 0.87) };
        }
        world.entity_mut(label).insert(if spot.is_some() { Visibility::Visible } else { Visibility::Hidden });
    }
    // Without a page to show them: the header, the ready queue and what to press.
    let status = world.get_resource::<crate::network::Network>().map(|net| net.status.clone()).unwrap_or_default();
    let mut lines = vec!["HOTDOG SIMULATOR".to_string()];
    if lobby.stage == Stage::Opening {
        lines.push("The fridge opens".into());
    } else if lobby.waiting {
        lines.push("Round in progress: you join the next one".into());
    } else {
        lines.push("Waiting for game".into());
    }
    if members.is_empty() {
        lines.push(status);
    } else {
        lines.push(format!(
            "{:02} / 05 | {}",
            members.len(),
            match (lobby.stage, lobby.countdown) {
                (Stage::Opening, _) => "the fridge opens".to_string(),
                (_, Some(left)) => format!("starting in {}", left.ceil().max(1.) as u32),
                _ => format!("{} ready", members.iter().filter(|m| m.ready).count()),
            }
        ));
        // What is next, as the leader chose it (once the fridge opens, the map that is played), and who chooses.
        let playing = {
            let g = world.resource::<arena::Game>();
            playing_line(g.feast.mode().name(), &g.feast.level.map.name)
        };
        let next = if lobby.stage == Stage::Opening { playing } else { lobby.choice.clone() };
        if !next.is_empty() {
            lines.push(format!("Next: {next}{}", if lobby.leader.is_empty() { String::new() } else { format!("  (chosen by {})", lobby.leader) }));
        }
        for member in &members {
            lines.push(format!(
                "{} {}{}{}",
                if member.ready { "[x]" } else { "[ ]" },
                member.name,
                if member.leader { "  (leader)" } else { "" },
                if member.id == lobby.me { "  (you)" } else { "" }
            ));
        }
        if lobby.stage == Stage::Lobby && !lobby.waiting {
            lines.push(if me.is_some_and(|m| m.ready) { "ENTER  not ready".into() } else { "ENTER  ready".into() });
        }
    }
    let content = lines.join("\n");
    if let Some(mut text) = world.get_mut::<Text>(hud) {
        if text.0 != content {
            text.0 = content;
        }
    }
    world.entity_mut(hud).insert(if page || counter { Visibility::Hidden } else { Visibility::Visible });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn five_hotdogs_stand_apart_on_the_shelf_between_the_package_and_the_door_facing_the_middle() {
        let places: Vec<(Vec3, f32)> = (1..=5).map(place).collect();
        for (i, (at, yaw)) in places.iter().enumerate() {
            assert_eq!(at.y, SHELF);
            // Clear of the package behind (its front edge) and of the jar and the carton at the sides, with the top of its
            // head (6 mm across at that height) clear of the door's bin, which reaches in to 0.0925 m just above it.
            assert!(at.z - 0.045 > PACK.z + 0.1135 && at.z + 0.006 < -0.0925 && at.z + 0.045 < -0.01 && at.x.abs() + 0.045 < 0.185, "{at:?}");
            let forward = Quat::from_rotation_y(*yaw) * Vec3::NEG_Z;
            assert!(forward.dot((CENTRE - *at).normalize()) > 0.999, "slot {i} faces the middle");
            for (other, _) in &places[i + 1..] {
                assert!(at.distance(*other) > 0.12, "{at:?} {other:?}");
            }
        }
        // Slot six shares slot one's place rather than standing somewhere new.
        assert!(place(6).0.distance(place(1).0) < 1e-6);
    }
    #[test]
    fn a_hotdog_turns_the_short_way_round_to_whoever_is_talking() {
        let (a, _) = place(1);
        let (b, _) = place(3);
        let yaw = facing(a, b);
        assert!((Quat::from_rotation_y(yaw) * Vec3::NEG_Z).dot((b - a).normalize()) > 0.999);
        assert!((wrap(3.0 - -3.0) - (6.0 - TAU)).abs() < 1e-5 && (wrap(0.2) - 0.2).abs() < 1e-6);
    }
    #[test]
    fn the_start_sequence_fits_its_time_in_order() {
        assert!(DOOR.0 < LAMP + 0.5 && DOOR.0 + DOOR.1 < CUT && CUT < PEEL.0 && PEEL.0 + PEEL.1 <= POP + 0.3 && POP + 4. * 0.14 + 0.45 < SEQUENCE);
        assert!((8. ..=12.).contains(&SEQUENCE));
    }
    /// A frame as JSON clients see it (the debug encoding), typed.
    fn framed(json: serde_json::Value) -> kitchen_sim::net::Frame {
        serde_json::from_value(json).unwrap()
    }
    #[test]
    fn a_snapshot_says_who_is_in_the_fridge_and_whether_this_player_is_held_there() {
        let mut world = World::new();
        world.insert_resource(Time::<Real>::default());
        world.insert_resource(Lobby { online: true, holding: true, ..default() });
        let player = |id: u32, human: bool, waiting: bool, name: &str| {
            serde_json::json!({"id":id,"human":human,"waiting":waiting,"ready":id==1,"name":if waiting {"BOT"} else {name},"color":"blue","talk":0.5,
                "profile":if human {serde_json::json!({"name":name,"color":"green"})} else {serde_json::Value::Null}})
        };
        let snapshot = |stage: &str| serde_json::json!({"stage":stage,"countdown":2.4,"opening":1.5,"opening_for":9.0,"alert":"x",
            "players":[player(1,true,false,"Ann"),player(2,true,true,"Late"),player(3,false,false,"MUSTARD")]});
        let mut held = |me: u32, stage: &str| {
            read(&mut world, &framed(snapshot(stage)), me);
            let lobby = world.resource::<Lobby>();
            (lobby.holding, lobby.members.iter().map(|m| (m.id, m.name.clone(), m.color.clone(), m.ready)).collect::<Vec<_>>())
        };
        // Seated: held in the lobby and through the start sequence, released into the round; bots are not in the fridge.
        assert_eq!(held(1, "Lobby"), (true, vec![(1, "Ann".into(), "green".into(), true)]));
        assert!(held(1, "Opening").0);
        for stage in ["Warmup", "Wave", "FreeForAll", "RoundOver"] {
            assert!(!held(1, stage).0, "{stage}");
        }
        // Waiting for the next round: held throughout, among the others who are waiting, under the name it chose.
        assert_eq!(held(2, "Wave"), (true, vec![(2, "Late".into(), "green".into(), false)]));
    }
    #[test]
    fn a_snapshot_says_who_leads_what_is_next_and_in_forts_the_sides() {
        let mut world = World::new();
        world.insert_resource(Time::<Real>::default());
        world.insert_resource(Lobby { online: true, holding: true, ..default() });
        let player = |id: u32, name: &str| serde_json::json!({"id":id,"human":true,"waiting":false,"ready":false,"name":name,"color":"red","talk":0.});
        let forts = serde_json::json!({"stage":"Lobby","leader":2,"choice":{"name":"pillow-fort","title":"Pillow Fort","mode":"forts","theme":"kitchen"},
            "sides":{"1":1,"2":0},"players":[player(1,"Ann"),player(2,"Ben"),player(3,"Cy")]});
        read(&mut world, &framed(forts), 1);
        let lobby = world.resource::<Lobby>();
        assert_eq!(lobby.members.iter().map(|m| (m.id, m.leader, m.side)).collect::<Vec<_>>(), vec![(1, false, Some(1)), (2, true, Some(0)), (3, false, None)]);
        assert_eq!((lobby.choice.as_str(), lobby.leader.as_str()), ("Forts | Pillow Fort | kitchen", "Ben"));
        // Random, a siege, and nobody leading: no sides, no crown.
        let random = serde_json::json!({"stage":"Lobby","leader":0,"choice":{"name":"random","title":"Random","mode":"siege","theme":"","random":true},"players":[player(1,"Ann")]});
        read(&mut world, &framed(random), 1);
        let lobby = world.resource::<Lobby>();
        assert_eq!((lobby.choice.as_str(), lobby.leader.as_str(), lobby.members[0].leader, lobby.members[0].side), ("Siege | Random map", "", false, None));
    }
    #[test]
    fn a_name_tag_is_written_in_the_colour_its_player_chose() {
        let mut world = World::new();
        world.insert_resource(Time::<Real>::default());
        world.insert_resource(Lobby { online: true, ..default() });
        let player = |id: u32, human: bool, color: &str| serde_json::json!({"id":id,"human":human,"waiting":false,"name":"x","color":color,"talk":0.});
        let round = serde_json::json!({"stage":"Wave","players":[player(1,true,"green"),player(2,true,"black"),player(3,false,"")]});
        read(&mut world, &framed(round), 1);
        // This player and the other person in the round, in the colours they chose; a bot, or nobody, in none.
        let lobby = world.resource::<Lobby>();
        assert_eq!([1, 2, 3, 4].map(|id| lobby.colour(id)), [player_model::tag_rgb("green"), player_model::tag_rgb("black"), None, None]);
    }
    #[test]
    fn the_choice_names_its_mode_fully_loaded_included() {
        let mut world = World::new();
        world.insert_resource(Time::<Real>::default());
        world.insert_resource(Lobby { online: true, holding: true, ..default() });
        let player = serde_json::json!({"id":1,"human":true,"waiting":false,"ready":false,"name":"Ann","color":"red","talk":0.});
        let mut chosen = |name: &str, title: &str, mode: &str, theme: &str, random: bool| {
            let frame = serde_json::json!({"stage":"Lobby","leader":1,"choice":{"name":name,"title":title,"mode":mode,"theme":theme,"random":random},"players":[player.clone()]});
            read(&mut world, &framed(frame), 1);
            world.resource::<Lobby>().choice.clone()
        };
        // Fully Loaded is played on Counter Siege alone, so the server names that map for it.
        assert_eq!(chosen("counter-siege", "Counter Siege", "cards", "kitchen", false), "Fully Loaded | Counter Siege | kitchen");
        assert_eq!(chosen("counter-siege", "Counter Siege", "cards", "", false), "Fully Loaded | Counter Siege");
        // The others are named as they were.
        assert_eq!(chosen("counter-siege", "Counter Siege", "siege", "kitchen", false), "Siege | Counter Siege | kitchen");
        assert_eq!(chosen("pillow-fort", "Pillow Fort", "forts", "kitchen", false), "Forts | Pillow Fort | kitchen");
        assert_eq!(chosen("pillow-fort", "Pillow Fort", "ffa", "kitchen", false), "Free For All | Pillow Fort | kitchen");
        assert_eq!(chosen("random", "Random", "forts", "", true), "Forts | Random map");
        // Kung Fu, on its dojo.
        assert_eq!(chosen("kung-fu-dojo", "Kung Fu Dojo", "kungfu", "kitchen", false), "Kung Fu | Kung Fu Dojo | kitchen");
        assert_eq!(chosen("random", "Random", "kungfu", "", true), "Kung Fu | Random map");
        // Titan, on the kitchen floor.
        assert_eq!(chosen("titans-kitchen", "Titan's Kitchen", "titan", "night", false), "Titan | Titan's Kitchen | night");
        assert_eq!(chosen("", "", "", "", false), "", "nothing chosen yet");
    }
    #[test]
    fn the_next_line_names_the_mode_of_the_round_being_played() {
        // Once the fridge has opened the "Next:" line says what is played: the mode (the game's, not the map's), then the
        // map.
        assert_eq!(playing_line("cards", "Counter Siege"), "Fully Loaded | Counter Siege");
        assert_eq!(playing_line("siege", "Counter Siege"), "Siege | Counter Siege");
        assert_eq!(playing_line("forts", "Pillow Fort"), "Forts | Pillow Fort");
        assert_eq!(playing_line("siege", "Pillow Fort"), "Siege | Pillow Fort", "a Forts map played as a siege");
        assert_eq!(playing_line("ffa", "Horde Night"), "Free For All | Horde Night");
        assert_eq!(playing_line("kungfu", "Kung Fu Dojo"), "Kung Fu | Kung Fu Dojo");
        assert_eq!(playing_line("titan", "Titan's Kitchen"), "Titan | Titan's Kitchen");
        // As the game says it.
        let mut g = kitchen_sim::feast::Feast::with_map(kitchen_sim::map::Map::parse(include_str!("../assets/maps/pillow-fort.json")).unwrap(), false);
        assert_eq!(g.mode().name(), "forts");
        g.set_rules(kitchen_sim::rules::Rules::parse("mode=siege"));
        assert_eq!(playing_line(g.mode().name(), &g.level.map.name), "Siege | Countertop Fort");
    }
    #[test]
    fn each_seat_has_its_own_frank_in_the_tray_and_its_own_place_on_the_shelf() {
        // Five franks, a frank's thickness (3.4 cm) apart and more, all inside the tray's red edges (0.109 m either side of its middle).
        let slots: Vec<f32> = (0..5).map(slot).collect();
        for pair in slots.windows(2) {
            assert!(pair[1] - pair[0] > 0.036, "{slots:?}");
        }
        assert!(slots.iter().all(|x| x.abs() + 0.017 < 0.109), "{slots:?}");
        // Hotdog slots 1 to 5 are seats 0 to 4, left to right; a sixth (Forts has six hotdogs) would share the first's.
        assert_eq!((1..=6).map(seat).collect::<Vec<_>>(), vec![0, 1, 2, 3, 4, 0]);
        // The tray lies behind the ring, clear of every place in it.
        for id in 1..=5 {
            let (at, _) = place(id);
            assert!(at.z - 0.045 > PACK.z + 0.114, "{at:?}");
        }
    }
}
