// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The kitchen's people: Maya, the breakfast chef, and her husband (`people`), made entirely here at load, from
//! shapes generated in code (`shapes`) and coloured by their vertices, with three small cloth patterns painted in code.
//! No model file, no photograph, no texture is loaded for them.
//!
//! * **The cast** ([`Cast`]): every piece's mesh, shared by whoever wears it, and the few materials they are drawn with.
//!   The meshes follow the graphics level: the detailed set, or at Low and Lowest (`quality::Detail::lean`) a lean one
//!   with about a quarter of the triangles, remade in place ([`follow_detail`]).
//! * **The rig** ([`Rig`]): a person is a root (where they stand and which way they face), a pelvis, a chest that bends
//!   at the waist and a head that nods and turns on the neck, each carrying its pieces; and arms and legs whose
//!   segments are placed every frame between joints found by two-bone reaching ([`solve`]) to where the hands and feet
//!   are asked to be, so that a hand that holds something is exactly where the thing is. Eyes look and blink, the mouth
//!   takes one of six shapes ([`Viseme`]), and each hand is open or gripping.
//! * **The pose** ([`Pose`]): all a frame asks of a person, in their own frame. The breakfast scene's choreography makes
//!   them (`maya::act`); [`idle`] makes the `counter-map` scene's host's.
mod people;
pub mod shapes;

use bevy::{light::NotShadowCaster, prelude::*};
use std::collections::HashMap;

pub use people::{Figure, Grasp, Person, Piece};
pub use shapes::{Fine, Viseme};
use people::Finish;

/// Every person; the round hides them when it starts (`arena`).
#[derive(Component)]
pub struct Human;

/// The pieces' meshes and the materials, made once.
#[derive(Resource)]
pub struct Cast {
    meshes: HashMap<(Person, Piece), Handle<Mesh>>,
    materials: HashMap<Finish, Handle<StandardMaterial>>,
    detailed: bool,
}

impl Cast {
    pub fn new(meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>, images: &mut Assets<Image>, detailed: bool) -> Cast {
        let fine = Fine::of(detailed);
        let mut made = HashMap::new();
        for person in Person::ALL {
            for (piece, _) in person.pieces() {
                made.insert((person, piece), meshes.add(people::piece(person, piece, fine).mesh()));
            }
        }
        // (Texture coordinates are in metres: a tile of check is 4.5 cm, of knit 6 cm, of corduroy 3 cm.)
        let cloth = |materials: &mut Assets<StandardMaterial>, texture: Option<Handle<Image>>, roughness: f32, tile: f32| {
            materials.add(StandardMaterial {
                base_color_texture: texture,
                perceptual_roughness: roughness,
                reflectance: 0.3,
                uv_transform: bevy::math::Affine2::from_scale(Vec2::splat(1. / tile)),
                ..default()
            })
        };
        let finishes = [
            (Finish::Matte, cloth(materials, None, 0.86, 1.)),
            (
                Finish::Sheet,
                materials.add(StandardMaterial { perceptual_roughness: 0.86, reflectance: 0.3, cull_mode: None, double_sided: true, ..default() }),
            ),
            (Finish::Skin, materials.add(StandardMaterial { perceptual_roughness: 0.52, reflectance: 0.4, ..default() })),
            (Finish::Glossy, materials.add(StandardMaterial { perceptual_roughness: 0.22, reflectance: 0.5, ..default() })),
            (Finish::Check, cloth(materials, Some(images.add(people::tiles::check())), 0.8, 0.045)),
            (Finish::Knit, cloth(materials, Some(images.add(people::tiles::knit())), 0.9, 0.06)),
            (Finish::Cord, cloth(materials, Some(images.add(people::tiles::cord())), 0.85, 0.03)),
        ];
        Cast { meshes: made, materials: finishes.into_iter().collect(), detailed }
    }
    pub fn mesh(&self, person: Person, piece: Piece) -> Handle<Mesh> {
        self.meshes[&(person, piece)].clone()
    }
    fn material(&self, person: Person, piece: Piece) -> Handle<StandardMaterial> {
        let finish = person.pieces().into_iter().find(|(p, _)| *p == piece).map_or(Finish::Matte, |(_, f)| f);
        self.materials[&finish].clone()
    }
    /// Remakes every piece's mesh in place, as finely as `detailed` says. Returns whether anything changed.
    pub fn refine(&mut self, meshes: &mut Assets<Mesh>, detailed: bool) -> bool {
        if detailed == self.detailed {
            return false;
        }
        self.detailed = detailed;
        let fine = Fine::of(detailed);
        for ((person, piece), handle) in &self.meshes {
            let _ = meshes.insert(handle, people::piece(*person, *piece, fine).mesh());
        }
        true
    }
    pub fn detailed(&self) -> bool {
        self.detailed
    }
    /// How many triangles a person is made of (with one of each hand and the resting mouth).
    #[cfg(test)]
    pub fn triangles(person: Person, fine: Fine) -> usize {
        person
            .pieces()
            .into_iter()
            .filter(|(piece, _)| match piece {
                Piece::Mouth(v) => *v == Viseme::Rest,
                Piece::Hand { grasp, .. } => *grasp == Grasp::Open,
                _ => true,
            })
            .map(|(piece, _)| {
                let n = people::piece(person, piece, fine).triangles();
                match piece {
                    Piece::UpperArm | Piece::Forearm | Piece::Thigh | Piece::Shin | Piece::Foot | Piece::Eye => n * 2,
                    _ => n,
                }
            })
            .sum()
    }
}

