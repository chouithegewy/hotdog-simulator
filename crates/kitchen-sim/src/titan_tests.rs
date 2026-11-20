// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Titan's tests (titan.rs): the move table, the body and its zones, the stagger, the moves' effects, the footing, the
//! pool and the ends, the choice of titan, determinism, and bot matches on Titan's Kitchen; the balance runs are
//! `#[ignore]`d (run them with `--ignored`, as Kung Fu's: `balance` prints the table).
use super::*;
use crate::feast::{FLOOR as GROUND, MAX_HP, PLAYER_DAMAGE, Stage, muzzle};
use crate::map::Map;
use crate::rules::{Mode, Rules};

const DT: f32 = 1. / 30.;

fn read(path: &str) -> String {
    std::fs::read_to_string(format!("{}/../../{path}", env!("CARGO_MANIFEST_DIR"))).unwrap_or_else(|e| panic!("{path}: {e}"))
}
/// Titan's Kitchen (TI3's arena).
fn arena_map() -> Map {
    Map::parse(&read("assets/maps/titans-kitchen.json")).unwrap()
}
/// A bare floor eight metres by six tagged for Titan: the titan's start in the middle, five more at the edges, a pillar
/// at (6.6, 3), a low block (cover) at x 2.6..2.7, a stool (a deck 0.3 m high) over x 5..5.6, z 0.4..0.9, and a table
/// top (a den) over x 0.3..1.5, z 3.6..4.6, and a low shelf (a den 0.2 m up) over x 0.3..1.0, z 1.6..2.2.
fn floor_map() -> Map {
    Map::parse(
        r#"{"format": "hotdog-map", "version": 2, "name": "Floor", "mode": "titan", "bounds": [0, 0, 8, 6],
        "starts": [{"p": [4, 3]}, {"p": [0.6, 0.6]}, {"p": [7.4, 0.6]}, {"p": [0.6, 5.4]}, {"p": [7.4, 5.4]}, {"p": [4, 5.6]}],
        "spawns": [{"p": [0.3, 0.3]}],
        "blocks": [{"rect": [6.5, 2.8, 6.7, 3.2], "height": 0.7}, {"rect": [2.6, 1.0, 2.7, 2.0], "height": 0.3}],
        "decks": [{"rect": [5.0, 0.4, 5.6, 0.9], "height": 0.3}],
        "props": [{"kind": "box", "rect": [0.3, 3.6, 1.5, 4.6], "base": 0.5, "height": 0.04}, {"kind": "box", "rect": [0.3, 1.6, 1.0, 2.2], "base": 0.2, "height": 0.03}]}"#,
    )
    .unwrap()
}
/// A round of Titan on the bare floor, hotdog 1 the titan, nobody a bot (nothing moves unless told).
fn floor(rules: &str) -> Feast {
    let mut s = Feast::with_map(floor_map(), false);
    s.card_seed = 11;
    s.set_rules(Rules::parse(&format!("mode=titan titan=1 {rules}")));
    for p in &mut s.players {
        p.bot = false;
    }
    s
}
fn run(s: &mut Feast, seconds: f32) {
    for _ in 0..(seconds / DT).round() as usize {
        s.tick(DT);
    }
}
fn titan(s: &Feast) -> &Player {
    s.player(s.titan_id().unwrap()).unwrap()
}
fn hp(s: &Feast, id: u32) -> f32 {
    s.player(id).unwrap().hp
}
/// Puts the titan at (x, z) facing `yaw` (the way it was moving forgotten).
fn place_titan(s: &mut Feast, x: f32, z: f32, yaw: f32) {
    let id = s.titan_id().unwrap();
    let p = s.player_mut(id).unwrap();
    (p.p, p.yaw) = ([x, GROUND, z], yaw);
    let t = s.titan.as_mut().unwrap();
    (t.last, t.want) = (Vec2::new(x, z), yaw);
    s.titan_body();
}
/// Puts small hotdog `id` at (x, z) on the floor.
fn put(s: &mut Feast, id: u32, x: f32, z: f32) {
    let p = s.player_mut(id).unwrap();
    p.p = [x, GROUND, z];
    s.embody();
}
/// Parks the small hotdogs but `keep` in the far corners, out of everything's way.
fn park(s: &mut Feast, keep: &[u32]) {
    let ids: Vec<u32> = s.players.iter().map(|p| p.id).filter(|id| !keep.contains(id) && !s.is_titan(*id)).collect();
    for (k, id) in ids.into_iter().enumerate() {
        put(s, id, 7.7, 0.3 + 0.3 * k as f32);
    }
}
fn events(s: &Feast) -> Vec<Event> {
    s.titan.as_ref().unwrap().events.clone()
}
fn told(s: &Feast, kind: EventKind) -> Vec<Event> {
    events(s).into_iter().filter(|e| e.kind == Some(kind)).collect()
}
/// Runs a step at a time for up to `limit` seconds, gathering every event (they are only kept EVENT_LIFE).
fn gather(s: &mut Feast, limit: f32) -> Vec<Event> {
    let mut seen: Vec<Event> = vec![];
    for _ in 0..(limit / DT).round() as usize {
        s.tick(DT);
        for e in events(s) {
            if !seen.iter().any(|x| x.id == e.id) {
                seen.push(e);
            }
        }
    }
    seen
}
/// A titan figure on its own (for the shape's geometry): at the origin, facing -z.
fn figure(pitch: f32, crouch: bool) -> Player {
    Player { id: 1, p: [0., GROUND, 0.], yaw: 0., pitch, crouch, alive: true, grounded: true, ..Default::default() }
}

// ----- The table and the body.

#[test]
fn every_move_has_its_number_and_sane_data_and_every_attack_a_readable_tell_and_a_cooldown() {
    assert_eq!(MOVES, Move::ALL.len());
    let mut names = std::collections::HashSet::new();
    for (i, mv) in Move::ALL.into_iter().enumerate() {
        let d = mv.data();
        assert_eq!((mv as usize, Move::from_u8(i as u8), d.mv), (i, Some(mv), mv), "{mv:?}");
        assert!(names.insert(d.name), "{} named twice", d.name);
        for t in [d.startup, d.active, d.recovery, d.cooldown, d.reach, d.arc, d.damage, d.edge, d.knock, d.lift, d.wave] {
            assert!(t.is_finite() && t >= 0., "{mv:?}: {d:?}");
        }
        assert!(d.edge <= d.damage && d.band[0] <= d.band[1] && d.arc <= std::f32::consts::PI + 1e-6, "{mv:?}");
        if d.attacks() {
            // Every attack is telegraphed (at least a quarter second of wind-up), has a cooldown, reaches and hurts.
            assert!(d.startup >= 0.3 && d.cooldown > 0.5 && d.reach > 0.2 && d.damage > 0. && d.knock > 0., "{mv:?}: {d:?}");
            assert!(mv.ready_slot().is_some(), "{mv:?}");
        } else {
            assert!(mv.ready_slot().is_none() && d.damage == 0., "{mv:?}");
        }
    }
    // The waves are the smash's and the landing's; the stagger lasts about three seconds, the stumble less.
    assert!(Move::ALL.iter().all(|m| (m.data().wave > 0.) == matches!(m, Move::Smash | Move::Jump)));
    assert!((Move::Stagger.data().total() - 3.).abs() < 0.3 && Move::Stumble.data().total() < Move::Stagger.data().total());
    // A high swat passes over a hotdog on the floor; a low one sweeps the floor (and under the tables).
    let (high, low) = (Move::SwatHigh.data(), Move::SwatLow.data());
    assert!(high.band[0] > actor::HEIGHT as f32 && low.band[0] == 0. && low.band[1] >= actor::HEIGHT as f32);
    assert!(high.band[1] < TALL && low.band[1] < DEN_CLEAR + actor::HEIGHT as f32 * 2.);
    // The charge runs CHARGE_TIME at most; the jump's air time is its take-off speed's under the game's gravity, which
    // reaches JUMP_APEX.
    let g = actor::GRAVITY as f32;
    assert_eq!(Move::Charge.data().active, CHARGE_TIME);
    assert!((Move::Jump.data().active - 2. * JUMP_SPEED / g).abs() < 1e-4 && (JUMP_SPEED * JUMP_SPEED / (2. * g) - JUMP_APEX).abs() < 0.01);
    // Every size comes from SCALE: twelve hotdogs tall, its radius twelve hotdogs', crouched 1.1 m.
    assert_eq!(SCALE, 12.);
    assert!((TALL - 1.8).abs() < 1e-5 && (RADIUS - 0.36).abs() < 1e-5 && (CROUCH_TALL - 1.1).abs() < 1e-5 && KNEEL_TALL < CROUCH_TALL);
    assert!(WALK < actor::WALK as f32 && CROUCH_WALK < WALK && CHARGE_SPEED > actor::WALK as f32);
    assert_eq!(MULTIPLIER, [0.75, 1., 1.5, 3., 0.75]);
}

