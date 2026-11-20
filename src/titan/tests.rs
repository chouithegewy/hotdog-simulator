// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The titan's checks: every move animated and timed by the simulation's table, its poses whole, the drawn body where
//! the simulation's hit zones are, the shockwave drawn to the simulation's front, the cameras never inside it, and the
//! budgets of the lower graphics levels.
use super::anims::{self, library};
use super::body::{self, Lod, Shapes};
use super::play::{self, Animator, Drive};
use super::pose::{self, L, R, Skeleton, solve};
use bevy::prelude::*;
use kitchen_sim::feast::Player;
use kitchen_sim::titan::{self as sim, Capsule, Move, Shape};

/// A titan standing at the origin facing -z, looking `pitch`, crouched or not.
fn titan(pitch: f32, crouch: bool) -> Player {
    Player { id: 2, p: [0., 0., 0.], yaw: 0., pitch, crouch, alive: true, grounded: true, ..Default::default() }
}
/// The drawn skeleton of a stance (0 standing, 1 crouched, 2 kneeling) looking `pitch`, in the world (the root at the
/// origin, scaled to the simulation's size).
fn drawn(stance: usize, pitch: f32) -> Skeleton {
    let pose = match stance {
        0 => anims::stance(),
        1 => anims::crouched(),
        _ => library().anim(Move::Stagger).pose_at(1.5),
    };
    solve(&pose, None, play::sim_nod(stance, pitch))
}
fn root() -> Transform {
    Transform::from_scale(Vec3::splat(super::rig::scale()))
}
/// The middle of the drawn head (its sphere's, as the simulation has one).
fn head_middle(s: &Skeleton) -> Vec3 {
    s.on_head(Vec3::new(0., HEAD_MIDDLE, 0.))
}
/// Where in the head link (above the neck) its widest middle is.
const HEAD_MIDDLE: f32 = 0.21;
fn gap_to_segment(p: Vec3, c: &Capsule) -> f32 {
    let ab = c.b - c.a;
    let t = ((p - c.a).dot(ab) / ab.length_squared().max(1e-9)).clamp(0., 1.);
    p.distance(c.a + ab * t)
}
/// How far the drawn torso, legs and arms are from the simulation's capsules' axes (`root` puts the drawn skeleton in the
/// world): the most, along the drawn torso's axis from its bottom to its top, from the simulation's torso and chest; the
/// most of each hip from its thigh, knee from its thigh or shin, and ankle from its shin; the most of each shoulder and
/// elbow from its upper arm and elbow and wrist from its forearm.
fn body_gaps(s: &Skeleton, root: Transform, shape: &Shape) -> [f32; 3] {
    let w = |q: Vec3| root.transform_point(q);
    let near = |q: Vec3, cs: &[&Capsule]| cs.iter().map(|c| gap_to_segment(w(q), c)).fold(f32::INFINITY, f32::min);
    let scale = super::rig::scale();
    let (bottom, top) = (body::BOTTOM + sim::TORSO_RADIUS / scale, body::TORSO_TOP - sim::CHEST_RADIUS / scale);
    let torso = (0..=8).map(|k| near(s.axis(bottom + (top - bottom) * k as f32 / 8.), &[&shape.torso, &shape.chest])).fold(0f32, f32::max);
    let legs = [L, R]
        .iter()
        .map(|&i| near(s.hip[i], &[&shape.thighs[i]]).max(near(s.knee[i], &[&shape.thighs[i], &shape.legs[i]])).max(near(s.ankle[i], &[&shape.legs[i]])))
        .fold(0f32, f32::max);
    let arms = [L, R]
        .iter()
        .map(|&i| {
            let (upper, fore) = (&shape.arms[2 * i], &shape.arms[2 * i + 1]);
            near(s.shoulder[i], &[upper]).max(near(s.elbow[i], &[upper, fore])).max(near(s.wrist[i], &[fore]))
        })
        .fold(0f32, f32::max);
    [torso, legs, arms]
}
/// How far each drawn part is from the simulation's: the head's middle, the face, the torso's axis, the legs, the arms
/// (`body_gaps`), and the drawn weak spot (its pick, its ring) from the simulation's `weak_point`.
fn errors(stance: usize, pitch: f32) -> [f32; 6] {
    let p = titan(pitch, stance == 1);
    let mv = if stance == 2 { Move::Stagger } else { Move::Idle };
    let shape = Shape::of(&p, mv);
    let s = drawn(stance, pitch);
    let w = |q: Vec3| root().transform_point(q);
    let head = w(head_middle(&s)).distance(shape.head);
    let (spot, _) = s.spot();
    let face = w(s.on_head(Vec3::new(0., HEAD_MIDDLE + 0.02, -body::head_radius(HEAD_MIDDLE + 0.02)))).distance(shape.face_point());
    let [torso, legs, arms] = body_gaps(&s, root(), &shape);
    [head, face, torso, legs, arms, w(spot).distance(shape.weak_point())]
}

#[test]
#[ignore]
fn print_the_figures_against_the_simulations() {
    for stance in 0..3 {
        for pitch in [-1.2f32, -0.6, 0., 0.6, 1.2] {
            let e = errors(stance, pitch);
            println!("stance {stance} pitch {pitch:5.2}: head {:.3} face {:.3} torso {:.3} legs {:.3} arms {:.3} spot {:.3}", e[0], e[1], e[2], e[3], e[4], e[5]);
        }
        let s = drawn(stance, 0.);
        let p = titan(0., stance == 1);
        let shape = Shape::of(&p, if stance == 2 { Move::Stagger } else { Move::Idle });
        println!("  drawn head {:?} sim head {:?}", head_middle(&s), shape.head);
        println!("  drawn axis {:?} .. {:?}  sim torso {:?} .. {:?} r {}", s.axis(body::BOTTOM + 0.2), s.axis(body::TORSO_TOP - 0.2), shape.torso.a, shape.torso.b, shape.torso.radius);
        println!("  drawn hip {:?} knee {:?} ankle {:?}  sim leg {:?} .. {:?}", s.hip[R], s.knee[R], s.ankle[R], shape.legs[R].a, shape.legs[R].b);
        println!("  drawn crown {:?} top", s.crown());
    }
}

#[test]
#[ignore]
fn print_the_drawn_figures_in_hotdog_measures() {
    let k = 1. / sim::SCALE;
    for stance in 0..3 {
        let s = drawn(stance, 0.);
        let (a, b) = (s.axis(0.8), s.axis(1.16));
        let neck = s.head.translation;
        println!(
            "stance {stance}: leg_x {:.4} leg y [{:.4}, {:.4}] leg_z [{:.4}, {:.4}] torso [{:.4}, {:.4}] torso_z [{:.4}, {:.4}] pivot [{:.4}, {:.4}]",
            (s.knee[R].x + s.ankle[R].x) * 0.5 * k,
            s.ankle[R].y * k,
            s.knee[R].y * k,
            s.ankle[R].z * k,
            s.knee[R].z * k,
            a.y * k,
            b.y * k,
            a.z * k,
            b.z * k,
            neck.y * k,
            neck.z * k
        );
    }
}

/// Every move the simulation has (by its number, as the interface lists them) has an animation, which lasts exactly as
/// long as the simulation's table (and the view's length) says the move does.
#[test]
fn every_move_has_its_animation_and_it_lasts_as_long_as_the_move() {
    let lib = library();
    assert_eq!(lib.moves.len(), sim::MOVES);
    for (id, mv) in Move::ALL.iter().enumerate() {
        assert_eq!(*mv as usize, id, "the moves in order");
        let a = lib.anim(*mv);
        assert!(!a.name.is_empty() && !a.keys.is_empty(), "{mv:?}");
        let d = mv.data();
        if d.total() <= 0. {
            // The stance loops (Idle, Walk): no length of their own.
            assert!(matches!(mv, Move::Idle | Move::Walk), "{mv:?}");
            continue;
        }
        assert_eq!(a.keys.first().map(|k| k.at), Some(0.), "{mv:?} starts on beat 0");
        assert!(a.keys.windows(2).all(|w| w[0].at < w[1].at), "{mv:?}: its keys in order");
        if a.cycle != anims::Cycle::Once {
            continue;
        }
        assert_eq!(a.keys.last().map(|k| k.at), Some(3.), "{mv:?} ends on beat 3");
        // At the table's timing, and at another length the view may give it (a charge cut short, a long stagger).
        for len in [d.total(), d.total() * 0.7, d.total() * 1.3] {
            let timing = anims::timing(*mv, len);
            assert!((timing.total() - len).abs() < 1e-3 || len < d.startup + 0.1, "{mv:?}: {} for {len}", timing.total());
            assert!(timing.startup >= 0. && timing.active > 0. && timing.recovery > 0., "{mv:?}: {timing:?}");
            let end = timing.total();
            assert!(a.beat(end - 0.02, timing) < 3., "{mv:?} is still going just before its end");
            assert!(a.beat(end, timing) > 2.9999, "{mv:?} has ended at its end");
            // The tell is the startup: the blow (beat 1) comes exactly when the simulation's active window opens.
            assert!((timing.time(1.) - timing.startup).abs() < 1e-4, "{mv:?}");
        }
        // Played through by the animator from start to end, it reaches its last beat at the end and not before.
        let timing = anims::timing(*mv, d.total());
        let mut animator = Animator::default();
        let mut t = 0.;
        let dt = 1. / 60.;
        let mut last = 0.;
        while t <= timing.total() + 1e-4 {
            let frame = animator.update(&drive(*mv, t, timing), dt);
            assert!(frame.beat + 1e-4 >= last || frame.beat == 0., "{mv:?}: its beat runs on ({} after {last})", frame.beat);
            last = frame.beat;
            t += dt;
        }
        assert!(last > 2.97, "{mv:?} played to its end: {last}");
    }
}
fn drive(mv: Move, t: f32, timing: crate::kungfu::Timing) -> Drive {
    Drive { mv, t, timing, crouch: mv == Move::SwatLow || mv == Move::Kick, airborne: false, moved: Vec2::ZERO, nod: 0., stare: None, stride: None }
}

