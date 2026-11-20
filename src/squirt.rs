// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The mayonnaise bottle's stream as it is seen and heard. (Its rules are kitchen_sim's: `feast::MAYO`.)
//!
//! * **One ribbon.** The simulation flies a dozen droplets a second; a squeeze bottle squirts one unbroken
//!   stream. So each bottle's stream is drawn as one tube through the arcs of its droplets. Between two droplets
//!   that left the nozzle one after the other, the stream is what would have left it in between: from a nozzle and
//!   at a velocity part of the way from the one to the other, flown for as long (`feast::arc`). A sweep of the aim
//!   therefore bends the stream as it bends a real one, what left the bottle earlier being further along the old
//!   aim. Where a droplet was stopped (it landed, or hit somebody) the stream is broken, and its far end runs on
//!   down to where it lands.
//! * **This player's own stream** is drawn from what this screen squeezed out (`squeeze`), a bead a frame, so it
//!   leaves the bottle in the hand at once, online too, and follows the aim from the first frame; it falls with the
//!   same gravity and stops where it meets the level, a solid loose object, a cat or a hotdog; through a light one (a
//!   pillow) it goes on as the simulation's droplets do, on a new, slower arc from there (`feast::LIGHT_SPEED`), and
//!   ends in it once that leaves it slower than `feast::STALL`. The simulation's (or the server's) droplets do the
//!   damage and make the splats.
//! * **Leaving the bottle.** Rounds leave the muzzle of the simulation, at the eye; the stream is drawn leaving the
//!   bottle in the hand (in first person) or the bottle the hotdog is seen holding, and eases onto its true path
//!   within half a second.
//! * **The squelch** loops for as long as this player squeezes, and another while anyone else's bottle squirts
//!   nearby, quieter the further off it is.
use bevy::{asset::RenderAssetUsages, audio::{AudioSinkPlayback, Volume}, camera::visibility::NoFrustumCulling, light::NotShadowCaster, mesh::Indices, prelude::*, render::render_resource::PrimitiveTopology};
use kitchen_sim::feast::{self, Feast, Projectile, MAYO, MAYO_FALL};
use std::collections::HashMap;

/// Seconds between two droplets of one stream, at the least: a gap of more than BREAK of them is a break.
const GAP: f32 = 1. / 12.;
const BREAK: f32 = 1.7;
/// Pieces the stream is drawn in between two droplets (others' streams), and sides round the tube. (The droplets fly
/// `feast::SPEEDUP` times as fast as they once did, so they are as many times further apart: as many times the pieces
/// keep the tube as smooth.)
const BETWEEN: usize = 5 * feast::SPEEDUP as usize;
const SIDES: usize = 6;
/// Seconds between two of this player's own beads, at the least (a fast screen does not make a finer stream).
const BEAD: f32 = 1. / 75.;
/// Seconds over which the stream eases from the bottle in the hand onto its true path: slowly enough that a stream
/// aimed level does not climb from the bottle, lower than the eye, up to its path.
const EASE: f32 = 0.45;
/// The stream's radius as it leaves the nozzle, and as it has spread a little in the air.
const THIN: f32 = 0.0042;
const FULL: f32 = 0.0058;

/// One moment of a stream: when it left the nozzle (on the stream's own clock), from where, at what velocity, how
/// far it was drawn from its true path then (to the bottle in the hand), and, once it has met something, where.
#[derive(Clone, Copy, Debug)]
struct Bead {
    at: f32,
    from: Vec3,
    launch: Vec3,
    offset: Vec3,
    stopped: Option<Vec3>,
    last: Vec3,
    /// The light loose objects it has passed through (none: usize::MAX), each slowing it once.
    soaked: [usize; 6],
}
impl Bead {
    /// Where it is drawn `now`.
    fn place(&self, now: f32) -> Vec3 {
        if let Some(at) = self.stopped {
            return at;
        }
        let age = (now - self.at).max(0.);
        feast::arc(self.from, self.launch, MAYO_FALL, age).0 + self.offset * (1. - smooth(age / EASE))
    }
}
fn smooth(t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    t * t * (3. - 2. * t)
}

/// What the streams need each frame: the cats and hotdogs a stream stops at (centre and radius), the visible
/// nozzles of the hotdogs that are seen holding a bottle, and this player's id.
pub struct Scene<'a> {
    pub feast: &'a Feast,
    pub targets: Vec<(Vec3, f32)>,
    pub nozzles: HashMap<u32, Vec3>,
    pub me: u32,
    /// Draw this player's stream from the simulation's droplets, as everyone else's (the demo, a spectator).
    pub watching: bool,
}

