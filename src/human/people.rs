// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Who the people are: their proportions ([`Figure`]) and every piece of them, built from [`shapes`]: Maya, the
//! breakfast chef (a tall pleated toque, a white double-breasted jacket with its sleeves rolled up, a red neckerchief, a
//! mustard bistro apron with a pocket and a bow, black-and-white check trousers and black clogs), and her husband (round
//! glasses, a ginger moustache and fringe round a bald crown, a teal cable-knit jumper over a round belly, corduroy
//! trousers and slippers). Cartoon people: big heads, big eyes like the hotdogs', soft round limbs. Colours are the
//! vertices'; the cloth patterns are small tiles painted in code ([`tiles`]).
use super::shapes::{self, Builder, Fine, MouthColors, Viseme, on_ellipsoid};
use bevy::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Person {
    Maya,
    Husband,
}
impl Person {
    pub const ALL: [Person; 2] = [Person::Maya, Person::Husband];
    pub fn figure(self) -> &'static Figure {
        match self {
            Person::Maya => &MAYA,
            Person::Husband => &HUSBAND,
        }
    }
}

/// A person's proportions, in metres, in their own frame: standing on y = 0, facing -z, their right towards +x.
/// The joints of the right side; the left mirrors them.
#[derive(Debug)]
pub struct Figure {
    /// The middle of the hips (the pelvis's pivot) at rest, and each hip joint from it.
    pub hip: f32,
    pub hip_joint: Vec3,
    /// The waist (the spine's pivot) above the hips, and each shoulder from the waist.
    pub waist: f32,
    pub shoulder: Vec3,
    /// The neck's pivot above the waist; the head's middle from it, and its radii.
    pub neck: f32,
    pub head: Vec3,
    pub head_radii: Vec3,
    pub upper_arm: f32,
    pub forearm: f32,
    pub thigh: f32,
    pub shin: f32,
    /// The ankle above the floor at rest.
    pub ankle: f32,
    /// How thick the upper arm, forearm, thigh and shin are at each end.
    pub limbs: [(f32, f32); 4],
    /// Where each eye's middle is on the face, as shares of the head's radii (x out from the middle, y up).
    pub eyes: Vec2,
    /// The eye's radii.
    pub eye: Vec3,
    /// How far the hands hang from the body and how low (a share of the arm's reach), at rest.
    pub rest_hand: Vec3,
    /// How deep the body is in front, for the hands to stay outside it.
    pub front: f32,
}
impl Figure {
    /// The right ankle's place at rest (the left mirrors it).
    pub fn foot(&self) -> Vec3 {
        Vec3::new(self.hip_joint.x + 0.004, self.ankle, 0.)
    }
    /// The height of the top of the head (without a hat).
    #[cfg(test)]
    pub fn height(&self) -> f32 {
        self.hip + self.waist + self.neck + self.head.y + self.head_radii.y
    }
}

pub const MAYA: Figure = Figure {
    hip: 0.82,
    hip_joint: Vec3::new(0.085, -0.06, 0.),
    waist: 0.08,
    shoulder: Vec3::new(0.195, 0.355, 0.005),
    neck: 0.42,
    head: Vec3::new(0., 0.148, -0.012),
    head_radii: Vec3::new(0.152, 0.162, 0.148),
    upper_arm: 0.265,
    forearm: 0.245,
    thigh: 0.36,
    shin: 0.34,
    ankle: 0.075,
    limbs: [(0.050, 0.044), (0.040, 0.032), (0.078, 0.062), (0.060, 0.052)],
    eyes: Vec2::new(0.36, 0.06),
    eye: Vec3::new(0.034, 0.043, 0.020),
    rest_hand: Vec3::new(0.24, 0.875, -0.06),
    front: 0.15,
};

pub const HUSBAND: Figure = Figure {
    hip: 0.88,
    hip_joint: Vec3::new(0.10, -0.06, 0.),
    waist: 0.08,
    shoulder: Vec3::new(0.228, 0.395, 0.01),
    neck: 0.455,
    head: Vec3::new(0., 0.156, -0.016),
    head_radii: Vec3::new(0.166, 0.172, 0.16),
    upper_arm: 0.29,
    forearm: 0.27,
    thigh: 0.39,
    shin: 0.37,
    ankle: 0.08,
    limbs: [(0.058, 0.050), (0.047, 0.040), (0.088, 0.072), (0.068, 0.060)],
    eyes: Vec2::new(0.35, 0.10),
    eye: Vec3::new(0.031, 0.037, 0.018),
    rest_hand: Vec3::new(0.285, 0.95, -0.07),
    front: 0.25,
};