/// Every pose of every move, and of the walks, the run, the crouch and the flinch, is made of finite numbers, and so is
/// every part of the rig it is solved into; the limbs keep their lengths.
#[test]
fn every_pose_is_finite_and_its_limbs_keep_their_lengths() {
    let lib = library();
    let mut poses: Vec<(String, pose::Pose)> = vec![];
    for mv in Move::ALL {
        let a = lib.anim(mv);
        for k in 0..=60 {
            let beat = k as f32 / 20.;
            poses.push((format!("{mv:?} {beat}"), a.pose_at(beat)));
        }
    }
    for k in 0..=40 {
        let phase = k as f32 / 40. * std::f32::consts::TAU * 2.;
        poses.push((format!("walk {phase}"), anims::walk(phase, 1.)));
        poses.push((format!("crawl {phase}"), anims::crawl(phase, 1.)));
        poses.push((format!("run {phase}"), anims::run(phase)));
        poses.push((format!("flinch {phase}"), anims::flinched(&anims::walk(phase, 0.5), (k as f32 / 40.).min(1.))));
    }
    for (name, p) in &poses {
        assert!(p.finite(), "{name}");
        for look in [-sim::KNEEL_BOW, -0.8, 0., 0.4, 0.9] {
            let s = solve(p, Some(Vec3::new(0.5, 0.2, -2.)), look);
            let all = [s.torso, s.head, s.eyes[0], s.eyes[1], s.upper[0], s.upper[1], s.fore[0], s.fore[1], s.hands[0], s.hands[1], s.thighs[0], s.thighs[1], s.shins[0], s.shins[1], s.feet[0], s.feet[1]];
            for t in all {
                assert!(t.translation.is_finite() && t.rotation.is_finite() && t.scale.is_finite() && t.rotation.is_normalized(), "{name}: {t:?}");
            }
            for i in [L, R] {
                assert!((s.shoulder[i].distance(s.elbow[i]) - body::UPPER).abs() < 1e-3 && (s.elbow[i].distance(s.wrist[i]) - body::FORE).abs() < 2e-3, "{name}: the arm");
                assert!((s.hip[i].distance(s.knee[i]) - body::THIGH).abs() < 1e-3 && (s.knee[i].distance(s.ankle[i]) - body::SHIN).abs() < 2e-3, "{name}: the leg");
            }
        }
    }
}