#[test]
fn the_body_is_its_figure_turned_to_its_yaw_its_head_nodding_with_its_look_and_lower_crouched_and_kneeling() {
    let stand = Shape::of(&figure(0., false), Move::Idle);
    let crouch = Shape::of(&figure(0., true), Move::Idle);
    let kneel = Shape::of(&figure(0., false), Move::Stagger);
    let top = |s: &Shape| s.head.y + s.head_radius - GROUND;
    assert!((top(&stand) - TALL).abs() < 0.1, "{}", top(&stand));
    assert!((top(&crouch) - CROUCH_TALL).abs() < 0.12, "{}", top(&crouch));
    assert!(top(&kneel) <= KNEEL_TALL + 0.05 && kneel.head.y - GROUND < 0.75, "{}", top(&kneel));
    // Facing -z, its face is to -z and its nape to +z; turned half round, the other way.
    assert!(stand.face.z < -0.9 && stand.nape.z > 0.5);
    let turned = Shape::of(&Player { yaw: std::f32::consts::PI, ..figure(0., false) }, Move::Idle);
    assert!(turned.face.z > 0.9 && turned.nape.z < -0.5);
    // Looking down bows the head forward and turns the nape to face back; looking up tips it down.
    let (down, up) = (Shape::of(&figure(-1.5, false), Move::Idle), Shape::of(&figure(1.5, false), Move::Idle));
    assert!(down.head.z < stand.head.z && down.nape.y > stand.nape.y && up.nape.y < stand.nape.y);
    assert!(down.nape.y > EXPOSED && stand.nape.y < EXPOSED);
    // Crouched the nape is tucked down; kneeling it faces back.
    assert!(crouch.nape.y < stand.nape.y && kneel.nape.y > EXPOSED);
    // In the air it is its standing figure, lifted.
    let mut air = figure(0., false);
    (air.p[1], air.grounded) = (GROUND + 0.2, false);
    assert!((Shape::of(&air, Move::Jump).head.y - stand.head.y - 0.2).abs() < 1e-4);
}

/// The zone a round from `from` straight at `at` meets first on `shape` (None: it misses).
fn zone(shape: &Shape, from: Vec3, at: Vec3) -> Option<Zone> {
    hit(shape, from, (at - from).normalize(), 0.01).map(|h| h.zone)
}

#[test]
fn rounds_find_its_zones_from_front_back_above_and_below_at_each_stance_and_look() {
    for pitch in [-1.5, -0.7, 0., 0.7, 1.5] {
        let s = Shape::of(&figure(pitch, false), Move::Idle);
        // From the front, at the face: the face; at the chest: the body; at a knee: the legs.
        let front = Vec3::new(0., GROUND + 0.4, -3.);
        assert_eq!(zone(&s, front, s.face_point()), Some(Zone::Face), "pitch {pitch}");
        assert_eq!(zone(&s, front, (s.torso.a + s.torso.b) * 0.5), Some(Zone::Body), "pitch {pitch}");
        assert_eq!(zone(&s, front, s.knee(0)), Some(Zone::Legs), "pitch {pitch}");
        // From behind, at the legs and the body: never the face, never the weak spot.
        let back = Vec3::new(0., GROUND + 0.135, 3.);
        assert_eq!(zone(&s, back, s.knee(1)), Some(Zone::Legs));
        assert_eq!(zone(&s, back, (s.torso.a + s.torso.b) * 0.5), Some(Zone::Body));
        // From straight above, the crown: the head, but neither face nor weak spot.
        let crown = s.head + Vec3::Y * s.head_radius;
        assert_eq!(zone(&s, crown + Vec3::Y * 2., crown), Some(Zone::Body), "pitch {pitch}");
        // From below in front (a hotdog at its toes shooting up at its chin): never the weak spot.
        assert_ne!(zone(&s, Vec3::new(0., GROUND + 0.1, -0.6), s.head - Vec3::Y * s.head_radius * 0.9), Some(Zone::Weak), "pitch {pitch}");
    }
    // The weak spot, for a hotdog on the floor three metres behind shooting at where it shows (the patch over the
    // shoulders' mound): there looking level or down, hidden looking up (the nape tipped down into the neck).
    let floor_back = Vec3::new(0., GROUND + 0.135, 3.);
    for (pitch, weak) in [(-1.5, true), (-0.7, true), (0., true), (1.5, false)] {
        let s = Shape::of(&figure(pitch, false), Move::Idle);
        assert_eq!(zone(&s, floor_back, s.weak_point()) == Some(Zone::Weak), weak, "pitch {pitch}");
    }
    // (The weak spot's middle is behind the drawn shoulders' mound; where it shows is inside its cap.)
    let s = Shape::of(&figure(0., false), Move::Idle);
    assert!(s.spot.normalize().dot(s.nape) > s.weak_arc.cos());
    // The cap is always as large as it is (as the drawn patch is): looking up or tucked, the nape hides behind the
    // shoulders instead.
    for (pitch, crouch) in [(1.5, false), (-1.5, false), (0., true)] {
        assert_eq!(Shape::of(&figure(pitch, crouch), Move::Idle).weak_arc, WEAK_ARC);
    }
    // From behind at the head's own height: the skull overhangs the nape looking level; bowed, it faces the shot.
    let level = |s: &Shape| Vec3::new(0., s.head.y, 3.);
    let s = Shape::of(&figure(0., false), Move::Idle);
    assert_eq!(zone(&s, level(&s), s.head), Some(Zone::Body));
    let s = Shape::of(&figure(-1.5, false), Move::Idle);
    assert_eq!(zone(&s, level(&s), s.weak_point()), Some(Zone::Weak));
    // Crouched, the nape is tucked down: hidden from the floor behind and from the head's height behind.
    let s = Shape::of(&figure(0., true), Move::Idle);
    for aim in [s.head, s.weak_point()] {
        assert_ne!(zone(&s, floor_back, aim), Some(Zone::Weak));
        assert_ne!(zone(&s, level(&s), aim), Some(Zone::Weak));
    }
    // Kneeling (a stagger) the head is low, bowed over its hands: the weak spot's patch shows over its back to anybody
    // behind it, on the floor close or far, or up on a stool or a box.
    let s = Shape::of(&figure(0., false), Move::Stagger);
    for (height, far) in [(0.135, 1.5), (0.135, 4.), (0.4, 2.5), (0.6, 1.5), (0.8, 1.5)] {
        assert_eq!(zone(&s, Vec3::new(0., GROUND + height, far), s.weak_point()), Some(Zone::Weak), "kneeling, from {height} m up {far} m behind");
    }
    // A round that misses the figure misses.
    let s = Shape::of(&figure(0., false), Move::Idle);
    assert_eq!(zone(&s, Vec3::new(2., GROUND + 0.5, -3.), Vec3::new(2., GROUND + 0.5, 3.)), None);
}

#[test]
fn each_zone_multiplies_a_rounds_damage_and_a_glob_and_the_strike_go_by_the_nearest_zone() {
    let rifle = crate::feast::RIFLE;
    for (z, x) in [(Zone::Legs, 0.75), (Zone::Body, 1.), (Zone::Face, 1.5), (Zone::Weak, 3.), (Zone::Arms, 0.75)] {
        let mut s = floor("");
        park(&mut s, &[2]);
        place_titan(&mut s, 4., 3., 0.);
        // Hotdog 2 in front (face, body, legs) or behind (the weak spot, the titan looking down at the floor).
        let behind = z == Zone::Weak;
        if behind {
            s.player_mut(1).unwrap().pitch = -1.5;
        }
        put(&mut s, 2, 4., if behind { 5.5 } else { 0.5 });
        let shape = Shape::of(titan(&s), Move::Idle);
        let mark = match z {
            Zone::Legs => shape.knee(0),
            Zone::Body => (shape.torso.a + shape.torso.b) * 0.5,
            Zone::Face => shape.face_point(),
            Zone::Weak => shape.weak_point(),
            // (A forearm, its hand hanging by the knee.)
            Zone::Arms => (shape.arms[1].a + shape.arms[1].b) * 0.5,
        };
        let o = muzzle(s.player(2).unwrap());
        assert_eq!(zone(&shape, o, mark), Some(z));
        let before = hp(&s, 1);
        s.launch(2, o, (mark - o).normalize(), rifle);
        run(&mut s, 0.5);
        let took = before - hp(&s, 1);
        assert!((took - PLAYER_DAMAGE[rifle] * x).abs() < 0.01, "{z:?}: {took}");
        // (Only the legs fill the stagger meter: an arm is a limb, but it does not stagger the giant.)
        if z != Zone::Legs {
            assert_eq!(s.titan.as_ref().unwrap().legs, 0., "{z:?}");
        }
        let small = s.titan.as_ref().unwrap().small(2).unwrap().clone();
        assert!((small.dealt - took).abs() < 0.01 && small.weak == behind as u32, "{small:?}");
        assert_eq!(told(&s, EventKind::WeakHit).len(), behind as usize);
        assert_eq!(s.player(2).unwrap().score, took.floor() as u32 + if behind { WEAK_BONUS } else { 0 });
        // On the record: the rifle's round, marked a face shot on the face and the weak spot.
        let record = titan(&s).last_damage;
        assert_eq!((crate::feast::plain_kind(record.kind), crate::feast::faced(record.kind), record.attacker), (crate::feast::SYRUP, x > 1., 2));
    }
    // The F strike: its damage times the zone it meets, never a kill from behind.
    let mut s = floor("");
    park(&mut s, &[2]);
    place_titan(&mut s, 4., 3., 0.);
    put(&mut s, 2, 4.14, 3. + RADIUS + 0.06);
    s.player_mut(2).unwrap().yaw = 0.;
    let before = hp(&s, 1);
    assert!(s.melee(2, -Vec3::Z));
    assert!((before - hp(&s, 1) - MAX_HP / 2. * 0.75).abs() < 0.01, "{}", before - hp(&s, 1));
    // A relish glob bursting at its feet: the legs' share.
    let mut s = floor("");
    park(&mut s, &[2]);
    place_titan(&mut s, 4., 3., 0.);
    put(&mut s, 2, 4., 0.8);
    let o = muzzle(s.player(2).unwrap());
    let feet = Vec3::new(4.14, GROUND + 0.02, 2.84);
    let before = hp(&s, 1);
    s.launch(2, o, (feet - o).normalize(), 4);
    run(&mut s, 1.);
    let took = before - hp(&s, 1);
    assert!(took > 0. && took <= PLAYER_DAMAGE[4] * 0.75 + 0.01, "{took}");
    assert!(s.titan.as_ref().unwrap().tally.hits[Zone::Legs as usize] >= 1);
}