/// The pieces a person is made of, each one mesh. The limbs, eyes and hands are the same on both sides (a left hand is
/// a right one mirrored).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Piece {
    /// The hips, in the trousers' cloth.
    Pelvis,
    /// What hangs from the waist and swings with the legs: Maya's apron.
    Apron,
    /// The upper body in its cloth (the jacket, the jumper).
    Torso,
    /// The upper body's trim: collar, buttons, neckerchief.
    Trim,
    /// The head, neck, hair, nose, ears, brows and cheeks.
    Head,
    /// What sits on the head and is cloth: Maya's toque.
    Hat,
    /// The husband's glasses.
    Glasses,
    Eye,
    UpperArm,
    Forearm,
    /// A right hand, open, shaking hands or gripping; the left is the right mirrored.
    Hand { grasp: Grasp, left: bool },
    Thigh,
    Shin,
    Foot,
    Mouth(Viseme),
}

/// How a hand is closed.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum Grasp {
    /// Relaxed, the fingers a little bent.
    #[default]
    Open,
    /// Half closed, round another hand.
    Shake,
    /// Closed round a handle or a rim.
    Grip,
}
impl Grasp {
    pub const ALL: [Grasp; 3] = [Grasp::Open, Grasp::Shake, Grasp::Grip];
    pub fn curl(self) -> f32 {
        match self {
            Grasp::Open => shapes::OPEN,
            Grasp::Shake => 0.55,
            Grasp::Grip => shapes::GRIP,
        }
    }
}

/// The surfaces the pieces are drawn with (the colours are in the vertices).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Finish {
    /// Cloth and hair: rough.
    Matte,
    /// Cloth seen from both sides (the apron).
    Sheet,
    Skin,
    /// Eyes, glasses, shoes.
    Glossy,
    /// Maya's check trousers.
    Check,
    /// The husband's knitted jumper.
    Knit,
    /// The husband's corduroy trousers.
    Cord,
}

impl Person {
    /// The pieces this person has, and the finish each is drawn with.
    pub fn pieces(self) -> Vec<(Piece, Finish)> {
        let mut out = vec![(Piece::Head, Finish::Skin), (Piece::Trim, Finish::Matte), (Piece::Eye, Finish::Glossy), (Piece::Foot, Finish::Glossy)];
        for viseme in Viseme::ALL {
            out.push((Piece::Mouth(viseme), Finish::Skin));
        }
        for grasp in Grasp::ALL {
            for left in [false, true] {
                out.push((Piece::Hand { grasp, left }, Finish::Skin));
            }
        }
        match self {
            Person::Maya => out.extend([
                (Piece::Pelvis, Finish::Check),
                (Piece::Apron, Finish::Sheet),
                (Piece::Torso, Finish::Matte),
                (Piece::Hat, Finish::Matte),
                (Piece::UpperArm, Finish::Matte),
                (Piece::Forearm, Finish::Skin),
                (Piece::Thigh, Finish::Check),
                (Piece::Shin, Finish::Check),
            ]),
            Person::Husband => out.extend([
                (Piece::Pelvis, Finish::Cord),
                (Piece::Torso, Finish::Knit),
                (Piece::Glasses, Finish::Glossy),
                (Piece::UpperArm, Finish::Knit),
                (Piece::Forearm, Finish::Knit),
                (Piece::Thigh, Finish::Cord),
                (Piece::Shin, Finish::Cord),
            ]),
        }
        out
    }
}

struct Palette {
    skin: [f32; 4],
    blush: [f32; 4],
    nose: [f32; 4],
    hair: [f32; 4],
    brow: [f32; 4],
    iris: [f32; 4],
    lip: [f32; 4],
}
const MAYA_COLORS: Palette = Palette {
    skin: [0.62, 0.40, 0.27, 1.],
    blush: [0.80, 0.36, 0.33, 1.],
    nose: [0.66, 0.40, 0.28, 1.],
    hair: [0.16, 0.075, 0.05, 1.],
    brow: [0.11, 0.05, 0.035, 1.],
    iris: [0.33, 0.19, 0.08, 1.],
    lip: [0.47, 0.17, 0.15, 1.],
};
const HUSBAND_COLORS: Palette = Palette {
    skin: [0.93, 0.70, 0.56, 1.],
    blush: [0.96, 0.50, 0.46, 1.],
    nose: [0.94, 0.58, 0.50, 1.],
    hair: [0.66, 0.33, 0.14, 1.],
    brow: [0.55, 0.26, 0.10, 1.],
    iris: [0.20, 0.34, 0.42, 1.],
    lip: [0.66, 0.30, 0.28, 1.],
};
impl Person {
    fn colors(self) -> &'static Palette {
        match self {
            Person::Maya => &MAYA_COLORS,
            Person::Husband => &HUSBAND_COLORS,
        }
    }
}