/// Keeps the people's meshes as fine as the graphics level says (`quality::Detail::lean`).
pub fn follow_detail(detail: Option<Res<crate::quality::Detail>>, cast: Option<ResMut<Cast>>, mut meshes: ResMut<Assets<Mesh>>) {
    let (Some(detail), Some(mut cast)) = (detail, cast) else {
        return;
    };
    if cast.detailed() == detail.lean {
        cast.refine(&mut meshes, !detail.lean);
    }
}

/// The entities of one person that a pose moves.
#[derive(Component, Clone)]
pub struct Rig {
    pub person: Person,
    pub pelvis: Entity,
    pub chest: Entity,
    pub head: Entity,
    apron: Option<Entity>,
    /// Upper arm, forearm, thigh and shin, left then right.
    limbs: [[Entity; 4]; 2],
    /// Each hand's holder, and its shapes (`Grasp::ALL`).
    hands: [(Entity, [Entity; 3]); 2],
    feet: [Entity; 2],
    eyes: [Entity; 2],
    mouth: Entity,
}

/// Puts a person in the world at `place` (their root: feet on the floor, facing along its -z), at rest. Returns the
/// root, which carries [`Human`] and the [`Rig`].
pub fn spawn(commands: &mut Commands, cast: &Cast, person: Person, place: Transform) -> Entity {
    let fig = person.figure();
    let part = |commands: &mut Commands, parent: Entity, piece: Piece, at: Transform| {
        let entity = commands
            .spawn((Mesh3d(cast.mesh(person, piece)), MeshMaterial3d(cast.material(person, piece)), at, Visibility::Inherited))
            .id();
        commands.entity(parent).add_child(entity);
        entity
    };
    let joint = |commands: &mut Commands, parent: Entity, at: Vec3| {
        let entity = commands.spawn((Transform::from_translation(at), Visibility::Inherited)).id();
        commands.entity(parent).add_child(entity);
        entity
    };
    let root = commands.spawn((Name::new(format!("{person:?}")), Human, place, Visibility::Visible)).id();
    let pelvis = joint(commands, root, Vec3::Y * fig.hip);
    part(commands, pelvis, Piece::Pelvis, Transform::IDENTITY);
    let apron = person.pieces().iter().any(|(p, _)| *p == Piece::Apron).then(|| part(commands, pelvis, Piece::Apron, Transform::IDENTITY));
    let chest = joint(commands, pelvis, Vec3::Y * fig.waist);
    part(commands, chest, Piece::Torso, Transform::IDENTITY);
    part(commands, chest, Piece::Trim, Transform::IDENTITY);
    let head = joint(commands, chest, Vec3::Y * fig.neck);
    part(commands, head, Piece::Head, Transform::IDENTITY);
    for piece in [Piece::Hat, Piece::Glasses] {
        if person.pieces().iter().any(|(p, _)| *p == piece) {
            part(commands, head, piece, Transform::IDENTITY);
        }
    }
    let eyes = people::eye_places(fig).map(|(at, normal)| {
        let eye = part(commands, head, Piece::Eye, Transform::from_translation(at).with_rotation(Quat::from_rotation_arc(Vec3::NEG_Z, normal)));
        commands.entity(eye).insert(NotShadowCaster);
        eye
    });
    let mouth = part(commands, head, Piece::Mouth(Viseme::Rest), Transform::IDENTITY);
    commands.entity(mouth).insert(NotShadowCaster);
    let limbs = [0, 1].map(|_| [Piece::UpperArm, Piece::Forearm, Piece::Thigh, Piece::Shin].map(|piece| part(commands, root, piece, Transform::IDENTITY)));
    let hands = [true, false].map(|left| {
        let holder = joint(commands, root, Vec3::ZERO);
        let shapes = Grasp::ALL.map(|grasp| {
            let shape = part(commands, holder, Piece::Hand { grasp, left }, Transform::IDENTITY);
            if grasp != Grasp::Open {
                commands.entity(shape).insert(Visibility::Hidden);
            }
            shape
        });
        (holder, shapes)
    });
    let feet = [0, 1].map(|_| part(commands, root, Piece::Foot, Transform::IDENTITY));
    let rig = Rig { person, pelvis, chest, head, apron, limbs, hands, feet, eyes, mouth };
    commands.entity(root).insert(rig);
    root
}