#[test]
fn the_titans_bulk_stops_rounds_and_its_health_is_the_rules_for_the_small_hotdogs_there_are() {
    let mut s = floor("");
    let t = s.titan.as_ref().unwrap();
    assert_eq!((t.max, t.pool, t.reserve), (HP_BASE + HP_PER * SMALLS as f32, PER_SMALL * SMALLS as u32, PER_SMALL * SMALLS as u32));
    assert_eq!((hp(&s, 1), s.players.len(), titan(&s).owned.iter().any(|o| *o)), (t.max, 1 + SMALLS, false));
    // Three small hotdogs: less health, a smaller pool.
    let few = floor("smalls=3");
    let t = few.titan.as_ref().unwrap();
    assert_eq!((few.players.len(), t.max, t.pool), (4, HP_BASE + HP_PER * 3., 9));
    // A round at hotdog 3 behind the titan from hotdog 2 is stopped by the titan's body (its leg, here: low and
    // through the middle, a round passes between its legs).
    park(&mut s, &[2, 3]);
    place_titan(&mut s, 4., 3., 0.);
    put(&mut s, 2, 4.14, 0.8);
    put(&mut s, 3, 4.14, 5.2);
    s.player_mut(3).unwrap().team = 1;
    let o = muzzle(s.player(2).unwrap());
    let t3 = crate::feast::chest(s.player(3).unwrap());
    s.launch(2, o, (t3 - o).normalize(), 1);
    run(&mut s, 0.6);
    assert_eq!(hp(&s, 3), 100.);
    assert!(hp(&s, 1) < s.titan.as_ref().unwrap().max);
}

// ----- The stagger.

#[test]
fn leg_damage_in_a_few_seconds_drops_it_to_its_knees_for_about_three_seconds_then_the_meter_rests() {
    let mut s = floor("");
    park(&mut s, &[2]);
    place_titan(&mut s, 4., 3., 0.);
    let leg = |s: &mut Feast, hp: f32| {
        let knee = Shape::of(titan(s), s.titan.as_ref().unwrap().mv).knee(0);
        s.strike_titan(2, hp / 0.75, Zone::Legs, knee, Vec3::Z, 1, knee - Vec3::Z);
    };
    // Spread out, leg damage drains away: no stagger.
    for _ in 0..8 {
        leg(&mut s, STAGGER_LEGS / 4.);
        run(&mut s, STAGGER_WINDOW / 2.);
    }
    assert_eq!(s.titan.as_ref().unwrap().mv.data().kind, Kind::Stance);
    // In a burst: a stagger, told; it cannot attack, and its head comes down.
    leg(&mut s, STAGGER_LEGS * 0.6);
    assert!(s.titan_view().unwrap().stagger > 0.5);
    leg(&mut s, STAGGER_LEGS * 0.6);
    let t = s.titan.as_ref().unwrap();
    assert_eq!(t.mv, Move::Stagger);
    assert_eq!(told(&s, EventKind::StaggerStart).len(), 1);
    assert!(s.titan_view().unwrap().exposed);
    run(&mut s, 0.5);
    assert!(Shape::of(titan(&s), Move::Stagger).head.y - GROUND < 0.8);
    s.titan_press(Move::Smash);
    run(&mut s, 0.3);
    assert_eq!(s.titan.as_ref().unwrap().mv, Move::Stagger, "no attack on its knees");
    // It gets up after the stagger's length; then leg damage fills nothing for STAGGER_REST.
    let seen = gather(&mut s, Move::Stagger.data().total());
    assert!(seen.iter().any(|e| e.kind == Some(EventKind::StaggerEnd)));
    assert_eq!(s.titan.as_ref().unwrap().mv.data().kind, Kind::Stance);
    leg(&mut s, STAGGER_LEGS * 2.);
    assert_eq!(s.titan.as_ref().unwrap().mv.data().kind, Kind::Stance);
    run(&mut s, STAGGER_REST);
    leg(&mut s, STAGGER_LEGS * 1.1);
    assert_eq!(s.titan.as_ref().unwrap().mv, Move::Stagger, "rested, it can be staggered again");
    assert_eq!(s.titan.as_ref().unwrap().tally.staggers, 2);
}

// ----- The moves.

#[test]
fn the_charge_knocks_a_hotdog_in_its_way_aside_with_damage_and_a_wall_stops_it_in_a_stumble() {
    let mut s = floor("");
    park(&mut s, &[2]);
    place_titan(&mut s, 4., 3., std::f32::consts::FRAC_PI_2);
    // Facing -x (yaw pi/2); hotdog 2 1.4 m ahead, a little to one side.
    put(&mut s, 2, 2.6, 3.05);
    s.titan_press(Move::Charge);
    let seen = gather(&mut s, Move::Charge.data().startup + 1.2);
    assert!(seen.iter().any(|e| e.kind == Some(EventKind::Tell) && e.mv == Move::Charge as u8));
    assert!(seen.iter().any(|e| e.kind == Some(EventKind::ChargeStart)));
    let hit: Vec<&Event> = seen.iter().filter(|e| e.kind == Some(EventKind::ChargeHit)).collect();
    assert_eq!((hit.len(), hit[0].to), (1, 2));
    assert!(hp(&s, 2) <= 100. - Move::Charge.data().damage + 0.01);
    run(&mut s, 0.6);
    // Aside: off the titan's line (z = 3), well clear of its body.
    let q = s.player(2).unwrap().p;
    assert!((q[2] - 3.).abs() > 0.25, "{q:?}");
    assert!(titan(&s).p[0] < 2.6, "it ran on: {:?}", titan(&s).p);
    // Into the pillar at (6.6, 3): stopped short of it, stumbling, told.
    let mut s = floor("");
    park(&mut s, &[]);
    place_titan(&mut s, 4.5, 3., -std::f32::consts::FRAC_PI_2);
    s.titan_press(Move::Charge);
    let seen = gather(&mut s, Move::Charge.data().startup + 2.);
    assert!(seen.iter().any(|e| e.kind == Some(EventKind::ChargeWall)));
    assert!(seen.iter().any(|e| e.kind == Some(EventKind::StaggerStart) && e.mv == Move::Stumble as u8));
    let x = titan(&s).p[0];
    assert!(x < 6.5 - RADIUS + 0.01 && x > 5.5, "{x}");
    assert!(s.titan.as_ref().unwrap().footing.clear(x, titan(&s).p[2]));
    let seen = gather(&mut s, Move::Stumble.data().total());
    assert!(seen.iter().any(|e| e.kind == Some(EventKind::StaggerEnd)));
    assert_eq!(s.titan.as_ref().unwrap().mv.data().kind, Kind::Stance);
}

/// The smash on the bare floor facing -z from (4, 3): the impact is `Smash.ahead` in front. Hotdogs 2.. at `gaps`
/// metres from the impact (their nearest point), along +x, -x and -z; returns what each took.
fn smash_at(gaps: &[f32], setup: impl Fn(&mut Feast)) -> Vec<f32> {
    let mut s = floor("");
    park(&mut s, &[]);
    place_titan(&mut s, 4., 3., 0.);
    let d = Move::Smash.data();
    let impact = Vec2::new(4., 3. - d.ahead);
    let ways = [Vec2::X, -Vec2::X, -Vec2::Y, Vec2::new(0.7, -0.7), Vec2::new(-0.7, -0.7)];
    for (k, gap) in gaps.iter().enumerate() {
        let at = impact + ways[k] * (gap + crate::feast::HOTDOG_RADIUS as f32);
        put(&mut s, 2 + k as u32, at.x, at.y);
    }
    setup(&mut s);
    s.titan_press(Move::Smash);
    run(&mut s, d.startup + d.active + d.reach / d.wave + 0.3);
    (0..gaps.len()).map(|k| 100. - hp(&s, 2 + k as u32)).collect()
}

