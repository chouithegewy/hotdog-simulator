// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
use super::people::{self, Grasp, Person, Piece};
use super::shapes::{self, Builder, Fine, Viseme};
use super::*;

fn sound(b: &Builder, what: &str) {
    assert!(b.triangles() > 0, "{what} has no triangles");
    assert_eq!(b.positions.len(), b.normals.len(), "{what}");
    assert_eq!(b.positions.len(), b.uvs.len(), "{what}");
    assert_eq!(b.positions.len(), b.colors.len(), "{what}");
    for (p, n) in b.positions.iter().zip(&b.normals) {
        assert!(p.is_finite() && p.length() < 2.5, "{what}: a vertex at {p}");
        assert!((n.length() - 1.).abs() < 1e-3, "{what}: a normal {n}");
    }
    assert!(b.indices.iter().all(|i| (*i as usize) < b.positions.len()), "{what}: an index out of range");
    // Every triangle faces the way its corners' normals do (no inside-out patches).
    for t in b.indices.chunks(3) {
        let [a, c, d] = [t[0], t[1], t[2]].map(|i| i as usize);
        let face = (b.positions[c] - b.positions[a]).cross(b.positions[d] - b.positions[a]);
        let out = b.normals[a] + b.normals[c] + b.normals[d];
        assert!(face.dot(out) >= -1e-12, "{what}: a triangle turned inside out");
    }
}

#[test]
fn every_piece_of_both_people_is_a_sound_mesh_in_both_sets() {
    for person in Person::ALL {
        for fine in [Fine::DETAILED, Fine::LEAN] {
            for (piece, _) in person.pieces() {
                sound(&people::piece(person, piece, fine), &format!("{person:?} {piece:?} {fine:?}"));
            }
        }
    }
}

#[test]
fn the_people_are_light_and_lighter_at_low_detail() {
    for person in Person::ALL {
        let detailed = Cast::triangles(person, Fine::DETAILED);
        let lean = Cast::triangles(person, Fine::LEAN);
        eprintln!("{person:?}: {detailed} triangles detailed, {lean} lean");
        assert!(detailed < 60_000, "{person:?}: {detailed} triangles");
        // Giants next to 15 cm hotdogs: Maya about 1.63 m to the top of her head, her husband taller.
        assert!((1.55..1.95).contains(&person.figure().height()), "{person:?}");
        assert!(lean * 5 < detailed * 2, "{person:?}: the lean set ({lean}) is not much leaner than the detailed one ({detailed})");
    }
}

#[test]
fn the_cast_remakes_its_meshes_in_place_for_the_graphics_level() {
    let mut meshes = Assets::<Mesh>::default();
    let mut materials = Assets::<StandardMaterial>::default();
    let mut images = Assets::<Image>::default();
    let mut cast = Cast::new(&mut meshes, &mut materials, &mut images, true);
    // Three cloth patterns, painted here; nothing loaded.
    assert_eq!(images.len(), 3);
    let handle = cast.mesh(Person::Maya, Piece::Head);
    let count = |meshes: &Assets<Mesh>| meshes.get(&handle).unwrap().indices().unwrap().len();
    let detailed = count(&meshes);
    assert!(!cast.refine(&mut meshes, true), "already detailed");
    assert!(cast.refine(&mut meshes, false));
    assert!(count(&meshes) * 2 < detailed);
    assert_eq!(cast.mesh(Person::Maya, Piece::Head), handle, "the same handle, remade");
    assert!(cast.refine(&mut meshes, true));
    assert_eq!(count(&meshes), detailed);
}

#[test]
fn a_spawned_person_has_every_part_of_their_rig() {
    let mut world = World::new();
    let mut meshes = Assets::<Mesh>::default();
    let mut materials = Assets::<StandardMaterial>::default();
    let mut images = Assets::<Image>::default();
    let cast = Cast::new(&mut meshes, &mut materials, &mut images, false);
    let roots = Person::ALL.map(|person| spawn(&mut world.commands(), &cast, person, Transform::from_xyz(1., 0., 2.)));
    world.flush();
    for (root, person) in roots.into_iter().zip(Person::ALL) {
        let rig = world.get::<Rig>(root).unwrap().clone();
        assert_eq!(rig.person, person);
        assert!(world.get::<Human>(root).is_some());
        let mut drawn = 0;
        let mut stack = vec![root];
        while let Some(e) = stack.pop() {
            if world.get::<Mesh3d>(e).is_some() {
                drawn += 1;
            }
            if let Some(children) = world.get::<Children>(e) {
                stack.extend(children.iter());
            }
        }
        // Every piece but the mouth's other poses is drawn by an entity of its own (hands three ways, limbs twice).
        let pieces = person.pieces().iter().filter(|(p, _)| !matches!(p, Piece::Mouth(v) if *v != Viseme::Rest)).count();
        let doubled = 6; // upper arm, forearm, thigh, shin, foot and eye: one each side
        assert_eq!(drawn, pieces + doubled, "{person:?}");
    }
}