/// The drawn titan is where the simulation's hit zones are, standing, crouched and on its knees, at every pitch of its
/// look: its head (the middle of the sphere rounds hit), its weak spot and its face within a few centimetres, its torso,
/// its legs and its arms within FIT of their capsules' axes (the simulation's figures are made from the drawn titan's:
/// `generate_the_simulations_figures`).
#[test]
fn the_drawn_body_is_where_the_simulation_hits_it() {
    for stance in 0..3 {
        for k in 0..=12 {
            let pitch = -1.45 + 2.9 * k as f32 / 12.;
            let [head, face, torso, legs, arms, spot] = errors(stance, pitch);
            assert!(head < 0.05, "stance {stance}, pitch {pitch}: the head is {head} m off");
            // (The patch itself is the simulation's weak cap: `a_round_is_on_the_weak_spot_exactly_where_the_drawn_patch_is`.)
            // (And the spot is drawn where the simulation has it, for the bots to aim at and the HUD to ring.)
            assert!(spot < 0.02, "stance {stance}, pitch {pitch}: the drawn weak spot is {spot} m from the simulation's");
            assert!(face < 0.06, "stance {stance}, pitch {pitch}: the face is {face} m off");
            assert!(torso < FIT, "stance {stance}, pitch {pitch}: the torso is {torso} m off");
            assert!(legs < FIT, "stance {stance}, pitch {pitch}: the legs are {legs} m off");
            assert!(arms < FIT, "stance {stance}, pitch {pitch}: the arms are {arms} m off");
        }
    }
}
/// How far a drawn part's axis may be from the simulation's capsule's.
const FIT: f32 = 0.05;
/// Where a ray first meets the drawn head (its own frame from `head`, the head link's place in the world: a round body
/// `body::head_radius` across about its upright), and the place it meets in the head's frame.
fn drawn_head_hit(head: Transform, from: Vec3, way: Vec3) -> Option<(f32, Vec3)> {
    let back = head.compute_affine().inverse();
    let (o, d) = (back.transform_point3(from), back.transform_vector3(way).normalize());
    let scale = back.transform_vector3(way).length();
    let inside = |t: f32| {
        let q = o + d * t;
        q.y > -0.08 && q.y < body::TOP - body::NECK.y && Vec2::new(q.x, q.z).length() <= body::head_radius(q.y)
    };
    let (mut t, step) = (0f32, 0.002);
    while t < 6. {
        if inside(t + step) {
            let (mut a, mut b) = (t, t + step);
            for _ in 0..20 {
                let m = 0.5 * (a + b);
                if inside(m) { b = m } else { a = m }
            }
            return Some((b / scale, o + d * b));
        }
        t += step;
    }
    None
}
/// Every round over the back of the head is on the weak spot exactly when what it meets first is the drawn pale patch
/// (the simulation's weak cap, body.rs `in_patch`), but within a centimetre of the patch's rim or of the edge of whatever
/// hides it or of the head's outline: standing, crouched and kneeling, looking down to looking up, from behind, above,
/// below and the sides.
#[test]
fn a_round_is_on_the_weak_spot_exactly_where_the_drawn_patch_is() {
    let (mut rounds, mut on, mut banded, mut wrong) = (0, 0, 0, vec![]);
    for stance in 0..3 {
        let pose = [anims::stance(), anims::crouched(), anims::kneeling()][stance];
        for pitch in [-1.2f32, -0.6, 0., 0.6, 1.2] {
            let p = titan(pitch, stance == 1);
            let shape = Shape::posed(&p, sim::stance_frame(stance), stance);
            let mut parts = shape;
            (parts.neck, parts.head) = (Vec3::splat(1e4), Vec3::splat(1e4));
            let s = solve(&pose, None, play::sim_nod(stance, pitch));
            let head = root().mul_transform(s.head);
            let centre = head.transform_point(body::cap_centre());
            let nape = (head.rotation * body::nape()).normalize();
            for azimuth in [-1.9f32, -1.2, -0.6, 0., 0.6, 1.2, 1.9] {
                for elevation in [-0.5f32, -0.1, 0.3, 0.8, 1.3] {
                    let from = centre + Quat::from_rotation_y(azimuth) * (Vec3::new(0., elevation.sin(), elevation.cos()) * 2.5);
                    for i in -6..=6 {
                        for j in -6..=6 {
                            let way_out = (Quat::from_rotation_x(i as f32 * 0.2) * Quat::from_rotation_y(j as f32 * 0.2) * body::nape()).normalize();
                            let target = head.transform_point(body::on_head_along(way_out));
                            let way = (target - from).normalize();
                            let Some((t, local)) = drawn_head_hit(head, from, way) else {
                                continue;
                            };
                            let hidden = sim::hit(&parts, from, way, 0.).map(|h| h.t);
                            if hidden.is_some_and(|h| h < t - 0.01) {
                                continue;
                            }
                            rounds += 1;
                            let patch = body::in_patch(local) > 0.5;
                            let weak = sim::hit(&shape, from, way, 0.).is_some_and(|h| h.zone == sim::Zone::Weak);
                            on += patch as u32;
                            if patch != weak {
                                let rim = ((local - body::cap_centre()).angle_between(body::nape()) - sim::WEAK_ARC).abs() * (local - body::cap_centre()).length();
                                // (At the patch's rim, at the edge of what hides it, or grazing the head's outline.)
                                let grazing = shape.ray_head(from, way, 0.).is_none() && shape.ray_head(from, way, 0.01).is_some();
                                if rim < 0.01 || hidden.is_some_and(|h| (h - t).abs() < 0.01) || grazing {
                                    banded += 1;
                                } else {
                                    let h = sim::hit(&shape, from, way, 0.);
                                    wrong.push(format!("stance {stance} pitch {pitch} from {from:.2}: patch {patch} weak {weak} rim {rim:.3} drawn t {t:.3} sim {:?} head t {:?} parts {:?}", h.map(|h| (h.t, h.zone)), shape.ray_head(from, way, 0.), hidden));
                                }
                            }
                        }
                    }
                }
            }
            let _ = nape;
        }
    }
    println!("{rounds} rounds on the head, {on} on the patch, {banded} at an edge, {} wrong", wrong.len());
    assert!(rounds > 2000 && on > 400, "{rounds} {on}");
    assert!(wrong.is_empty(), "{} wrong, as {:?}", wrong.len(), &wrong[..wrong.len().min(8)]);
}
/// The drawn time `t` seconds into a move with a hit-stop: held at the blow, then catching up (play.rs `shown`).
fn shown_time(mv: Move, t: f32, timing: crate::kungfu::Timing) -> f32 {
    let a = library().anim(mv);
    if a.strike.is_none() || a.hitstop <= 0. {
        return t;
    }
    let at = timing.time(if a.blow == anims::Blow::Land { 2. } else { 1. });
    let (length, total) = (a.hitstop, timing.total());
    if t < at {
        t
    } else if t < at + length {
        at
    } else {
        let rest = (total - at).max(1e-3);
        at + (t - at - length) * rest / (rest - length).max(1e-3)
    }
}
/// Every move's drawn pose, all through the move, is where the simulation's figure for that instant is (its torso, its
/// legs and its arms within FIT, its head within a few centimetres), crouched for the low swat and the kick, on its knees
/// in a stagger, a stumble and its fall. (The charge's run is drawn as a run cycle that keeps step with the ground it
/// covers: there the legs are left out.)
#[test]
fn every_move_is_drawn_where_the_simulation_hits_it() {
    let mut worst = vec![];
    for mv in Move::ALL.into_iter().filter(|m| !matches!(m, Move::Idle | Move::Walk)) {
        let a = library().anim(mv);
        let timing = anims::timing(mv, 0.);
        let crouch = matches!(mv, Move::SwatLow | Move::Kick);
        let mut most = [0f32; 4];
        let mut when = [0f32; 4];
        for k in 0..=96 {
            let t = timing.total() * k as f32 / 96.;
            let beat = a.beat(shown_time(mv, t, timing), timing);
            let p = titan(-0.4, crouch);
            let stance = play::stance_of(mv, crouch);
            let s = solve(&a.pose_at(beat), None, play::sim_nod(stance, p.pitch));
            let shape = Shape::posed(&p, &sim::move_frame(mv, t, 0.), sim::stance_of(&p, mv));
            let [torso, legs, arms] = body_gaps(&s, root(), &shape);
            let running = a.running.is_some_and(|(from, to)| (from..to).contains(&beat));
            let head = root().transform_point(head_middle(&s)).distance(shape.head);
            for (i, e) in [torso, if running { 0. } else { legs }, arms, head].into_iter().enumerate() {
                if e > most[i] {
                    (most[i], when[i]) = (e, t);
                }
            }
        }
        println!("{mv:?} (the worst at {:.2} {:.2} {:.2} {:.2} s of {:.2})", when[0], when[1], when[2], when[3], timing.total());
        worst.push((mv, most));
    }
    // Idle, the titan breathes (the animator's own clock): the simulation's standing figure is its stance.
    let idle = library().anim(Move::Idle);
    let still = Shape::of(&titan(-0.4, false), Move::Idle);
    let mut most = [0f32; 4];
    for k in 0..=72 {
        let s = solve(&idle.sample_loop(3.6 * k as f32 / 72.), None, play::sim_nod(0, -0.4));
        let [torso, legs, arms] = body_gaps(&s, root(), &still);
        let head = root().transform_point(head_middle(&s)).distance(still.head);
        most = [most[0].max(torso), most[1].max(legs), most[2].max(arms), most[3].max(head)];
    }
    worst.push((Move::Idle, most));
    for (mv, [torso, legs, arms, head]) in &worst {
        println!("{mv:?}: torso {torso:.3} legs {legs:.3} arms {arms:.3} head {head:.3}");
    }
    for (mv, [torso, legs, arms, head]) in &worst {
        assert!(*torso < FIT && *legs < FIT && *arms < FIT, "{mv:?}: torso {torso} legs {legs} arms {arms} m off");
        assert!(*head < 0.05, "{mv:?}: the head is {head} m off");
    }
}
/// The animator itself, driven by a round's titan as the client drives it (its view, frame by frame: walking forward
/// and back, crouching and crawling, a low swat, a kick, up again, a jump, a charge, a stagger, walking again, and
/// felled), draws the body where the round's own figure for it is, the walk's and the crawl's strides, the blends
/// between moves, the crouch's easing and the head's nod included: all of it within FIT while it can be hit; felled (the
/// round over: nothing hits it) its torso and legs.
#[test]
fn the_animator_draws_the_titan_where_the_round_hits_it_frame_by_frame() {
    use kitchen_sim::feast::Feast;
    use kitchen_sim::net::{Command, button};
    let mut f = Feast::with_map(titan_map(), false);
    f.set_rules(kitchen_sim::rules::Rules::parse("mode=titan titan=1"));
    for p in &mut f.players {
        p.bot = false;
    }
    let mut a = Animator::default();
    let dt = 1. / 60.;
    let mut worst: std::collections::BTreeMap<String, [f32; 4]> = Default::default();
    let mut when: std::collections::BTreeMap<String, f32> = Default::default();
    let mut felled = [0f32; 4];
    let mut was = None;
    for k in 0..(25. / dt) as usize {
        let time = k as f32 * dt;
        let within = |from: f32, to: f32| (from..to).contains(&time);
        let pressed = |at: f32| (time - at).abs() < dt * 0.5;
        let mut buttons = if within(4., 8.) { button::CROUCH } else { 0 };
        if pressed(6.) {
            buttons |= button::FIRE;
        }
        if pressed(7.3) {
            buttons |= button::MELEE;
        }
        if pressed(9.5) {
            buttons |= button::JUMP;
        }
        if pressed(12.) {
            buttons |= button::STIM;
        }
        if pressed(16.) {
            f.titan_press(Move::Stagger);
        }
        if pressed(21.5) {
            f.hurt(1, 1e9);
        }
        // (The stick's -y is ahead.)
        let stick = if within(0.5, 2.5) || within(4., 5.5) || within(19.5, 21.) {
            Vec2::new(0., -1.)
        } else if within(2.5, 3.5) {
            Vec2::new(0., 1.)
        } else {
            Vec2::ZERO
        };
        f.titan_command(1, &Command::new(k as u32 + 1, stick, 0., -0.4, buttons, 0, None), dt);
        f.tick(dt);
        let view = f.titan_view().unwrap();
        let p = f.player(1).unwrap().clone();
        let mv = if p.alive { view.mv() } else { Move::Down };
        let at = Vec3::from_array(p.p);
        let step = Quat::from_rotation_y(-p.yaw) * (at - was.unwrap_or(at));
        was = Some(at);
        let stride = Some(play::Stride { phase: view.stride, drive: view.drive, exact: true });
        let d = Drive { mv, t: view.t(view.elapsed), timing: anims::timing(mv, view.len), crouch: view.crouch, airborne: !p.grounded, moved: Vec2::new(step.x, -step.z), nod: play::sim_nod(play::stance_of(mv, view.crouch), p.pitch), stare: None, stride };
        let frame = a.update(&d, dt);
        let shape = f.titan_shape().unwrap();
        let place = Transform { translation: at, rotation: Quat::from_rotation_y(p.yaw), scale: Vec3::splat(super::rig::scale()) };
        let [torso, legs, arms] = body_gaps(&frame.skeleton, place, &shape);
        let head = place.transform_point(head_middle(&frame.skeleton)).distance(shape.head);
        let gait = if !matches!(mv, Move::Idle | Move::Walk) { "" } else if view.drive > 0.3 && view.crouch { " crawling" } else if view.drive > 0.3 { " walking" } else { "" };
        let e = [torso, legs, arms, head];
        if !p.alive {
            felled = [0, 1, 2, 3].map(|i| felled[i].max(e[i]));
        } else {
            let key = format!("{mv:?}{gait}");
            let w = worst.entry(key.clone()).or_default();
            if e.iter().zip(w.iter()).any(|(a, b)| a > b) && e.iter().fold(0f32, |a, b| a.max(*b)) >= w.iter().fold(0f32, |a, b| a.max(*b)) {
                when.insert(key, time);
            }
            *w = [0, 1, 2, 3].map(|i| w[i].max(e[i]));
        }
    }
    for (what, [torso, legs, arms, head]) in &worst {
        println!("{what}: torso {torso:.3} legs {legs:.3} arms {arms:.3} head {head:.3} (the worst at {:.2} s)", when.get(what).copied().unwrap_or(0.));
    }
    println!("felled: torso {:.3} legs {:.3} arms {:.3} head {:.3}", felled[0], felled[1], felled[2], felled[3]);
    for want in ["Walk walking", "Walk crawling", "Charge", "SwatLow", "Kick", "Jump", "Stagger"] {
        assert!(worst.contains_key(want), "{want} was not seen: {:?}", worst.keys());
    }
    assert!(felled[3] > 0., "it was felled");
    for (what, [torso, legs, arms, head]) in &worst {
        assert!(*torso < FIT && *legs < FIT && *arms < FIT && *head < FIT, "{what}: torso {torso} legs {legs} arms {arms} head {head} m off");
    }
    assert!(felled[0] < FIT && felled[1] < FIT, "felled: torso {} legs {} m off", felled[0], felled[1]);
}
/// How far the motions the simulation does not follow take the drawn body from its figure (printed, for the record):
/// the walk's and the crawl's strides (their phase is the client's), the flinch when the weak spot is hit, and the tuck
/// in the air off a ledge.
#[test]
#[ignore]
fn how_far_the_motions_the_simulation_does_not_follow_go() {
    let (standing, crouched) = (Shape::of(&titan(-0.4, false), Move::Idle), Shape::of(&titan(-0.4, true), Move::Idle));
    let gaps = |pose: &pose::Pose, shape: &Shape, stance: usize| {
        let s = solve(pose, None, play::sim_nod(stance, -0.4));
        let [torso, legs, arms] = body_gaps(&s, root(), shape);
        [torso, legs, arms, root().transform_point(head_middle(&s)).distance(shape.head)]
    };
    let most = |f: &dyn Fn(f32) -> [f32; 4]| (0..=48).map(|k| f(k as f32 / 48.)).fold([0f32; 4], |a, b| [0, 1, 2, 3].map(|i| a[i].max(b[i])));
    let tau = std::f32::consts::TAU;
    for (name, m) in [
        ("walk", most(&|u| gaps(&anims::walk(u * tau, 1.), &standing, 0))),
        ("crawl", most(&|u| gaps(&anims::crawl(u * tau, 1.), &crouched, 1))),
        ("flinch", most(&|u| gaps(&anims::flinched(&anims::stance(), u), &standing, 0))),
        ("tuck", most(&|u| gaps(&anims::stance().lerp(&library().anim(Move::Jump).pose_at(1.5), u), &standing, 0))),
    ] {
        println!("{name}: torso {:.3} legs {:.3} arms {:.3} head {:.3}", m[0], m[1], m[2], m[3]);
    }
    // (Against the simulation's own stride figure, at its pace and at part of it.)
    for (name, crouch, gait) in [("walk", false, &(|u: f32, d: f32| anims::walk(u, d)) as &dyn Fn(f32, f32) -> pose::Pose), ("crawl", true, &|u: f32, d: f32| anims::crawl(u, d))] {
        for d in [1f32, 0.7, 0.4] {
            let m = (0..=96).map(|i| {
                let φ = tau * i as f32 / 96.;
                let shape = Shape::posed(&titan(-0.4, crouch), &sim::stride_frame(crouch as u8 as f32, d, φ), crouch as usize);
                gaps(&gait(φ, d), &shape, crouch as usize)
            }).fold([0f32; 4], |a, b| [0, 1, 2, 3].map(|i| a[i].max(b[i])));
            println!("{name} at {d} of its pace, against the simulation's: torso {:.3} legs {:.3} arms {:.3} head {:.3}", m[0], m[1], m[2], m[3]);
        }
    }
}