#[test]
fn the_smash_hurts_by_distance_to_nothing_at_its_reach_cover_shields_and_a_jump_in_time_takes_little() {
    let d = Move::Smash.data();
    let took = smash_at(&[0.1, 0.6, 1.1, d.reach + 0.2], |_| {});
    assert!((took[0] - d.damage.min(100.)).abs() < 0.5, "{took:?}");
    assert!(took[0] > took[1] && took[1] > took[2] && took[2] > 0. && took[3] == 0., "{took:?}");
    // Behind the block at x 2.6..2.7 (the impact at (4, 2.45)): hotdog 2 at (2.3, 1.5) has the block between.
    let mut s = floor("");
    park(&mut s, &[2, 3]);
    place_titan(&mut s, 4., 3., 0.);
    put(&mut s, 2, 2.35, 1.5);
    put(&mut s, 3, 3.0, 1.2);
    s.titan_press(Move::Smash);
    run(&mut s, d.total() + 0.5);
    assert_eq!(hp(&s, 2), 100., "cover");
    assert!(hp(&s, 3) < 100.);
    // Jumping as the front comes: the airborne share only.
    let gap = 0.9;
    let standing = smash_at(&[gap], |_| {})[0];
    let mut s = floor("");
    park(&mut s, &[2]);
    place_titan(&mut s, 4., 3., 0.);
    let impact = Vec2::new(4., 3. - d.ahead);
    let at = impact + Vec2::X * (gap + crate::feast::HOTDOG_RADIUS as f32);
    put(&mut s, 2, at.x, at.y);
    s.titan_press(Move::Smash);
    let front = d.startup + (gap - DIRECT) / d.wave;
    run(&mut s, front - 0.15);
    s.jump(2, s.tune.leap(false));
    run(&mut s, 0.6);
    let jumped = 100. - hp(&s, 2);
    assert!(jumped > 0. && (jumped - standing * AIRBORNE_SHARE).abs() < 1., "{jumped} vs {standing}");
    // A hotdog up on the stool is out of the floor's wave; with the fists on the stool itself, it is in it.
    for (titan_z, reached) in [(1.8, false), (1.32, true)] {
        let mut s = floor("");
        park(&mut s, &[2]);
        place_titan(&mut s, 5.3, titan_z, 0.);
        s.player_mut(2).unwrap().p = [5.3, GROUND + 0.3, 0.6];
        s.embody();
        run(&mut s, 0.1);
        assert!((s.player(2).unwrap().p[1] - GROUND - 0.3).abs() < 0.01);
        s.titan_press(Move::Smash);
        let seen = gather(&mut s, d.total() + 0.3);
        let at = seen.iter().find(|e| e.kind == Some(EventKind::Smash)).unwrap().p;
        assert_eq!((at[1] - GROUND - 0.3).abs() < 0.01, reached, "{at:?}");
        assert_eq!(hp(&s, 2) < 100., reached, "titan at z {titan_z}");
    }
}

#[test]
fn a_den_halves_the_wave_and_keeps_the_high_swat_out_but_the_low_swat_reaches_in() {
    let d = Move::Smash.data();
    // Hotdog 2 under the table top (x 0.3..1.5, z 3.6..4.6), hotdog 3 the same distance out in the open.
    let run_smash = |s: &mut Feast| {
        s.titan_press(Move::Smash);
        run(s, d.total() + 0.3);
    };
    let mut s = floor("");
    park(&mut s, &[2, 3]);
    place_titan(&mut s, 2.4, 4.1, std::f32::consts::FRAC_PI_2);
    put(&mut s, 2, 1.3, 4.1);
    put(&mut s, 3, 2.4 - d.ahead, 4.1 + (2.4 - d.ahead - 1.3));
    assert!(s.in_den(2) && !s.in_den(3));
    run_smash(&mut s);
    let (den, open) = (100. - hp(&s, 2), 100. - hp(&s, 3));
    assert!(open > 0. && (den - open * DEN_SHARE).abs() < 1.5, "{den} {open}");
    // The swats: standing at the table's edge facing it.
    for (mv, crouch, reached) in [(Move::SwatHigh, false, false), (Move::SwatLow, true, true)] {
        let mut s = floor("");
        park(&mut s, &[2]);
        place_titan(&mut s, 1.5 + RADIUS + 0.02, 4.1, std::f32::consts::FRAC_PI_2);
        s.player_mut(1).unwrap().crouch = crouch;
        put(&mut s, 2, 1.2, 4.1);
        // (Raised, as a hotdog standing on something: a high swat would reach it but for the table top.)
        if !crouch {
            s.player_mut(2).unwrap().p[1] = GROUND + 0.3;
            s.embody();
        }
        s.titan_press(mv);
        run(&mut s, mv.data().total() + 0.1);
        assert_eq!(hp(&s, 2) < 100., reached, "{mv:?}");
    }
}

#[test]
fn a_swat_sweeps_its_height_a_stomp_its_foot_a_kick_ahead_and_a_jump_lands_in_a_wave() {
    // A high swat passes over a hotdog on the floor and hits one standing on something; a low swat the reverse.
    for (mv, crouch) in [(Move::SwatHigh, false), (Move::SwatLow, true)] {
        let mut s = floor("");
        park(&mut s, &[2, 3]);
        place_titan(&mut s, 4., 3., 0.);
        s.player_mut(1).unwrap().crouch = crouch;
        put(&mut s, 2, 3.7, 2.4);
        put(&mut s, 3, 4.3, 2.4);
        s.player_mut(3).unwrap().p[1] = GROUND + 0.4;
        s.embody();
        // (Hold hotdog 3 up: as if it stood on a stool.)
        let mut floor_hit = false;
        let mut up_hit = false;
        s.titan_press(mv);
        for _ in 0..((mv.data().total() + 0.1) / DT) as usize {
            s.player_mut(3).unwrap().p[1] = GROUND + 0.4;
            s.embody();
            s.tick(DT);
            floor_hit |= hp(&s, 2) < 100.;
            up_hit |= hp(&s, 3) < 100.;
        }
        assert_eq!((floor_hit, up_hit), (mv == Move::SwatLow, mv == Move::SwatHigh), "{mv:?}");
        assert!(told(&s, EventKind::Swat).len() <= 1);
    }
    // A stomp: the hotdog at its foot, not the one beside it.
    let mut s = floor("");
    park(&mut s, &[2, 3]);
    place_titan(&mut s, 4., 3., 0.);
    let foot = 3. - Move::Stomp.data().ahead;
    put(&mut s, 2, 4., foot);
    put(&mut s, 3, 4.8, foot);
    s.titan_press(Move::Stomp);
    run(&mut s, Move::Stomp.data().total());
    assert!(hp(&s, 2) <= 100. - Move::Stomp.data().edge && hp(&s, 3) == 100.);
    // A kick: ahead, low, crouched.
    let mut s = floor("");
    park(&mut s, &[2, 3]);
    place_titan(&mut s, 4., 3., 0.);
    s.player_mut(1).unwrap().crouch = true;
    put(&mut s, 2, 4., 2.4);
    put(&mut s, 3, 3.2, 3.);
    s.titan_press(Move::Kick);
    run(&mut s, Move::Kick.data().total());
    assert!(hp(&s, 2) < 100. && hp(&s, 3) == 100.);
    // A jump: up and down, it lands on nothing raised, and its landing is a wave.
    let mut s = floor("");
    park(&mut s, &[2]);
    place_titan(&mut s, 4., 3., 0.);
    put(&mut s, 2, 4.8, 3.);
    s.titan_press(Move::Jump);
    let mut top: f32 = 0.;
    let mut seen = vec![];
    for _ in 0..(3. / DT) as usize {
        s.tick(DT);
        top = top.max(titan(&s).p[1] - GROUND);
        seen.extend(events(&s));
    }
    assert!((top - JUMP_APEX).abs() < 0.03, "{top}");
    assert!(seen.iter().any(|e| e.kind == Some(EventKind::JumpLand)));
    assert!(hp(&s, 2) < 100.);
    assert_eq!(titan(&s).p[1], GROUND);
}

/// The titan's stride is the round's (the client draws its walk from it): its phase runs with the ground it covers over
/// its gait's cycle, back as it backs up, its drive eases to its pace, and a foot falls (a footfall, at that foot) each
/// time the phase passes the foot's, the left at 0 and the right at PI, the feet in turn.
#[test]
fn the_stride_runs_with_the_ground_covered_and_the_feet_fall_in_turn_on_its_beat() {
    let mut s = floor("");
    park(&mut s, &[2]);
    place_titan(&mut s, 4., 5.4, 0.);
    let run = |s: &mut Feast, stick: Vec2, secs: f32| {
        let (mut walked, mut turned, mut falls) = (0f32, 0f32, std::collections::BTreeMap::new());
        let mut last = flat(titan(s).p);
        for k in 0..(secs / DT) as usize {
            let was = s.titan.as_ref().unwrap().stride;
            s.titan_command(1, &Command::new(k as u32 + 1, stick, 0., 0., 0, 0, None), DT);
            s.tick(DT);
            let at = flat(titan(s).p);
            walked += (at - last).length();
            last = at;
            turned += (s.titan.as_ref().unwrap().stride - was + PI).rem_euclid(TAU) - PI;
            for e in events(s).into_iter().filter(|e| e.kind == Some(EventKind::Step)) {
                falls.insert(e.id, e.p[0]);
            }
        }
        (walked, turned, falls.into_values().collect::<Vec<f32>>())
    };
    let (walked, turned, falls) = run(&mut s, Vec2::new(0., -1.), 3.);
    let t = s.titan.as_ref().unwrap();
    assert!(walked > 2. && (turned - walked / gait_cycle(false, false) * TAU).abs() < 0.05, "walked {walked} m, the stride turned {turned}");
    assert!(t.drive > 0.9 && t.drive <= DRIVE_MAX, "{}", t.drive);
    // A foot every half cycle, the feet in turn (the left is to its left: -x, facing -z).
    assert!((falls.len() as f32 - turned / PI).abs() <= 1.5, "{} footfalls over {turned}", falls.len());
    let x = titan(&s).p[0];
    for pair in falls.windows(2) {
        assert!((pair[0] - x).signum() != (pair[1] - x).signum(), "{falls:?} about {x}");
    }
    // Backing up, the stride runs back.
    let (walked, turned, _) = run(&mut s, Vec2::new(0., 1.), 1.5);
    assert!(walked > 0.3 && turned < -0.5, "backed {walked} m, the stride turned {turned}");
}