const JACKET: [f32; 4] = [0.96, 0.95, 0.92, 1.];
const APRON: [f32; 4] = [0.90, 0.62, 0.13, 1.];
const APRON_DARK: [f32; 4] = [0.70, 0.45, 0.08, 1.];
const KERCHIEF: [f32; 4] = [0.80, 0.11, 0.10, 1.];
const BUTTON: [f32; 4] = [0.13, 0.13, 0.15, 1.];
const CLOG: [f32; 4] = [0.07, 0.07, 0.08, 1.];
const SOLE: [f32; 4] = [0.85, 0.82, 0.75, 1.];
const SLIPPER: [f32; 4] = [0.45, 0.10, 0.12, 1.];
const SHIRT: [f32; 4] = [0.93, 0.93, 0.88, 1.];
const KNIT_RIB: [f32; 4] = [0.78, 0.84, 0.84, 1.];
const FRAME: [f32; 4] = [0.20, 0.11, 0.06, 1.];
const MOUTH_INSIDE: [f32; 4] = [0.22, 0.04, 0.05, 1.];
const TEETH: [f32; 4] = [0.98, 0.96, 0.90, 1.];
const TONGUE: [f32; 4] = [0.85, 0.36, 0.38, 1.];

/// A point on the head's surface (in the head bone's frame) at (x, y) shares of its radii, pushed out by `lift`.
fn face(fig: &Figure, x: f32, y: f32, lift: f32) -> (Vec3, Vec3) {
    let r = fig.head_radii;
    let z = -(1. - x * x - y * y).max(0.02).sqrt();
    let (p, n) = on_ellipsoid(fig.head, r, Vec3::new(x * r.x, y * r.y, z * r.z));
    (p + n * lift, n)
}

/// Where each eye sits (in the head bone's frame) and which way it faces; left then right.
pub fn eye_places(fig: &Figure) -> [(Vec3, Vec3); 2] {
    [-1., 1.].map(|side| {
        let (p, n) = face(fig, side * fig.eyes.x, fig.eyes.y, 0.);
        (p - n * fig.eye.z * 0.35, n)
    })
}

/// The middle of the mouth on the face, as seen from the head's middle.
pub fn mouth_dir(fig: &Figure) -> Vec3 {
    let r = fig.head_radii;
    // (His sits a little lower, under the moustache.)
    let y = if fig.eyes.y > 0.08 { -0.47 } else { -0.40 };
    Vec3::new(0., y * r.y, -0.9 * r.z)
}