#[test]
fn a_gripping_hand_closes_round_its_handle() {
    for grasp in [Grasp::Grip, Grasp::Shake] {
        let curl = grasp.curl();
        let centre = shapes::grip_point(curl);
        // In front of the palm (its -x side), below the knuckles, on the knuckles' line across.
        assert!(centre.x < -0.005 && centre.y < -0.07 && centre.y > -0.14 && centre.z == 0., "{grasp:?}: {centre}");
    }
    let centre = shapes::grip_point(shapes::GRIP);
    // The middle finger's joints are all about one radius from the handle's middle: the finger wraps it.
    let hand = shapes::hand(shapes::GRIP, Fine::DETAILED, shapes::WHITE);
    let near = hand.positions.iter().filter(|p| p.z.abs() < 0.004).map(|p| p.distance(centre)).fold(f32::MAX, f32::min);
    assert!(near > 0.004 && near < 0.03, "the fingers are {near} m from the handle");
}

#[test]
fn hand_turns_point_the_fingers_and_the_palm() {
    let fingers = Vec3::new(0.3, -0.4, -1.).normalize();
    let palm = Vec3::new(-1., 0.2, 0.).normalize();
    for side in [0, 1] {
        let q = hand_turn(side, fingers, palm);
        assert!((q.length() - 1.).abs() < 1e-4);
        assert!((q * Vec3::NEG_Y).dot(fingers) > 0.999);
        let palm_axis = if side == 1 { Vec3::NEG_X } else { Vec3::X };
        let p = q * palm_axis;
        let square = (palm - fingers * palm.dot(fingers)).normalize();
        assert!(p.dot(fingers).abs() < 1e-4 && p.dot(square) > 0.999, "{side}: {p}");
        // A handle's axis runs through the closed hand along its z.
        let g = grip_turn(side, Vec3::X, Vec3::NEG_Y);
        assert!((g * Vec3::Z).dot(Vec3::X) > 0.999 && (g * palm_axis).dot(Vec3::NEG_Y) > 0.999);
    }
}

#[test]
fn a_pose_puts_the_hands_and_feet_where_it_asks() {
    for person in Person::ALL {
        let fig = person.figure();
        let mut pose = Pose::rest(fig);
        let solved = solve(person, &pose);
        assert!(solved.short.iter().all(|s| *s < 1e-3), "{person:?} at rest: {:?}", solved.short);
        // A hand out in front at chest height, and one beyond reach.
        pose.hands[1].at = Vec3::new(0.15, fig.hip + 0.3, -0.35);
        pose.hands[0].at = Vec3::new(-2., 1., -2.);
        let solved = solve(person, &pose);
        assert!(solved.short[1] < 1e-3);
        assert!(solved.short[0] > 1.);
        assert!((solved.hands[1].translation - pose.hands[1].at).length() < 1e-3);
        // The arm's segments join: the forearm starts where the upper arm ends.
        for side in 0..2 {
            let elbow = solved.limbs[side][0].transform_point(Vec3::NEG_Y * fig.upper_arm);
            assert!(elbow.distance(solved.limbs[side][1].translation) < 1e-4);
            let knee = solved.limbs[side][2].transform_point(Vec3::NEG_Y * fig.thigh);
            assert!(knee.distance(solved.limbs[side][3].translation) < 1e-4);
        }
    }
}

#[test]
fn the_mouth_takes_six_shapes() {
    let fig = Person::Maya.figure();
    let size = |v: Viseme| {
        let b = people::piece(Person::Maya, Piece::Mouth(v), Fine::DETAILED);
        let (lo, hi) = b.positions.iter().fold((Vec3::MAX, Vec3::MIN), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
        hi - lo
    };
    let rest = size(Viseme::Rest);
    eprintln!("mouth sizes: rest {rest}, ah {}, ee {}, oh {}, mm {}", size(Viseme::Ah), size(Viseme::Ee), size(Viseme::Oh), size(Viseme::Mm));
    assert!(size(Viseme::Ah).y > rest.y * 1.6, "Ah opens");
    assert!(size(Viseme::Oh).x < size(Viseme::Ee).x * 0.7, "Oh is narrower than Ee");
    assert!(size(Viseme::Mm).y < size(Viseme::Ah).y * 0.5, "Mm is closed");
    // On the face, below the nose, on the head's surface.
    let b = people::piece(Person::Maya, Piece::Mouth(Viseme::Ah), Fine::DETAILED);
    for p in &b.positions {
        let local = (*p - fig.head) / fig.head_radii;
        assert!((local.length() - 1.).abs() < 0.08 && p.z < fig.head.z && p.y < fig.head.y, "{p}");
    }
}

#[test]
fn the_idle_host_moves_and_talks() {
    let a = idle_pose(Person::Husband, 1.);
    let b = idle_pose(Person::Husband, 2.3);
    assert_ne!(a.hands[0].at, b.hands[0].at);
    let mouths: std::collections::HashSet<Viseme> = (0..400).map(|i| idle_pose(Person::Husband, i as f32 * 0.02).mouth).collect();
    assert!(mouths.len() >= 4);
    assert!(solve(Person::Husband, &a).short.iter().all(|s| *s < 0.01));
}