#[derive(Resource, Default)]
pub struct Streams {
    /// Each owner's tube: its entity and mesh.
    tubes: HashMap<u32, (Entity, Handle<Mesh>)>,
    material: Option<Handle<StandardMaterial>>,
    sound: Option<Handle<AudioSource>>,
    /// This player's stream, newest last, on `clock`, and whether it squeezed last frame.
    own: Vec<Bead>,
    clock: f32,
    squeezing: bool,
    /// The squelch loops: this player's, and everyone else's; and how long since each last had a stream.
    loops: [Option<Entity>; 2],
    quiet: [f32; 2],
}

/// Called every frame for this player: while `squeezing` the bottle (the trigger held, the bottle in hand and not
/// empty, nothing in the way of firing) a bead leaves `nozzle` (where the bottle is drawn), its true path from
/// `from` (the simulation's muzzle) at the velocity `launch`. The scene stops the beads in flight.
pub fn squeeze(world: &mut World, squeezing: bool, nozzle: Vec3, from: Vec3, launch: Vec3, dt: f32, scene: &Scene) {
    let mut s = world.get_resource_or_insert_with(Streams::default);
    s.clock += dt;
    let now = s.clock;
    if squeezing && s.own.last().is_none_or(|b| now - b.at >= BEAD || !s.squeezing) {
        s.own.push(Bead { at: now, from, launch, offset: nozzle - from, stopped: None, last: nozzle, soaked: [usize::MAX; 6] });
    }
    s.squeezing = squeezing;
    // Each bead in flight goes on along its arc until it meets something.
    for b in s.own.iter_mut().filter(|b| b.stopped.is_none()) {
        let to = b.place(now);
        let d = to - b.last;
        let length = d.length();
        if length > 1e-6 {
            let dir = d / length;
            let stop = meets(scene, b.last, dir, length, now - b.at > EASE * 0.5);
            // Through a pillow first: on from there, slowed, on a new arc (or stalled in it), as a droplet goes.
            if let Some((t, id)) = soaks(scene, b.last, dir, stop.unwrap_or(length), &b.soaked) {
                let hit = b.last + dir * t;
                let when = now - dt * (1. - t / length);
                let v = feast::arc(b.from, b.launch, MAYO_FALL, (when - b.at).max(0.)).1 * feast::LIGHT_SPEED;
                if let Some(slot) = b.soaked.iter_mut().find(|k| **k == usize::MAX) {
                    *slot = id;
                }
                (b.from, b.launch, b.at, b.offset) = (hit, v, when, Vec3::ZERO);
                if v.length() < feast::STALL {
                    b.stopped = Some(hit);
                }
                b.last = hit;
                continue;
            }
            if let Some(t) = stop {
                b.stopped = Some(b.last + dir * t);
            }
        }
        b.last = to;
    }
    // A bead that has stopped and the beads squeezed before it are of no more use once the next has stopped too.
    while s.own.len() > 1 && s.own[0].stopped.is_some() && s.own[1].stopped.is_some() {
        s.own.remove(0);
    }
    s.own.retain(|b| now - b.at < 4.);
}
/// Where along a step from `a` along `dir` (`length` long) a stream meets the counter, a block, a loose object or,
/// once it is clear of the bottle (`far`), a cat or another hotdog.
fn meets(scene: &Scene, a: Vec3, dir: Vec3, length: f32, far: bool) -> Option<f32> {
    let mut near = scene.feast.cover(a, dir).min(f32::MAX);
    let b = a + dir * length;
    let ground = floor(scene.feast, b);
    if b.y < ground && a.y > ground {
        near = near.min((a.y - ground) / (a.y - b.y) * length);
    }
    if far {
        for (centre, radius) in &scene.targets {
            if let Some(t) = kitchen_sim::ray_sphere(a, dir, *centre, *radius) {
                near = near.min(t);
            }
        }
    }
    (near <= length).then_some(near)
}
/// The first light loose object (a pillow) a step from `a` along `dir` passes within `length`, not one already passed
/// (`soaked`): where along the step, and which.
fn soaks(scene: &Scene, a: Vec3, dir: Vec3, length: f32, soaked: &[usize]) -> Option<(f32, usize)> {
    let props = &scene.feast.props;
    let hit = props.raycast_past(a.as_dvec3().to_array(), dir.as_dvec3().to_array(), length as f64, soaked)?;
    (props.material(hit.id) == kitchen_sim::props::Material::Light).then_some((hit.distance as f32, hit.id))
}
/// The height a stream lands at under a point: the counter, a deck or a ramp there, or past its edge the floor.
fn floor(feast: &Feast, p: Vec3) -> f32 {
    let l = &feast.level;
    let (x, z) = (p.x as f64, p.z as f64);
    if x >= l.x0 && x < l.x1 && z >= l.z0 && z < l.z1 { l.ground(x, z) as f32 } else { feast.bodies.config.kitchen_floor as f32 }
}

