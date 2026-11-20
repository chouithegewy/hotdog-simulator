// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! How Maya and her husband move through the breakfast scene, frame by frame: the choreography's paths
//! (`choreography`) turned into poses for their rigs (`human::Pose`). Planted steps, a lean into the fridge and two
//! hands on the tray's rims, the handshake and the hug, the carry and the set-down, the knife held by its handle and
//! the film lifted by its edge, blinks, glances and the lines they say.
use super::choreography::{self as c, Beat, Stride};
use crate::human::{self, Figure, Foot, Grasp, Hand, Person, Pose, Viseme};
use bevy::prelude::*;

/// Both step plans, made once.
pub struct Steps {
    pub maya: Vec<Stride>,
    pub husband: Vec<Stride>,
}
impl Steps {
    pub fn plan() -> Steps {
        let feet = |fig: &Figure| [fig.foot() * Vec3::new(-1., 1., 1.), fig.foot()];
        Steps {
            maya: c::plan_steps(|t| { let m = c::motion(t); (m.root, m.yaw) }, feet(Person::Maya.figure())),
            husband: c::plan_steps(c::husband, feet(Person::Husband.figure())),
        }
    }
}

/// A person's place and pose at a moment.
#[derive(Clone, Copy, Debug)]
pub struct Actor {
    pub root: Vec3,
    pub yaw: f32,
    pub pose: Pose,
}
impl Actor {
    pub fn place(&self, scale: f32) -> Transform {
        Transform::from_translation(self.root).with_rotation(Quat::from_rotation_y(self.yaw)).with_scale(Vec3::splat(scale))
    }
    /// A point in the world in this person's own frame.
    pub fn local(&self, world: Vec3) -> Vec3 {
        Quat::from_rotation_y(-self.yaw) * (world - self.root)
    }
    pub fn local_dir(&self, world: Vec3) -> Vec3 {
        Quat::from_rotation_y(-self.yaw) * world
    }
}

/// Lines, as the subtitles have them (or the chat around them), and when each begins in clip time.
const LINES: &[(Person, f32, &str)] = &[
    (Person::Husband, c::GREETING_START + 0.35, "Morning, sunshine!"),
    (Person::Maya, c::GREETING_START + 1.5, "Good morning, you."),
    (Person::Husband, c::GREETING_START + 8.9, "Mmm. Breakfast."),
    (Person::Maya, c::REACH_START + 0.15, "All six of you? Got it. Let me get a good grip."),
    (Person::Maya, c::KNIFE_START + 0.2, "Right. Buns are ready, the crew's hungry. Let's make breakfast."),
];
/// Seconds a letter takes to say.
const LETTER: f32 = 0.062;

/// The mouth's shape `t` seconds into saying `line`, or None once it is said.
pub fn say(line: &str, t: f32) -> Option<Viseme> {
    if t < 0. {
        return None;
    }
    let mut clock = 0.;
    let letters: Vec<char> = line.chars().collect();
    for (i, ch) in letters.iter().enumerate() {
        let length = match ch {
            '.' | '?' | '!' => 0.32,
            ',' => 0.18,
            ' ' => 0.05,
            _ => LETTER,
        };
        if t < clock + length {
            let lower = ch.to_ascii_lowercase();
            return Some(match lower {
                'a' => Viseme::Ah,
                'e' | 'i' | 'y' => Viseme::Ee,
                'o' | 'u' | 'w' => Viseme::Oh,
                'm' | 'b' | 'p' => Viseme::Mm,
                '.' | '?' | '!' | ',' | ' ' | '\'' => Viseme::Rest,
                // Other consonants: the vowel that follows, half said.
                _ => match letters[i + 1..].iter().find(|c| "aeiouy".contains(c.to_ascii_lowercase())).map(|c| c.to_ascii_lowercase()) {
                    Some('a') => Viseme::Ee,
                    Some('o') | Some('u') => Viseme::Oh,
                    _ => Viseme::Ee,
                },
            });
        }
        clock += length;
    }
    None
}