/// One hand's goal, in the person's frame: where its wrist goes, how it is turned, and how it is closed.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Hand {
    pub at: Vec3,
    pub turn: Quat,
    pub grasp: Grasp,
    /// How far the elbow is raised out to the side (0 hanging down and back, 1 up and out, as for a chopping knife).
    pub lift: f32,
}
/// One foot's goal, in the person's frame: the ankle's place, its turn about the vertical, how far its toes tip up.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Foot {
    pub at: Vec3,
    pub yaw: f32,
    pub pitch: f32,
}

/// All a frame asks of a person, in their own frame (x right, y up, -z ahead; left side first).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Pose {
    /// How far the pelvis is moved from rest (down when bending or walking, sideways with the gait).
    pub hips: Vec3,
    /// The hips' roll with the gait.
    pub roll: f32,
    /// The upper body's lean forward at the waist and its twist, radians.
    pub bend: f32,
    pub twist: f32,
    /// The head's nod (negative looks down) and turn (positive to the left).
    pub nod: f32,
    pub turn: f32,
    pub hands: [Hand; 2],
    pub feet: [Foot; 2],
    /// 0 eyes open, 1 shut.
    pub blink: f32,
    /// What the eyes look at, in the person's frame.
    pub look: Option<Vec3>,
    pub mouth: Viseme,
}

impl Pose {
    /// Standing at rest: arms down, feet together, looking ahead.
    pub fn rest(fig: &Figure) -> Pose {
        let foot = fig.foot();
        Pose {
            hips: Vec3::ZERO,
            roll: 0.,
            bend: 0.,
            twist: 0.,
            nod: 0.,
            turn: 0.,
            hands: [0, 1].map(|side| {
                let sign = if side == 0 { -1. } else { 1. };
                let at = fig.rest_hand * Vec3::new(sign, 1., 1.);
                Hand { at, turn: hand_turn(side, Vec3::new(0., -1., -0.12), Vec3::new(-sign, 0., -0.1)), grasp: Grasp::Open, lift: 0. }
            }),
            feet: [Foot { at: foot * Vec3::new(-1., 1., 1.), ..default() }, Foot { at: foot, ..default() }],
            blink: 0.,
            look: None,
            mouth: Viseme::Rest,
        }
    }
}

