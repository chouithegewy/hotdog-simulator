// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Health over heads, in every mode (the siege, the demo, multiplayer and Forts).
//!
//! * [`place`] floats a small tag over every other hotdog: its name over a bar of its health, the bar green
//!   when full, amber at half and red when nearly gone, the name and the edge in the hotdog's colour (its
//!   team's in Forts, as its legs and ring wear it; otherwise the colour its player chose, as its ring wears it,
//!   or for a bot, and offline, the colour of its place). The tag sits just over the
//!   head (over the flag on its pole in Forts), and is a little larger near and smaller far, in whole
//!   pixels between a floor and a ceiling ([`size`]). It fades out behind a block, a raised deck or a
//!   loose object (the forcefield is glass, and is seen through: [`hidden`]) and far away ([`reach`]),
//!   and is not drawn off the screen, behind the camera, or up among the head-up display's lines, the
//!   alert and the hint panel at the top of the screen (it fades out there). A hotdog that dies shows OUT for a moment;
//!   an eaten one shows nothing ([`wear`]). The player's own hotdog has none, alive (the camera is in
//!   its head) or dead (the death camera watches its body, and needs no label on it); the death camera
//!   sees everybody else's, and the spectator and the demo everybody's.
//! * Titan: the giant wears none (a tag sized for a hotdog would sit at its ankles, and the boss bar across the top
//!   names it); to the giant's player the small hotdogs' tags reach across the whole room ([`reach`]), the way it
//!   finds specks on the floor, with the rings round their feet (titan/camera.rs).
//! * The player's own health, which it cannot see over its own head, is the head-up display's (hud.rs): a heart, a
//!   bar and a number in the bottom-left corner.
//!
//! Both are UI nodes. The tags are placed once the camera and the bodies have their places for the frame
//! and before the UI is laid out, so they keep to the bodies instead of trailing a frame behind.
use crate::arena::Game;
use crate::maya::HotdogCamera;
use crate::player_model::{Gait, PlayerBody, Rig};
use bevy::prelude::*;
use kitchen_sim::feast::{Feast, Player};
use std::collections::HashMap;

/// A tag's name is BASE_PX pixels high for a hotdog BASE_AT metres from the eye, and grows and shrinks
/// with the square root of the distance between MIN_PX and MAX_PX, in whole pixels.
const BASE_PX: f32 = 13.;
const BASE_AT: f32 = 0.9;
const MIN_PX: f32 = 10.;
const MAX_PX: f32 = 15.;
/// How far the size the distance calls for has to be from the size shown before it changes, so that a
/// hotdog standing about a boundary does not flicker between two.
const HYSTERESIS: f32 = 0.7;
/// Tags fade out between these distances from the eye, in metres (the siege's counter is 3.6 m long,
/// the Forts map 6 m); and to the titan's player, across Titan's Kitchen (7.2 m by 6 m, its eye 1.7 m up: 9.6 m from
/// corner to corner).
const FADE: f32 = 3.6;
const FAR: f32 = 4.6;
const GIANT_FADE: f32 = 10.;
const GIANT_FAR: f32 = 11.5;
/// A tag this close in front of the eye, or behind it, is not drawn.
const NEAR: f32 = 0.06;
/// Seconds a hotdog that has just died shows OUT, and the seconds that then takes to fade.
const MARK: f32 = 2.2;
const MARK_FADE: f32 = 0.6;
/// How fast a tag fades in or out as its hotdog goes behind cover or comes out: all the way in a fifth
/// of a second.
const BLINK: f32 = 5.;
/// Pixels between the top of the head and the bottom of the tag.
const GAP: f32 = 3.;
/// The foot of the band across the top of the screen that the head-up display's score, clock and objective take
/// (hud.rs, at a 720-pixel window: it grows with the window as they do), lower where the alert or the hint panel is
/// up: a tag fades out over the SHELF pixels under it, and is gone by the time it would reach up into it.
const CEILING: f32 = 84.;
const SHELF: f32 = 24.;
/// The middle of the sausage in the rig's torso frame (deathcam.rs), which a dead hotdog's OUT is over.
const MIDDLE: Vec3 = Vec3::new(0., 0.098, 0.);
/// The grey a dead hotdog's OUT is written in.
const GREY: [f32; 3] = [0.62, 0.64, 0.63];