fn speaking(person: Person, t: f32) -> Option<Viseme> {
    LINES.iter().filter(|(who, _, _)| *who == person).find_map(|(_, start, line)| say(line, t - start))
}

/// The blinks: Maya's as she always had them, and his of his own.
fn blink(person: Person, t: f32) -> f32 {
    let beats: &[f32] = match person {
        Person::Maya => &[1.2, 4.7, 7.3, 11.3, 15.7, c::IDLE_START + 1.6, c::CARRY_START + 1.1, c::CARRY_START + 6.1, c::COMPLETE_START + 1.7, c::KNIFE_START + 3.1, c::KNIFE_START + 8.4],
        Person::Husband => &[2.4, 6.1, 9.8, 13.9, 18.2, 22.7, 27.0, 31.3, 35.9, 40.2, 44.8, 49.3, 53.5],
    };
    beats
        .iter()
        .map(|beat| {
            let dt = t - beat;
            if !(0. ..0.19).contains(&dt) {
                0.
            } else if dt < 0.065 {
                c::smooth(dt / 0.065)
            } else {
                1. - c::smooth((dt - 0.065) / 0.125)
            }
        })
        .fold(0., f32::max)
}

/// Where both of them are and how they stand at clip time `t`.
pub fn act(t: f32, steps: &Steps) -> [Actor; 2] {
    let m = c::motion(t);
    let (him, his_yaw) = c::husband(t);
    let mut maya = Actor { root: m.root, yaw: m.yaw, pose: Pose::rest(Person::Maya.figure()) };
    let mut husband = Actor { root: him, yaw: his_yaw, pose: Pose::rest(Person::Husband.figure()) };
    let maya_pose = pose(Person::Maya, t, &maya, &husband, c::stride_at(&steps.maya, t));
    let husband_pose = pose(Person::Husband, t, &husband, &maya, c::stride_at(&steps.husband, t));
    maya.pose = maya_pose;
    husband.pose = husband_pose;
    [maya, husband]
}

/// The package's transform at `t`.
pub fn package(t: f32) -> Transform {
    let m = c::motion(t);
    Transform::from_translation(m.package).with_rotation(Quat::from_rotation_y(m.package_yaw))
}

/// Where a person's head is (roughly: the middle of their face), in the world, standing.
fn face_of(actor: &Actor, person: Person) -> Vec3 {
    let fig = person.figure();
    actor.root + Vec3::Y * (fig.hip + fig.waist + fig.neck + fig.head.y) + Quat::from_rotation_y(actor.yaw) * Vec3::new(0., 0., -0.1)
}