/// The shockwave's ring is drawn with its leading edge exactly where the simulation's wave front is: a smash in the
/// simulation hits each small hotdog when the drawn edge reaches it, and not before.
#[test]
fn the_drawn_shockwave_is_the_simulations_front() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/maps/titans-kitchen.json")).unwrap();
    let map = kitchen_sim::map::Map::parse(&text).unwrap();
    let mut feast = kitchen_sim::feast::Feast::with_map(map, false);
    feast.set_rules(kitchen_sim::rules::Rules::parse("sandbox mode=titan titan=2 botmode=idle"));
    let titan = feast.titan_id().expect("a titan");
    let (centre, yaw) = (Vec2::new(2.0, -9.0), 0f32);
    let smalls: Vec<u32> = feast.players.iter().map(|p| p.id).filter(|id| *id != titan).take(4).collect();
    let d = Move::Smash.data();
    let ahead = sim::facing(yaw) * d.ahead;
    let impact = centre + ahead;
    // The small hotdogs in a line out from where the fists land, off to the side of the titan.
    let spots: Vec<f32> = vec![0.55, 0.85, 1.15, 1.45];
    let place = |feast: &mut kitchen_sim::feast::Feast| {
        for (k, id) in smalls.iter().enumerate() {
            let at = impact - Vec2::new(spots[k], 0.);
            let p = feast.player_mut(*id).unwrap();
            p.p = [at.x, kitchen_sim::feast::FLOOR, at.y];
            (p.v, p.grounded) = ([0.; 3], true);
            let pinned = Vec3::from_array(p.p);
            if let Some(body) = feast.bodies.get_mut(*id) {
                body.place(pinned);
            }
        }
        let p = feast.player_mut(titan).unwrap();
        (p.p, p.yaw, p.pitch) = ([centre.x, kitchen_sim::feast::FLOOR, centre.y], yaw, 0.);
    };
    place(&mut feast);
    feast.tick(1. / 120.);
    {
        let t = feast.titan.as_mut().unwrap();
        (t.mv, t.started, t.len, t.phase) = (Move::Smash, feast.elapsed, d.total(), 0);
        (t.brain.think, t.brain.choose) = (1e6, 1e6);
    }
    let mut struck_at: Vec<Option<f32>> = vec![None; smalls.len()];
    let mut smashed: Option<(f32, Vec3)> = None;
    for _ in 0..(3. * 120.) as usize {
        place(&mut feast);
        feast.tick(1. / 120.);
        if let Some(t) = feast.titan.as_mut() {
            (t.brain.think, t.brain.choose) = (1e6, 1e6);
        }
        if smashed.is_none() {
            smashed = feast.titan_view().and_then(|v| v.events.iter().find(|e| e.kind == Some(sim::EventKind::Smash)).map(|e| (e.time, Vec3::from_array(e.p))));
        }
        let hits: Vec<u32> = feast.titan_view().map_or(vec![], |v| v.events.iter().filter(|e| e.kind == Some(sim::EventKind::Hit) && e.mv == Move::Smash as u8).map(|e| e.to).collect());
        for (k, id) in smalls.iter().enumerate() {
            if struck_at[k].is_none() && hits.contains(id) {
                struck_at[k] = Some(feast.elapsed);
            }
        }
    }
    let (when, at) = smashed.expect("the fists landed");
    assert!(Vec2::new(at.x, at.z).distance(impact) < 0.02, "the ring is drawn where they landed: {at} {impact}");
    for (k, struck) in struck_at.iter().enumerate() {
        let struck = struck.unwrap_or_else(|| panic!("hotdog {k} at {} was hit", spots[k]));
        // Its nearest point: its middle less its radius (feast::body_gap on the floor).
        let near = spots[k] - kitchen_sim::feast::HOTDOG_RADIUS as f32;
        let edge = super::fx::front(struck - when, d.wave, d.reach);
        let edge_before = super::fx::front(struck - when - 1. / 120., d.wave, d.reach);
        assert!(edge + 1e-3 >= near, "hotdog at {}: hit when the drawn edge was at {edge}", spots[k]);
        assert!(edge_before < near + 0.03, "hotdog at {}: the drawn edge passed it ({edge_before}) before it was hit", spots[k]);
    }
    // And the ring's card is drawn with its leading edge at that front, frame by frame.
    let mut app = fx_app(crate::quality::Level::High);
    super::fx::ring(app.world_mut(), Vec3::new(impact.x, kitchen_sim::feast::FLOOR, impact.y), d.wave, d.reach, true);
    let mut t = 0.;
    for _ in 0..90 {
        step(&mut app, 1. / 60.);
        t += 1. / 60.;
        let fx = app.world().resource::<super::fx::Fx>();
        let drawn = fx.drawn_edges(app.world());
        if let Some(edge) = drawn.first() {
            assert!((edge - super::fx::front(t, d.wave, d.reach)).abs() < 1e-3, "at {t}: drawn {edge}");
            assert!(*edge <= d.reach + 1e-4, "never past its reach");
        }
    }
}
/// A world with what the effects need, drawn at `level`.
fn fx_app(level: crate::quality::Level) -> App {
    let mut app = App::new();
    app.add_plugins((bevy::app::TaskPoolPlugin::default(), AssetPlugin::default()));
    app.init_asset::<Mesh>().init_asset::<StandardMaterial>().init_asset::<Image>().init_asset::<AudioSource>().init_asset::<super::rig::TitanMaterial>();
    app.init_resource::<Time<Real>>().init_resource::<super::Titans>().init_resource::<super::fx::Fx>().init_resource::<super::camera::View>();
    app.insert_resource(crate::quality::Detail::of(level, crate::visuals::Quality::Full));
    app.add_systems(Update, super::fx::update);
    app.world_mut().resource_mut::<Time<Real>>().update_with_duration(std::time::Duration::ZERO);
    app.update();
    app
}
fn step(app: &mut App, dt: f32) {
    app.world_mut().resource_mut::<Time<Real>>().update_with_duration(std::time::Duration::from_secs_f32(dt));
    app.update();
}