/// A hand's turn with its fingers along `fingers` and its palm facing `palm` (side 0 the left hand, 1 the right).
pub fn hand_turn(side: usize, fingers: Vec3, palm: Vec3) -> Quat {
    let y = -fingers.normalize_or(Vec3::NEG_Y);
    let across = if side == 1 { -palm } else { palm };
    let x = (across - y * across.dot(y)).normalize_or(y.any_orthonormal_vector());
    Quat::from_mat3(&Mat3::from_cols(x, y, x.cross(y)))
}
/// A gripping hand's turn round a handle along `axis` (the thumb towards `-axis`: towards a knife's blade), its palm
/// facing `palm`.
pub fn grip_turn(side: usize, axis: Vec3, palm: Vec3) -> Quat {
    let z = axis.normalize_or(Vec3::Z);
    let across = if side == 1 { -palm } else { palm };
    let x = (across - z * across.dot(z)).normalize_or(z.any_orthonormal_vector());
    Quat::from_mat3(&Mat3::from_cols(x, z.cross(x), z))
}
/// Where a gripping hand holds its handle, in the hand's frame.
pub fn grip_point(side: usize, scale: f32) -> Vec3 {
    let p = shapes::grip_point(shapes::GRIP) * scale;
    if side == 0 { Vec3::new(-p.x, p.y, p.z) } else { p }
}
/// The wrist's place for a gripping hand turned by `turn` to hold a handle at `point`.
pub fn wrist_for(person: Person, side: usize, point: Vec3, turn: Quat) -> Vec3 {
    point - turn * grip_point(side, hand_scale(person))
}
pub fn hand_scale(person: Person) -> f32 {
    if person == Person::Maya { 1. } else { 1.1 }
}

/// Everything a pose puts on a rig: the bones' own transforms, the limbs' and hands' and feet's in the person's frame,
/// and how far each hand and foot fell short of its goal.
#[derive(Clone, Debug)]
pub struct Solved {
    pub pelvis: Transform,
    pub chest: Transform,
    pub head: Transform,
    pub apron: Transform,
    pub limbs: [[Transform; 4]; 2],
    pub hands: [Transform; 2],
    pub feet: [Transform; 2],
    pub eyes: [Transform; 2],
    pub grasps: [Grasp; 2],
    pub mouth: Viseme,
    /// How far each hand (left, right) and each foot (left, right) fell short of its goal, in metres (for the checks).
    #[cfg_attr(not(test), allow(dead_code))]
    pub short: [f32; 4],
}

/// The elbow (or knee) and the end of a two-bone limb from `a` reaching for `target`, bending towards `pole`.
pub fn two_bone(a: Vec3, target: Vec3, l1: f32, l2: f32, pole: Vec3) -> (Vec3, Vec3) {
    let delta = target - a;
    let d = delta.length().clamp((l1 - l2).abs() + 0.001, l1 + l2 - 0.0005);
    let n = delta.normalize_or(Vec3::NEG_Y);
    let along = (l1 * l1 - l2 * l2 + d * d) / (2. * d);
    let out = pole - a;
    let out = (out - n * out.dot(n)).normalize_or(n.any_orthonormal_vector());
    let h = (l1 * l1 - along * along).max(0.).sqrt();
    (a + n * along + out * h, a + n * d)
}

fn segment(a: Vec3, b: Vec3) -> Transform {
    Transform::from_translation(a).with_rotation(Quat::from_rotation_arc(Vec3::NEG_Y, (b - a).normalize_or(Vec3::NEG_Y)))
}