#[test]
fn stepping_on_a_small_hotdog_hurts_it_and_shoves_it_out_and_standing_still_does_not() {
    let mut s = floor("");
    park(&mut s, &[2]);
    place_titan(&mut s, 4., 3., 0.);
    put(&mut s, 2, 4., 2.4);
    // The titan walks into it.
    let c = Command::new(1, Vec2::new(0., -1.), 0., 0., 0, 0, None);
    let mut seen = vec![];
    for _ in 0..(1.2 / DT) as usize {
        s.titan_command(1, &c, DT);
        s.tick(DT);
        seen.extend(events(&s));
        // Never inside its body at the end of a step.
        let gap = (flat(s.player(2).unwrap().p) - flat(titan(&s).p)).length();
        assert!(gap > RADIUS + SMALL_RADIUS - 0.01, "{gap}");
    }
    assert!(seen.iter().any(|e| e.kind == Some(EventKind::Stepped) && e.to == 2));
    assert!(hp(&s, 2) <= 100. - STEP_DAMAGE + 0.01);
    assert!(seen.iter().any(|e| e.kind == Some(EventKind::Step)), "footfalls");
    // A hotdog walking into a titan that stands still is stopped by it, unhurt.
    let mut s = floor("");
    park(&mut s, &[2]);
    place_titan(&mut s, 4., 3., 0.);
    put(&mut s, 2, 4., 2.3);
    for _ in 0..(1.5 / DT) as usize {
        let step = Command::new(1, Vec2::new(0., 1.), 0., 0., 0, 0, None).stride(1.25, DT);
        s.walk(2, step, 0.);
        s.tick(DT);
    }
    let gap = (flat(s.player(2).unwrap().p) - flat(titan(&s).p)).length();
    assert!(gap > RADIUS + SMALL_RADIUS - 0.01 && hp(&s, 2) == 100., "{gap}");
}

// ----- The footing.

#[test]
fn its_footing_keeps_its_body_out_of_every_block_ramp_deck_and_den_and_off_the_edge_and_it_never_sticks() {
    let s = Feast::with_map(arena_map(), false);
    let f = s.titan.as_ref().unwrap().footing.clone();
    let map = &s.level.map;
    // Every den is out of it (the table top, the chairs, the stool, the toe-kick): their middles and their edges.
    let dens: Vec<[f32; 4]> = f.dens.iter().map(|(r, _)| *r).collect();
    assert!(dens.len() >= 4, "{dens:?}");
    for r in &dens {
        for (x, z) in [((r[0] + r[2]) / 2., (r[1] + r[3]) / 2.), (r[0], r[1]), (r[2], r[3])] {
            assert!(!f.clear(x, z), "den at {x} {z}");
            assert!(s.titan.as_ref().unwrap().footing.den((r[0] + r[2]) / 2., (r[1] + r[3]) / 2.));
        }
    }
    // A random walk, charge and jump for a long while: never inside anything, never off the floor, never stuck.
    let mut s = Feast::with_map(arena_map(), false);
    s.set_rules(Rules::parse("mode=titan titan=1"));
    for p in &mut s.players {
        p.bot = p.id != 1;
    }
    let mut dice = crate::spawns::Dice::new(&[5]);
    let (mut stick, mut yaw) = (Vec2::ZERO, 0.);
    let mut last = flat(titan(&s).p);
    let mut window = vec![];
    for step in 0..(240. / DT) as usize {
        if step % 20 == 0 {
            let a = (dice.roll() % 6283) as f32 / 1000.;
            (stick, yaw) = (Vec2::new(0., -1.), a);
        }
        let mut buttons = 0;
        match dice.roll() % 400 {
            0 => buttons |= button::STIM,
            1 => buttons |= button::JUMP,
            2 => buttons |= button::SCOPE,
            3 => buttons |= button::CROUCH,
            _ => {}
        }
        let c = Command::new(step as u32, stick, yaw, -0.5, buttons, 0, None);
        s.titan_command(1, &c, DT);
        s.tick(DT);
        if s.stage == Stage::RoundOver {
            break;
        }
        let p = titan(&s).p;
        assert!(f.clear(p[0], p[2]), "inside something at {p:?}, step {step}");
        for b in &map.blocks {
            let r = b.rect.map(|v| v as f32);
            let near = Vec2::new(p[0].clamp(r[0], r[2]), p[2].clamp(r[1], r[3]));
            assert!((near - flat(p)).length() >= RADIUS - 1e-3, "touching a block at {p:?}");
        }
        assert!(p[1] >= GROUND - 1e-4 && p[1] < GROUND + JUMP_APEX + 0.05);
        let at = flat(p);
        window.push((at - last).length());
        last = at;
        if window.len() == 150 {
            // Over five seconds of walking every which way, it got somewhere.
            let moved: f32 = window.iter().sum();
            assert!(moved > 0.3, "stuck at {p:?} (step {step})");
            window.clear();
        }
    }
}

#[test]
fn a_titan_cannot_walk_under_a_table_or_into_the_toe_kick_but_a_small_hotdog_can() {
    let mut s = Feast::with_map(arena_map(), false);
    s.set_rules(Rules::parse("mode=titan titan=1"));
    for p in &mut s.players {
        p.bot = false;
    }
    // The table top's middle (-1.3, -9.92); the titan walks at it from the open floor in front of it.
    place_titan(&mut s, -1.3, -11.4, 0.);
    let c = Command::new(1, Vec2::new(0., -1.), std::f32::consts::PI, 0., 0, 0, None);
    for _ in 0..(5. / DT) as usize {
        s.titan_command(1, &c, DT);
        s.tick(DT);
    }
    let z = titan(&s).p[2];
    assert!(z < -10.35 - RADIUS + 0.02, "under the table: {z}");
    // The toe-kick under the cabinets (z -7.6..-7.5, its underside 0.17 m up): the titan stays out, a hotdog walks in.
    place_titan(&mut s, -2.5, -8.6, 0.);
    let c = Command::new(1, Vec2::new(0., -1.), 0., 0., 0, 0, None);
    for _ in 0..(4. / DT) as usize {
        s.titan_command(1, &c, DT);
        s.tick(DT);
    }
    assert!(titan(&s).p[2] < -7.6 - RADIUS + 0.02, "{:?}", titan(&s).p);
    let small = s.players.iter().find(|p| p.id != 1).unwrap().id;
    put(&mut s, small, -2.0, -8.0);
    for _ in 0..(1.5 / DT) as usize {
        let step = Command::new(1, Vec2::new(0., -1.), std::f32::consts::PI, 0., 0, 0, None).stride(1.25, DT);
        s.walk(small, step, std::f32::consts::PI);
        s.tick(DT);
    }
    let q = s.player(small).unwrap().p;
    assert!(q[2] > -7.6, "the hotdog walked under the cabinet: {q:?}");
    assert!(s.in_den(small));
}

// ----- The pool and the ends.

#[test]
fn a_small_hotdog_comes_back_from_the_pool_and_with_the_pool_empty_stays_dead_and_the_titan_wins() {
    let mut s = floor("smalls=2");
    park(&mut s, &[]);
    assert_eq!(s.titan.as_ref().unwrap().reserve, 6);
    s.hurt(2, 500.);
    let v = s.titan_view().unwrap();
    assert_eq!((v.reserve, v.small(2).unwrap().respawn), (5, RESPAWN));
    let seen = gather(&mut s, RESPAWN + 0.1);
    let back = s.player(2).unwrap();
    assert!(back.alive && back.hp == 100. && back.shield > 0. && crate::feast::Feast::low(back) == false);
    let came = seen.iter().find(|e| e.kind == Some(EventKind::Reinforcement)).expect("told");
    assert_eq!((came.to, came.amount), (2, 5.));
    // Back far from the titan.
    assert!((flat(back.p) - flat(titan(&s).p)).length() > 2.);
    // The pool runs dry: the dead stay dead (out), and once every small hotdog is, the titan has won.
    run(&mut s, crate::rules::SHIELD + 0.1);
    s.titan.as_mut().unwrap().reserve = 0;
    s.hurt(2, 500.);
    run(&mut s, RESPAWN + 0.5);
    assert!(!s.player(2).unwrap().alive && s.titan_view().unwrap().small(2).unwrap().out);
    assert_eq!(s.stage, Stage::FreeForAll);
    s.hurt(3, 500.);
    run(&mut s, 0.1);
    let v = s.titan_view().unwrap();
    assert_eq!((s.stage, v.over, s.winner), (Stage::RoundOver, OVER_TITAN, Some(1)));
    assert_eq!(v.mv(), Move::Roar);
    assert!(s.alert.contains("TITAN") || s.alert.contains("WINS"), "{}", s.alert);
}