fn pose(person: Person, t: f32, me: &Actor, partner: &Actor, stride: Stride) -> Pose {
    let fig = person.figure();
    let maya = person == Person::Maya;
    let (kind, s) = c::beat(t);
    let g = c::greeting(t);
    let mut reach = match kind {
        Beat::Reach => c::smooth((s - 0.9) / 3.1),
        Beat::Carry => 1.,
        Beat::Set => 1. - c::smooth((s - 2.4) / 1.),
        _ => 0.,
    };
    let mut bend = reach;
    if kind == Beat::Carry {
        bend = 1. - 0.7 * c::smooth((s - 2.2) / 1.);
    }
    if kind == Beat::Set {
        bend = (0.3 + 0.5 * c::smooth(s / 2.4)) * (1. - c::smooth((s - 2.4) / 1.));
    }
    if !maya {
        (reach, bend) = (0., g.hug * 0.12);
    }
    if kind == Beat::Greet && maya {
        bend = g.hug * 0.32;
    }
    if kind == Beat::Knife && maya {
        bend = 0.75 * c::smooth(s / 1.4);
    }
    let mut pose = Pose::rest(fig);
    let rear = if maya { 0. } else { (0.018 + 0.007 * (t * 1.1).sin()) * (1. - g.hug) };
    let sway = if maya { 0.004 * (t * 0.8).sin() } else { 0.012 + 0.004 * (t * 0.9).sin() };
    // Down a little into the knees for each step (and more in the lean; `human::solve` goes lower if a long stride
    // needs it), to the side over the planted foot.
    pose.hips = Vec3::new(0.010 * stride.gait + sway, -0.07 * bend - 0.012 * stride.gait.abs(), rear);
    pose.roll = stride.gait * 0.035;
    // The lean into the fridge comes mostly from the hips and the arms: the head stays clear of the shelf above.
    pose.bend = 0.04 + 0.30 * bend + 0.005 * (t * 1.8).sin();
    if maya && kind == Beat::Knife {
        // Over the counter to cut, her head above the knife.
        // (Once the hotdog's eye has cut to the close shot of the seal, 4.5 s in: before that it sees from under her chin.)
        pose.bend += 0.22 * c::smooth((s - 4.0) / 0.9);
    }
    pose.twist = -stride.gait * 0.06;
    pose.nod = if maya { -0.20 - 0.10 * bend } else { 0.015 * (t * 0.8).sin() };
    if kind == Beat::Greet {
        pose.nod = if maya { -0.06 } else { -0.10 };
    }
    if maya && matches!(kind, Beat::Done | Beat::Knife) {
        pose.nod = -0.38;
    }
    pose.blink = blink(person, t);
    pose.mouth = speaking(person, t).unwrap_or(Viseme::Rest);
    if kind == Beat::Greet && (3.6..6.4).contains(&s) && speaking(person, t).is_none() {
        // Laughing into the hug.
        pose.mouth = if ((s * 7.).sin() > 0.) == maya { Viseme::Laugh } else { Viseme::Rest };
    }
    if maya && kind == Beat::Knife && (4.6..9.4).contains(&s) && speaking(person, t).is_none() {
        // Concentrating on the cut.
        pose.mouth = Viseme::Mm;
    }

    // What they look at: the tray, each other, the way ahead, the counter, the knife.
    let package = package(t);
    let look = if kind == Beat::Greet {
        face_of(partner, if maya { Person::Husband } else { Person::Maya })
    } else if maya {
        match kind {
            Beat::Carry if s > 3.4 => c::COUNTER + Vec3::Y * 0.1,
            Beat::Knife => c::knife(s).translation.lerp(package.translation, c::smooth((s - 5.) / 2.)),
            _ => package.translation,
        }
    } else if matches!(kind, Beat::Idle) && t < c::GREETING_START {
        package.translation + Vec3::Y * 0.2
    } else {
        face_of(partner, Person::Maya).lerp(package.translation, 0.3)
    };
    let look_local = me.local(look);
    pose.look = Some(look_local);
    // The head turns some of the way.
    let flat = Vec2::new(look_local.x, -look_local.z);
    pose.turn = (-flat.x.atan2(flat.y) * 0.45).clamp(-0.55, 0.55);

    // The feet, planted where the step plan has them.
    for side in 0..2 {
        let f = stride.feet[side];
        pose.feet[side] = Foot { at: me.local(f.at), yaw: f.yaw - me.yaw, pitch: f.pitch };
    }

    // The hands.
    let rest = Pose::rest(fig);
    for side in 0..2 {
        let sign = if side == 0 { -1. } else { 1. };
        let mut hand = rest.hands[side];
        if stride.walking && reach < 0.2 {
            hand.at.z -= stride.gait * 0.05 * sign;
        }
        // Both hands on the tray's rims, from outside, fingers curled under.
        if reach > 0. {
            let rim = package.transform_point(Vec3::new(sign * 0.113, 0.0, 0.03));
            let q = Quat::from_rotation_y(-me.yaw) * package.rotation;
            let held = human::grip_turn(side, q * Vec3::Z, q * Vec3::new(-sign, -0.35, 0.));
            let target = Hand { at: human::wrist_for(person, side, me.local(rim), held), turn: held, grasp: Grasp::Grip, lift: 0. };
            hand = blend(hand, target, reach);
        }
        // The right hands meet in the handshake: palms together at the centre, fingers towards the other.
        if side == 1 && g.shake > 0. {
            let toward = Vec3::new(g.centre.x - me.root.x, 0., g.centre.z - me.root.z).normalize_or(Vec3::NEG_Z);
            let fingers = me.local_dir(toward);
            let palm = Vec3::Y.cross(fingers).normalize();
            let turn = human::hand_turn(side, fingers + Vec3::new(0., -0.15, 0.), palm);
            let scale = human::hand_scale(person);
            let palm_point = turn * (Vec3::new(-0.022, -0.052, 0.) * scale);
            let at = me.local(g.centre) - palm_point - palm * 0.004;
            hand = blend(hand, Hand { at, turn, grasp: Grasp::Shake, lift: 0. }, g.shake);
        }
        // The left hands round each other in the hug: on the other's shoulder and upper back.
        if side == 0 && g.hug > 0. {
            let other = if maya { Person::Husband } else { Person::Maya };
            let ofig = other.figure();
            let shoulder = Vec3::new(ofig.shoulder.x * 0.85, ofig.hip + ofig.waist + ofig.shoulder.y - 0.07, 0.07);
            let contact = partner.root + Quat::from_rotation_y(partner.yaw) * shoulder;
            let body = partner.root + Vec3::Y * contact.y;
            let palm = me.local_dir((body - contact).normalize_or(Vec3::X));
            let back = me.local_dir(Quat::from_rotation_y(partner.yaw) * Vec3::Z);
            let turn = human::hand_turn(side, back + Vec3::new(0., -0.4, 0.), palm);
            let at = me.local(contact) - turn * (Vec3::new(0.024, -0.05, 0.) * human::hand_scale(person)) - palm * 0.006;
            hand = blend(hand, Hand { at, turn, grasp: Grasp::Open, lift: 0. }, g.hug);
        }
        // The knife, by its handle; the film, by its edge.
        if maya && kind == Beat::Knife {
            let target = if side == 1 {
                let knife = c::knife(s);
                let handle = knife.transform_point(c::KNIFE_HANDLE);
                let axis = me.local_dir(knife.rotation * Vec3::Z);
                let turn = human::grip_turn(side, axis, Vec3::NEG_Y);
                // The elbow up and out, the forearm coming down to the handle, as one cuts.
                Hand { at: human::wrist_for(person, side, me.local(handle), turn), turn, grasp: Grasp::Grip, lift: 1. }
            } else {
                // The other hand steadies the package by its rim, near her, on her left, while she cuts.
                let rim = package.transform_point(Vec3::new(-0.113, 0.0, 0.07));
                let q = Quat::from_rotation_y(-me.yaw) * package.rotation;
                let turn = human::grip_turn(side, q * Vec3::Z, q * Vec3::new(1., -0.35, 0.));
                Hand { at: human::wrist_for(person, side, me.local(rim), turn), turn, grasp: Grasp::Grip, lift: 0. }
            };
            let when = if side == 1 { c::smooth(s / 2.) } else { c::smooth((s - 0.8) / 1.8) };
            hand = blend(hand, target, when);
        }
        pose.hands[side] = hand;
    }
    pose
}

fn blend(a: Hand, b: Hand, k: f32) -> Hand {
    if k <= 0. {
        return a;
    }
    if k >= 1. {
        return b;
    }
    Hand { at: a.at.lerp(b.at, k), turn: a.turn.slerp(b.turn, k), grasp: if k > 0.6 { b.grasp } else { a.grasp }, lift: a.lift + (b.lift - a.lift) * k }
}