pub struct Plugin;
impl bevy::app::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Tags>().add_systems(
            PostUpdate,
            place.after(bevy::transform::TransformSystems::Propagate).before(bevy::ui::UiSystems::Content),
        );
    }
}

/// One hotdog's tag: its nodes, and what is eased from frame to frame.
struct Tag {
    root: Entity,
    name: Entity,
    trough: Entity,
    fill: Entity,
    /// How high over the feet the tag stands (eased, so that crouching or dying does not make it jump), the
    /// size of its name, the size its nodes were last laid out for, and how much of it shows (0 to 1).
    lift: f32,
    px: f32,
    laid: f32,
    alpha: f32,
    /// Whether the hotdog was alive on the last frame, and when it was seen to die (never, for one that was
    /// already dead when its tag was made).
    alive: bool,
    died: f32,
}
#[derive(Resource, Default)]
struct Tags {
    by: HashMap<u32, Tag>,
}

/// What a hotdog's tag shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Show {
    Nothing,
    Health,
    Out,
}
/// Which tag a hotdog wears, and how much of it shows: nothing over the player's own hotdog (`own`),
/// played from inside it while it lives and watched from the death camera once it is dead, nothing over
/// the titan (`titan`: the boss bar names it), nothing over a hotdog that has been eaten, OUT for a moment over one
/// that has just died (`dead_for` seconds ago), and otherwise its health.
pub fn wear(own: bool, titan: bool, alive: bool, consumed: bool, dead_for: f32) -> (Show, f32) {
    if consumed || own || titan {
        return (Show::Nothing, 0.);
    }
    if alive {
        return (Show::Health, 1.);
    }
    let left = 1. - ((dead_for - MARK) / MARK_FADE).clamp(0., 1.);
    if left > 0. { (Show::Out, left) } else { (Show::Nothing, 0.) }
}

/// The size of a tag's name, in whole pixels, for a hotdog `distance` metres from the eye whose tag is now
/// `shown` pixels: nearer is larger and further smaller, between MIN_PX and MAX_PX, and the size only changes
/// once the distance calls for one HYSTERESIS pixels away from it.
pub fn size(distance: f32, shown: f32) -> f32 {
    let wanted = (BASE_PX * (BASE_AT / distance.max(0.01)).sqrt()).clamp(MIN_PX, MAX_PX);
    if (MIN_PX..=MAX_PX).contains(&shown) && (wanted - shown).abs() < HYSTERESIS {
        shown
    } else {
        wanted.round()
    }
}

/// How much of a tag shows at `distance` metres from the eye: all of it out to FADE, none beyond FAR; to the titan's
/// player (`giant`), out to GIANT_FADE and GIANT_FAR.
pub fn reach(distance: f32, giant: bool) -> f32 {
    let (fade, far) = if giant { (GIANT_FADE, GIANT_FAR) } else { (FADE, FAR) };
    (1. - (distance - fade) / (far - fade)).clamp(0., 1.)
}

/// The colour of a health bar holding `hp` of 100: green when full, amber at half, red when nearly gone.
pub fn health_rgb(hp: f32) -> [f32; 3] {
    const LOW: [f32; 3] = [0.96, 0.17, 0.12];
    const HALF: [f32; 3] = [0.98, 0.72, 0.16];
    const FULL: [f32; 3] = [0.34, 0.86, 0.36];
    let t = (hp / 100.).clamp(0., 1.);
    let (a, b, u) = if t < 0.5 { (LOW, HALF, t * 2.) } else { (HALF, FULL, t * 2. - 1.) };
    [0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * u)
}

/// A hotdog's own colour: its team's in Forts, as its legs and ring wear it; otherwise the colour its player chose
/// (`chosen`, from the lobby), as its ring wears it, or without one (a bot, and offline) the colour of its place.
pub(crate) fn colour(p: &Player, forts: bool, chosen: Option<[f32; 3]>) -> [f32; 3] {
    if forts {
        crate::forts::TEAM_RGB[(p.team as usize).min(1)]
    } else {
        chosen.unwrap_or(kitchen_sim::feast::COLORS[(p.id as usize).saturating_sub(1) % 5])
    }
}