/// Every pose's skeleton: the moves at every few beats, the gaits through their cycles.
fn every_skeleton() -> Vec<(String, Skeleton)> {
    let lib = library();
    let mut out = vec![];
    for mv in Move::ALL {
        for k in 0..=30 {
            let beat = k as f32 / 10.;
            for stance in [0usize, 1, 2] {
                out.push((format!("{mv:?} {beat} {stance}"), solve(&lib.anim(mv).pose_at(beat), None, play::sim_nod(stance, 0.3))));
            }
        }
    }
    for k in 0..24 {
        let phase = k as f32 / 24. * std::f32::consts::TAU;
        out.push((format!("walk {phase}"), solve(&anims::walk(phase, 1.), None, 0.)));
        out.push((format!("crawl {phase}"), solve(&anims::crawl(phase, 1.), None, play::sim_nod(1, 0.))));
        out.push((format!("run {phase}"), solve(&anims::run(phase), None, 0.2)));
    }
    out
}

/// No camera is ever left inside the giant: wherever a small hotdog's eye (or a death camera, or a replay's) ends up
/// as it steps over it, charges through it or falls on it, the camera is moved out of every part of its body. The
/// titan's own player's eye, in its head, is clear of its arms and legs (all it is shown of itself); and its view from
/// behind is outside it.
#[test]
fn no_camera_is_ever_inside_the_giant() {
    let root = Transform::from_translation(Vec3::new(0.4, kitchen_sim::feast::FLOOR, -10.)).with_rotation(Quat::from_rotation_y(0.7)).with_scale(Vec3::splat(super::rig::scale()));
    let level = {
        let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/maps/titans-kitchen.json")).unwrap();
        kitchen_sim::map::Level::new(kitchen_sim::map::Map::parse(&text).unwrap())
    };
    for (name, s) in every_skeleton() {
        let hulls = super::camera::hulls(&s, &root);
        // Eyes at a small hotdog's height and above it (on a stool, flung, a camera over the shoulder), all round it.
        for i in 0..14 {
            for j in 0..14 {
                for h in [0.135f32, 0.4, 0.8, 1.3] {
                    let eye = root.translation + Vec3::new(-1.3 + i as f32 * 0.2, h, -1.3 + j as f32 * 0.2);
                    let out = super::camera::keep_out(eye, &hulls);
                    for (a, b, r) in &hulls {
                        let ab = *b - *a;
                        let t = ((out - *a).dot(ab) / ab.length_squared().max(1e-9)).clamp(0., 1.);
                        assert!(out.distance(*a + ab * t) > r + super::camera::CLEAR * 0.5, "{name}: a camera at {eye} left inside the giant at {out}");
                    }
                }
            }
        }
        // Its own eye: clear of everything of itself it is shown (arms, hands, shins, feet).
        let eye = root.transform_point(s.eye());
        let shown: Vec<(Vec3, Vec3, f32)> = hulls.iter().skip(2).enumerate().filter(|(k, _)| k % 6 != 3).map(|(_, h)| *h).collect();
        for (a, b, r) in shown {
            let ab = b - a;
            let t = ((eye - a).dot(ab) / ab.length_squared().max(1e-9)).clamp(0., 1.);
            assert!(eye.distance(a + ab * t) > r * 0.6 + super::camera::NEAR, "{name}: its own eye is inside its limb");
        }
    }
    // From behind: outside it, whatever it does, wherever it looks.
    for (name, s) in every_skeleton().into_iter().step_by(7) {
        let hulls = super::camera::hulls(&s, &root);
        for pitch in [-0.8f32, 0., 0.6] {
            let cam = super::camera::behind(&level, root.translation, 0.7, pitch).translation;
            assert_eq!(super::camera::keep_out(cam, &hulls), cam, "{name}: the view from behind is inside it");
        }
    }
}

/// At Low and Lowest the titan is made of far fewer triangles, its effects of far fewer particles, and it is the same
/// handful of entities and shared materials at every level.
#[test]
fn the_low_and_lowest_budgets_hold() {
    let full = Shapes::new(Lod::Full).triangles();
    let lean = Shapes::new(Lod::Lean).triangles();
    let least = Shapes::new(Lod::Least).triangles();
    println!("triangles: full {full}, lean {lean}, least {least}");
    assert!(lean * 3 <= full, "Low: {lean} of {full}");
    assert!(least * 9 <= full && least <= 9_000, "Lowest: {least} of {full}");
    use crate::quality::Level;
    let budgets = Level::ALL.map(super::fx::Budget::of);
    for w in budgets.windows(2) {
        assert!(w[1].entities() <= w[0].entities() && w[1].rate <= w[0].rate, "each level no more than the one above");
    }
    let (low, lowest) = (super::fx::Budget::of(Level::Low), super::fx::Budget::of(Level::Lowest));
    assert!(low.entities() <= 60 && lowest.entities() <= 32, "{low:?} {lowest:?}");
    // In the game: a titan and everything it sets off at once, at Lowest, stays within the budget.
    for level in [Level::Low, Level::Lowest] {
        let mut app = fx_app(level);
        let world = app.world_mut();
        let kit = super::rig::Kit::make(world, body::Lod::of(level));
        let root = super::rig::spawn(world, &kit, 2, Vec3::ZERO, 0.);
        let parts = world.get::<super::rig::Rig>(root).unwrap().parts().len();
        let entities = world.query::<&Mesh3d>().iter(world).count();
        assert!(parts <= 30 && entities <= 26, "{parts} parts, {entities} meshes");
        world.resource_mut::<super::Titans>().drawn = Some((2, frame(), Transform::IDENTITY));
        let view = kitchen_sim::titan::View { titan: 2, hp: 100., max: 1000., ..Default::default() };
        let events: Vec<sim::Event> = (1..40)
            .map(|k| sim::Event { id: k, kind: Some([sim::EventKind::Smash, sim::EventKind::JumpLand, sim::EventKind::ChargeWall, sim::EventKind::Stomp, sim::EventKind::WeakHit, sim::EventKind::Hit][k as usize % 6]), p: [0.1 * k as f32, 0., 0.], ..Default::default() })
            .collect();
        super::fx::events(world, &view, &events, 1, 0., 0.);
        for _ in 0..120 {
            step(&mut app, 1. / 60.);
            let world = app.world_mut();
            world.resource_mut::<super::Titans>().drawn = Some((2, frame(), Transform::IDENTITY));
            let fx = world.resource::<super::fx::Fx>();
            assert!(fx.entities() <= super::fx::Budget::of(level).entities(), "{level:?}: {} effects' entities", fx.entities());
        }
    }
}
/// A frame of the titan standing, venting hard (all its steam).
fn frame() -> play::Frame {
    let p = anims::stance().vent(1.);
    play::Frame { pose: p, skeleton: solve(&p, None, 0.), quake: 0., footfalls: [None; 2], beat: 0. }
}

/// Online the server's `{type:'titan'}` message is the client's copy of the titan; any other is not.
#[test]
fn the_servers_word_is_heard() {
    let mut app = crate::arena::test_app("");
    let mut g = app.world_mut().resource_mut::<crate::arena::Game>();
    let message = serde_json::json!({"type": "titan", "titan": 3, "hp": 900.5, "max": 7000., "mv": 3, "started": 12.5, "events": [{"id": 4, "kind": "weak_hit", "to": 3}]});
    assert!(super::hear(&mut g, &message));
    let view = g.titan.clone().unwrap();
    assert_eq!((view.titan, view.mv(), view.events[0].kind), (3, Move::Smash, Some(sim::EventKind::WeakHit)));
    assert!(super::drawn_as_titan(&g, 3) && !super::drawn_as_titan(&g, 1));
    assert!(!super::hear(&mut g, &serde_json::json!({"type": "kungfu"})));
}