/// Works out where every part of `person` goes for `pose`.
pub fn solve(person: Person, pose: &Pose) -> Solved {
    let fig = person.figure();
    let mut pelvis = Transform::from_translation(Vec3::Y * fig.hip + pose.hips).with_rotation(Quat::from_rotation_z(pose.roll));
    // Down into the knees as far as a long stride needs, so that both feet stay where they are planted.
    let reach = (fig.thigh + fig.shin) * 0.99;
    let mut drop = 0f32;
    for side in 0..2 {
        let hip = pelvis.transform_point(fig.hip_joint * Vec3::new(if side == 0 { -1. } else { 1. }, 1., 1.));
        let to = pose.feet[side].at - hip;
        let across = Vec2::new(to.x, to.z).length();
        if across < reach {
            drop = drop.max(hip.y - (pose.feet[side].at.y + (reach * reach - across * across).sqrt()));
        }
    }
    pelvis.translation.y -= drop;
    let chest = Transform::from_translation(Vec3::Y * fig.waist).with_rotation(Quat::from_rotation_y(pose.twist) * Quat::from_rotation_x(-pose.bend));
    let head = Transform::from_translation(Vec3::Y * fig.neck).with_rotation(Quat::from_rotation_y(pose.turn) * Quat::from_rotation_x(pose.nod));
    let (p, c) = (pelvis.to_matrix(), chest.to_matrix());
    let upper = p * c;
    let head_world = upper * head.to_matrix();
    let mut limbs = [[Transform::IDENTITY; 4]; 2];
    let mut hands = [Transform::IDENTITY; 2];
    let mut feet = [Transform::IDENTITY; 2];
    let mut short = [0.; 4];
    let mut forward = [0f32; 2];
    for side in 0..2 {
        let sign = if side == 0 { -1. } else { 1. };
        let mirror = Vec3::new(sign, 1., 1.);
        // The arm: from the shoulder (where the bent upper body puts it) to the wrist, the elbow out and back.
        let shoulder = upper.transform_point3(fig.shoulder * mirror);
        let hand = pose.hands[side];
        let down = upper.transform_vector3(Vec3::new(sign * 0.45, -0.5, 0.55)).normalize();
        let up = Vec3::new(sign * 0.6, 0.8, 0.15).normalize();
        let pole = shoulder + down.lerp(up, hand.lift.clamp(0., 1.));
        let (elbow, wrist) = two_bone(shoulder, hand.at, fig.upper_arm, fig.forearm, pole);
        limbs[side][0] = segment(shoulder, elbow);
        limbs[side][1] = segment(elbow, wrist);
        hands[side] = Transform::from_translation(wrist).with_rotation(hand.turn).with_scale(Vec3::splat(1.));
        short[side] = wrist.distance(hand.at);
        // The leg: from the hip joint to the ankle, the knee ahead.
        let foot = pose.feet[side];
        let hip = p.transform_point3(fig.hip_joint * mirror);
        let ahead = Quat::from_rotation_y(foot.yaw) * Vec3::NEG_Z;
        let pole = hip + ahead * 0.6 + Vec3::new(sign * 0.08, -0.25, 0.);
        let (knee, ankle) = two_bone(hip, foot.at, fig.thigh, fig.shin, pole);
        limbs[side][2] = segment(hip, knee);
        limbs[side][3] = segment(knee, ankle);
        feet[side] = Transform::from_translation(ankle).with_rotation(Quat::from_rotation_y(foot.yaw) * Quat::from_rotation_x(foot.pitch));
        short[2 + side] = ankle.distance(foot.at);
        let d = (knee - hip).normalize_or(Vec3::NEG_Y);
        forward[side] = (-d.z).atan2(-d.y);
    }
    // The apron swings forward over the leading knee.
    let swing = forward[0].max(forward[1]).max(0.) * 0.8 + pose.bend.max(0.) * 0.25;
    let apron = Transform::from_translation(Vec3::Y * 0.07).with_rotation(Quat::from_rotation_x(swing)) * Transform::from_translation(Vec3::Y * -0.07);
    // The eyes: each turned towards what is looked at (a little), and squashed shut by a blink.
    let places = people::eye_places(fig);
    let eyes = [0, 1].map(|side| {
        let (at, normal) = places[side];
        let rest = Quat::from_rotation_arc(Vec3::NEG_Z, normal);
        let between = (places[0].0 + places[1].0) * 0.5;
        let gaze = pose
            .look
            .map(|target| {
                let local = head_world.inverse().transform_point3(target) - between;
                let to = local.normalize_or(normal);
                let q = Quat::from_rotation_arc(normal, to);
                let (axis, angle) = q.to_axis_angle();
                Quat::from_axis_angle(axis, angle.min(0.42))
            })
            .unwrap_or(Quat::IDENTITY);
        Transform::from_translation(at).with_rotation(gaze * rest).with_scale(Vec3::new(1., (1. - pose.blink).max(0.08), 1.))
    });
    Solved {
        pelvis,
        chest,
        head,
        apron,
        limbs,
        hands,
        feet,
        eyes,
        grasps: [pose.hands[0].grasp, pose.hands[1].grasp],
        mouth: pose.mouth,
        short,
    }
}