#[test]
fn the_small_hotdogs_win_by_bringing_its_health_to_nothing_and_the_titan_by_lasting_the_clock() {
    let mut s = floor("");
    park(&mut s, &[2, 3]);
    place_titan(&mut s, 4., 3., 0.);
    let max = s.titan.as_ref().unwrap().max;
    // Hotdog 3 deals the most; hotdog 2 the last blow, to the weak spot.
    let head = Shape::of(titan(&s), Move::Idle).weak_point();
    s.strike_titan(3, max * 0.6, Zone::Body, head, Vec3::Z, 1, head);
    s.strike_titan(2, max / 12., Zone::Weak, head, -Vec3::Z, 2, head);
    let v = s.titan_view().unwrap();
    assert_eq!((v.over, s.stage, v.mv(), titan(&s).alive), (OVER_NONE, Stage::FreeForAll, Move::Idle, true));
    s.strike_titan(2, max, Zone::Weak, head, -Vec3::Z, 2, head);
    let seen = gather(&mut s, 0.1);
    let v = s.titan_view().unwrap();
    assert_eq!((v.over, s.stage, v.mv(), titan(&s).alive, v.hp), (OVER_SMALLS, Stage::RoundOver, Move::Down, false, 0.));
    assert!(seen.iter().any(|e| e.kind == Some(EventKind::TitanDown) && e.by == 2) || told(&s, EventKind::TitanDown).iter().any(|e| e.by == 2));
    assert_eq!(s.player(2).unwrap().kills, 1, "the killing blow");
    assert_eq!(s.winner, Some(3), "who dealt the most");
    assert_eq!(s.titan.as_ref().unwrap().tally.titan_killed_by, Some(Zone::Weak));
    assert!(s.alert.starts_with("THE TITAN FALLS"), "{}", s.alert);
    // The clock: five minutes, and the titan still standing wins.
    let mut s = floor("");
    park(&mut s, &[]);
    assert_eq!(s.time_left(), Some(TIME));
    s.ends = s.elapsed + 0.1;
    run(&mut s, 0.2);
    assert_eq!((s.stage, s.titan_view().unwrap().over, s.winner), (Stage::RoundOver, OVER_TITAN, Some(1)));
    assert!(s.alert.starts_with("TIME."), "{}", s.alert);
    // The next round is Titan again, set out afresh.
    s.reset();
    let v = s.titan_view().unwrap();
    assert_eq!((s.stage, v.over, v.reserve, s.time_left()), (Stage::FreeForAll, OVER_NONE, v.pool, Some(TIME)));
    assert!(s.players.iter().all(|p| p.alive && p.deaths == 0));
}

// ----- Choosing the titan.

#[test]
fn the_titan_is_cast_by_the_pick_and_bots_and_people_take_turns_round_after_round() {
    let people = |n: usize, k: &[u32]| (1..=n as u32).map(|id| k.contains(&id)).collect::<Vec<bool>>();
    // Auto: a bot with fewer than three people, a person with three or more.
    let one = people(6, &[1]);
    assert!(!one[cast(Pick::Auto, &one, None, 3) as usize - 1]);
    let three = people(6, &[1, 2, 5]);
    assert!(three[cast(Pick::Auto, &three, None, 3) as usize - 1]);
    // Bot, Random, Me and a seat; with none of the kind asked for, the other.
    assert!(!one[cast(Pick::Bot, &one, None, 9) as usize - 1]);
    assert_eq!(cast(Pick::Random, &one, None, 9), 1);
    assert_eq!(cast(Pick::Me, &three, None, 9), 1);
    assert_eq!(cast(Pick::Seat(4), &three, None, 9), 4);
    assert!(people(6, &[]).iter().all(|h| !h) && cast(Pick::Random, &people(6, &[]), None, 9) > 0);
    assert_eq!(cast(Pick::Me, &people(6, &[]), Some(2), 9), 3, "no person: the next bot");
    // Turns: the next of the kind after the last round's titan.
    assert_eq!(cast(Pick::Random, &three, Some(1), 0), 2);
    assert_eq!(cast(Pick::Random, &three, Some(2), 0), 5);
    assert_eq!(cast(Pick::Random, &three, Some(5), 0), 1);
    assert_eq!(cast(Pick::Bot, &one, Some(4), 0), 5);
    assert_eq!(cast(Pick::Bot, &one, Some(6), 0), 2);
    // The rules say it (and write it back).
    for (line, pick) in [("mode=titan titan=bot", Pick::Bot), ("mode=titan titan=random", Pick::Random), ("mode=titan titan=me", Pick::Me), ("mode=titan titan=4 smalls=3", Pick::Seat(4))] {
        let r = Rules::parse(line);
        assert_eq!((r.mode(), r.pick), (Some(Mode::Titan), pick));
        assert_eq!(Rules::parse(&r.text()), r, "{line}");
    }
    assert_eq!(Rules::parse("mode=titan").text(), "mode=titan");
    // Offline: `titan=me` makes the person the titan; a bot titan by default, taking turns as rounds are played.
    let mut s = Feast::with_map(floor_map(), false);
    s.set_rules(Rules::parse("mode=titan titan=me"));
    assert_eq!(s.titan_id(), Some(1));
    let mut s = Feast::with_map(floor_map(), false);
    assert_eq!(s.mode(), Mode::Titan, "the arena plays Titan by itself");
    let first = s.titan_id().unwrap();
    assert!(s.player(first).unwrap().bot && s.player(first).unwrap().name == NAME);
    let mut turns = vec![first];
    for _ in 0..5 {
        run(&mut s, 0.2);
        s.reset();
        // (A round set out and reset again without being played does not take a turn.)
        s.reset();
        turns.push(s.titan_id().unwrap());
    }
    for pair in turns.windows(2) {
        assert_ne!(pair[0], pair[1], "{turns:?}");
    }
    assert!(turns.iter().all(|id| s.player(*id).unwrap().bot), "{turns:?}");
    // Its next titan is the next bot after the last.
    let bots: Vec<u32> = s.players.iter().filter(|p| p.bot).map(|p| p.id).collect();
    for pair in turns.windows(2) {
        let next = bots.iter().copied().find(|id| *id > pair[0]).unwrap_or(bots[0]);
        assert_eq!(pair[1], next, "{turns:?}");
    }
    // A titan cast again (as the server does once it knows who is a person) is the same while nothing changed.
    let id = s.titan_id();
    s.cast_titan();
    assert_eq!(s.titan_id(), id);
}

// ----- Determinism and bot matches.

/// A bot match on Titan's Kitchen with every seat a bot: (over, seconds, tally, the titan's health left as a share).
pub(super) fn bot_match(rules: &str, seed: u64, limit: f32) -> (u8, f32, Tally, f32) {
    bot_match_scaled(rules, seed, limit, 1.)
}
/// The same with the titan's health scaled by `scale` (the balance runs' knob).
pub(super) fn bot_match_scaled(rules: &str, seed: u64, limit: f32, scale: f32) -> (u8, f32, Tally, f32) {
    let mut s = Feast::with_map(arena_map(), false);
    s.card_seed = seed;
    s.set_rules(Rules::parse(rules));
    for p in &mut s.players {
        p.bot = true;
    }
    s.cast_titan();
    let id = s.titan_id().unwrap();
    let t = s.titan.as_mut().unwrap();
    t.max *= scale;
    let max = t.max;
    s.player_mut(id).unwrap().hp = max;
    let start = s.elapsed;
    while s.stage != Stage::RoundOver && s.elapsed - start < limit {
        s.tick(DT);
    }
    let t = s.titan.as_ref().unwrap();
    let hp = s.player(t.id).map_or(0., |p| p.hp) / t.max;
    (t.over, s.elapsed - start, t.tally.clone(), hp)
}

#[test]
fn the_same_game_plays_the_same_way() {
    let play = |seed: u64| {
        let mut s = Feast::with_map(arena_map(), false);
        s.card_seed = seed;
        s.set_rules(Rules::parse("mode=titan titan=bot"));
        run(&mut s, 40.);
        (s.players.iter().map(|p| (p.p, p.hp.to_bits(), p.alive)).collect::<Vec<_>>(), s.titan_view().unwrap())
    };
    let (a, b) = (play(21), play(21));
    assert_eq!(a.0, b.0);
    assert_eq!(a.1, b.1);
    assert_ne!(play(22).0, a.0, "another seed plays another game");
}

#[test]
fn a_bot_match_on_titans_kitchen_runs_to_a_finish_with_both_sides_fighting() {
    for (seed, skill) in [(1, "normal"), (2, "easy"), (3, "hard")] {
        let mut s = Feast::with_map(arena_map(), false);
        s.card_seed = seed;
        s.set_rules(Rules::parse(&format!("mode=titan titan=bot skill={skill}")));
        let footing = s.titan.as_ref().unwrap().footing.clone();
        let (mut shots, mut smalls_hurt) = (0, false);
        while s.stage != Stage::RoundOver && s.elapsed < TIME + 5. {
            s.tick(DT);
            let p = titan(&s);
            assert!(!p.alive || footing.clear(p.p[0], p.p[2]), "{skill}: the titan inside something at {:?}", p.p);
            shots += s.shots.len();
            smalls_hurt |= s.players.iter().any(|q| q.id != p.id && q.hp < 100.);
        }
        let t = s.titan.as_ref().unwrap();
        assert_eq!(s.stage, Stage::RoundOver, "{skill}: it ends");
        assert_ne!(t.over, OVER_NONE);
        assert!(shots > 50 && smalls_hurt, "{skill}");
        // The titan used several of its moves; the small hotdogs hit it in more than one zone.
        let kinds = t.tally.moves.iter().filter(|n| **n > 0).count();
        assert!(kinds >= 3, "{skill}: {:?}", t.tally.moves);
        assert!(t.tally.hits.iter().filter(|n| **n > 0).count() >= 2, "{skill}: {:?}", t.tally.hits);
    }
}