/// How high over its feet a hotdog's tag stands: over its head, standing, crouching or prone, and in Forts
/// over the block of team colour on the pole at its back; a dead one's over the middle of its body.
fn crown(p: &Player, forts: bool) -> f32 {
    if !p.alive {
        return 0.07;
    }
    let head = if p.prone {
        0.08
    } else if p.crouch {
        0.14
    } else {
        0.19
    };
    head + match (forts, p.prone) {
        (false, _) => 0.,
        (true, false) => 0.06,
        (true, true) => 0.02,
    }
}

/// The heights over its feet of the points on a hotdog the eye must see one of for its tag to show: its
/// head and its middle.
fn marks(p: &Player) -> [f32; 2] {
    if p.prone || !p.alive {
        [0.05, 0.03]
    } else if p.crouch {
        [0.10, 0.055]
    } else {
        [0.15, 0.08]
    }
}

/// Whether anything that hides a hotdog stands between the eye and `target`: a block, a raised deck or a
/// loose object (but not `carried`, the one the player holds in front of its own eyes). The forcefield,
/// which stops everything while it stands, is blue glass, and is seen through.
pub fn hidden(feast: &Feast, eye: Vec3, target: Vec3, carried: Option<usize>) -> bool {
    let to = target - eye;
    let length = to.length();
    if length < 1e-4 {
        return false;
    }
    let way = to / length;
    let field = feast.level.fort.as_ref().map(|fort| fort.field.map(|v| v as f32));
    let (mut from, mut left) = (eye, length);
    let mut clear = false;
    for _ in 0..3 {
        let wall = feast.level.cover_from(from, way);
        let stop = feast
            .props
            .raycast(from.as_dvec3().to_array(), way.as_dvec3().to_array(), wall.min(left) as f64, carried)
            .map_or(wall, |hit| hit.distance as f32);
        if stop >= left - 0.004 {
            clear = true;
            break;
        }
        // Stopped at the forcefield: look on through it, from where the line leaves it.
        let at = from + way * stop;
        let Some([x0, z0, x1, z1]) = field.filter(|[x0, z0, x1, z1]| at.x > x0 - 0.003 && at.x < x1 + 0.003 && at.z > z0 - 0.003 && at.z < z1 + 0.003) else {
            return true;
        };
        let out = leaves(from, way, Vec2::new(x0, z0), Vec2::new(x1, z1)) + 0.002;
        if out <= stop {
            return true;
        }
        (from, left) = (from + way * out, left - out);
        if left <= 0. {
            clear = true;
            break;
        }
    }
    // A raised deck is a box from the counter up to its top, which the shots' cover leaves out.
    !clear
        || feast.level.map.decks.iter().any(|deck| {
            let [x0, z0, x1, z1] = deck.rect.map(|v| v as f32);
            let floor = kitchen_sim::map::FLOOR as f32;
            let (min, max) = (Vec3::new(x0, floor - 0.01, z0), Vec3::new(x1, floor + deck.height as f32, z1));
            let inside = |p: Vec3| p.cmpge(min).all() && p.cmple(max).all();
            !inside(eye) && !inside(target) && kitchen_sim::ray_box(eye, way, min, max).is_some_and(|t| t < length - 0.004)
        })
}
/// How far along `way` from `from` a line leaves the column over the rectangle `low`..`high` (x, z).
fn leaves(from: Vec3, way: Vec3, low: Vec2, high: Vec2) -> f32 {
    let mut far = f32::INFINITY;
    for (o, d, a, b) in [(from.x, way.x, low.x, high.x), (from.z, way.z, low.y, high.y)] {
        if d.abs() > 1e-6 {
            far = far.min(((a - o) / d).max((b - o) / d));
        }
    }
    if far.is_finite() { far } else { 0. }
}

fn rgba(c: [f32; 3], a: f32) -> Color {
    Color::srgba(c[0], c[1], c[2], a)
}
/// A colour between `c` and white.
fn lighter(c: [f32; 3], by: f32) -> [f32; 3] {
    c.map(|v| v + (1. - v) * by)
}