/// Builds one piece of a person.
pub fn piece(person: Person, piece: Piece, fine: Fine) -> Builder {
    let fig = person.figure();
    let colors = person.colors();
    let mut b = Builder::new();
    let maya = person == Person::Maya;
    match piece {
        Piece::Pelvis => {
            let r = if maya { Vec3::new(0.162, 0.12, 0.128) } else { Vec3::new(0.172, 0.125, 0.142) };
            b.ellipsoid(Vec3::new(0., -0.025, 0.004), r, Quat::IDENTITY, fine, shapes::WHITE);
        }
        Piece::Apron => {
            // A bistro apron from the waist to the knee, round the front of the hips and flaring a little.
            let (top, bottom) = (0.075, -0.40);
            let width = |u: f32| (0.172 + 0.035 * u * u, 0.140 + 0.05 * u * u);
            b.patch(
                fine.rings,
                fine.sides,
                false,
                |u, v| {
                    let y = top + (bottom - top) * u;
                    let (rx, rz) = width(u);
                    let a = (v * 2. - 1.) * 1.95;
                    // Soft folds along the hem.
                    let fold = 1. + 0.025 * u * (a * 5.).sin();
                    Vec3::new(rx * a.sin() * fold, y, -rz * a.cos() * fold)
                },
                |p| Vec3::new(0., p.y, 0.),
                |u, v| Vec2::new(v, u),
                |u, _| if u > 0.965 { APRON_DARK } else { APRON },
            );
            // The pocket, a patch on the front with a darker hem.
            b.patch(
                3,
                8,
                false,
                |u, v| {
                    let y = -0.10 - 0.11 * u;
                    let a = (v * 2. - 1.) * 0.42 - 0.18;
                    let (rx, rz) = width((top - y) / (top - bottom));
                    Vec3::new((rx + 0.004) * a.sin(), y, -(rz + 0.004) * a.cos())
                },
                |p| Vec3::new(0., p.y, 0.),
                |u, v| Vec2::new(v, u),
                |u, _| if u < 0.2 { APRON_DARK } else { APRON },
            );
            // The waistband all round, and the bow at the back.
            b.revolve(Vec3::new(0., top - 0.006, 0.), Quat::IDENTITY, fine.scaled(0.5), |u, a| {
                let r = 1. / ((a.cos() / 0.175).powi(2) + (a.sin() / 0.143).powi(2)).sqrt();
                (r + 0.006 * (u * std::f32::consts::PI).sin(), (u - 0.5) * 0.026)
            }, APRON_DARK);
            for side in [-1., 1.] {
                b.torus(
                    Vec3::new(side * 0.045, top - 0.01, 0.15),
                    Quat::from_rotation_z(side * 1.2) * Quat::from_rotation_x(1.45),
                    0.035,
                    0.008,
                    fine.scaled(0.5),
                    APRON_DARK,
                );
                b.tube(&[(Vec3::new(side * 0.012, top - 0.02, 0.152), 0.008), (Vec3::new(side * 0.035, top - 0.11, 0.16), 0.007)], fine.scaled(0.4), APRON_DARK);
            }
            b.ellipsoid(Vec3::new(0., top - 0.012, 0.152), Vec3::new(0.016, 0.013, 0.01), Quat::IDENTITY, fine.scaled(0.4), APRON_DARK);
        }
        Piece::Torso => {
            let rows: &[(f32, f32, f32, f32)] = if maya {
                &[
                    (-0.13, 0.010, 0.010, 0.),
                    (-0.11, 0.150, 0.124, 0.),
                    (0.0, 0.152, 0.122, 0.),
                    (0.10, 0.160, 0.126, -0.008),
                    (0.19, 0.172, 0.134, -0.016),
                    (0.27, 0.190, 0.124, -0.006),
                    (0.33, 0.198, 0.108, 0.004),
                    (0.375, 0.150, 0.086, 0.006),
                    (0.405, 0.070, 0.060, 0.006),
                    (0.418, 0.010, 0.010, 0.006),
                ]
            } else {
                &[
                    (-0.15, 0.010, 0.010, 0.),
                    (-0.13, 0.176, 0.150, 0.),
                    (-0.03, 0.192, 0.176, -0.03),
                    (0.07, 0.206, 0.196, -0.05),
                    (0.17, 0.212, 0.186, -0.046),
                    (0.27, 0.216, 0.152, -0.02),
                    (0.36, 0.228, 0.126, 0.),
                    (0.41, 0.168, 0.096, 0.005),
                    (0.445, 0.075, 0.062, 0.005),
                    (0.458, 0.010, 0.010, 0.005),
                ]
            };
            b.loft(rows, fine, shapes::WHITE);
        }
        Piece::Trim => {
            if maya {
                // The collar, standing round the neck, and the neckerchief knotted at the side.
                b.torus(Vec3::new(0., 0.395, 0.004), Quat::from_rotation_x(-0.12), 0.060, 0.017, fine.scaled(0.6), JACKET);
                b.torus(Vec3::new(0., 0.378, 0.0), Quat::from_rotation_x(-0.12), 0.066, 0.013, fine.scaled(0.6), KERCHIEF);
                b.ellipsoid(Vec3::new(0.032, 0.36, -0.065), Vec3::new(0.020, 0.016, 0.014), Quat::from_rotation_z(0.5), fine.scaled(0.4), KERCHIEF);
                for (k, side) in [-1., 1.].into_iter().enumerate() {
                    b.ellipsoid(
                        Vec3::new(0.036 + side * 0.012, 0.335, -0.07 - 0.004 * k as f32),
                        Vec3::new(0.012, 0.026, 0.006),
                        Quat::from_rotation_z(side * 0.4),
                        fine.scaled(0.4),
                        KERCHIEF,
                    );
                }
                // The double-breasted front: two rows of three buttons, and the flap's edge between them.
                for y in [0.07, 0.15, 0.23] {
                    for x in [-0.055, 0.055] {
                        let (p, n) = torso_front(fig, maya, x, y);
                        b.ellipsoid(p + n * 0.002, Vec3::new(0.011, 0.011, 0.005), Quat::from_rotation_arc(Vec3::NEG_Z, n), fine.scaled(0.35), BUTTON);
                    }
                }
                let flap: Vec<(Vec3, f32)> = [(-0.028, 0.33), (0.0, 0.25), (0.022, 0.15), (0.03, 0.02)]
                    .iter()
                    .map(|&(x, y)| {
                        let (p, n) = torso_front(fig, maya, x, y);
                        (p + n * 0.001, 0.0035)
                    })
                    .collect();
                b.tube(&flap, fine.scaled(0.35), [0.82, 0.81, 0.78, 1.]);
            } else {
                // A shirt collar peeping out of the jumper's neck, and its ribbed hem.
                for side in [-1., 1.] {
                    b.ellipsoid(
                        Vec3::new(side * 0.034, 0.43, -0.052),
                        Vec3::new(0.030, 0.012, 0.024),
                        Quat::from_rotation_z(side * -0.55) * Quat::from_rotation_x(-0.5),
                        fine.scaled(0.4),
                        SHIRT,
                    );
                }
                b.torus(Vec3::new(0., 0.43, 0.004), Quat::from_rotation_x(-0.1), 0.064, 0.014, fine.scaled(0.6), KNIT_RIB);
                b.revolve(Vec3::new(0., -0.115, 0.), Quat::IDENTITY, fine.scaled(0.6), |u, a| {
                    let r = 1. / ((a.cos() / 0.180).powi(2) + (a.sin() / 0.156).powi(2)).sqrt();
                    (r + 0.006 * (u * std::f32::consts::PI).sin() + 0.0015 * (a * 60.).cos(), (u - 0.5) * 0.04)
                }, KNIT_RIB);
            }
        }
        Piece::Head => {
            let c = fig.head;
            let r = fig.head_radii;
            b.ellipsoid(c, r, Quat::IDENTITY, fine, colors.skin);
            // The neck, down into the collar.
            b.tube(&[(Vec3::new(0., -0.07, 0.006), 0.05), (Vec3::new(0., c.y - 0.06, 0.004), 0.048)], fine.scaled(0.5), colors.skin);
            // Ears.
            for side in [-1., 1.] {
                b.ellipsoid(c + Vec3::new(side * r.x * 0.97, -0.01, 0.012), Vec3::new(0.018, 0.034, 0.024), Quat::from_rotation_y(side * 0.3), fine.scaled(0.4), colors.skin);
            }
            // The nose: a soft round button.
            let (p, n) = face(fig, 0., -0.10, 0.);
            let nose = if maya { Vec3::new(0.019, 0.016, 0.013) } else { Vec3::new(0.032, 0.028, 0.024) };
            b.ellipsoid(p, nose, Quat::from_rotation_arc(Vec3::NEG_Z, n), fine.scaled(0.45), colors.nose);
            // Cheeks: a blush under each eye.
            for side in [-1., 1.] {
                let (p, n) = face(fig, side * 0.55, -0.22, 0.);
                b.ellipsoid(p - n * 0.006, Vec3::new(0.030, 0.019, 0.010), Quat::from_rotation_arc(Vec3::NEG_Z, n), fine.scaled(0.4), colors.blush);
            }
            // Brows: arched (Maya) or bushy (him), above the eyes.
            for side in [-1., 1.] {
                let points: Vec<(Vec3, f32)> = [(0.17, 0.32, 0.8), (0.33, 0.38, 1.0), (0.50, 0.33, 0.75)]
                    .iter()
                    .map(|&(x, y, w)| {
                        let (p, _) = face(fig, side * (x + fig.eyes.x - 0.36), y + fig.eyes.y - 0.06, 0.004);
                        (p, if maya { 0.0062 } else { 0.0105 } * w)
                    })
                    .collect();
                b.tube(&points, fine.scaled(0.35), colors.brow);
            }
            if maya {
                // Hair: dark, close round the head from temple to temple under the toque, framing the face and
                // longer at the nape, gathered into a bun at the back.
                b.revolve(c, Quat::IDENTITY, fine, |u, a| {
                    // How far round from the face (0) to the back of the head (1).
                    let back = ((a.sin() + 0.62) / 1.0).clamp(0., 1.);
                    let low = -0.045 - 0.075 * back;
                    let y = low + u * (0.165 - low);
                    let ring = 1. / ((a.cos() / r.x).powi(2) + (a.sin() / r.z).powi(2)).sqrt();
                    let skin = ring * (1. - (y / r.y).powi(2)).max(0.).sqrt();
                    let edge = (u * std::f32::consts::PI).sin().max(0.).powf(0.4);
                    let wave = 1. + 0.12 * (a * 9. + u * 4.).sin() * back;
                    (skin - 0.003 + (0.006 + 0.012 * back.sqrt()) * edge * wave * back.powf(0.25), y)
                }, colors.hair);
                b.ellipsoid(c + Vec3::new(0., 0.03, 0.17), Vec3::new(0.060, 0.056, 0.05), Quat::IDENTITY, fine.scaled(0.6), colors.hair);
                // A curl before each ear.
                for side in [-1., 1.] {
                    let curl: Vec<(Vec3, f32)> = [(0.80, 0.30, 0.012), (0.86, 0.10, 0.011), (0.80, -0.08, 0.009), (0.70, -0.16, 0.006)]
                        .iter()
                        .map(|&(x, y, w)| (face(fig, side * x, y, 0.004).0, w))
                        .collect();
                    b.tube(&curl, fine.scaled(0.4), colors.hair);
                }
                // Lashes at the outer corner of each eye.
                for side in [-1., 1.] {
                    let (p, n) = face(fig, side * (fig.eyes.x + 0.2), fig.eyes.y + 0.17, 0.0);
                    b.tube(&[(p - n * 0.004, 0.0035), (p + n * 0.006 + Vec3::new(side * 0.012, 0.008, 0.), 0.002)], fine.scaled(0.3), colors.brow);
                }
            } else {
                // A ginger fringe round the back of a bald crown, sideburns, and a big curled moustache.
                // The fringe: a band of tufts round the back of the head from ear to ear, thickest at the back.
                b.revolve(c + Vec3::new(0., 0., 0.004), Quat::IDENTITY, fine, |u, a| {
                    let back = (a.sin() * 1.25 + 0.25).clamp(0., 1.);
                    let band = (u * std::f32::consts::PI).sin().max(0.).powf(0.7);
                    let y = -0.06 + u * 0.085 * (0.55 + 0.45 * back);
                    let ring = 1. / ((a.cos() / r.x).powi(2) + (a.sin() / r.z).powi(2)).sqrt();
                    let tufts = 1. + 0.35 * (a * 14.).sin().abs() * band;
                    (ring * (1. - (y / r.y).powi(2)).max(0.).sqrt() - 0.004 + 0.017 * band * back * tufts, y)
                }, colors.hair);
                for side in [-1., 1.] {
                    b.ellipsoid(c + Vec3::new(side * r.x * 0.93, -0.035, -0.02), Vec3::new(0.016, 0.045, 0.028), Quat::IDENTITY, fine.scaled(0.4), colors.hair);
                    let curl: Vec<(Vec3, f32)> = [(0.02, -0.25, 0.011), (0.16, -0.28, 0.013), (0.31, -0.30, 0.011), (0.43, -0.26, 0.008), (0.47, -0.18, 0.005)]
                        .iter()
                        .map(|&(x, y, w)| (face(fig, side * x, y, 0.007).0, w))
                        .collect();
                    b.tube(&curl, fine.scaled(0.45), colors.hair);
                }
            }
        }
        Piece::Hat => {
            // A tall pleated toque on a band, tipped back a little.
            let c = fig.head + Vec3::new(0., fig.head_radii.y * 0.50, 0.012);
            let tip = Quat::from_rotation_x(0.16);
            b.revolve(c, tip, fine.scaled(0.6), |u, _| (0.142 + 0.004 * (u * std::f32::consts::PI).sin(), u * 0.07), shapes::WHITE);
            b.revolve(c + tip * Vec3::Y * 0.06, tip, fine, |u, a| {
                let bulge = (u * std::f32::consts::PI * 0.95).sin();
                let pleat = 1. + 0.05 * (a * 10.).cos() * (1. - u * 0.7);
                let r = (0.142 + 0.055 * bulge) * pleat * (1. - u.powi(6));
                (r, u * 0.20 - 0.05 * u.powi(6))
            }, shapes::WHITE);
        }
        Piece::Glasses => {
            for side in [-1., 1.] {
                let (p, n) = face(fig, side * fig.eyes.x, fig.eyes.y, 0.026);
                b.torus(p, Quat::from_rotation_arc(Vec3::Y, n), 0.042, 0.0045, fine.scaled(0.6), FRAME);
                // The arm back to the ear.
                let (ear, _) = face(fig, side * 0.97, 0.05, 0.0);
                b.tube(&[(p + Vec3::new(side * 0.042, 0., 0.), 0.004), (ear + Vec3::new(side * 0.004, 0.0, 0.03), 0.0035)], fine.scaled(0.3), FRAME);
            }
            let inner = fig.eyes.x - 0.042 / fig.head_radii.x;
            let (left, _) = face(fig, -inner, fig.eyes.y + 0.06, 0.027);
            let (right, _) = face(fig, inner, fig.eyes.y + 0.06, 0.027);
            let (mid, _) = face(fig, 0., fig.eyes.y + 0.10, 0.03);
            b.tube(&[(left, 0.004), (mid, 0.004), (right, 0.004)], fine.scaled(0.3), FRAME);
        }
        Piece::Eye => {
            // The white, a big brown iris, the pupil and a glint, facing -z.
            let e = fig.eye;
            b.ellipsoid(Vec3::ZERO, e, Quat::IDENTITY, fine.scaled(0.7), [0.97, 0.96, 0.93, 1.]);
            let front = |x: f32, y: f32, lift: f32| {
                let z = -(1. - (x / e.x).powi(2) - (y / e.y).powi(2)).max(0.).sqrt() * e.z;
                Vec3::new(x, y, z - lift)
            };
            let iris = e.x * 0.66;
            b.ellipsoid(front(0., -0.002, 0.), Vec3::new(iris, iris * 1.08, e.z * 0.18), Quat::IDENTITY, fine.scaled(0.6), colors.iris);
            b.ellipsoid(front(0., -0.002, 0.0025), Vec3::new(iris * 0.55, iris * 0.6, e.z * 0.1), Quat::IDENTITY, fine.scaled(0.5), [0.02, 0.015, 0.012, 1.]);
            b.ellipsoid(front(-iris * 0.4, iris * 0.45, 0.004), Vec3::splat(iris * 0.24), Quat::IDENTITY, fine.scaled(0.35), [1.6, 1.6, 1.6, 1.]);
            b.ellipsoid(front(iris * 0.35, -iris * 0.42, 0.004), Vec3::splat(iris * 0.11), Quat::IDENTITY, fine.scaled(0.3), [1.3, 1.3, 1.3, 1.]);
        }
        Piece::UpperArm => {
            let (a, z) = fig.limbs[0];
            b.tube(&[(Vec3::ZERO, a), (Vec3::new(0., -fig.upper_arm * 0.5, 0.), (a + z) * 0.5), (Vec3::new(0., -fig.upper_arm, 0.), z)], fine.scaled(0.7), shapes::WHITE);
        }
        Piece::Forearm => {
            let (a, z) = fig.limbs[1];
            let l = fig.forearm;
            if maya {
                // Bare forearms, the jacket's sleeves rolled up over the elbows.
                b.tube(&[(Vec3::new(0., -0.02, 0.), a), (Vec3::new(0., -l * 0.5, 0.), (a + z) * 0.5), (Vec3::new(0., -l + 0.01, 0.), z)], fine.scaled(0.7), colors.skin);
                b.torus(Vec3::new(0., -0.035, 0.), Quat::IDENTITY, a + 0.008, 0.014, fine.scaled(0.6), JACKET);
                b.torus(Vec3::new(0., -0.012, 0.), Quat::IDENTITY, a + 0.012, 0.013, fine.scaled(0.6), JACKET);
            } else {
                b.tube(&[(Vec3::ZERO, a), (Vec3::new(0., -l * 0.5, 0.), (a + z) * 0.5), (Vec3::new(0., -l + 0.03, 0.), z)], fine.scaled(0.7), shapes::WHITE);
                // The ribbed cuff.
                b.revolve(Vec3::new(0., -l + 0.005, 0.), Quat::IDENTITY, fine.scaled(0.5), |u, angle| (z + 0.002 + 0.0015 * (angle * 24.).cos(), u * 0.035), KNIT_RIB);
            }
        }
        Piece::Hand { grasp, left } => {
            let curl = grasp.curl();
            let hand = shapes::hand(curl, fine.scaled(0.8), colors.skin);
            let scale = if maya { 1. } else { 1.1 };
            let mut hand = if left { hand.mirrored() } else { hand };
            for p in hand.positions.iter_mut() {
                *p *= scale;
            }
            b.append(&hand);
        }
        Piece::Thigh => {
            let (a, z) = fig.limbs[2];
            b.tube(&[(Vec3::ZERO, a), (Vec3::new(0., -fig.thigh * 0.5, 0.), (a + z) * 0.5 + 0.004), (Vec3::new(0., -fig.thigh, 0.), z)], fine.scaled(0.7), shapes::WHITE);
        }
        Piece::Shin => {
            let (a, z) = fig.limbs[3];
            let l = fig.shin;
            b.tube(&[(Vec3::ZERO, a), (Vec3::new(0., -l * 0.6, 0.), z), (Vec3::new(0., -l + 0.035, 0.), z + 0.006)], fine.scaled(0.7), shapes::WHITE);
        }
        Piece::Foot => {
            // A round shoe, toes along -z, with a pale sole; the ankle at the origin.
            let (top, sole) = if maya { (CLOG, [0.2, 0.18, 0.17, 1.]) } else { (SLIPPER, SOLE) };
            let size = if maya { Vec3::new(0.055, 0.052, 0.125) } else { Vec3::new(0.062, 0.05, 0.135) };
            b.ellipsoid(Vec3::new(0., -0.026, -0.052), size, Quat::IDENTITY, fine.scaled(0.7), top);
            b.ellipsoid(Vec3::new(0., -fig.ankle + 0.012, -0.052), Vec3::new(size.x * 1.04, 0.014, size.z * 1.03), Quat::IDENTITY, fine.scaled(0.6), sole);
        }
        Piece::Mouth(viseme) => {
            let palette = MouthColors { lip: colors.lip, inside: MOUTH_INSIDE, teeth: TEETH, tongue: TONGUE };
            b = shapes::mouth(viseme, fig.head, fig.head_radii, mouth_dir(fig), palette, fine);
        }
    }
    b
}