/// Every part of the titan faces outwards (its normals and its triangles), so it is drawn from outside and lit on the
/// side the light is on.
#[test]
fn every_part_faces_out() {
    let s = Shapes::new(Lod::Lean);
    let axis = |p: Vec3| Vec3::new(p.x, 0., p.z);
    let neck = body::NECK;
    let checks: Vec<(&str, &super::shape::Shape, Box<dyn Fn(Vec3) -> Vec3>)> = vec![
        ("torso", &s.torso, Box::new(move |p: Vec3| axis(p))),
        ("head", &s.head, Box::new(move |p: Vec3| axis(p))),
        ("maw", &s.maw, Box::new(move |p: Vec3| axis(p))),
        ("upper arm", &s.upper, Box::new(move |p: Vec3| axis(p))),

        ("thigh", &s.thigh, Box::new(move |p: Vec3| axis(p))),

        ("eye", &s.eye, Box::new(move |p: Vec3| p)),
    ];
    let _ = neck;
    // The bun's plates are shells round the body: their outer faces (the crust) face out.
    let outer: Vec<(Vec3, Vec3)> = s.bun.positions.iter().zip(&s.bun.normals).filter(|(p, _)| Vec2::new(p.x, p.z).length() > 0.37 && p.y < 0.5).map(|(p, n)| (*p, *n)).collect();
    let good = outer.iter().filter(|(p, n)| axis(*p).normalize().dot(*n) > 0.).count();
    assert!(outer.len() > 50 && good * 10 >= outer.len() * 8, "bun: {good} of {} outer normals face out", outer.len());
    for (name, shape, out) in checks {
        let total = shape.positions.len();
        let good = shape.positions.iter().zip(&shape.normals).filter(|(p, n)| out(**p).normalize_or_zero().dot(**n) > 0.).count();
        // (A limb's round ends, past its joints, face along it.)
        assert!(good * 10 >= total * 8, "{name}: {good} of {total} normals face out");
        let tris = shape.indices.chunks(3).filter(|t| {
            let [a, b, c] = [0, 1, 2].map(|k| shape.positions[t[k] as usize]);
            (b - a).cross(c - a).dot(out((a + b + c) / 3.)) > 0.
        });
        assert!(tris.count() * 10 >= shape.triangles() * 8, "{name}: its triangles face out");
    }
}

// ----- The client's plumbing for the titan: its command offline, its ghost online, its own prediction online.

fn titan_map() -> kitchen_sim::map::Map {
    kitchen_sim::map::Map::parse(include_str!("../../assets/maps/titans-kitchen.json")).unwrap()
}

/// Offline, the player who is the titan walks it with the stick through the round's own `Feast::titan_command` (at the
/// titan's pace, not a hotdog's), and its drawn titan (and its camera) go where the round has it.
#[test]
fn offline_the_titans_player_walks_its_giant_through_the_round() {
    use crate::arena::{Game, load_map, test_app, test_frame, tick};
    let mut app = test_app("sandbox mode=titan titan=me botmode=idle");
    app.add_systems(Update, (tick, super::animate).chain());
    load_map(app.world_mut(), titan_map());
    for _ in 0..3 {
        test_frame(&mut app, 1. / 30.);
    }
    let (me, start) = {
        let g = app.world().resource::<Game>();
        assert_eq!(super::titan_id(g), Some(g.me()), "this player is the titan");
        (g.me(), g.feast.player(g.me()).map(|p| Vec3::from_array(p.p)).unwrap())
    };
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyW);
    for _ in 0..30 {
        test_frame(&mut app, 1. / 30.);
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyW);
    }
    let g = app.world().resource::<Game>();
    let p = g.feast.player(me).unwrap();
    let walked = Vec2::new(p.p[0] - start.x, p.p[2] - start.z).length();
    assert!(walked > 0.5 * sim::WALK && walked < 1.1 * sim::WALK + 0.05, "it walked {walked} m in a second at the titan's pace");
    assert!(g.position.distance(Vec3::from_array(p.p)) < 1e-3, "the camera's place is the round's");
    assert_eq!(g.titan.as_ref().map(|v| v.mv()), Some(Move::Walk));
    let drawn = app.world().resource::<super::Titans>().drawn.map(|(_, _, root)| root.translation).unwrap();
    assert!(drawn.distance(Vec3::from_array(p.p)) < 1e-3, "the giant is drawn where it is");
}

/// Online, the titan's body in the client's world of bodies is a giant's: a small hotdog walking into it (predicted, as
/// the server will) is stopped at its footprint and does not walk through it.
#[test]
fn online_a_small_hotdog_is_stopped_by_the_titans_ghost() {
    use crate::arena::online::{Context, Predictor};
    use kitchen_sim::actor::{Actor, World};
    let mut feast = kitchen_sim::feast::Feast::with_map(titan_map(), false);
    let floor = kitchen_sim::feast::FLOOR;
    let (titan, me) = (2u32, 1u32);
    let at = Vec3::new(1.5, floor, -10.);
    let mut app = crate::arena::test_app("");
    let mut g = app.world_mut().resource_mut::<crate::arena::Game>();
    g.titan = Some(sim::View { titan, ..Default::default() });
    g.online.latest = vec![Player { id: titan, p: at.to_array(), alive: true, grounded: true, ..Default::default() }];
    let giant = super::ghost_of(&g, titan).expect("the titan's ghost");
    assert!((giant.radius - sim::RADIUS as f64).abs() < 1e-6 && giant.height > 1.5, "{} {}", giant.radius, giant.height);
    assert!(super::ghost_of(&g, me).is_none(), "a small hotdog's is its own");
    let mut ghosts = World::default();
    ghosts.add(giant);
    let start = at + Vec3::new(-1.2, 0., 0.);
    let mut body = Actor { radius: kitchen_sim::feast::HOTDOG_RADIUS, ..Actor::hotdog(me, start.as_dvec3().to_array()) };
    body.grounded = true;
    ghosts.add(body);
    let ctx = Context { me, dt: 1. / 60., stim: false, burden: 1., kungfu: None, titan: None };
    let mut predictor = Predictor::default();
    // Walking straight at it (+x) for three seconds.
    let yaw = -std::f32::consts::FRAC_PI_2;
    for seq in 1..=180 {
        let c = kitchen_sim::net::Command::new(seq, Vec2::new(0., -1.), yaw, 0., 0, 1, None);
        predictor.step(&mut feast, &mut ghosts, &c, &ctx);
    }
    let end = ghosts.get(me).unwrap().position();
    let gap = Vec2::new(end.x - at.x, end.z - at.z).length();
    assert!(end.x > start.x + 0.3, "it walked: {end}");
    assert!(gap > sim::RADIUS + kitchen_sim::feast::HOTDOG_RADIUS as f32 - 0.02, "it was stopped at the giant's footprint ({gap} m from its middle)");
}