#[test]
fn every_skill_plays_both_sides_and_easy_titans_are_slower_to_strike_than_hard_ones() {
    let mut rates = vec![];
    for skill in ["easy", "hard"] {
        let mut total = 0;
        for seed in 1..=3 {
            let (_, secs, tally, _) = bot_match(&format!("mode=titan titan=bot skill={skill}"), seed, 60.);
            total += (tally.moves.iter().sum::<u32>() as f32 / secs * 60.) as u32;
        }
        rates.push(total);
    }
    assert!(rates[0] < rates[1], "attacks a minute: easy {} hard {}", rates[0], rates[1]);
}

// ----- The balance runs (ignored: `cargo test --release ... titan -- --ignored --nocapture`).

/// One row of the balance table.
#[derive(Default, Debug)]
struct Row {
    games: u32,
    titan_wins: u32,
    by_clock: u32,
    secs: Vec<f32>,
    weak_finish: u32,
    body_finish: u32,
    leg_finish: u32,
    face_finish: u32,
    arm_finish: u32,
    deaths: [u32; MOVES],
    stepped: u32,
    other: u32,
    staggers: u32,
    hits: [u32; ZONES],
    damage: [f32; ZONES],
    moves: [u32; MOVES],
    blows: [u32; MOVES],
    reeling: f32,
    weak_reeling: u32,
}
fn table(rules: &str, seeds: std::ops::RangeInclusive<u64>, scale: f32) -> Row {
    let mut row = Row::default();
    for seed in seeds {
        let (over, secs, tally, _) = bot_match_scaled(rules, seed, TIME + 30., scale);
        row.games += 1;
        row.secs.push(secs);
        if over == OVER_TITAN {
            row.titan_wins += 1;
            row.by_clock += (secs >= TIME - 0.5) as u32;
        }
        match tally.titan_killed_by {
            Some(Zone::Weak) => row.weak_finish += 1,
            Some(Zone::Body) => row.body_finish += 1,
            Some(Zone::Legs) => row.leg_finish += 1,
            Some(Zone::Face) => row.face_finish += 1,
            Some(Zone::Arms) => row.arm_finish += 1,
            None => {}
        }
        for k in 0..MOVES {
            row.deaths[k] += tally.small_deaths[k];
        }
        (row.stepped, row.other, row.staggers) = (row.stepped + tally.stepped_deaths, row.other + tally.other_deaths, row.staggers + tally.staggers);
        for z in 0..ZONES {
            (row.hits[z], row.damage[z]) = (row.hits[z] + tally.hits[z], row.damage[z] + tally.damage[z]);
        }
        for k in 0..MOVES {
            row.moves[k] += tally.moves[k];
            row.blows[k] += tally.blows[k];
        }
        (row.reeling, row.weak_reeling) = (row.reeling + tally.reeling, row.weak_reeling + tally.weak_reeling);
    }
    row
}
fn print_row(label: &str, row: &Row) {
    let mut secs = row.secs.clone();
    secs.sort_by(f32::total_cmp);
    let mean = secs.iter().sum::<f32>() / secs.len().max(1) as f32;
    let pick = |q: f32| secs.get(((secs.len() as f32 - 1.) * q).round() as usize).copied().unwrap_or(0.);
    let n = row.games.max(1) as f32;
    if std::env::var("TITAN_DETAIL").is_ok() {
        let total: f32 = row.damage.iter().sum();
        println!("  {label}: hits by zone (legs, body, face, weak, arms) {:?}, damage share {:?}, moves a game {:?}", row.hits, row.damage.map(|d| (d / total.max(1.) * 100.).round()), row.moves.map(|m| (m as f32 / n * 10.).round() / 10.));
        println!("  {label}: blows a move {:?}, damage taken on its knees {:.0}% (weak hits then {} of {})", (0..MOVES).map(|k| if row.moves[k] > 0 { (row.blows[k] as f32 / row.moves[k] as f32 * 100.).round() / 100. } else { 0. }).collect::<Vec<_>>(), row.reeling / total.max(1.) * 100., row.weak_reeling, row.hits[3]);
    }
    println!(
        "| {label} | {} | {:.0}% / {:.0}% | {:.0}s ({:.0}–{:.0}, p10 {:.0}, p90 {:.0}) | {} clock | weak {} body {} legs {} face {} arms {} | smash {} swatL {} swatH {} kick {} stomp {} charge {} landing {} stepped {} other {} | staggers {:.1}/game |",
        row.games,
        row.titan_wins as f32 / n * 100.,
        (row.games - row.titan_wins) as f32 / n * 100.,
        mean,
        pick(0.),
        pick(1.),
        pick(0.1),
        pick(0.9),
        row.by_clock,
        row.weak_finish,
        row.body_finish,
        row.leg_finish,
        row.face_finish,
        row.arm_finish,
        row.deaths[Move::Smash as usize],
        row.deaths[Move::SwatLow as usize],
        row.deaths[Move::SwatHigh as usize],
        row.deaths[Move::Kick as usize],
        row.deaths[Move::Stomp as usize],
        row.deaths[Move::Charge as usize],
        row.deaths[Move::Jump as usize],
        row.stepped,
        row.other,
        row.staggers as f32 / n,
    );
}

/// The balance table: 5 small bots against a bot titan, at each skill (both sides the same), over many seeds (the
/// TITAN_SEEDS environment variable, 60 by default).
#[test]
#[ignore]
fn balance() {
    let seeds: u64 = std::env::var("TITAN_SEEDS").ok().and_then(|v| v.parse().ok()).unwrap_or(60);
    println!("| skill | games | titan / small wins | match length | titan wins by clock | titan killed by | small hotdogs killed by | staggers |");
    println!("|---|---|---|---|---|---|---|---|");
    for skill in ["normal", "easy", "hard"] {
        let row = table(&format!("mode=titan titan=bot skill={skill}"), 1..=seeds, 1.);
        print_row(skill, &row);
    }
}

/// The same with the titan's health scaled (TITAN_SCALES, comma separated): for tuning.
#[test]
#[ignore]
fn balance_sweep() {
    let seeds: u64 = std::env::var("TITAN_SEEDS").ok().and_then(|v| v.parse().ok()).unwrap_or(20);
    let scales: Vec<f32> = std::env::var("TITAN_SCALES").ok().map(|v| v.split(',').filter_map(|x| x.parse().ok()).collect()).unwrap_or(vec![1.]);
    let skill = std::env::var("TITAN_SKILL").unwrap_or("normal".into());
    for scale in scales {
        let row = table(&format!("mode=titan titan=bot skill={skill}"), 1..=seeds, scale);
        print_row(&format!("x{scale}"), &row);
    }
}

#[test]
#[ignore]
fn diag_timeline() {
    let mut s = Feast::with_map(arena_map(), false);
    s.card_seed = std::env::var("TITAN_SEED").ok().and_then(|v| v.parse().ok()).unwrap_or(3);
    s.set_rules(Rules::parse("mode=titan titan=bot"));
    for p in &mut s.players {
        p.bot = true;
    }
    s.cast_titan();
    let mut next = 0.;
    while s.stage != Stage::RoundOver && s.elapsed < 330. {
        s.tick(DT);
        if s.elapsed >= next {
            next += 3.;
            let t = s.titan.as_ref().unwrap();
            let p = s.player(t.id).unwrap();
            let smalls: Vec<String> = s.players.iter().filter(|q| q.id != t.id).map(|q| format!("{}:{}{:.0}@({:.1},{:.1})", q.id, if q.alive { "" } else { "x" }, q.hp, q.p[0], q.p[2])).collect();
            println!("{:5.1} hp {:5.0} at ({:.2},{:.2}) mv {:?} target {} crouch {} reserve {} | {}", s.elapsed, p.hp, p.p[0], p.p[2], t.mv, t.brain.target, p.crouch, t.reserve, smalls.join(" "));
        }
    }
    println!("{:?}", s.titan.as_ref().unwrap().tally);
}


// ----- The dens' roofs.

#[test]
fn a_dens_roof_stops_rounds_both_ways_the_titan_is_hit_from_under_a_table_only_through_its_mouth() {
    // Under the table top (x 0.3..1.5, z 3.6..4.6, its underside 0.5 m up), facing the titan beside it.
    let setup = || {
        let mut s = floor("");
        park(&mut s, &[2]);
        place_titan(&mut s, 2.3, 4.1, std::f32::consts::FRAC_PI_2);
        put(&mut s, 2, 1.0, 4.1);
        assert!(s.in_den(2));
        s
    };
    // Up at its head: the round meets the table top on its way out, and the titan is not hurt.
    let mut s = setup();
    let o = muzzle(s.player(2).unwrap());
    let head = Shape::of(titan(&s), Move::Idle).head;
    let way = (head - o).normalize();
    assert!(s.level.cover_from(o, way) < o.distance(head) && s.cover(o, way) < o.distance(head));
    let max = hp(&s, 1);
    s.launch(2, o, way, crate::feast::RIFLE);
    run(&mut s, 0.5);
    assert_eq!(hp(&s, 1), max, "through the roof");
    // Out of the den's mouth, low, at its leg: hurt.
    let knee = Shape::of(titan(&s), Move::Idle).knee(1) - Vec3::Y * 0.15;
    s.launch(2, o, (knee - o).normalize(), crate::feast::RIFLE);
    run(&mut s, 0.5);
    assert!(hp(&s, 1) < max, "from the mouth");
}