/// Puts a solved pose on a rig.
pub fn apply(rig: &Rig, solved: &Solved, cast: &Cast, transforms: &mut Query<&mut Transform>, shown: &mut Query<&mut Visibility>, meshes: &mut Query<&mut Mesh3d>) {
    let mut set = |entity: Entity, value: Transform| {
        if let Ok(mut t) = transforms.get_mut(entity)
            && *t != value
        {
            *t = value;
        }
    };
    set(rig.pelvis, solved.pelvis);
    set(rig.chest, solved.chest);
    set(rig.head, solved.head);
    if let Some(apron) = rig.apron {
        set(apron, solved.apron);
    }
    for side in 0..2 {
        for k in 0..4 {
            set(rig.limbs[side][k], solved.limbs[side][k]);
        }
        set(rig.hands[side].0, solved.hands[side]);
        set(rig.feet[side], solved.feet[side]);
        set(rig.eyes[side], solved.eyes[side]);
        for (grasp, entity) in Grasp::ALL.into_iter().zip(rig.hands[side].1) {
            let wanted = solved.grasps[side] == grasp;
            if let Ok(mut v) = shown.get_mut(entity) {
                v.set_if_neq(if wanted { Visibility::Inherited } else { Visibility::Hidden });
            }
        }
    }
    if let Ok(mut mesh) = meshes.get_mut(rig.mouth) {
        let wanted = cast.mesh(rig.person, Piece::Mouth(solved.mouth));
        if mesh.0 != wanted {
            mesh.0 = wanted;
        }
    }
}

/// A person who just stands about: the `counter-map` scene's host (a giant behind the counter), shifting their weight,
/// gesturing and chatting to nobody in short phrases.
#[derive(Component)]
pub struct Idle;

/// The idle stance at time `t`.
pub fn idle_pose(person: Person, t: f32) -> Pose {
    let fig = person.figure();
    let mut pose = Pose::rest(fig);
    let gesture = (t * 0.75).sin() * 0.5 + 0.5;
    pose.bend = 0.05 + 0.04 * gesture + (t * 1.7).sin() * 0.008;
    pose.twist = (t * 0.43).sin() * 0.05;
    pose.nod = 0.06 + (t * 0.82).sin() * 0.035;
    pose.turn = (t * 0.51).sin() * 0.12;
    pose.hips = Vec3::new((t * 0.9).sin() * 0.008, 0., 0.);
    for side in 0..2 {
        let sign = if side == 0 { -1. } else { 1. };
        // Forearms up, hands talking in front of the belly.
        let lift = 0.12 + 0.10 * gesture + 0.03 * (t * 0.75 + 0.4 + side as f32).sin();
        pose.hands[side].at = Vec3::new(sign * (fig.rest_hand.x - 0.03), fig.rest_hand.y + lift, -fig.front - 0.06 - 0.04 * gesture);
        pose.hands[side].turn = hand_turn(side, Vec3::new(sign * 0.3, -0.2, -1.), Vec3::new(-sign * 0.4, 0.8, 0.));
    }
    // Short phrases between pauses (no sound: an idle expression).
    let phrase = t % 8.;
    let talking = phrase < 5.3;
    pose.mouth = if talking { speech_shape((t * 7.3) as usize) } else { Viseme::Rest };
    let blink = (t % 3.7) / 0.19;
    pose.blink = if blink < 1. { (blink * std::f32::consts::PI).sin() } else { 0. };
    pose
}

/// A mouth shape for a syllable counted `n` from the start of a phrase.
fn speech_shape(n: usize) -> Viseme {
    [Viseme::Ah, Viseme::Mm, Viseme::Ee, Viseme::Oh, Viseme::Rest, Viseme::Ah, Viseme::Ee][n % 7]
}

/// Poses every idle person (the `counter-map` scene's host).
pub fn idle(
    time: Res<Time>,
    game: Option<Res<crate::arena::Game>>,
    cast: Option<Res<Cast>>,
    rigs: Query<&Rig, With<Idle>>,
    mut transforms: Query<&mut Transform>,
    mut shown: Query<&mut Visibility>,
    mut meshes: Query<&mut Mesh3d>,
) {
    let Some(cast) = cast else { return };
    if rigs.is_empty() || game.is_some_and(|g| g.playing()) {
        return;
    }
    for rig in &rigs {
        let solved = solve(rig.person, &idle_pose(rig.person, time.elapsed_secs()));
        apply(rig, &solved, &cast, &mut transforms, &mut shown, &mut meshes);
    }
}

/// The `counter-map` scene's host: Maya's husband, a giant behind the counter.
pub fn spawn_host(commands: &mut Commands, cast: &Cast) {
    // (Turned to the scene's camera, at the front of the room.)
    let place = Transform::from_xyz(2.2, 0.02, -4.4).with_rotation(Quat::from_rotation_y(2.94)).with_scale(Vec3::splat(3.3));
    let host = spawn(commands, cast, Person::Husband, place);
    commands.entity(host).insert((Idle, Name::new("Kitchen host")));
}

#[cfg(test)]
mod tests;