/// Draws every bottle's stream (see the module's documentation) from the mayonnaise among `rounds` and this
/// player's own beads, and plays the squelch.
pub fn draw(world: &mut World, rounds: &[Projectile], scene: &Scene, camera: Vec3, dt: f32) {
    // Each owner's droplets, oldest first.
    let mut owners: HashMap<u32, Vec<&Projectile>> = HashMap::new();
    for q in rounds.iter().filter(|q| q.weapon == MAYO && (q.owner != scene.me || scene.watching)) {
        owners.entry(q.owner).or_default().push(q);
    }
    let mut strands: HashMap<u32, Vec<Vec<(Vec3, f32)>>> = HashMap::new();
    let mut nearest = f32::INFINITY;
    for (owner, mut drops) in owners {
        drops.sort_by(|a, b| a.born.total_cmp(&b.born));
        let now = drops.iter().map(|q| q.born + q.age).fold(f32::MIN, f32::max);
        let newest = drops.last().copied();
        // The hotdog's bottle as it is seen: the stream is drawn leaving it.
        let sim = newest.map_or(Vec3::ZERO, |q| Vec3::from_array(q.from));
        let shift = scene.nozzles.get(&owner).map_or(Vec3::ZERO, |n| *n - sim);
        let bead = |q: &Projectile| Bead { at: q.born, from: Vec3::from_array(q.from), launch: Vec3::from_array(q.launch), offset: shift, stopped: None, last: Vec3::ZERO, soaked: [usize::MAX; 6] };
        let mut beads: Vec<Bead> = drops.iter().map(|q| bead(q)).collect();
        // Still flowing: the stream runs on from the newest droplet to the bottle.
        if let Some(q) = newest.filter(|q| now - q.born < GAP * BREAK) {
            beads.push(Bead { at: now, ..bead(q) });
            if let Some(n) = scene.nozzles.get(&owner) {
                nearest = nearest.min(n.distance(camera));
            }
        }
        strands.insert(owner, strands_of(scene.feast, &beads, now, BETWEEN));
    }
    let own = {
        let s = world.get_resource_or_insert_with(Streams::default);
        (!s.own.is_empty() && !scene.watching).then(|| own_strands(&s.own, s.clock))
    };
    if let Some(own) = own {
        strands.insert(scene.me, own);
    }
    tubes(world, strands);
    let mine = world.resource::<Streams>().squeezing && !scene.watching;
    let others = nearest.is_finite().then(|| 0.55 * (1. - nearest / 3.).clamp(0.15, 1.));
    hum(world, 0, mine.then_some(0.75), dt);
    hum(world, 1, others, dt);
}