#[test]
fn a_dens_roof_stops_rounds_and_a_globs_blast_coming_in_from_above() {
    // Free For All on the floor (everyone may hurt everyone): hotdog 2 under the low shelf (0.2 m up), hotdog 3 in the
    // open beside it; rounds straight down from above each, and a glob bursting on each one's roof (or the floor).
    let mut s = Feast::with_map(floor_map(), false);
    s.set_rules(Rules::parse("mode=ffa"));
    for p in &mut s.players {
        (p.bot, p.shield) = (false, 0.);
    }
    for (k, p) in s.players.iter_mut().enumerate() {
        p.p = [7.5, GROUND, 0.4 + 0.5 * k as f32];
    }
    s.embody();
    put(&mut s, 2, 0.65, 1.9);
    put(&mut s, 3, 2.0, 2.9);
    // (The shooter, hotdog 4, fires from a metre up: where it stands is its own business.)
    for (id, w) in [(2, 1), (3, 1)] {
        let at = Vec3::from_array(s.player(id).unwrap().p);
        let before = hp(&s, id);
        s.launch(4, at + Vec3::Y * 1., -Vec3::Y, w);
        run(&mut s, 0.4);
        assert_eq!(hp(&s, id) < before, id == 3, "a round down at {id}");
    }
    // A glob on the shelf's roof, right over hotdog 2's head (0.08 m above it): no harm and no throw through the roof.
    let at = Vec3::from_array(s.player(2).unwrap().p);
    let before = hp(&s, 2);
    s.launch(4, at + Vec3::new(0.02, 1., 0.), -Vec3::Y, 4);
    run(&mut s, 0.6);
    assert_eq!(hp(&s, 2), before, "a blast through the roof");
    assert!(!s.player(2).unwrap().launched);
    // The same glob on the floor beside hotdog 3: hurt.
    let at = Vec3::from_array(s.player(3).unwrap().p);
    let before = hp(&s, 3);
    s.launch(4, at + Vec3::new(0.1, 1., 0.), -Vec3::Y, 4);
    run(&mut s, 0.6);
    assert!(hp(&s, 3) < before);
    // And from inside out: hotdog 2 firing straight up splats on the shelf's underside.
    let round = s.launch(2, at_muzzle(&s, 2), Vec3::Y, 1);
    run(&mut s, 0.1);
    let splat = s.splats.iter().find(|sp| sp.projectile == round).map(|sp| sp.p[1] - GROUND).unwrap_or(f32::NAN);
    assert!((splat - 0.2).abs() < 0.02, "{splat}");
}
fn at_muzzle(s: &Feast, id: u32) -> Vec3 {
    muzzle(s.player(id).unwrap())
}

#[test]
fn a_jump_begun_on_the_move_carries_its_walk_through_the_air() {
    let mut s = floor("");
    park(&mut s, &[]);
    place_titan(&mut s, 4., 4.5, 0.);
    // Walking ahead (to -z) for a second, then the jump: it crouches for it standing, and takes off with that walk.
    let walk = Command::new(1, Vec2::new(0., -1.), 0., 0., 0, 0, None);
    for _ in 0..30 {
        s.titan_command(1, &walk, DT);
        s.tick(DT);
    }
    let from = titan(&s).p[2];
    s.titan_command(1, &Command::new(2, Vec2::ZERO, 0., 0., button::JUMP, 0, None), DT);
    let mut landed = None;
    for _ in 0..(2. / DT) as usize {
        s.tick(DT);
        if landed.is_none() && s.titan.as_ref().unwrap().landed.is_some() {
            landed = Some(titan(&s).p[2]);
        }
    }
    let went = from - landed.expect("it landed");
    let air = Move::Jump.data().active;
    assert!((went - WALK * air).abs() < 0.15, "{went} vs {}", WALK * air);
}

#[test]
fn a_hard_bot_titan_tucks_its_nape_down_when_a_small_hotdog_is_behind_it() {
    for (skill, tucks) in [("hard", true), ("normal", false)] {
        let mut s = floor(&format!("skill={skill}"));
        park(&mut s, &[2]);
        place_titan(&mut s, 4., 3., 0.);
        s.player_mut(1).unwrap().bot = true;
        // Hotdog 2 in front, three metres off (its target, out of reach); hotdog 3 behind it.
        put(&mut s, 2, 4., 0.4);
        put(&mut s, 3, 4., 5.);
        run(&mut s, 0.5);
        assert_eq!(titan(&s).crouch, tucks, "{skill}");
    }
}

/// Where the weak spot can be hit from behind (printed): for each stance and look, from the floor, a stool and a box at
/// 1.5, 2.5 and 4 m, whether a round at the head's middle (H), at the drawn patch (`weak_point`, W) and at the sphere under
/// the patch (P) finds the weak spot.
#[test]
#[ignore]
fn diag_weak_spot_map() {
    for (name, crouch, mv) in [("standing", false, Move::Idle), ("crouched", true, Move::Idle), ("kneeling", false, Move::Stagger)] {
        for pitch in [-1.5f32, -0.7, 0., 1.5] {
            let s = Shape::of(&figure(pitch, crouch), mv);
            let mut row = String::new();
            for height in [0.135f32, 0.4, 0.8] {
                for far in [1.5f32, 2.5, 4.] {
                    let from = Vec3::new(0., GROUND + height, far);
                    let a = zone(&s, from, s.head) == Some(Zone::Weak);
                    let b = zone(&s, from, s.weak_point()) == Some(Zone::Weak);
                    let c = zone(&s, from, s.head + s.spot.normalize() * s.head_radius) == Some(Zone::Weak);
                    row += &format!(" h{height}/d{far}:{}{}{}", if a { "H" } else { "-" }, if b { "W" } else { "-" }, if c { "P" } else { "-" });
                }
            }
            println!("{name:9} pitch {pitch:5.2}:{row}");
        }
    }
}


/// The head is the drawn head (a round body `head_profile` across about its upright from the neck), and its zones are
/// where on it a round lands: every way out from the head's middle, a round coming straight at that place meets the
/// head there, on the weak spot within WEAK_ARC of the nape's way (the drawn pale patch), on the face within FACE_ARC of
/// the face's way, and on the body elsewhere.
#[test]
fn the_head_is_the_drawn_head_and_its_zones_are_where_a_round_lands_on_it() {
    // (Crouched and looking up, the tucked nape is behind the shoulders: no round straight at the cap reaches it.)
    for (pitch, crouch, mv, open) in [(0., false, Move::Idle, true), (-1.2, false, Move::Idle, true), (0.8, true, Move::Idle, false), (0., false, Move::Stagger, true)] {
        let s = Shape::of(&figure(pitch, crouch), mv);
        let (mut met, mut weak) = (0, 0);
        for i in 0..36 {
            for j in 1..18 {
                let (round, up) = (i as f32 / 36. * TAU, j as f32 / 18. * PI);
                let n = Vec3::new(up.sin() * round.cos(), up.cos(), up.sin() * round.sin());
                // The place on the drawn head that way from its middle.
                let inside = |r: f32| {
                    let q = s.turn.inverse() * (s.head + n * r - s.neck);
                    Vec2::new(q.x, q.z).length() <= head_profile(q.y)
                };
                let (mut a, mut b) = (0f32, 0.6f32);
                for _ in 0..30 {
                    let m = 0.5 * (a + b);
                    if inside(m) { a = m } else { b = m }
                }
                let at = s.head + n * a;
                let Some(h) = hit(&s, at + n * 2., -n, 0.) else {
                    continue;
                };
                if h.t < 2. - 0.001 {
                    // (Something else is in front of that place: the shoulders, the chest at the neck's foot.)
                    continue;
                }
                met += 1;
                // (On the drawn head, there.)
                let q = s.turn.inverse() * (h.point - s.neck);
                assert!((Vec2::new(q.x, q.z).length() - head_profile(q.y)).abs() < 0.003 && h.point.distance(at) < 0.004, "pitch {pitch} {mv:?}: met at {} not {at}", h.point);
                let n = (h.point - s.head).normalize();
                let (to_weak, to_face) = (n.angle_between(s.nape) - WEAK_ARC, n.angle_between(s.face) - FACE_ARC);
                if to_weak.abs() < 0.01 || to_face.abs() < 0.01 {
                    continue;
                }
                let want = if to_weak < 0. { Zone::Weak } else if to_face < 0. { Zone::Face } else { Zone::Body };
                weak += (want == Zone::Weak) as u32;
                assert_eq!(h.zone, want, "pitch {pitch} {mv:?}: way {n}");
            }
        }
        assert!(met > 300 && (weak > 30) == open, "pitch {pitch} {mv:?}: {met} met, {weak} on the weak spot");
    }
}