/// A new tag, hidden until it is placed.
fn spawn(commands: &mut Commands, alive: bool) -> Tag {
    let name = commands
        .spawn((Text::new(""), TextFont { font_size: FontSize::Px(BASE_PX), ..default() }, TextColor(Color::NONE), TextLayout::no_wrap()))
        .id();
    let fill = commands.spawn((Node { width: Val::Percent(100.), height: Val::Percent(100.), ..default() }, BackgroundColor(Color::NONE))).id();
    let trough = commands.spawn((Node { width: Val::Px(60.), height: Val::Px(5.), ..default() }, BackgroundColor(Color::NONE))).add_child(fill).id();
    let root = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(2.),
                border: UiRect::all(Val::Px(1.)),
                border_radius: BorderRadius::all(Val::Px(4.)),
                ..default()
            },
            BackgroundColor(Color::NONE),
            BorderColor::all(Color::NONE),
            // Under the scope's mask (arena.rs, ZIndex -1) and the head-up display.
            ZIndex(-2),
            Visibility::Hidden,
        ))
        .add_children(&[name, trough])
        .id();
    Tag { root, name, trough, fill, lift: -1., px: BASE_PX, laid: 0., alpha: 0., alive, died: f32::NEG_INFINITY }
}

fn paint(paints: &mut Query<&mut BackgroundColor>, entity: Entity, color: Color) {
    if let Ok(mut paint) = paints.get_mut(entity) {
        if paint.0 != color {
            paint.0 = color;
        }
    }
}
fn show(shown: &mut Query<&mut Visibility>, entity: Entity, on: bool) {
    let want = if on { Visibility::Visible } else { Visibility::Hidden };
    if let Ok(mut vis) = shown.get_mut(entity) {
        if *vis != want {
            *vis = want;
        }
    }
}

/// Whether the tags are up: in a round only (not in the fridge between rounds, `holding`, nor in the breakfast scene),
/// and not at the card table, which keeps its own clear of its scoreboard (cards.rs).
pub fn up(g: &Game, holding: bool) -> bool {
    g.playing() && !holding && !crate::cards::at_table(g)
}