/// The pieces of a stream, from the bottle outwards, through `beads` (oldest first) at `now`: between two beads
/// that left one after the other it is drawn in `between` pieces, its nozzle and velocity taken part of the way
/// from one to the next; the oldest runs on to where it lands.
fn strands_of(feast: &Feast, beads: &[Bead], now: f32, between: usize) -> Vec<Vec<(Vec3, f32)>> {
    let mut strands = vec![];
    let mut strand: Vec<(Vec3, f32)> = vec![];
    let push = |strand: &mut Vec<(Vec3, f32)>, b: &Bead| strand.push((b.place(now), now - b.at));
    for (i, b) in beads.iter().enumerate() {
        let next = beads.get(i + 1).filter(|n| n.at - b.at <= GAP * BREAK);
        if strand.is_empty() {
            // The far end: what left the bottle just before the oldest bead, down to where it lands.
            let mut tail = vec![];
            for k in (1..=between * 2).rev() {
                let earlier = Bead { at: b.at - GAP * k as f32 / between as f32, ..*b };
                let p = earlier.place(now);
                if p.y < floor(feast, p) {
                    tail.clear();
                    continue;
                }
                tail.push((p, now - earlier.at));
            }
            strand.extend(tail);
        }
        push(&mut strand, b);
        match next {
            Some(n) => {
                for k in 1..between {
                    let t = k as f32 / between as f32;
                    let mid = Bead { at: b.at + (n.at - b.at) * t, from: b.from.lerp(n.from, t), launch: b.launch.lerp(n.launch, t), offset: b.offset.lerp(n.offset, t), ..*b };
                    push(&mut strand, &mid);
                }
            }
            None => {
                strands.push(std::mem::take(&mut strand));
            }
        }
    }
    // Nozzle first.
    for s in &mut strands {
        s.reverse();
    }
    strands.retain(|s| s.len() > 1);
    strands
}
/// This player's stream: its beads in order, broken where one stopped.
fn own_strands(beads: &[Bead], now: f32) -> Vec<Vec<(Vec3, f32)>> {
    let mut strands = vec![];
    let mut strand = vec![];
    for b in beads {
        strand.push((b.place(now), now - b.at));
        if b.stopped.is_some() {
            if strand.len() > 1 {
                strands.push(std::mem::take(&mut strand));
            }
            strand.clear();
            strand.push((b.place(now), now - b.at));
        }
    }
    if strand.len() > 1 {
        strands.push(strand);
    }
    for s in &mut strands {
        s.reverse();
    }
    strands
}