/// A point on the front of the torso at (x, y) in the chest's frame, and the surface's normal there (roughly).
fn torso_front(fig: &Figure, maya: bool, x: f32, y: f32) -> (Vec3, Vec3) {
    let _ = fig;
    // The rows the torso is lofted through, sampled straight (the buttons sit where the bust is gentle).
    let rows: &[(f32, f32, f32, f32)] =
        if maya { &[(0.0, 0.152, 0.122, 0.), (0.10, 0.160, 0.126, -0.008), (0.19, 0.172, 0.134, -0.016), (0.27, 0.190, 0.124, -0.006), (0.33, 0.198, 0.108, 0.004)] } else { &[(0.07, 0.206, 0.196, -0.05)] };
    let k = rows.iter().position(|r| r.0 >= y).unwrap_or(rows.len() - 1).max(1).min(rows.len() - 1);
    let (a, b) = (rows[k - 1.min(k)], rows[k]);
    let t = if (b.0 - a.0).abs() < 1e-6 { 0. } else { ((y - a.0) / (b.0 - a.0)).clamp(0., 1.) };
    let (rx, rz, cz) = (a.1 + (b.1 - a.1) * t, a.2 + (b.2 - a.2) * t, a.3 + (b.3 - a.3) * t);
    let s = (x / rx).clamp(-0.95, 0.95);
    let z = cz - rz * (1. - s * s).sqrt();
    let n = Vec3::new(s / rx, 0., (z - cz) / (rz * rz)).normalize();
    (Vec3::new(x, y, z), n)
}