/// Every frame: each hotdog's tag where its head is on the screen, or hidden.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn place(
    mut commands: Commands,
    time: Res<Time<Real>>,
    game: Option<Res<Game>>,
    options: Res<crate::runtime::Options>,
    lobby: Option<Res<crate::lobby::Lobby>>,
    cameras: Query<(&Camera, &GlobalTransform), With<HotdogCamera>>,
    bodies: Query<(&Gait, &GlobalTransform, &InheritedVisibility, Option<&Rig>), With<PlayerBody>>,
    joints: Query<&GlobalTransform>,
    mut tags: ResMut<Tags>,
    mut nodes: Query<(&mut Node, &ComputedNode)>,
    mut paints: Query<&mut BackgroundColor>,
    mut roots: Query<(&mut BorderColor, &mut ZIndex)>,
    mut texts: Query<(&mut Text, &mut TextFont, &mut TextColor)>,
    mut shown: Query<&mut Visibility>,
    banners: Query<(&ComputedNode, &UiGlobalTransform, &InheritedVisibility), Or<(With<crate::arena::FoodAlert>, With<crate::hints::Board>)>>,
) {
    let (now, dt) = (time.elapsed_secs(), time.delta_secs().min(0.1));
    // The alert under the lines at the top (arena.rs: FoodAlert) and the hint panel under it (hints.rs), where they
    // are up: a tag under one would show in bits round its edges, so the ceiling is its foot over the width it spans.
    let shelves: Vec<(f32, f32, f32)> = banners
        .iter()
        .filter(|(_, _, up)| up.get())
        .map(|(laid, at, _)| {
            let (half, middle) = (laid.size() * laid.inverse_scale_factor / 2., at.translation * laid.inverse_scale_factor);
            (middle.x - half.x, middle.x + half.x, middle.y + half.y)
        })
        .collect();
    let game = game.filter(|g| up(g, lobby.as_ref().is_some_and(|lobby| lobby.holding)));
    let camera = cameras.iter().find(|(camera, _)| camera.is_active);
    let (Some(g), Some((camera, view))) = (game, camera) else {
        for tag in tags.by.values_mut() {
            tag.alpha = 0.;
            show(&mut shown, tag.root, false);
        }
        return;
    };
    // A hotdog that has left the roster (a Forts map after a siege, say) takes its tag with it.
    tags.by.retain(|id, tag| {
        let kept = g.roster.iter().any(|p| p.id == *id);
        if !kept {
            commands.entity(tag.root).despawn();
        }
        kept
    });
    let spectator = options.demo || g.demo;
    let forts = g.fort.is_some();
    let (eye, ahead) = (view.translation(), view.forward());
    let screen = camera.logical_viewport_size().unwrap_or(Vec2::new(1280., 720.));
    let carried = g.roster.iter().find(|p| p.id == g.me()).and_then(|p| usize::try_from(p.carry).ok());
    let titan = g.feast.titan_id();
    let giant = !spectator && titan == Some(g.me());
    let mut order = Vec::with_capacity(g.roster.len());
    for p in &g.roster {
        let tag = tags.by.entry(p.id).or_insert_with(|| spawn(&mut commands, p.alive));
        if tag.alive && !p.alive {
            tag.died = now;
        }
        tag.alive = p.alive;
        let (what, fading) = wear(p.id == g.me() && !spectator, titan == Some(p.id), p.alive, p.consumed, now - tag.died);
        // Where the body stands; for the dead, where the ragdoll has its middle, a little over the counter.
        let feet = bodies.iter().find(|(gait, _, seen, _)| gait.id() == p.id && seen.get()).map(|(_, at, _, rig)| {
            let lying = rig.filter(|_| !p.alive).and_then(|rig| joints.get(rig.joints().torso).ok());
            lying.map_or(at.translation(), |torso| torso.transform_point(MIDDLE))
        });
        let (Some(feet), false) = (feet, what == Show::Nothing) else {
            tag.alpha = 0.;
            show(&mut shown, tag.root, false);
            continue;
        };
        let crown = crown(p, forts);
        tag.lift = if tag.lift < 0. { crown } else { tag.lift + (crown - tag.lift) * (1. - (-dt * 10.).exp()) };
        let top = feet + Vec3::Y * tag.lift;
        let distance = top.distance(eye);
        let spot = camera.world_to_viewport(view, top).ok().filter(|spot| {
            (top - eye).dot(*ahead) > NEAR && spot.x > -80. && spot.x < screen.x + 80. && spot.y > -4. && spot.y < screen.y + 60.
        });
        let extent = nodes.get(tag.root).map(|(_, laid)| laid.size() * laid.inverse_scale_factor).unwrap_or_default();
        // Off the screen, behind the camera or not laid out yet: nothing to fade, it is simply not there.
        let Some(spot) = spot.filter(|_| extent.x > 0.) else {
            tag.alpha = 0.;
            show(&mut shown, tag.root, false);
            continue;
        };
        let seen = marks(p).iter().any(|h| !hidden(&g.feast, eye, feet + Vec3::Y * *h, carried));
        let ceiling = shelves
            .iter()
            .filter(|(left, right, _)| spot.x + extent.x / 2. > *left && spot.x - extent.x / 2. < *right)
            .fold(CEILING * (screen.y / crate::hud::SCALE).clamp(0.8, 4.), |ceiling, (_, _, foot)| ceiling.max(foot + 2.));
        let below = ((spot.y - GAP - extent.y - ceiling) / SHELF).clamp(0., 1.);
        let wanted = if seen { reach(distance, giant) * fading * below } else { 0. };
        tag.alpha += (wanted - tag.alpha).clamp(-BLINK * dt, BLINK * dt);
        // The size: the name's pixels, and the bar and the padding in proportion. (Filled in while still
        // faded out, so that it is laid out by the time it shows.)
        tag.px = size(distance, tag.px);
        if tag.px != tag.laid {
            tag.laid = tag.px;
            let k = tag.px / BASE_PX;
            if let Ok((mut node, _)) = nodes.get_mut(tag.root) {
                node.padding = UiRect::new(Val::Px((5. * k).round()), Val::Px((5. * k).round()), Val::Px(1.), Val::Px((4. * k).round()));
            }
            if let Ok((mut node, _)) = nodes.get_mut(tag.trough) {
                (node.width, node.height) = (Val::Px((tag.px * 4.6).round()), Val::Px((tag.px * 0.38).round().max(3.)));
            }
            if let Ok((_, mut font, _)) = texts.get_mut(tag.name) {
                font.font_size = FontSize::Px(tag.px);
            }
        }
        if let Ok((mut node, _)) = nodes.get_mut(tag.root) {
            let (left, top) = (Val::Px((spot.x - extent.x / 2.).round()), Val::Px((spot.y - GAP - extent.y).round()));
            if node.left != left || node.top != top {
                (node.left, node.top) = (left, top);
            }
        }
        if let Ok((mut node, _)) = nodes.get_mut(tag.trough) {
            let display = if what == Show::Health { Display::Flex } else { Display::None };
            if node.display != display {
                node.display = display;
            }
        }
        if let Ok((mut node, _)) = nodes.get_mut(tag.fill) {
            let width = Val::Percent(p.hp.clamp(0., 100.));
            if node.width != width {
                node.width = width;
            }
        }
        let a = tag.alpha;
        let own = colour(p, forts, lobby.as_ref().and_then(|lobby| lobby.colour(p.id)));
        // Forts: whoever carries the bacon says so.
        let bacon = g.fort.as_ref().is_some_and(|fort| fort.bacon.state == kitchen_sim::forts::HELD && fort.bacon.holder == p.id);
        let (line, ink) = match what {
            Show::Out => (format!("{} OUT", p.name), GREY),
            _ if bacon => (format!("{} BACON", p.name), lighter(own, 0.3)),
            _ => (p.name.clone(), lighter(own, 0.3)),
        };
        if let Ok((mut text, _, mut color)) = texts.get_mut(tag.name) {
            if text.0 != line {
                text.0 = line;
            }
            if color.0 != rgba(ink, a) {
                color.0 = rgba(ink, a);
            }
        }
        paint(&mut paints, tag.root, Color::srgba(0.02, 0.03, 0.04, 0.8 * a));
        paint(&mut paints, tag.trough, Color::srgba(0.07, 0.07, 0.08, 0.92 * a));
        paint(&mut paints, tag.fill, rgba(health_rgb(p.hp), a));
        if let Ok((mut edge, _)) = roots.get_mut(tag.root) {
            let wanted = BorderColor::all(rgba(if what == Show::Out { GREY } else { own }, 0.85 * a));
            if *edge != wanted {
                *edge = wanted;
            }
        }
        show(&mut shown, tag.root, a >= 0.01);
        if a >= 0.01 {
            order.push((tag.root, distance));
        }
    }
    // Nearer tags over further ones (all under the scope's mask).
    order.sort_by(|a, b| a.1.total_cmp(&b.1));
    for (rank, (root, _)) in order.into_iter().enumerate() {
        if let Ok((_, mut z)) = roots.get_mut(root) {
            let wanted = ZIndex(-2 - rank as i32);
            if *z != wanted {
                *z = wanted;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kitchen_sim::map::{Level, Map};

    const FLOOR: f32 = 0.9025;

    #[test]
    fn a_tag_is_larger_near_and_smaller_far_in_whole_pixels_within_bounds() {
        assert_eq!(size(0.2, BASE_PX), MAX_PX);
        assert_eq!(size(BASE_AT, BASE_PX), BASE_PX);
        assert_eq!(size(3.5, BASE_PX), MIN_PX);
        // Walking away and back: whole pixels, never outside the bounds, never larger further off.
        let mut shown = BASE_PX;
        let mut last = f32::INFINITY;
        for i in 1..=600 {
            shown = size(i as f32 * 0.01, shown);
            assert!(shown.fract() == 0. && (MIN_PX..=MAX_PX).contains(&shown) && shown <= last, "{shown} at {}", i as f32 * 0.01);
            last = shown;
        }
        for i in (1..=600).rev() {
            let before = shown;
            shown = size(i as f32 * 0.01, shown);
            assert!(shown >= before);
        }
        assert_eq!(shown, MAX_PX);
        // About a boundary (where 12.5 pixels are called for) the size holds whichever it has.
        let edge = BASE_AT * (BASE_PX / 12.5f32).powi(2);
        assert_eq!((size(edge, 12.), size(edge, 13.)), (12., 13.));
        assert_eq!((size(edge * 0.98, 12.), size(edge * 1.02, 13.)), (12., 13.));
        // An unset size takes the one called for.
        assert_eq!(size(BASE_AT, 0.), BASE_PX);
    }

    #[test]
    fn tags_fade_out_far_away() {
        assert_eq!((reach(0.1, false), reach(FADE, false)), (1., 1.));
        assert_eq!((reach(FAR, false), reach(FAR + 3., false)), (0., 0.));
        let middle = reach((FADE + FAR) / 2., false);
        assert!((middle - 0.5).abs() < 1e-5);
    }

    #[test]
    fn the_titan_wears_no_tag_and_its_player_sees_every_small_hotdogs_across_the_room() {
        // The giant: nothing over it, alive or just dead, to anyone (its own player has none in any case).
        assert_eq!(wear(false, true, true, false, f32::INFINITY), (Show::Nothing, 0.));
        assert_eq!(wear(false, true, false, false, 0.).0, Show::Nothing);
        assert_eq!(wear(true, true, true, false, f32::INFINITY).0, Show::Nothing);
        // A small hotdog still wears its own, the giant's player looking or not.
        assert_eq!(wear(false, false, true, false, f32::INFINITY), (Show::Health, 1.));
        // To the giant's eye, from one corner of Titan's Kitchen to the far one, every small hotdog's tag shows whole;
        // to a small hotdog's, the counter maps' reach holds.
        let map = kitchen_sim::map::Map::parse(include_str!("../assets/maps/titans-kitchen.json")).unwrap();
        let [x0, z0, x1, z1] = map.bounds.map(|v| v as f32);
        let floor = kitchen_sim::map::FLOOR as f32;
        let corners = Vec3::new(x0, floor + kitchen_sim::titan::TALL, z0).distance(Vec3::new(x1, floor, z1));
        assert!(corners > FAR && reach(corners, true) == 1., "{corners} m");
        assert_eq!(reach(corners, false), 0.);
        // And nothing in the room hides a small hotdog out on the floor from the giant's eye (in a corner, by the fridge,
        // by the table: one of the points its tag needs seen is).
        let feast = Feast::with_map(map, false);
        let eye = Vec3::new(2.7, floor + kitchen_sim::titan::TALL - 0.1, -7.7);
        for (x, z) in [(0.3, -10.), (-1.5, -11.5), (2.0, -11.8), (-0.5, -8.5)] {
            assert!([0.15, 0.08].iter().any(|h| !hidden(&feast, eye, Vec3::new(x, floor + h, z), None)), "({x}, {z})");
        }
    }

    #[test]
    fn health_runs_from_green_through_amber_to_red() {
        let ([r, g, b], half, [lr, lg, _]) = (health_rgb(100.), health_rgb(50.), health_rgb(5.));
        assert!(g > r + 0.3 && g > b + 0.3, "full is green");
        assert!(half[0] > 0.9 && half[1] > 0.6 && half[2] < 0.3, "half is amber");
        assert!(lr > 0.9 && lg < 0.3, "nearly gone is red");
        assert_eq!((health_rgb(140.), health_rgb(-20.)), (health_rgb(100.), health_rgb(0.)));
        // Redder all the way down.
        for hp in 1..=100 {
            assert!(health_rgb(hp as f32)[1] - health_rgb(hp as f32)[0] >= health_rgb(hp as f32 - 1.)[1] - health_rgb(hp as f32 - 1.)[0] - 1e-6);
        }
    }

    #[test]
    fn a_tag_is_in_the_colour_its_player_chose_and_in_forts_its_team() {
        let p = Player { id: 2, team: 1, ..default() };
        let chosen = [0.07, 0.30, 0.90];
        assert_eq!(colour(&p, false, None), kitchen_sim::feast::COLORS[1]);
        assert_eq!(colour(&p, false, Some(chosen)), chosen);
        assert_eq!(colour(&p, true, Some(chosen)), crate::forts::TEAM_RGB[1]);
    }
    #[test]
    fn who_wears_what() {
        // Everybody else's health; not the player's own while it lives and is played from its eyes.
        assert_eq!(wear(false, false, true, false, f32::INFINITY), (Show::Health, 1.));
        assert_eq!(wear(true, false, true, false, f32::INFINITY).0, Show::Nothing);
        // A hotdog that dies shows OUT for a moment, then fades and is gone.
        assert_eq!(wear(false, false, false, false, 0.), (Show::Out, 1.));
        assert_eq!(wear(false, false, false, false, MARK), (Show::Out, 1.));
        let (what, left) = wear(false, false, false, false, MARK + MARK_FADE / 2.);
        assert!(what == Show::Out && (left - 0.5).abs() < 1e-5);
        assert_eq!(wear(false, false, false, false, MARK + MARK_FADE).0, Show::Nothing);
        // One already dead when it was first seen has no OUT to show.
        assert_eq!(wear(false, false, false, false, f32::INFINITY).0, Show::Nothing);
        // The player's own body, watched from the death camera, has no tag at all, from the moment it dies.
        for dead_for in [0., MARK / 2., MARK, MARK + MARK_FADE / 2., f32::INFINITY] {
            assert_eq!(wear(true, false, false, false, dead_for), (Show::Nothing, 0.));
        }
        // Eaten: nothing, at once.
        assert_eq!(wear(false, false, false, true, 0.).0, Show::Nothing);
    }

    #[test]
    fn a_block_a_deck_or_a_loose_object_hides_a_hotdog_and_the_forcefield_does_not() {
        let feast = Feast::with_map(Map::bundled(), false);
        let eye = |x: f32, z: f32| Vec3::new(x, FLOOR + 0.135, z);
        let body = |x: f32, z: f32, h: f32| Vec3::new(x, FLOOR + h, z);
        // The steel block at x -0.45..-0.23, z 2.7..2.92 (0.25 m high) stands between these two; beside it nothing does.
        assert!(hidden(&feast, eye(-0.34, 2.45), body(-0.34, 3.05, 0.15), None));
        assert!(!hidden(&feast, eye(-0.8, 2.45), body(-0.8, 3.05, 0.15), None));
        // Seen over it from high up, the hotdog is in view.
        assert!(!hidden(&feast, Vec3::new(-0.34, FLOOR + 0.9, 2.2), body(-0.34, 3.05, 0.15), None));
        // The turret deck (x 1.25..1.76, z 3.16..3.61, 0.28 m high) hides a hotdog behind it, not one on it.
        assert!(hidden(&feast, eye(1.5, 2.6), body(1.5, 3.62, 0.08), None));
        assert!(!hidden(&feast, Vec3::new(1.5, FLOOR + 0.6, 2.6), body(1.4, 3.4, 0.28 + 0.15), None));
        // Forts: the forcefield stops everything while it stands, but the other side can be seen through it.
        let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/maps/pillow-fort.json")).unwrap();
        let map = Map::parse(&text).unwrap();
        let fielded = Feast::on(Level::fielded(map.clone()), false);
        // (Across the field at z 2.78, clear of the stock each side puts out by it.)
        let (from, to) = (eye(-0.5, 2.78), body(0.5, 2.78, 0.15));
        assert!(fielded.level.cover_from(from, (to - from).normalize()) < from.distance(to), "the field stops the line");
        assert!(!hidden(&fielded, from, to, None));
        assert!(!hidden(&fielded, to + Vec3::Y * 0.01, from - Vec3::Y * 0.01, None));
        // A pile of plates on the Mustard Eaters' side hides what is behind it, unless it is the one the player carries.
        let (from, to) = (eye(-2.2, 2.75), body(-1.3, 2.75, 0.05));
        let crate_ = fielded.props.raycast(from.as_dvec3().to_array(), (to - from).normalize().as_dvec3().to_array(), 2., None).map(|hit| hit.id);
        assert!(crate_.is_some() && hidden(&fielded, from, to, None));
        assert!(!hidden(&fielded, from, to, crate_));
    }

    #[test]
    fn tags_stand_over_the_head_and_lower_with_the_hotdog() {
        let mut p = Feast::new(false).players[0].clone();
        let standing = crown(&p, false);
        assert!(standing > 0.178 && standing < 0.21, "just over the top of the casing");
        assert!(crown(&p, true) > 0.244, "over the flag in Forts");
        p.crouch = true;
        let crouching = crown(&p, false);
        p.prone = true;
        let prone = crown(&p, false);
        assert!(standing > crouching && crouching > prone);
        p.alive = false;
        assert!(crown(&p, true) <= prone + 0.03);
        // The points that must be seen are on the body, under the tag.
        for (crouch, prone) in [(false, false), (true, false), (false, true)] {
            (p.alive, p.crouch, p.prone) = (true, crouch, prone);
            assert!(marks(&p).iter().all(|h| *h > 0. && *h < crown(&p, false)));
        }
    }
}