/// Online, the titan's player predicts its own giant as the server moves it: over a scripted walk (turning as it goes)
/// and a charge, the prediction a few commands ahead of the server's word is where the server then puts it.
#[test]
fn online_the_titans_prediction_keeps_to_the_servers_giant() {
    use kitchen_sim::net::{Command, button};
    let mut server = kitchen_sim::feast::Feast::with_map(titan_map(), false);
    server.set_rules(kitchen_sim::rules::Rules::parse("sandbox mode=titan titan=me botmode=idle"));
    let id = server.titan_id().expect("a titan");
    let dt = 1. / 60.;
    // Out into the open, facing across the room.
    if let Some(p) = server.player_mut(id) {
        (p.p, p.yaw) = ([1.6, kitchen_sim::feast::FLOOR, -9.0], -std::f32::consts::FRAC_PI_2);
    }
    for _ in 0..30 {
        server.tick(dt);
    }
    // The script: walk ahead turning slowly; stop; then a charge (Shift) held to its end; then walk back.
    let mut commands: Vec<Command> = vec![];
    let mut yaw = server.player(id).unwrap().yaw;
    for seq in 1..=560u32 {
        let (stick, buttons) = match seq {
            1..=150 => {
                yaw += 0.004;
                (Vec2::new(0.2, -1.), 0)
            }
            151..=170 => (Vec2::ZERO, 0),
            171 => (Vec2::ZERO, button::STIM),
            172..=330 => (Vec2::ZERO, 0),
            331..=440 => (Vec2::new(0., 1.), 0),
            // A running jump: it takes off with the walk it had as it began the jump.
            441 => (Vec2::new(0., 1.), button::JUMP),
            _ => (Vec2::new(0., 1.), 0),
        };
        commands.push(Command::new(seq, stick, yaw, -0.2, buttons, 0, None));
    }
    #[derive(Clone)]
    struct Said {
        p: [f32; 3],
        yaw: f32,
        v: [f32; 3],
        grounded: bool,
        crouch: bool,
        view: sim::View,
        elapsed: f32,
    }
    let mut said: Vec<Said> = vec![];
    for c in &commands {
        server.titan_command(id, c, dt);
        server.tick(dt);
        let p = server.player(id).unwrap();
        said.push(Said { p: p.p, yaw: p.yaw, v: p.v, grounded: p.grounded, crouch: p.crouch, view: server.titan_view().unwrap(), elapsed: server.elapsed });
    }
    let charged = said.iter().any(|s| s.view.mv() == Move::Charge);
    assert!(charged, "the script charged");
    assert!(said.iter().any(|s| !s.grounded), "the script jumped");
    let footing = sim::Footing::of(&server.level);
    let lag = 8usize;
    let (mut worst_walk, mut worst_run, mut worst_air) = (0f32, 0f32, 0f32);
    for k in lag..commands.len() {
        let ack = k - lag;
        let s = &said[ack];
        // (In a jump, the walk it had while it last stood free: as the client keeps it, `Online::titan_run_up`.)
        let run_up = said[..=ack].iter().rev().find(|q| matches!(q.view.mv(), Move::Idle | Move::Walk)).map_or(Vec2::ZERO, |q| Vec2::new(q.v[0], q.v[2]));
        let walk = if s.view.mv() == Move::Jump { run_up } else { Vec2::new(s.v[0], s.v[2]) };
        let f = super::predict::Foresight { footing: footing.clone(), yaw: s.yaw, walk, mv: s.view.mv(), started: s.view.started, len: s.view.len, clock: (commands[ack].seq, s.elapsed) };
        // (As the server's word puts it, `Mine::onto`: where, on the ground or not, rising or falling how fast.)
        let mut body = sim::ghost(id, s.p, 0);
        (body.grounded, body.v[1]) = (s.grounded, s.v[1] as f64);
        for j in ack + 1..=k {
            f.step(&mut body, &commands[j], &commands[..j], dt);
        }
        let truth = Vec2::new(said[k].p[0], said[k].p[2]);
        let off = (Vec2::new(body.p[0] as f32, body.p[2] as f32) - truth).length();
        let up = (body.p[1] as f32 - said[k].p[1]).abs();
        if said[k].view.mv() == Move::Jump || s.view.mv() == Move::Jump {
            worst_air = worst_air.max(off).max(up);
        } else if said[k].view.mv() == Move::Charge || s.view.mv() == Move::Charge {
            worst_run = worst_run.max(off);
        } else {
            worst_walk = worst_walk.max(off);
        }
    }
    println!("prediction {lag} commands ahead: walking at worst {worst_walk} m off, charging {worst_run} m off, jumping {worst_air} m off");
    // (It takes off with the walk it had as it began the jump, `Online::titan_run_up`, and as its wind-up ends; its
    // rise is a step or two out at the take-off, eased as any correction.)
    assert!(worst_air < 0.08, "jumping, the prediction is {worst_air} m off the server's");
    assert!(worst_walk < 0.02, "walking, the prediction is {worst_walk} m off the server's");
    // (Into a wall the run stops: the one step the server slid along it first is all that differs.)
    assert!(worst_run < 0.05, "charging, the prediction is {worst_run} m off the server's");
}

/// A replay's seek (or a late word from the server) puts the titan into another move, or back into the same one at an
/// earlier second: the animation takes it up at once (no hit-stop or blend left over from where it was), whole.
#[test]
fn a_seek_takes_the_animation_up_at_once() {
    let mut a = Animator::default();
    let dt = 1. / 60.;
    let smash = anims::timing(Move::Smash, 0.);
    for k in 0..90 {
        a.update(&drive(Move::Smash, k as f32 * dt, smash), dt);
    }
    // Sought into a stagger halfway through, then back to the smash's wind-up.
    let stagger = anims::timing(Move::Stagger, 0.);
    for (mv, t, timing) in [(Move::Stagger, 1.5, stagger), (Move::Smash, 0.2, smash), (Move::Down, 2.4, anims::timing(Move::Down, 0.))] {
        let mut last = None;
        for k in 0..30 {
            let f = a.update(&drive(mv, t + k as f32 * dt, timing), dt);
            assert!(f.pose.finite(), "{mv:?}");
            last = Some(f);
        }
        let f = last.unwrap();
        let want = library().anim(mv).beat(t + 29. * dt, timing);
        assert!((f.beat - want).abs() < 0.02, "{mv:?}: at beat {} not {want}", f.beat);
    }
}

// ----- The simulation's figures, from the drawn titan (TI1).