/// The cloth patterns, painted in code: Maya's black-and-white check (a 32-pixel tile, two checks), his knitted
/// jumper's cables and his corduroy's ribs. Each repeats.
pub mod tiles {
    use super::shapes::tile;
    use bevy::prelude::*;
    pub fn check() -> Image {
        tile(|x, y| {
            let (a, b) = ((x / 8) % 2, (y / 8) % 2);
            // Houndstooth-ish: a check with a tooth cut into each square's corner.
            let tooth = (x % 8 + y % 8) < 4 && (x % 8) < 3;
            let dark = (a ^ b == 1) ^ tooth;
            let c = if dark { 0.10 } else { 0.90 };
            [c, c, c * 1.01]
        })
    }
    pub fn knit() -> Image {
        tile(|x, y| {
            // Rows of little V stitches, and a cable down every other column.
            let col = x % 8;
            let v = ((col as f32 - 3.5).abs() - (y % 6) as f32 * 0.6).abs() < 1.;
            let cable = (x / 8) % 2 == 0 && ((y as f32 * 0.52).sin() * 2.2 - (col as f32 - 3.5)).abs() < 1.3;
            let shade = if cable { 1.08 } else if v { 1.0 } else { 0.86 };
            [0.13 * shade, 0.43 * shade, 0.45 * shade]
        })
    }
    pub fn cord() -> Image {
        tile(|x, _| {
            let rib = ((x as f32 / 4. * std::f32::consts::TAU).cos() * 0.5 + 0.5) * 0.18 + 0.82;
            [0.42 * rib, 0.27 * rib, 0.16 * rib]
        })
    }
}