/// Builds each owner's tube from its strands (bottle first), reusing its mesh; a tube with nothing to draw is hidden.
fn tubes(world: &mut World, mut strands: HashMap<u32, Vec<Vec<(Vec3, f32)>>>) {
    let material = match world.resource::<Streams>().material.clone() {
        Some(m) => m,
        None => {
            let m = world.resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial {
                base_color: Color::srgb(0.96, 0.93, 0.80),
                perceptual_roughness: 0.26,
                reflectance: 0.55,
                ..default()
            });
            world.resource_mut::<Streams>().material = Some(m.clone());
            m
        }
    };
    let known: Vec<u32> = world.resource::<Streams>().tubes.keys().copied().collect();
    for owner in known.iter().chain(strands.keys().filter(|o| !known.contains(o))).copied().collect::<Vec<_>>() {
        let pieces = strands.remove(&owner).unwrap_or_default();
        let existing = world.resource::<Streams>().tubes.get(&owner).cloned();
        let (entity, mesh) = match existing {
            Some(t) => t,
            None if pieces.is_empty() => continue,
            None => {
                let mesh = world.resource_mut::<Assets<Mesh>>().add(Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default()));
                let entity = world
                    .spawn((Name::new("Mayonnaise stream"), Mesh3d(mesh.clone()), MeshMaterial3d(material.clone()), Transform::default(), NoFrustumCulling, NotShadowCaster))
                    .id();
                world.resource_mut::<Streams>().tubes.insert(owner, (entity, mesh.clone()));
                (entity, mesh)
            }
        };
        let shown = if pieces.is_empty() { Visibility::Hidden } else { Visibility::Visible };
        if world.get::<Visibility>(entity) != Some(&shown) {
            world.entity_mut(entity).insert(shown);
        }
        if pieces.is_empty() {
            continue;
        }
        let (positions, normals, indices) = tube(&pieces);
        if let Some(mut m) = world.resource_mut::<Assets<Mesh>>().get_mut(&mesh) {
            let count = positions.len();
            m.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
            m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
            m.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0f32; 2]; count]);
            m.insert_indices(Indices::U32(indices));
        }
    }
}
/// A tube through each strand's points (bottle first), its radius by the age of each point: thin out of the nozzle,
/// a little fuller in the air, wavering, and closed to a point at either end.
fn tube(strands: &[Vec<(Vec3, f32)>]) -> (Vec<[f32; 3]>, Vec<[f32; 3]>, Vec<u32>) {
    let (mut positions, mut normals, mut indices) = (vec![], vec![], vec![]);
    for strand in strands {
        let n = strand.len();
        let mut normal = Vec3::ZERO;
        let base = positions.len() as u32;
        for (i, (p, age)) in strand.iter().enumerate() {
            let ahead = strand[(i + 1).min(n - 1)].0 - strand[i.saturating_sub(1)].0;
            let along = ahead.normalize_or(Vec3::NEG_Y);
            // Carried along the strand, so the tube does not twist.
            normal = (normal - along * normal.dot(along)).normalize_or_zero();
            if normal == Vec3::ZERO {
                normal = along.cross(if along.y.abs() > 0.9 { Vec3::X } else { Vec3::Y }).normalize();
            }
            let side = along.cross(normal);
            let ends = smooth(i as f32 / 1.5).min(smooth((n - 1 - i) as f32 / 1.5));
            let r = (THIN + (FULL - THIN) * smooth(age / 0.35)) * (1. + 0.14 * (age * 41.).sin()) * (0.35 + 0.65 * ends);
            for k in 0..SIDES {
                let (sin, cos) = (k as f32 / SIDES as f32 * std::f32::consts::TAU).sin_cos();
                let out = normal * cos + side * sin;
                positions.push((*p + out * r).to_array());
                normals.push(out.to_array());
            }
        }
        for i in 0..n as u32 - 1 {
            for k in 0..SIDES as u32 {
                let (a, b) = (base + i * SIDES as u32 + k, base + i * SIDES as u32 + (k + 1) % SIDES as u32);
                let (c, d) = (a + SIDES as u32, b + SIDES as u32);
                indices.extend([a, c, b, b, c, d]);
            }
        }
    }
    (positions, normals, indices)
}
/// Keeps squelch loop `which` (0 this player's, 1 everyone else's) playing at `volume` while it is Some, and stops
/// it after a moment without.
fn hum(world: &mut World, which: usize, volume: Option<f32>, dt: f32) {
    if world.resource::<Streams>().sound.is_none() {
        let sound = world.get_resource::<AssetServer>().map(|server| server.load("audio/squelch.wav"));
        world.resource_mut::<Streams>().sound = sound;
    }
    let (sound, playing) = {
        let s = world.resource::<Streams>();
        (s.sound.clone(), s.loops[which])
    };
    match volume {
        Some(volume) => {
            world.resource_mut::<Streams>().quiet[which] = 0.;
            match playing.filter(|e| world.get_entity(*e).is_ok()) {
                Some(e) => {
                    // (A playing sink is set directly, so the game's volume is applied here as Bevy applies it to a
                    // sound as it starts: settings.rs.)
                    let global = world.get_resource::<GlobalVolume>().map_or(Volume::Linear(1.), |g| g.volume);
                    if let Some(mut sink) = world.get_mut::<AudioSink>(e) {
                        sink.set_volume(Volume::Linear(volume) * global);
                    }
                }
                None => {
                    if let Some(sound) = sound {
                        let speed = if which == 0 { 1. } else { 0.94 };
                        let e = world.spawn((AudioPlayer::new(sound), PlaybackSettings::LOOP.with_volume(Volume::Linear(volume)).with_speed(speed))).id();
                        world.resource_mut::<Streams>().loops[which] = Some(e);
                    }
                }
            }
        }
        None => {
            let quiet = {
                let mut s = world.resource_mut::<Streams>();
                s.quiet[which] += dt;
                s.quiet[which]
            };
            if quiet > 0.06 {
                if let Some(e) = world.resource_mut::<Streams>().loops[which].take() {
                    if let Ok(e) = world.get_entity_mut(e) {
                        e.despawn();
                    }
                }
            }
        }
    }
}
/// Forgets every stream (a new round, a new map) and stops the squelch.
pub fn clear(world: &mut World) {
    let Some(mut s) = world.get_resource_mut::<Streams>() else {
        return;
    };
    s.own.clear();
    s.squeezing = false;
    let mut gone: Vec<Entity> = s.tubes.drain().map(|(_, (e, _))| e).collect();
    gone.extend(s.loops.iter_mut().filter_map(|l| l.take()));
    for e in gone {
        if let Ok(e) = world.get_entity_mut(e) {
            e.despawn();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bead(at: f32, aim: Vec3) -> Bead {
        Bead { at, from: Vec3::new(0., 1., 0.), launch: aim * feast::ROUND_SPEED[MAYO], offset: Vec3::ZERO, stopped: None, last: Vec3::ZERO, soaked: [usize::MAX; 6] }
    }
    #[test]
    fn a_stream_is_one_tube_from_the_bottle_through_its_droplets_and_bends_when_the_aim_sweeps() {
        let feast = Feast::new(false);
        // Five droplets a twelfth of a second apart, the aim swinging from +x to +z as they left.
        let beads: Vec<Bead> = (0..5).map(|i| bead(i as f32 * GAP, Vec3::new(1. - i as f32 * 0.2, 0.5, i as f32 * 0.2).normalize())).collect();
        let now = 4. * GAP + 0.01;
        let strands = strands_of(&feast, &beads, now, BETWEEN);
        assert_eq!(strands.len(), 1, "unbroken");
        let s = &strands[0];
        assert!(s[0].1 < s[s.len() - 1].1, "bottle first, oldest last");
        // Smooth: no step long (the oldest droplets, falling fastest, are furthest apart).
        let steps: Vec<f32> = s.windows(2).map(|w| w[0].0.distance(w[1].0)).collect();
        assert!(steps.iter().all(|d| *d < 0.12), "{steps:?}");
        // The stream bends: what left first is off along the old aim (+x), what left last along the new (+z).
        let (first, last) = (s[s.len() - 1].0, s[1].0);
        assert!(first.x > first.z && last.z > 0.);
        // A droplet missing from the middle (it hit somebody) breaks it in two.
        let mut gapped = beads.clone();
        gapped.remove(2);
        gapped.remove(2);
        assert_eq!(strands_of(&feast, &gapped, now, BETWEEN).len(), 2);
        // The tube is closed and finite.
        let (p, n, i) = tube(&strands);
        assert!(p.len() == n.len() && i.iter().all(|i| (*i as usize) < p.len()) && p.iter().flatten().all(|v| v.is_finite()));
    }

    /// This player's own stream goes on through a pillow as the simulation's droplets do: slowed to LIGHT_SPEED of its
    /// speed there, on a new arc from the pillow's face, rather than at full speed as though it were not there; and it
    /// still ends at a crate.
    #[test]
    fn this_players_stream_goes_on_through_a_pillow_slowed_as_the_droplets_do_and_ends_at_a_crate() {
        let map = kitchen_sim::map::Map::parse(
            r#"{"format": "hotdog-map", "version": 1, "name": "Range", "bounds": [0, 0, 3, 1], "starts": [{"p": [0.2, 0.5]}], "spawns": [{"p": [2.9, 0.1]}],
            "props": [{"kind": "pillow", "p": [0.6, 0.5], "base": 0, "yaw": 1.5708}, {"kind": "crate", "p": [1.6, 0.9], "base": 0, "yaw": 0}]}"#,
        )
        .unwrap();
        let feast = Feast::with_map(map, false);
        let scene = Scene { feast: &feast, targets: vec![], nozzles: HashMap::new(), me: 1, watching: false };
        let mut world = World::new();
        let floor = kitchen_sim::feast::FLOOR;
        // Aimed level at the pillow, 3.5 cm above the counter, from 0.15 m off.
        let speed = kitchen_sim::feast::ROUND_SPEED[MAYO];
        let (from, launch) = (Vec3::new(0.4, floor + 0.035, 0.5), Vec3::new(speed, 0., 0.));
        for _ in 0..40 {
            squeeze(&mut world, true, from, from, launch, 1. / 60., &scene);
        }
        let own = world.resource::<Streams>().own.clone();
        let through: Vec<&Bead> = own.iter().filter(|b| b.soaked[0] == 0).collect();
        assert!(!through.is_empty(), "some of it went through the pillow");
        for b in through {
            assert!((b.from.x - 0.55).abs() < 0.02, "on from the pillow's face: {:?}", b.from);
            assert!(b.launch.length() < speed * kitchen_sim::feast::LIGHT_SPEED + 0.05 && b.launch.length() > speed * 0.25, "slowed: {}", b.launch.length());
            assert!(b.stopped.is_none_or(|at| at.x > 0.6), "on into the pillow and beyond, not stopped at its face: {:?}", b.stopped);
        }
        // Aimed at the crate, it ends there.
        let mut world = World::new();
        let (from, launch) = (Vec3::new(1.2, floor + 0.05, 0.9), Vec3::new(speed, 0., 0.));
        for _ in 0..40 {
            squeeze(&mut world, true, from, from, launch, 1. / 60., &scene);
        }
        let own = world.resource::<Streams>().own.clone();
        assert!(own.iter().filter_map(|b| b.stopped).any(|at| (at.x - 1.55).abs() < 0.01), "it ends at the crate");
    }
}