/// How far (metres) a straight blend between two of a move's sampled figures may stray from the drawn figure between
/// them: the generator samples each move more finely wherever it moves fast or turns (its strikes), until it is within it.
const SAMPLE_FIT: f32 = 0.006;
/// The drawn head's radius is kept at HEAD_ROWS + 1 even steps up it.
const HEAD_ROWS: usize = 160;
/// One drawn pose as the simulation keeps it (kitchen_sim::titan `Frame`, in hotdog measures: metres / SCALE, the titan
/// at the origin facing -z): its skeleton's torso axis (from the drawn torso's bottom, inset by the simulation's body
/// radius, to the waist, `sim::WAIST` up it, and on to its top, inset by the chest's radius), shoulders, elbows, wrists,
/// hips, knees, ankles (left, right) and neck; the pose's own turn of the head (looking level); the chest's turn; and
/// what the pose places its limbs by (pose.rs `solve`): each hand's target and elbow's hint in the chest's frame, each
/// foot's target and knee's hint in the figure's.
fn figure_of(pose: &pose::Pose) -> sim::Frame {
    let s = solve(pose, None, 0.);
    let scale = super::rig::scale();
    let k = scale / sim::SCALE;
    let points = [
        s.axis(body::BOTTOM + sim::TORSO_RADIUS / scale),
        s.axis(sim::WAIST / scale),
        s.axis(body::TORSO_TOP - sim::CHEST_RADIUS / scale),
        s.shoulder[L],
        s.shoulder[R],
        s.elbow[L],
        s.elbow[R],
        s.wrist[L],
        s.wrist[R],
        s.hip[L],
        s.hip[R],
        s.knee[L],
        s.knee[R],
        s.ankle[L],
        s.ankle[R],
        s.head.translation,
    ];
    let limb = |i: usize, v: Vec3| pose::limb_frame(i, v);
    // (The feet's targets and the knees' hints are in the figure's frame: spun with the whole body, as solve spins it.)
    let (turn, about) = (pose::spin_rotation(pose.spin), pose::body_frame(pose.pivot));
    let spun = |at: Vec3| about + turn * (at - about);
    let h = [
        limb(L, pose.arms[L].hand) * k,
        limb(R, pose.arms[R].hand) * k,
        limb(L, pose.arms[L].elbow),
        limb(R, pose.arms[R].elbow),
        spun(limb(L, pose.legs[L].foot)) * k,
        spun(limb(R, pose.legs[R].foot)) * k,
        turn * limb(L, pose.legs[L].knee),
        turn * limb(R, pose.legs[R].knee),
    ];
    sim::Frame { p: points.map(|q| (q * k).to_array()), turn: s.head.rotation.normalize().to_array(), chest: s.torso.rotation.normalize().to_array(), h: h.map(|v| v.to_array()) }
}
/// A figure as the simulation's source has it.
fn frame_text(f: &sim::Frame) -> String {
    let list = |v: &[[f32; 3]], places: usize| v.iter().map(|q| format!("[{:.places$}, {:.places$}, {:.places$}]", q[0], q[1], q[2])).collect::<Vec<_>>().join(", ");
    let quat = |q: [f32; 4]| format!("[{:.6}, {:.6}, {:.6}, {:.6}]", q[0], q[1], q[2], q[3]);
    format!("Frame {{ p: [{}], turn: {}, chest: {}, h: [{}] }}", list(&f.p, 5), quat(f.turn), quat(f.chest), list(&f.h, 5))
}
/// How far the simulation's blend `u` of the way from figure `a` to `b` (its `Frame::lerp`) is from figure `c`: its
/// points (metres), and its head's turn as far as it carries the head's surface.
fn blend_gap(a: &sim::Frame, b: &sim::Frame, u: f32, c: &sim::Frame) -> f32 {
    let f = a.lerp(b, u);
    let points = (0..16).map(|i| Vec3::from_array(f.p[i]).distance(Vec3::from_array(c.p[i])) * sim::SCALE).fold(0f32, f32::max);
    points.max(Quat::from_array(f.turn).angle_between(Quat::from_array(c.turn)) * sim::HEAD_RADIUS)
}
/// The points from `from` to `to` (in `pieces` to start with) to sample `figure` at, so that a straight blend between
/// neighbours is within SAMPLE_FIT of it.
fn sample_points(figure: &dyn Fn(f32) -> sim::Frame, from: f32, to: f32, pieces: usize) -> Vec<f32> {
    fn split(figure: &dyn Fn(f32) -> sim::Frame, a: f32, b: f32, out: &mut Vec<f32>) {
        let (fa, fb) = (figure(a), figure(b));
        let off = [0.125f32, 0.25, 0.375, 0.5, 0.625, 0.75, 0.875].iter().map(|&u| blend_gap(&fa, &fb, u, &figure(a + (b - a) * u))).fold(0f32, f32::max);
        if off > SAMPLE_FIT && b - a > 1. / 1024. {
            let mid = 0.5 * (a + b);
            split(figure, a, mid, out);
            split(figure, mid, b, out);
        } else {
            out.push(b);
        }
    }
    let mut out = vec![from];
    let at = |k: usize| from + (to - from) * k as f32 / pieces as f32;
    for k in 0..pieces {
        split(figure, at(k), at(k + 1), &mut out);
    }
    out
}
/// Writes crates/kitchen-sim/src/titan_figures.rs: the three stances and every move's figures over its beats (sampled
/// finely enough, `sample_beats`), each move's hit-stop and each looping move's loop, from the drawn titan, so that what
/// a player sees of the giant is what its rounds hit. Run it (it is ignored) after changing the drawn titan's poses:
/// `cargo test --release ... titan::tests::generate -- --ignored`.
#[test]
#[ignore]
fn generate_the_simulations_figures() {
    use std::fmt::Write;
    let mut out = String::new();
    out += "// SPDX-License-Identifier: GPL-3.0-only\n// Copyright (C) 2026 David Nunes-Childs\n";
    out += "//! The titan's figures as it is drawn: generated by the client's test `titan::tests::generate_the_simulations_figures`\n";
    out += "//! (src/titan/tests.rs) from src/titan's poses. Do not edit by hand: run that test again after changing the drawn\n";
    out += "//! titan, and the client's `the_drawn_body_is_where_the_simulation_hits_it` keeps the two together.\n";
    out += "use super::Frame;\n\n";
    for (name, pose) in [("STANDING", anims::stance()), ("CROUCHED", anims::crouched()), ("KNEELING", anims::kneeling())] {
        let _ = writeln!(out, "pub(crate) static {name}: Frame = {};", frame_text(&figure_of(&pose)));
    }
    out += "/// Each move's figures over its beats, as (beat, figure), by its number (the stance moves, Idle and Walk, are the\n";
    out += "/// standing figure).\n";
    let _ = writeln!(out, "pub(crate) static MOVES: [&[(f32, Frame)]; {}] = [", Move::ALL.len());
    let (mut stops, mut loops, mut count) = (vec![], vec![], 0);
    for mv in Move::ALL {
        let a = library().anim(mv);
        let stance = matches!(mv, Move::Idle | Move::Walk);
        let figure = |beat: f32| figure_of(&if stance { anims::stance() } else { a.pose_at(beat) });
        let beats = if stance { vec![0., 3.] } else { sample_points(&figure, 0., 3., 12) };
        count += beats.len();
        let _ = writeln!(out, "    // {mv:?}\n    &[");
        for b in beats {
            let _ = writeln!(out, "        ({b:.7}, {}),", frame_text(&figure(b)));
        }
        out += "    ],\n";
        let once = a.cycle == anims::Cycle::Once;
        let contact = if a.blow == anims::Blow::Land { 2. } else { 1. };
        stops.push(if !stance && once && a.strike.is_some() && a.hitstop > 0. { (contact, a.hitstop) } else { (0., 0.) });
        loops.push(match a.cycle {
            anims::Cycle::Loop(seconds) if !stance => seconds,
            _ => 0.,
        });
    }
    out += "];\n";
    out += "/// Each move's hit-stop as drawn: the beat its blow lands on, and the seconds the drawn body is held there (0: none).\n";
    let _ = writeln!(out, "pub(crate) static HITSTOP: [(f32, f32); {}] = [{}];", Move::ALL.len(), stops.iter().map(|(b, s)| format!("({b:.1}, {s:.3})")).collect::<Vec<_>>().join(", "));
    // Crouching down and standing up: the drawn titan blends its stance into its crouch (play.rs, standing still).
    out += "/// The figures from standing (0) to crouched (1), as the drawn titan blends between them.\n";
    let crouching = |c: f32| figure_of(&anims::stance().lerp(&anims::crawl(0., 0.), c));
    let amounts = sample_points(&crouching, 0., 1., 4);
    count += amounts.len();
    out += "pub(crate) static CROUCHING: &[(f32, Frame)] = &[\n";
    for c in amounts {
        let _ = writeln!(out, "    ({c:.7}, {}),", frame_text(&crouching(c)));
    }
    out += "];\n";
    // The drawn head (a round body about its upright from the neck: `head_radius`), and the weak spot as it is seen
    // (body.rs `spot`) from the head's middle, in the head's own frame.
    let scale = super::rig::scale();
    let k = scale / sim::SCALE;
    let (low, high) = (-0.08f32, body::TOP - body::NECK.y);
    let rows: Vec<f32> = (0..=HEAD_ROWS).map(|i| body::head_radius(low + (high - low) * i as f32 / HEAD_ROWS as f32)).collect();
    out += "/// The drawn head: from HEAD_LOW to HEAD_HIGH up its upright from the neck, its radius at even steps, and its widest.\n";
    let _ = writeln!(out, "pub(crate) const HEAD_LOW: f32 = {:.6};\npub(crate) const HEAD_HIGH: f32 = {:.6};", low * k, high * k);
    let _ = writeln!(out, "pub(crate) const HEAD_WIDEST: f32 = {:.6};", rows.iter().fold(0f32, |a, b| a.max(*b)) * k);
    let _ = writeln!(out, "pub(crate) static HEAD: [f32; {}] = [{}];", rows.len(), rows.iter().map(|r| format!("{:.6}", r * k)).collect::<Vec<_>>().join(", "));
    let spot = (body::spot() - body::cap_centre()) * k;
    out += "/// Where the weak spot is seen and aimed at, from the head's middle, in the head's own frame (`Shape::spot`).\n";
    let _ = writeln!(out, "pub(crate) const SPOT: [f32; 3] = [{:.6}, {:.6}, {:.6}];", spot.x, spot.y, spot.z);
    // The gaits over a whole cycle (two steps), at their pace: the walk, the crouched crawl and the charge's run (the
    // walk is drawn from the round's stride, play.rs); the tuck in the air; each gait's cycle; the charge's run.
    let tau = std::f32::consts::TAU;
    for (name, gait) in [("WALK", &(|φ: f32| anims::walk(φ, 1.)) as &dyn Fn(f32) -> pose::Pose), ("CRAWL", &|φ: f32| anims::crawl(φ, 1.)), ("RUN", &|φ: f32| anims::run(φ))] {
        let figure = |φ: f32| figure_of(&gait(φ));
        let at = sample_points(&figure, 0., tau, 16);
        count += at.len();
        let _ = writeln!(out, "/// The {} over its cycle (phase radians, figure).\npub(crate) static {name}: &[(f32, Frame)] = &[", name.to_lowercase());
        for φ in at {
            let _ = writeln!(out, "    ({φ:.7}, {}),", frame_text(&figure(φ)));
        }
        out += "];\n";
    }
    let _ = writeln!(out, "/// Standing off the ground: its legs tucked (the jump's).\npub(crate) static TUCK: Frame = {};", frame_text(&figure_of(&library().anim(Move::Jump).pose_at(1.5))));
    let cycle = |crouched, running| anims::cycle_length(crouched, running) * k;
    let _ = writeln!(out, "/// Each gait's cycle (walk, crawl, run): what the body covers in one.\npub(crate) const CYCLES: (f32, f32, f32) = ({:.6}, {:.6}, {:.6});", cycle(false, false), cycle(true, false), cycle(false, true));
    let _ = writeln!(out, "/// The chest turns about this far above the hips' middle; the arms' and legs' bones (upper, fore, thigh, shin); the
/// elbows' and knees' fallback hints (pose.rs `solve`).\npub(crate) const PIVOT_ABOVE_HIPS: f32 = {:.6};", (body::PIVOT.y - body::HIP.y) * k);
    let _ = writeln!(out, "pub(crate) const BONES: (f32, f32, f32, f32) = ({:.6}, {:.6}, {:.6}, {:.6});", body::UPPER * k, body::FORE * k, body::THIGH * k, body::SHIN * k);
    let (elbow_out, knee_out) = pose::FALLBACKS;
    let _ = writeln!(out, "pub(crate) const ELBOW_OUT: [f32; 3] = [{:.4}, {:.4}, {:.4}];\npub(crate) const KNEE_OUT: [f32; 3] = [{:.4}, {:.4}, {:.4}];", elbow_out.x, elbow_out.y, elbow_out.z, knee_out.x, knee_out.y, knee_out.z);
    let (from, to) = library().anim(Move::Charge).running.unwrap();
    let _ = writeln!(out, "/// The charge's run: from beat to beat.\npub(crate) const CHARGE_RUN: (f32, f32) = ({from:.4}, {to:.4});");
    out += "/// Each looping move's loop (seconds for its three beats; 0: it plays once, by its timing).\n";
    let _ = writeln!(out, "pub(crate) static LOOPS: [f32; {}] = [{}];", Move::ALL.len(), loops.iter().map(|s| format!("{s:.3}")).collect::<Vec<_>>().join(", "));
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/crates/kitchen-sim/src/titan_figures.rs");
    std::fs::write(path, out).unwrap();
    println!("wrote {path}: {count} figures");
}
