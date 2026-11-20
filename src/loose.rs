// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Loose objects on screen: the big silver bowl, crates, pillows, napkins, chairs and trays, and the kitchen's own
//! pieces (cereal boxes, mugs, plates, tins, cutting boards, cereal bowls, rolls of paper towels, salt and pepper
//! shakers and a bread bin, at their true sizes beside a hotdog) that a map puts out and the kitchen then knocks about. Their physics is `kitchen_sim::props`; this file gives each one a
//! body to look at, built from the same part lists the physics uses (the bowl's profile turned on a lathe,
//! rounded boxes for the rest), keeps it where the physics has it, and makes the noise when it lands.
//!
//! Offline the props are the local `Feast`'s. Online the server owns them: `apply` copies the states a
//! snapshot carries into the client's own copy of the props, which the player's hotdog collides with and
//! which moves by itself between snapshots, so that a shove answers at once.
//!
//! A napkin is never drawn as the flat pad it is in the physics. Loose or carried it is a sheet of cloth
//! (`crate::cloth`, one for each napkin, kept in `Sheets`) that the pad drives: it hangs from its carrier's
//! hands, billows as it flies and settles in folds over what it lands on. Draped (kitchen_sim::drape), `cloth`
//! lays a sheet over the frozen props it covers, following their tops, with its sides hanging in folds down
//! to the counter round the drape's wall.
use bevy::{
    asset::RenderAssetUsages,
    audio::Volume,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};
use kitchen_sim::drape::{Drape, NAPKIN};
use kitchen_sim::feast::Feast;
use kitchen_sim::map::{Map, LOOSE};
use kitchen_sim::props::{Part, PropState, PropWorld, ShapeSpec, State};
use crate::cloth::{Body, Cloth, Drive, Solid, Surroundings};
use std::collections::{HashMap, VecDeque};
use std::f32::consts::{FRAC_PI_2, TAU};

const FLOOR: f64 = kitchen_sim::map::FLOOR;
/// How quickly a body on screen closes on where the physics has it, per second. The physics moves in steps of
/// 1/120 s and snapshots arrive 30 times a second; this evens both out.
const FOLLOW: f32 = 40.;
/// Further than this from where it should be, a body is put there at once: a new round, a new map.
const SNAP: f32 = 0.25;
/// Online, the client's copy of a moving prop is ahead of the server's by the journey there and back when
/// it is this player who shoves it, so the server's place for it is compared with where the copy has just
/// been (`TRAIL` frames of it). On that trail, within `SLACK` metres, there is nothing to correct; within
/// `REACH` and turned much the same way (`ALIGNED`, the cosine of half the angle between them), the copy
/// keeps its own motion and gives way by `GIVE` of the difference; further out it takes the server's word.
const TRAIL: usize = 48;
const SLACK: f64 = 0.01;
const REACH: f64 = 0.1;
const GIVE: f64 = 0.3;
const ALIGNED: f64 = 0.985;

/// The body of one loose object: the map's loose object number `id`, which is also its number in the props.
#[derive(Component)]
pub struct Loose {
    pub id: usize,
    /// Not placed yet: the first placing is not smoothed.
    fresh: bool,
}
/// The cloth of a draped napkin: the drape's number.
#[derive(Component)]
pub struct Draped {
    pub id: u32,
}
/// The sheet of a loose or carried napkin: the napkin's number among the props.
#[derive(Component)]
#[allow(dead_code)]
pub struct Sheet(usize);
/// A napkin somebody carries, and where its two grips are this frame: the carrier's hands, or for this
/// player in first person two points low in the view. `arena` works them out.
#[derive(Clone, Copy, Debug)]
pub struct Hold {
    pub prop: usize,
    pub grips: [Vec3; 2],
}
/// Every napkin's sheet: its entity, its mesh and the cloth itself, by the napkin's number.
#[derive(Resource, Default)]
pub struct Sheets {
    pub by: HashMap<usize, (Entity, Handle<Mesh>, Cloth)>,
    /// The napkins carried last frame and where their grips were, and what the sheets cost a frame (milliseconds,
    /// smoothed), for the checks.
    pub holds: Vec<Hold>,
    pub cost: f32,
}
impl Sheets {
    /// For the checks (`window.hotdogState.sheets`): each sheet's napkin, whether it sleeps, the steps it has
    /// taken, and the box round it.
    pub fn report(&self) -> serde_json::Value {
        let mut ids: Vec<&usize> = self.by.keys().collect();
        ids.sort();
        ids.iter()
            .map(|id| {
                let cloth = &self.by[*id].2;
                let (low, high) = cloth.bounds();
                let grips = self.holds.iter().find(|h| h.prop == **id).map(|h| h.grips.map(|g| g.to_array()));
                serde_json::json!({"id": id, "asleep": cloth.asleep, "steps": cloth.steps, "low": low.to_array(), "high": high.to_array(), "grips": grips})
            })
            .collect()
    }
    /// For the checks: what stepping and reshaping every sheet takes a frame, in milliseconds (smoothed).
    pub fn cost(&self) -> f32 {
        self.cost
    }
}

/// What each kind is made of: one mesh per part, where it sits in the shape's frame, and its material.
#[derive(Resource)]
struct Kit {
    kinds: Vec<Vec<(Handle<Mesh>, Vec3, Handle<StandardMaterial>)>>,
    /// How tall each kind stands.
    tall: Vec<f32>,
    /// The sounds; none without an asset server, as in the tests.
    ding: Option<Handle<AudioSource>>,
    boom: Option<Handle<AudioSource>>,
    /// The last knock heard, offline and online.
    heard: u32,
    told: u32,
    /// Online: the server's latest word on each prop it has mentioned.
    said: HashMap<usize, PropState>,
    /// Online: where each prop that is moving in the client's copy has just been, oldest first.
    trails: HashMap<usize, VecDeque<[f64; 3]>>,
    /// The napkin's cloth, for drapes.
    cloth: Handle<StandardMaterial>,
}
impl FromWorld for Kit {
    fn from_world(world: &mut World) -> Self {
        let server = world.get_resource::<AssetServer>();
        let (ding, boom) = (server.map(|server| server.load("audio/ding.wav")), server.map(|server| server.load("audio/boom.wav")));
        // The kitchen's printed and grained surfaces: every picture made here (`art`), none without an image store, as in
        // the tests.
        let printed = world.get_resource_mut::<Assets<Image>>().map(|mut images| [art::cereal(), art::check(), art::board(), art::bread()].map(|image| images.add(image)));
        let [cereal_art, check_art, board_art, bread_art] = match printed {
            Some(images) => images.map(Some),
            None => [None, None, None, None],
        };
        let mut materials = world.resource_mut::<Assets<StandardMaterial>>();
        let mut metal = |r: f32, g: f32, b: f32, roughness: f32| {
            materials.add(StandardMaterial { base_color: Color::srgb(r, g, b), metallic: 1., perceptual_roughness: roughness, reflectance: 0.6, ..default() })
        };
        // The owner's large silver baking bowl: polished enough to show the room, brushed by `visuals::surfaces`.
        let (silver, steel) = (metal(0.90, 0.93, 0.97, 0.12), metal(0.52, 0.56, 0.60, 0.28));
        // The shakers' caps a satin steel: half metal, so that `visuals::surfaces` leaves them as they are (its brushed
        // steel wants a tangent frame that their small turned meshes do not give it).
        let cap = materials.add(StandardMaterial { base_color: Color::srgb(0.80, 0.82, 0.85), metallic: 0.45, perceptual_roughness: 0.24, reflectance: 0.9, ..default() });
        // The napkin's cloth, loose, carried or draped: a red and white gingham (`art::check`), soft, seen from both
        // sides where a fold turns its back; its weave and hem are in the mesh's colours.
        let cloth = materials.add(StandardMaterial { base_color: Color::WHITE, base_color_texture: check_art.clone(), perceptual_roughness: 0.92, double_sided: true, cull_mode: None, ..default() });
        let mut plain = |r: f32, g: f32, b: f32, roughness: f32| crate::mat(&mut materials, Color::srgb(r, g, b), roughness);
        let (crate_wood, pillow, chair) = (plain(0.56, 0.37, 0.19, 0.7), plain(0.90, 0.87, 0.78, 0.95), plain(0.66, 0.46, 0.25, 0.6));
        let mut materials = world.resource_mut::<Assets<StandardMaterial>>();
        let mut textured = |image: Option<Handle<Image>>, roughness: f32, reflectance: f32| materials.add(StandardMaterial { base_color: Color::WHITE, base_color_texture: image, perceptual_roughness: roughness, reflectance, ..default() });
        // Printed card, a board's wood with its grain, the bread bin's enamel.
        let (card, board, enamel) = (textured(cereal_art, 0.75, 0.4), textured(board_art, 0.62, 0.35), textured(bread_art, 0.32, 0.55));
        // What the meshes' colours paint: paper and card, glazed china, tinplate, salt and pepper (rough grains).
        let paper = materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.85, ..default() });
        let china = materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.16, reflectance: 0.7, ..default() });
        let tinplate = materials.add(StandardMaterial { base_color: Color::WHITE, metallic: 0.15, perceptual_roughness: 0.32, reflectance: 0.6, ..default() });
        let grains = materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.95, ..default() });
        // The shakers' glass: clear, a little green at its thickest, glossy.
        let glass = materials.add(StandardMaterial {
            base_color: Color::srgba(0.84, 0.93, 0.91, 0.32),
            alpha_mode: AlphaMode::Blend,
            perceptual_roughness: 0.04,
            reflectance: 0.9,
            ..default()
        });
        let add = |world: &mut World, mesh: Mesh| world.resource_mut::<Assets<Mesh>>().add(mesh);
        // A turned piece: its shell, as few rows round its outline as its shape needs (`outline`), and its paint (`art::paint`:
        // bands, rings, speckles, a quilt, a cap's holes) in a picture of its own laid over it (`art::baked`), in a material of
        // its own that is `base` with that picture. Without an image store (the tests) it keeps `base` unpainted. Each paint
        // is one piece's, so a piece made twice (the salt's cap and the pepper's) is made once, mesh, picture and all.
        let store = world.contains_resource::<Assets<Image>>();
        let mut made: HashMap<String, (Handle<Mesh>, Handle<StandardMaterial>)> = HashMap::new();
        let mut turned = |world: &mut World, profile: &[Vec2], radius: f32, paint: &str, base: &Handle<StandardMaterial>| {
            if let Some(piece) = made.get(paint) {
                return piece.clone();
            }
            let rows = outline(profile, radius);
            let material = if store {
                let (image, uv) = art::baked(&rows, paint);
                let image = world.resource_mut::<Assets<Image>>().add(image);
                let mut materials = world.resource_mut::<Assets<StandardMaterial>>();
                let mut material = materials.get(base).cloned().unwrap_or_default();
                (material.base_color_texture, material.uv_transform) = (Some(image), uv);
                materials.add(material)
            } else {
                base.clone()
            };
            let piece = (add(world, turn(&rows)), material);
            made.insert(paint.to_string(), piece.clone());
            piece
        };
        let tall = LOOSE
            .iter()
            .map(|kind| {
                let top = |part: &Part| match part {
                    Part::Box { center, half, .. } => center[1] + half[1],
                    Part::Lathe { center, profile, radius, .. } => center[1] + radius + profile.iter().fold(0., |top: f64, p| top.max(p[1])),
                };
                ShapeSpec::named(kind).map_or(0., |spec| spec.parts.iter().map(top).fold(0., f64::max)) as f32
            })
            .collect();
        let v = |x: f32, y: f32| Vec2::new(x, y);
        let kinds = LOOSE
            .iter()
            .map(|kind| {
                let spec = ShapeSpec::named(kind).expect("every loose kind has a shape");
                // Most kinds are drawn from the physics' own parts; the kitchen's pieces have more to them (a mug's handle
                // and its coffee, a plate's rim and foot, the cardboard core of a roll of paper towels, a shaker's glass,
                // its salt and its cap), drawn round the same outline.
                let at = |(mesh, material): (Handle<Mesh>, Handle<StandardMaterial>), centre: Vec3| (mesh, centre, material);
                match *kind {
                    "cereal" => vec![(add(world, art::boxed(rounded_box([0.115, 0.165, 0.04], 0.003), [0.115, 0.165, 0.04])), Vec3::new(0., 0.165, 0.), card.clone())],
                    "breadbox" => vec![
                        (add(world, art::boxed(rounded_box([0.2, 0.1, 0.125], 0.012), [0.2, 0.1, 0.125])), Vec3::new(0., 0.1, 0.), enamel.clone()),
                        // The roll top's wooden knob, on the front.
                        (add(world, art::painted(knob(), "knob")), Vec3::new(0., 0.118, 0.125), china.clone()),
                    ],
                    "board" => vec![(add(world, art::slab([0.225, 0.0125, 0.15], [0.162, -0.05, 0.198, 0.05])), Vec3::new(0., 0.0125, 0.), board.clone())],
                    "cup" => vec![
                        at(turned(world, &[v(0., 0.0035), v(0.036, 0.0035), v(0.0395, 0.012), v(0.0395, 0.0975)], 0.0025, "cup", &china), Vec3::ZERO),
                        at(turned(world, &[v(0., 0.082), v(0.0372, 0.082)], 0.0012, "coffee", &china), Vec3::ZERO),
                        (add(world, art::painted(handle(0.022, 0.0052), "cup")), Vec3::new(0.0405, 0.052, 0.), china.clone()),
                    ],
                    "plate" => vec![
                        at(turned(world, &[v(0., 0.004), v(0.075, 0.004), v(0.095, 0.006), v(0.112, 0.012), v(0.1255, 0.0165)], 0.0025, "plate", &china), Vec3::ZERO),
                        at(turned(world, &[v(0.068, 0.0015), v(0.068, 0.0035)], 0.0015, "foot", &china), Vec3::ZERO),
                    ],
                    "dish" => vec![at(turned(world, &[v(0., 0.0035), v(0.038, 0.0035), v(0.064, 0.0315), v(0.0715, 0.0615)], 0.0035, "dish", &china), Vec3::ZERO)],
                    "roll" => vec![
                        at(turned(world, &[v(0.0215, 0.0015), v(0.058, 0.0015), v(0.058, 0.2785), v(0.0215, 0.2785)], 0.0015, "roll", &paper), Vec3::ZERO),
                        at(turned(world, &[v(0.0195, 0.0016), v(0.0195, 0.2786)], 0.0012, "core", &paper), Vec3::ZERO),
                    ],
                    "salt" | "pepper" => vec![
                        at(turned(world, &[v(0., 0.0026), v(0.0188, 0.0026), v(0.0193, 0.047), v(0., 0.047)], 0.0008, kind, &grains), Vec3::ZERO),
                        (add(world, lathe(&[v(0., 0.0012), v(0.0205, 0.0012), v(0.0213, 0.064)], 0.0012)), Vec3::ZERO, glass.clone()),
                        at(turned(world, &cap_profile(), 0.0012, "cap", &cap), Vec3::ZERO),
                    ],
                    _ => {
                        let material = match *kind {
                            "bowl" => silver.clone(),
                            "crate" => crate_wood.clone(),
                            "pillow" => pillow.clone(),
                            "napkin" => cloth.clone(),
                            "chair" => chair.clone(),
                            "tray" => steel.clone(),
                            "tin" => tinplate.clone(),
                            _ => crate_wood.clone(),
                        };
                        spec.parts
                            .iter()
                            .map(|part| match part {
                                Part::Box { center, half, radius, .. } => (add(world, rounded_box(half.map(|v| v as f32), *radius as f32)), Vec3::from_array(center.map(|v| v as f32)), material.clone()),
                                Part::Lathe { center, profile, radius, .. } => {
                                    let profile: Vec<Vec2> = profile.iter().map(|p| Vec2::new(p[0] as f32, p[1] as f32)).collect();
                                    let centre = Vec3::from_array(center.map(|v| v as f32));
                                    // The kitchen's turned pieces (a tin) are painted with their bands and rings; the bowl is plain silver.
                                    if *kind == "bowl" {
                                        (add(world, art::painted(lathe(&profile, *radius as f32), kind)), centre, material.clone())
                                    } else {
                                        at(turned(world, &profile, *radius as f32, kind, &material), centre)
                                    }
                                }
                            })
                            .collect()
                    }
                }
            })
            .collect();
        Kit { kinds, tall, ding, boom, heard: 0, told: 0, said: HashMap::new(), trails: HashMap::new(), cloth }
    }
}

/// A mug's handle: a round bar bent into half a ring `bend` metres about its middle (standing upright in the x-y plane,
/// bulging towards +x from its ends at x = 0), `thick` metres about.
fn handle(bend: f32, thick: f32) -> Mesh {
    // Steps along the bend and round the bar: no chord strays a third of a millimetre from the round.
    const ALONG: u32 = 12;
    const ROUND: u32 = 8;
    let (mut positions, mut normals, mut uvs, mut indices) = (vec![], vec![], vec![], vec![]);
    for i in 0..=ALONG {
        // From the bottom end (-90 degrees) over to the top end (+90): a little more, so that it sinks into the mug.
        let a = (i as f32 / ALONG as f32 - 0.5) * std::f32::consts::PI * 1.08;
        let (centre, out) = (Vec3::new(a.cos() * bend - 0.004, a.sin() * bend, 0.), Vec3::new(a.cos(), a.sin(), 0.));
        for j in 0..=ROUND {
            let b = j as f32 / ROUND as f32 * TAU;
            let normal = out * b.cos() + Vec3::Z * b.sin();
            positions.push((centre + normal * thick).to_array());
            normals.push(normal.to_array());
            uvs.push([i as f32 / ALONG as f32, j as f32 / ROUND as f32]);
        }
    }
    for i in 0..ALONG {
        for j in 0..ROUND {
            let (a, b) = (i * (ROUND + 1) + j, (i + 1) * (ROUND + 1) + j);
            indices.extend([a, a + 1, b, b, a + 1, b + 1]);
        }
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_indices(Indices::U32(indices))
}
/// The bread bin's knob: a little turned button standing out of its front (its axis along +z).
fn knob() -> Mesh {
    let mut mesh = lathe(&[Vec2::new(0., 0.), Vec2::new(0.009, 0.), Vec2::new(0.011, 0.006), Vec2::new(0.008, 0.013), Vec2::new(0., 0.014)], 0.0015);
    mesh.rotate_by(Quat::from_rotation_x(FRAC_PI_2));
    mesh
}

/// A salt or pepper shaker's cap, as a profile to turn: its skirt, its shoulder and its flat top, the top with points at
/// 8.5, 6.5 and 4.5 mm from the middle: rings of the mesh round the holes, which `art::paint` puts 6.5 mm out. (Without
/// them the top is one fan of long thin wedges to the middle, and the picture is smeared across each.)
fn cap_profile() -> Vec<Vec2> {
    let mut profile = vec![Vec2::new(0.0218, 0.0625), Vec2::new(0.0218, 0.072), Vec2::new(0.0205, 0.0845), Vec2::new(0.014, 0.0885)];
    profile.extend([0.0085, 0.0065, 0.0045, 0.].map(|r| Vec2::new(r, 0.0885 + 0.0001 * (1. - r / 0.014))));
    profile
}

/// The meshes a loose object kind (map::LOOSE's order) is drawn with, each with where it sits in the kind's own frame:
/// what the placement preview's ghost and its glows are drawn with (preview.rs), the same shape as the object. None
/// before the first map has been put out.
pub fn meshes(world: &World, kind: usize) -> Vec<(Handle<Mesh>, Vec3)> {
    world.get_resource::<Kit>().and_then(|kit| kit.kinds.get(kind)).map_or(vec![], |parts| parts.iter().map(|(mesh, centre, _)| (mesh.clone(), *centre)).collect())
}

/// A box of the given half extents whose edges and corners are rounded with `radius`, as the physics has it.
fn rounded_box(half: [f32; 3], radius: f32) -> Mesh {
    let radius = radius.min(half[0]).min(half[1]).min(half[2]).max(1e-5);
    // Lines across each face: its edges, where the rounding ends, and one line half way round each edge.
    let lines = |h: f32| [-h, -(h - 0.586 * radius), -(h - radius), h - radius, h - 0.586 * radius, h];
    let (mut positions, mut normals, mut uvs, mut indices) = (vec![], vec![], vec![], vec![]);
    for axis in 0..3 {
        let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
        for side in [-1f32, 1.] {
            let first = positions.len() as u32;
            let (us, vs) = (lines(half[u]), lines(half[v]));
            for a in us {
                for b in vs {
                    let mut q = [0f32; 3];
                    (q[axis], q[u], q[v]) = (side * half[axis], a, b);
                    let core = Vec3::from_array(std::array::from_fn(|k| q[k].clamp(-(half[k] - radius), half[k] - radius)));
                    let normal = (Vec3::from_array(q) - core).normalize_or(Vec3::from_array(std::array::from_fn(|k| if k == axis { side } else { 0. })));
                    positions.push((core + normal * radius).to_array());
                    normals.push(normal.to_array());
                    uvs.push([a * 6., b * 6.]);
                }
            }
            for i in 0..5u32 {
                for j in 0..5u32 {
                    let (p, q, r, s) = (first + i * 6 + j, first + (i + 1) * 6 + j, first + (i + 1) * 6 + j + 1, first + i * 6 + j + 1);
                    // u, v and the axis are right-handed in that order, so this winding faces out on the far side.
                    if side > 0. { indices.extend([p, q, r, p, r, s]) } else { indices.extend([p, r, q, p, s, r]) }
                }
            }
        }
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_indices(Indices::U32(indices))
}

/// A shell turned round the y axis: `profile` is its middle line as (distance from the axis, height), from the
/// axis outwards, and `radius` half its thickness. The outline runs along the outside, round the rim and back
/// along the inside.
fn lathe(profile: &[Vec2], radius: f32) -> Mesh {
    turn(&outline(profile, radius))
}
/// The outline a shell is turned from (see `lathe`), as the rows of its mesh: each a point (distance from the axis, height)
/// and the way the skin faces there. A row is put in only where the shape bends: a corner of the profile is rounded
/// outside in three rows and creased inside in one, a point on a straight stretch is one row either side, and the rim is
/// half a turn, in four steps on a thin piece and six on a thick one. (Its paint does not want more: `art::baked` paints it.)
fn outline(profile: &[Vec2], radius: f32) -> Vec<(Vec2, Vec2)> {
    let count = profile.len();
    // The outward side of each stretch of the profile: below the base, outside the wall.
    let sides: Vec<Vec2> = profile.windows(2).map(|pair| { let along = (pair[1] - pair[0]).normalize(); Vec2::new(along.y, -along.x) }).collect();
    let mut outline: Vec<(Vec2, Vec2)> = vec![(profile[0] + sides[0] * radius, sides[0])];
    // Outside, a corner of the profile is rounded; where the profile runs on straight (within 2 degrees), one row.
    for i in 1..count - 1 {
        let steps = if sides[i - 1].dot(sides[i]) > 0.9994 { 0 } else { 2 };
        for step in 0..=steps {
            let normal = sides[i - 1].lerp(sides[i], if steps == 0 { 0.5 } else { step as f32 / steps as f32 }).normalize();
            outline.push((profile[i] + normal * radius, normal));
        }
    }
    // The rim: half a turn from the outside to the inside.
    let (last, out) = (profile[count - 1], sides[count - 2]);
    let along = Vec2::new(-out.y, out.x);
    let steps = if radius <= 0.003 { 4 } else { 6 };
    for step in 0..=steps {
        let angle = step as f32 / steps as f32 * 2. * FRAC_PI_2;
        let normal = out * angle.cos() + along * angle.sin();
        outline.push((last + normal * radius, normal));
    }
    // Inside, a corner is a crease: one point where the two faces meet.
    for i in (1..count - 1).rev() {
        let normal = -(sides[i - 1] + sides[i]).normalize();
        let reach = radius / (-normal).dot(sides[i]).max(0.5);
        outline.push((profile[i] + normal * reach, normal));
    }
    outline.push((profile[0] - sides[0] * radius, -sides[0]));
    outline
}
/// How many steps round the axis a turned piece reaching `reach` metres from it is made in: enough that no chord strays
/// further than SAG inside the circle (a third of a pixel at a metre, seen at 1080 lines), in fours, from 12 to 48.
fn steps_round(reach: f32) -> u32 {
    const SAG: f32 = 0.00035;
    let steps = (std::f32::consts::PI / (1. - SAG / reach.max(SAG)).acos()).ceil() as u32;
    (steps.div_ceil(4) * 4).clamp(12, 48)
}
/// The mesh of an outline (`outline`) turned round the y axis. Its texture co-ordinates run four times round it across,
/// and eight times a metre along the outline down.
fn turn(outline: &[(Vec2, Vec2)]) -> Mesh {
    let around = steps_round(outline.iter().fold(0., |reach: f32, (point, _)| reach.max(point.x)));
    let (mut positions, mut normals, mut uvs, mut indices) = (vec![], vec![], vec![], vec![]);
    let mut travelled = 0.;
    for (i, (point, normal)) in outline.iter().enumerate() {
        if i > 0 {
            travelled += point.distance(outline[i - 1].0);
        }
        for step in 0..=around {
            let (sin, cos) = (step as f32 / around as f32 * TAU).sin_cos();
            positions.push([point.x * cos, point.y, point.x * sin]);
            normals.push([normal.x * cos, normal.y, normal.x * sin]);
            uvs.push([step as f32 / around as f32 * 4., travelled * 8.]);
        }
    }
    for i in 0..outline.len() as u32 - 1 {
        for step in 0..around {
            let (a, b) = (i * (around + 1) + step, (i + 1) * (around + 1) + step);
            indices.extend([a, b, a + 1, b, b + 1, a + 1]);
        }
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_indices(Indices::U32(indices))
}

/// Puts out a body for each of the map's loose objects, where the map has it. They are `MapPart`s, so the
/// next map replaces them. Called by `level::build`.
pub fn build(world: &mut World, map: &Map) {
    world.init_resource::<Kit>();
    let mut kit = world.resource_mut::<Kit>();
    kit.said.clear();
    kit.trails.clear();
    let kinds = world.resource::<Kit>().kinds.clone();
    // The sheets of the last map's napkins went with it (they are `MapPart`s).
    world.get_resource_or_insert_with(Sheets::default).by.clear();
    for (id, item) in map.loose.iter().enumerate() {
        let Some(kind) = LOOSE.iter().position(|kind| *kind == item.kind) else {
            continue;
        };
        let at = Transform::from_xyz(item.p[0] as f32, (FLOOR + item.base) as f32, item.p[1] as f32).with_rotation(Quat::from_rotation_y(item.yaw as f32));
        let root = world.spawn((crate::level::MapPart, Loose { id, fresh: true }, Name::new("Loose object"), at, Visibility::default())).id();
        for (mesh, centre, material) in &kinds[kind] {
            let part = world.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(material.clone()), Transform::from_translation(*centre))).id();
            world.entity_mut(root).add_child(part);
        }
    }
}

/// Every frame: the bodies follow the props, and the napkins' sheets their pads (`holds` says who carries
/// which, and where its grips are; `bodies` are the hotdogs the sheets bump into). Offline the round's new
/// knocks are heard; online a prop that has come to rest in the client's copy is put exactly where it rests
/// on the server.
/// `mine` is what this player carries in first person: its body in the world is hidden (`held` draws it in the
/// view instead; a napkin's sheet hangs from the view in the world).
pub fn update(world: &mut World, feast: &mut Feast, online: bool, dt: f32, holds: &[Hold], bodies: &[Body], mine: Option<usize>) {
    if !world.contains_resource::<Kit>() {
        return;
    }
    if online {
        let mut kit = world.resource_mut::<Kit>();
        for (id, theirs) in &kit.said {
            if *id < feast.props.len() && theirs.state != State::Awake {
                let mine = feast.props.get_state(*id);
                if mine.state != State::Awake && differs(&mine, theirs) {
                    feast.props.set_state(*id, theirs);
                }
            }
        }
        for id in 0..feast.props.len() {
            if feast.props.state(id) == State::Awake {
                let trail = kit.trails.entry(id).or_default();
                trail.push_back(feast.props.position(id));
                if trail.len() > TRAIL {
                    trail.pop_front();
                }
            } else {
                kit.trails.remove(&id);
            }
        }
    } else {
        // A new round counts its knocks from one again.
        let mut heard = world.resource::<Kit>().heard;
        if feast.knocks.iter().all(|knock| knock.id < heard) {
            heard = 0;
        }
        for knock in feast.knocks.iter().filter(|knock| knock.id > heard) {
            clang(world, &feast.props, knock.prop as usize, knock.other, knock.speed);
        }
        world.resource_mut::<Kit>().heard = feast.knocks.iter().map(|knock| knock.id).fold(heard, u32::max);
    }
    place(world, &feast.props, feast.bodies.config.kitchen_floor as f32, dt, mine);
    drapes(world, feast);
    sheets(world, feast, holds, bodies, dt);
}

/// A loose object kind's first-person model, hung from the game's camera so that it is drawn over the world on
/// the view model's layer (viewmodel.rs), as a gun is: never sunk into a wall in front, nor into the counter
/// where it is drawn lower than it is held (see `held_place`).
#[derive(Component)]
struct Held(usize);
/// How far under the line of sight, looking level, the top of a carried object is kept in first person, in
/// radians (7 degrees): the crosshair, and what it is aimed at, stay in view.
pub const CLEAR: f32 = 0.122;
/// The share of the view's sway (`arena::Game::sway`: sideways behind a turn, and the stride's pace) that a
/// carried object takes: a little.
const SWAY: f32 = 0.5;
/// The corners of a loose object kind, in its own frame: those of each box it is made of, and round the rim and the
/// foot of a turned piece (all of them, so that its highest point is among them however it is held: tipped, a box's
/// side is its top).
fn tops(kind: usize) -> Vec<Vec3> {
    let Some(spec) = LOOSE.get(kind).and_then(|name| ShapeSpec::named(name)) else {
        return vec![];
    };
    spec.parts
        .iter()
        .flat_map(|part| match part {
            Part::Box { center, half, .. } => {
                let (c, h) = (Vec3::from_array(center.map(|v| v as f32)), Vec3::from_array(half.map(|v| v as f32)));
                [(-1., -1., -1.), (-1., -1., 1.), (1., -1., -1.), (1., -1., 1.), (-1., 1., -1.), (-1., 1., 1.), (1., 1., -1.), (1., 1., 1.)].map(|(x, y, z)| c + Vec3::new(x * h.x, y * h.y, z * h.z)).to_vec()
            }
            Part::Lathe { center, profile, radius, .. } => {
                let wide = profile.iter().fold(0f32, |m, p| m.max(p[0] as f32)) + *radius as f32;
                let tall = profile.iter().fold(0f32, |m, p| m.max(p[1] as f32)) + *radius as f32;
                let c = Vec3::from_array(center.map(|v| v as f32));
                (0..32).map(|i| c + Vec3::new((i as f32 / 16. * TAU).cos() * wide, if i < 16 { tall } else { -*radius as f32 }, (i as f32 / 16. * TAU).sin() * wide)).collect()
            }
        })
        .collect()
}
/// Where a loose object of `kind` that this player carries is drawn in first person, in the frame of `eye` (the
/// game's camera, without its recoil): at its true size (a scale of 1) and where the simulation holds it (`held`,
/// its place in the world: in front of the hotdog and a little above its feet), turned as it is. Only if its
/// top would stand within CLEAR of the line of sight (looking level) is it let down, just far enough to keep the
/// crosshair clear: a pillow, the tray and a napkin are held low enough already; a crate, a chair and the bowl
/// stand taller than the eye where they are held. `sway` moves it a little sideways behind a turn and bobs it
/// with the stride (`sway.z` the stride's phase, which keeps step with the pace; `_now` no longer moves it).
pub fn held_place(kind: usize, held: Transform, eye: Transform, sway: Vec3, _now: f32) -> Transform {
    // Level, along the way it is held from the eye.
    let out = held.translation - eye.translation;
    let ahead = Vec3::new(out.x, 0., out.z).normalize_or(held.rotation * Vec3::NEG_Z);
    let lower = tops(kind)
        .into_iter()
        .map(|corner| {
            let at = held.transform_point(corner) - eye.translation;
            at.y + CLEAR.tan() * at.dot(ahead).max(0.)
        })
        .fold(0f32, f32::max);
    let side = Vec3::new(-ahead.z, 0., ahead.x);
    let bob = side * sway.x * SWAY + Vec3::Y * (sway.z.sin() * 0.004 * sway.y * SWAY - lower);
    let back = eye.rotation.inverse();
    Transform { translation: back * (out + bob), rotation: back * held.rotation, scale: Vec3::ONE }
}
/// The loose object this player carries, in first person (a crate, a chair, a pillow, the tray, the bowl; not
/// a napkin, whose sheet hangs from the view in the world): its kind and where the simulation holds it in the
/// world (`carried`), drawn at its true size and in its true place (`held_place`, from `eye`, the game's camera
/// without its recoil), swaying a little behind a turn and with the stride (`sway`: sideways, and the pace of
/// the walk). The models are made the first time one is wanted.
pub fn held(world: &mut World, camera: Entity, carried: Option<(usize, Transform)>, eye: Transform, now: f32, sway: Vec3) {
    let kind = carried.map(|(kind, _)| kind);
    if !world.contains_resource::<Kit>() {
        return;
    }
    let mut have: Vec<(Entity, usize)> = world.query::<(Entity, &Held)>().iter(world).map(|(e, held)| (e, held.0)).collect();
    if have.is_empty() {
        if kind.is_none() {
            return;
        }
        let kinds = world.resource::<Kit>().kinds.clone();
        for (k, parts) in kinds.iter().enumerate() {
            if k == NAPKIN {
                continue;
            }
            let root = world.spawn((Held(k), Name::new("Carried, in first person"), Transform::default(), Visibility::Hidden)).id();
            for (mesh, centre, material) in parts {
                let part = world.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(material.clone()), Transform::from_translation(*centre))).id();
                world.entity_mut(root).add_child(part);
            }
            world.entity_mut(camera).add_child(root);
            have.push((root, k));
        }
    }
    for (entity, k) in have {
        let shown = kind == Some(k);
        if world.get::<Visibility>(entity).is_some_and(|v| *v != if shown { Visibility::Inherited } else { Visibility::Hidden }) {
            world.entity_mut(entity).insert(if shown { Visibility::Inherited } else { Visibility::Hidden });
        }
        let Some((_, at)) = carried.filter(|_| shown) else {
            continue;
        };
        let place = held_place(k, at, eye, sway, now);
        if let Some(mut tr) = world.get_mut::<Transform>(entity) {
            *tr = place;
        }
    }
}

/// Each loose object kind's parts, as the physics has them, and the radius of a sphere round its origin that
/// holds it: what a sheet meets.
fn solids() -> &'static [(Vec<Part>, f32)] {
    static SOLIDS: std::sync::OnceLock<Vec<(Vec<Part>, f32)>> = std::sync::OnceLock::new();
    SOLIDS.get_or_init(|| {
        LOOSE
            .iter()
            .map(|kind| {
                let parts = ShapeSpec::named(kind).map_or(vec![], |spec| spec.parts);
                let reach = parts
                    .iter()
                    .map(|part| match part {
                        Part::Box { center, half, .. } => Vec3::from_array(center.map(|v| v as f32)).length() + Vec3::from_array(half.map(|v| v as f32)).length(),
                        Part::Lathe { center, profile, radius, .. } => Vec3::from_array(center.map(|v| v as f32)).length() + profile.iter().map(|p| (p[0] as f32).hypot(p[1] as f32)).fold(0., f32::max) + *radius as f32,
                    })
                    .fold(0., f32::max);
                (parts, reach)
            })
            .collect()
    })
}

/// The napkins' sheets: one for each napkin, made when it is first seen, driven by its pad (hidden while it
/// is draped or out of the round), and its mesh reshaped whenever it moves.
fn sheets(world: &mut World, feast: &Feast, holds: &[Hold], bodies: &[Body], dt: f32) {
    if !world.contains_resource::<Kit>() {
        return;
    }
    let props = &feast.props;
    let napkins: Vec<usize> = (0..props.len()).filter(|id| props.body(*id).shape == NAPKIN).collect();
    if napkins.is_empty() {
        return;
    }
    let size = ShapeSpec::named("napkin").and_then(|spec| match spec.parts.first() {
        Some(Part::Box { half, .. }) => Some([half[0] as f32 * 2., half[2] as f32 * 2.]),
        _ => None,
    });
    let Some(size) = size else {
        return;
    };
    let floor = feast.bodies.config.kitchen_floor as f32;
    let level = &feast.level;
    let ground = |x: f32, z: f32| level.ground(x as f64, z as f64) as f32;
    let bounds = [level.x0 as f32, level.z0 as f32, level.x1 as f32, level.z1 as f32];
    // The map's blocks, then the drapes' walls and the forcefield (as the bodies meet them: grown by the margin,
    // which is taken off again).
    let margin = kitchen_sim::map::MARGIN as f32;
    let mut blocks: Vec<[f32; 5]> = level.map.blocks.iter().map(|b| [b.rect[0] as f32, b.rect[1] as f32, b.rect[2] as f32, b.rect[3] as f32, (FLOOR + b.height) as f32]).collect();
    blocks.extend(feast.extra_walls().iter().map(|w| [w[0] as f32 + margin, w[1] as f32 + margin, w[2] as f32 - margin, w[3] as f32 - margin, w[4] as f32 - kitchen_sim::map::LIP as f32]));
    let kinds = solids();
    let drive = |id: usize| -> Drive {
        let (p, q) = (props.position(id), props.rotation(id));
        let (at, turn) = (Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32), Quat::from_xyzw(q[0] as f32, q[1] as f32, q[2] as f32, q[3] as f32).normalize());
        match props.state(id) {
            // Draped over the edge of a block, a deck or the counter (no loose objects under it), it is still its own
            // cloth, lying where it froze and hanging over that edge.
            State::Draped if feast.drapes.iter().any(|d| d.napkin == id && d.props.is_empty()) => Drive::Loose { at, turn, resting: true },
            State::Draped | State::Removed => Drive::Hidden,
            State::Carried => holds.iter().find(|h| h.prop == id).map_or(Drive::Loose { at, turn, resting: false }, |h| Drive::Carried(h.grips)),
            // Over the edge: down on the kitchen floor, the way up it was.
            State::Lost => Drive::Loose { at: Vec3::new(at.x, floor, at.z), turn: Quat::from_rotation_arc(turn * Vec3::Y, if (turn * Vec3::Y).y < 0. { Vec3::NEG_Y } else { Vec3::Y }) * turn, resting: true },
            State::Asleep | State::Locked => Drive::Loose { at, turn, resting: true },
            State::Awake => Drive::Loose { at, turn, resting: false },
        }
    };
    let material = world.resource::<Kit>().cloth.clone();
    world.resource_mut::<Sheets>().holds = holds.to_vec();
    let started = bevy::platform::time::Instant::now();
    for id in napkins {
        let drive = drive(id);
        if !world.resource::<Sheets>().by.contains_key(&id) {
            let cloth = Cloth::new(size, drive);
            let mesh = world.resource_mut::<Assets<Mesh>>().add(crate::cloth::mesh(&cloth));
            let entity = world
                .spawn((crate::level::MapPart, Sheet(id), Name::new("Napkin"), Mesh3d(mesh.clone()), MeshMaterial3d(material.clone()), Transform::default(), Visibility::Hidden))
                .id();
            world.resource_mut::<Sheets>().by.insert(id, (entity, mesh, cloth));
        }
        let Some((entity, mesh, mut cloth)) = world.resource_mut::<Sheets>().by.remove(&id) else {
            continue;
        };
        // What is near enough to touch it this frame: blocks, other loose objects and bodies round it, the
        // sheet's reach (its own size) beyond where it is.
        let (low, high) = cloth.bounds();
        let (low, high) = match drive {
            Drive::Carried(grips) => (low.min(grips[0]).min(grips[1]), high.max(grips[0]).max(grips[1])),
            Drive::Loose { at, .. } => (low.min(at), high.max(at)),
            Drive::Hidden => (low, high),
        };
        let reach = 0.4;
        let near = |x: f32, z: f32, r: f32| x > low.x - reach - r && x < high.x + reach + r && z > low.z - reach - r && z < high.z + reach + r;
        let around = Surroundings {
            ground: &ground,
            bounds,
            floor,
            blocks: blocks.iter().copied().filter(|b| near((b[0] + b[2]) / 2., (b[1] + b[3]) / 2., (b[2] - b[0]).max(b[3] - b[1]))).collect(),
            solids: (0..props.len())
                .filter(|other| *other != id && matches!(props.state(*other), State::Awake | State::Asleep | State::Locked | State::Carried))
                .filter_map(|other| {
                    let (shape, p, q) = (props.body(other).shape, props.position(other), props.rotation(other));
                    let (parts, r) = kinds.get(shape)?;
                    near(p[0] as f32, p[2] as f32, *r).then(|| Solid {
                        parts,
                        at: Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32),
                        turn: Quat::from_xyzw(q[0] as f32, q[1] as f32, q[2] as f32, q[3] as f32).normalize(),
                        reach: *r,
                        moving: !matches!(props.state(other), State::Asleep | State::Locked),
                    })
                })
                .collect(),
            bodies: bodies.iter().filter(|b| near(b.feet.x, b.feet.z, b.radius)).map(|b| Body { feet: b.feet, radius: b.radius, height: b.height }).collect(),
        };
        let moved = cloth.update(drive, &around, dt);
        if moved {
            if let Some(mut mesh) = world.resource_mut::<Assets<Mesh>>().get_mut(&mesh) {
                crate::cloth::reshape(&cloth, &mut mesh);
            }
        }
        let shown = if drive == Drive::Hidden { Visibility::Hidden } else { Visibility::Inherited };
        if world.get::<Visibility>(entity).is_some_and(|v| *v != shown) {
            world.entity_mut(entity).insert(shown);
        }
        world.resource_mut::<Sheets>().by.insert(id, (entity, mesh, cloth));
    }
    let spent = started.elapsed().as_secs_f32() * 1e3;
    let mut sheets = world.resource_mut::<Sheets>();
    sheets.cost += (spent - sheets.cost) * 0.05;
}

/// The drapes' cloths: one for each drape, laid when it appears and taken away when it goes.
fn drapes(world: &mut World, feast: &Feast) {
    let have: Vec<(Entity, u32)> = world.query::<(Entity, &Draped)>().iter(world).map(|(entity, cloth)| (entity, cloth.id)).collect();
    for (entity, id) in &have {
        if !feast.drapes.iter().any(|d| d.id == *id) {
            world.entity_mut(*entity).despawn();
        }
    }
    for d in &feast.drapes {
        // (One over a block's, a deck's or the counter's edge alone is its napkin's own cloth: `sheets`.)
        if have.iter().any(|(_, id)| *id == d.id) || d.props.is_empty() {
            continue;
        }
        // The cloth follows the tops of what it covers: the loose objects, and the map's blocks under it.
        let blocks = &feast.level.map.blocks;
        let mesh = cloth(d, &feast.props, |x, z| {
            let (x, z) = (x as f64, z as f64);
            blocks.iter().filter(|b| x >= b.rect[0] && x <= b.rect[2] && z >= b.rect[1] && z <= b.rect[3]).fold(feast.level.ground(x, z), |top, b| top.max(FLOOR + b.height)) as f32
        });
        let mesh = world.resource_mut::<Assets<Mesh>>().add(mesh);
        let material = world.resource::<Kit>().cloth.clone();
        world.spawn((crate::level::MapPart, Draped { id: d.id }, Name::new("Draped napkin"), Mesh3d(mesh), MeshMaterial3d(material), Transform::default(), Visibility::default()));
    }
}

/// A draped napkin's cloth, in world space: a sheet over the drape's wall that follows the tops of the props it
/// covers (where it spans a gap it sags, but no more steeply than SAG), and whose sides hang from the wall's edge
/// in folds down to the counter (`ground` is its height at [x, z]), flaring a little at the hem.
pub fn cloth(d: &Drape, props: &PropWorld, ground: impl Fn(f32, f32) -> f32) -> Mesh {
    // Rows and columns of the top, rows of each side; the cloth's thickness over what it covers, how steeply it
    // may fall between two tops, and its folds: how many to the metre round the sides, and how deep.
    const ACROSS: usize = 18;
    const DOWN: usize = 9;
    const THICK: f32 = 0.006;
    const SAG: f32 = 1.4;
    const FOLDS: f32 = 30.;
    const DEPTH: f32 = 0.008;
    let [x0, z0, x1, z1] = d.rect.map(|v| v as f32);
    let top = d.top as f32;
    let floor = ground((x0 + x1) / 2., (z0 + z1) / 2.);
    // What the covered props come up to under each point of the top (the counter where there is nothing). The
    // outermost rows look a little in from the wall's edge, where the props are (the wall is wider than they are),
    // and each point takes the highest of its own and its neighbours' readings, so that no edge of a prop between
    // two of them pokes through the cloth.
    let at = |i: usize, j: usize| Vec2::new(x0 + (x1 - x0) * i as f32 / ACROSS as f32, z0 + (z1 - z0) * j as f32 / ACROSS as f32);
    let inset = kitchen_sim::drape::SKIRT as f32 + 0.003;
    let mut reading = vec![0f32; (ACROSS + 1) * (ACROSS + 1)];
    for j in 0..=ACROSS {
        for i in 0..=ACROSS {
            let p = at(i, j);
            let (x, z) = (p.x.clamp(x0 + inset, x1 - inset), p.y.clamp(z0 + inset, z1 - inset));
            let from = [x as f64, top as f64 + 0.05, z as f64];
            reading[j * (ACROSS + 1) + i] = props.raycast_among(from, [0., -1., 0.], 0.6, &d.props).map_or(ground(x, z), |hit| hit.point[1] as f32);
        }
    }
    let height: Vec<f32> = (0..reading.len())
        .map(|k| {
            let (i, j) = ((k % (ACROSS + 1)) as i32, (k / (ACROSS + 1)) as i32);
            let mut most = reading[k];
            for (di, dj) in [(-1, 0), (1, 0), (0, -1), (0, 1), (-1, -1), (1, 1), (-1, 1), (1, -1)] {
                let (a, b) = (i + di, j + dj);
                if a >= 0 && b >= 0 && a <= ACROSS as i32 && b <= ACROSS as i32 {
                    most = most.max(reading[b as usize * (ACROSS + 1) + a as usize]);
                }
            }
            most
        })
        .collect();
    // Cloth does not follow every dip: it rests on the high points and falls between them no more steeply than SAG.
    let draped: Vec<f32> = (0..height.len())
        .map(|k| {
            let p = at(k % (ACROSS + 1), k / (ACROSS + 1));
            (0..height.len()).map(|n| height[n] - SAG * p.distance(at(n % (ACROSS + 1), n / (ACROSS + 1)))).fold(height[k], f32::max).min(top) + THICK
        })
        .collect();
    let (mut positions, mut colours, mut indices): (Vec<[f32; 3]>, Vec<[f32; 4]>, Vec<u32>) = (vec![], vec![], vec![]);
    // The napkins' weave (their gingham is the material's picture).
    let weave = crate::cloth::weave;
    // The top, with a gentle ripple.
    for j in 0..=ACROSS {
        for i in 0..=ACROSS {
            let p = at(i, j);
            let ripple = 0.0015 * ((p.x * 61.).sin() * (p.y * 47.).cos());
            positions.push([p.x, draped[j * (ACROSS + 1) + i] + ripple, p.y]);
            colours.push(weave(p.x, p.y));
        }
    }
    for j in 0..ACROSS as u32 {
        for i in 0..ACROSS as u32 {
            let (a, b, c, e) = (j * (ACROSS as u32 + 1) + i, j * (ACROSS as u32 + 1) + i + 1, (j + 1) * (ACROSS as u32 + 1) + i + 1, (j + 1) * (ACROSS as u32 + 1) + i);
            indices.extend([a, c, b, a, e, c]);
        }
    }
    // The sides: round the edge of the top, each point of the edge hangs a column down to the counter, pushed out
    // by the folds (deeper towards the hem) and turned out at the corners.
    let mut edge: Vec<(usize, Vec2)> = vec![];
    for i in 0..ACROSS {
        edge.push((i, Vec2::NEG_Y));
    }
    for j in 0..ACROSS {
        edge.push((j * (ACROSS + 1) + ACROSS, Vec2::X));
    }
    for i in (1..=ACROSS).rev() {
        edge.push((ACROSS * (ACROSS + 1) + i, Vec2::Y));
    }
    for j in (1..=ACROSS).rev() {
        edge.push((j * (ACROSS + 1), Vec2::NEG_X));
    }
    let first = positions.len() as u32;
    let mut along = 0f32;
    for (n, (k, side)) in edge.iter().enumerate() {
        let start = Vec3::from_array(positions[*k]);
        if n > 0 {
            along += start.distance(Vec3::from_array(positions[edge[n - 1].0]));
        }
        // At a corner the cloth falls out both ways.
        let corner = match *k {
            k if k == 0 => Vec2::new(-1., -1.),
            k if k == ACROSS => Vec2::new(1., -1.),
            k if k == ACROSS * (ACROSS + 1) + ACROSS => Vec2::new(1., 1.),
            k if k == ACROSS * (ACROSS + 1) => Vec2::new(-1., 1.),
            _ => *side,
        }
        .normalize();
        let hem = ground(start.x, start.z).max(floor - 0.05);
        for r in 0..=DOWN {
            let t = r as f32 / DOWN as f32;
            let fold = (along * FOLDS).sin() * DEPTH * t.sqrt() + 0.004 * t * t;
            let out = corner * (THICK * 0.5 + fold + 0.01 * t * t);
            positions.push([start.x + out.x, start.y + (hem + 0.001 - start.y) * t, start.z + out.y]);
            let shade = weave(along, start.y * (1. - t));
            colours.push([shade[0] * (0.85 + 0.15 * (1. - t)), shade[1] * (0.85 + 0.15 * (1. - t)), shade[2] * (0.85 + 0.15 * (1. - t)), 1.]);
        }
    }
    let count = edge.len() as u32;
    for n in 0..count {
        let m = (n + 1) % count;
        for r in 0..DOWN as u32 {
            let (a, b, c, e) = (first + n * (DOWN as u32 + 1) + r, first + m * (DOWN as u32 + 1) + r, first + m * (DOWN as u32 + 1) + r + 1, first + n * (DOWN as u32 + 1) + r + 1);
            indices.extend([a, b, c, a, c, e]);
        }
    }
    // Smooth normals, each the sum of the faces round a corner.
    let mut normals = vec![Vec3::ZERO; positions.len()];
    for t in indices.chunks(3) {
        let [a, b, c] = [t[0], t[1], t[2]].map(|i| Vec3::from_array(positions[i as usize]));
        let face = (b - a).cross(c - a);
        for i in t {
            normals[*i as usize] += face;
        }
    }
    let normals: Vec<[f32; 3]> = normals.iter().map(|n| n.normalize_or(Vec3::Y).to_array()).collect();
    let uvs: Vec<[f32; 2]> = positions.iter().map(|p| [p[0] * 8., p[2] * 8.]).collect();
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colours)
        .with_inserted_indices(Indices::U32(indices))
}
/// How far apart two states of a prop are, and the cosine of half the angle between them.
fn apart(a: &PropState, b: &PropState) -> (f64, f64) {
    ((0..3).map(|i| (a.position[i] - b.position[i]).powi(2)).sum::<f64>().sqrt(), (0..4).map(|i| a.rotation[i] * b.rotation[i]).sum::<f64>().abs())
}
/// Whether two states put a prop in visibly different places.
fn differs(a: &PropState, b: &PropState) -> bool {
    let (distance, aligned) = apart(a, b);
    a.state != b.state || distance > 1e-5 || aligned < 1. - 1e-9
}

/// Moves each body towards where the physics has its prop. One that has gone over the edge is seen all the
/// way down; the physics gives it up just above the kitchen floor, and its body comes to rest there, flat on
/// whichever of its top and underside was the nearer. A prop taken out of the world is hidden.
fn place(world: &mut World, props: &PropWorld, floor: f32, dt: f32, mine: Option<usize>) {
    let follow = 1. - (-dt * FOLLOW).exp();
    let tall = world.resource::<Kit>().tall.clone();
    let mut bodies = world.query::<(&mut Loose, &mut Transform, &mut Visibility)>();
    for (mut loose, mut transform, mut visibility) in bodies.iter_mut(world) {
        if loose.id >= props.len() {
            continue;
        }
        let (p, q, state) = (props.position(loose.id), props.rotation(loose.id), props.state(loose.id));
        let mut at = Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32);
        let mut turn = Quat::from_xyzw(q[0] as f32, q[1] as f32, q[2] as f32, q[3] as f32).normalize();
        if state == State::Lost {
            let up = turn * Vec3::Y;
            let over = up.y < 0.;
            turn = (Quat::from_rotation_arc(up, if over { Vec3::NEG_Y } else { Vec3::Y }) * turn).normalize();
            at.y = floor + if over { tall.get(props.body(loose.id).shape).copied().unwrap_or(0.) } else { 0. };
        }
        let (translation, rotation) = if loose.fresh || transform.translation.distance(at) > SNAP {
            loose.fresh = false;
            (at, turn)
        } else if transform.translation.distance(at) < 1e-5 && transform.rotation.angle_between(turn) < 1e-4 {
            // Arrived: put exactly there, and then left alone. (A body whose transform is untouched costs nothing a frame:
            // nothing is propagated to its parts or sent to the GPU again.)
            (at, turn)
        } else {
            (transform.translation.lerp(at, follow), transform.rotation.slerp(turn, follow))
        };
        if transform.translation != translation || transform.rotation != rotation {
            (transform.translation, transform.rotation) = (translation, rotation);
        }
        // A napkin is drawn as cloth (its sheet, or draped the drape's), never as the pad it is in the physics.
        let shown = if state == State::Removed || props.body(loose.id).shape == NAPKIN || Some(loose.id) == mine { Visibility::Hidden } else { Visibility::Inherited };
        if *visibility != shown {
            *visibility = shown;
        }
    }
}

/// The sound of a prop's impact, as (rings rather than knocks, playback speed, volume), or none: the bowl, the tray,
/// a tin and the bread bin ring (the microwave's bell, pitched down to the size of the pan, louder the harder the
/// knock), china and glass ring high and short (a plate, a mug, a cereal bowl, a shaker), wood knocks (a crate, a chair,
/// a cutting board), and cloth, card, paper and a pillow make no sound at all. Two props meeting sound like the more
/// resonant.
fn voice(kinds: &[&str], speed: f32) -> Option<(bool, f32, f32)> {
    let strength = (speed / 1.2).clamp(0.05, 1.);
    if let Some(metal) = kinds.iter().find(|kind| matches!(**kind, "bowl" | "tray" | "tin" | "breadbox")) {
        // A little variety from the speed itself, so two clangs in a row do not sound stamped out.
        let base = match *metal {
            "bowl" => 0.58,
            "breadbox" => 0.5,
            "tin" => 1.1,
            _ => 0.86,
        };
        Some((true, base + 0.1 * (speed * 7.3).fract(), 0.25 + 0.75 * strength))
    } else if kinds.iter().any(|kind| matches!(*kind, "plate" | "cup" | "dish" | "salt" | "pepper")) {
        Some((true, 1.7 + 0.2 * (speed * 6.1).fract(), 0.15 + 0.45 * strength))
    } else if kinds.iter().any(|kind| matches!(*kind, "crate" | "chair" | "board")) {
        Some((false, 2.6 + 0.4 * (speed * 5.1).fract(), 0.3 * strength))
    } else {
        None
    }
}
/// Sounds a knock: prop number `prop` met `other` (another prop's number, or the ground or a block, which
/// are negative) at `speed`.
fn clang(world: &mut World, props: &PropWorld, prop: usize, other: i32, speed: f32) {
    if prop >= props.len() {
        return;
    }
    let kind = |id: usize| LOOSE.get(props.body(id).shape).copied().unwrap_or("crate");
    let mut kinds = vec![kind(prop)];
    if other >= 0 && (other as usize) < props.len() {
        kinds.push(kind(other as usize));
    }
    let Some((rings, pitch, volume)) = voice(&kinds, speed) else {
        return;
    };
    let kit = world.resource::<Kit>();
    if let Some(sound) = if rings { kit.ding.clone() } else { kit.boom.clone() } {
        world.spawn((AudioPlayer::new(sound), PlaybackSettings::DESPAWN.with_speed(pitch).with_volume(Volume::Linear(volume))));
    }
}

/// For the checks (`window.hotdogState.loose`): each prop's kind, state as the physics numbers it, origin and
/// the height of its own up direction, which is 1 while it stands level and -1 upside down.
pub fn report(props: &PropWorld) -> serde_json::Value {
    (0..props.len())
        .map(|id| serde_json::json!({"kind": LOOSE.get(props.body(id).shape).copied().unwrap_or(""), "state": props.state(id) as u8, "p": props.position(id), "up": props.up(id)[1]}))
        .collect()
}

/// Online: takes the loose objects a frame carries into the client's copy of the props, and sounds its
/// new knocks. A prop the client is already moving much as the server has it keeps its own motion and is
/// only eased towards the server's place, so that a bowl being shoved does not stutter between the two.
pub fn apply(world: &mut World, feast: &mut Feast, frame: &kitchen_sim::net::Frame) {
    for entry in &frame.props {
        let id = entry.id;
        if id >= feast.props.len() {
            continue;
        }
        let state = State::from_number(entry.state);
        let mut mine = feast.props.get_state(id);
        // (The rotation is sent to 1e-5 a part: made a unit again.)
        let q = entry.q;
        let norm = q.iter().map(|x| x * x).sum::<f64>().sqrt();
        let rotation = if norm > 0.5 { q.map(|x| x / norm) } else { mine.rotation };
        let theirs = PropState { id, shape: mine.shape, state, holder: mine.holder, position: entry.p, rotation, velocity: entry.v, angular_velocity: entry.w };
        let mut kit = world.get_resource_mut::<Kit>();
        if let Some(kit) = &mut kit {
            kit.said.insert(id, theirs);
        }
        if mine.state == State::Awake && matches!(state, State::Awake | State::Asleep) {
            // Where the copy has been that is nearest to where the server has it.
            let away = |at: &[f64; 3]| (0..3).map(|i| (theirs.position[i] - at[i]).powi(2)).sum::<f64>();
            let trail = kit.as_mut().and_then(|kit| kit.trails.get_mut(&id));
            let nearest = trail.iter().flat_map(|trail| trail.iter()).fold(mine.position, |nearest, at| if away(at) < away(&nearest) { *at } else { nearest });
            if away(&nearest).sqrt() < REACH && apart(&mine, &theirs).1 > ALIGNED {
                // At rest on the server, it will come to rest here too, and `update` then puts it exactly there.
                if state == State::Awake && away(&nearest).sqrt() > SLACK {
                    let shift: [f64; 3] = std::array::from_fn(|i| (theirs.position[i] - nearest[i]) * GIVE);
                    for at in std::iter::once(&mut mine.position).chain(trail.into_iter().flat_map(|trail| trail.iter_mut())) {
                        for i in 0..3 {
                            at[i] += shift[i];
                        }
                    }
                    feast.props.set_state(id, &mine);
                }
                continue;
            }
        }
        if differs(&mine, &theirs) {
            feast.props.set_state(id, &theirs);
            if let Some(kit) = &mut kit {
                kit.trails.remove(&id);
            }
        }
    }
    // The drapes the server has, with their walls (the frozen props came with the props above).
    if frame.drapes != feast.drapes {
        feast.set_drapes(frame.drapes.clone());
    }
    if !world.contains_resource::<Kit>() {
        return;
    }
    let knocks: Vec<(u32, usize, i32, f32)> = frame.knocks.iter().map(|k| (k.id, k.prop as usize, k.other, k.speed)).collect();
    let mut told = world.resource::<Kit>().told;
    if knocks.iter().all(|knock| knock.0 < told) {
        told = 0;
    }
    for (_, prop, other, speed) in knocks.iter().filter(|knock| knock.0 > told) {
        clang(world, &feast.props, *prop, *other, *speed);
    }
    world.resource_mut::<Kit>().told = knocks.iter().map(|knock| knock.0).fold(told, u32::max);
}

/// The kitchen's building pieces, drawn: every picture made here, from nothing but sums (no image is read).
mod art {
    use bevy::{
        asset::RenderAssetUsages,
        image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
        math::Affine2,
        mesh::VertexAttributeValues,
        prelude::*,
        render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    };
    use std::f32::consts::TAU;

    /// A picture `w` by `h` from a colour for each texel ([0, 1] across and down), repeating.
    fn picture(w: u32, h: u32, colour: impl Fn(f32, f32) -> [f32; 3]) -> Image {
        let mut data = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                let c = colour((x as f32 + 0.5) / w as f32, (y as f32 + 0.5) / h as f32);
                data.extend(c.map(|v| (v.clamp(0., 1.) * 255.).round() as u8));
                data.push(255);
            }
        }
        let mut image = Image::new(Extent3d { width: w, height: h, depth_or_array_layers: 1 }, TextureDimension::D2, data, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default());
        image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: ImageAddressMode::Repeat,
            address_mode_v: ImageAddressMode::Repeat,
            mag_filter: ImageFilterMode::Linear,
            min_filter: ImageFilterMode::Linear,
            ..default()
        });
        image
    }
    fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
        let t = t.clamp(0., 1.);
        [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
    }
    /// Inside an ellipse, softened over its last `soft` (in the ellipse's own measure).
    fn inside(u: f32, v: f32, centre: [f32; 2], radius: [f32; 2], soft: f32) -> f32 {
        let d = (((u - centre[0]) / radius[0]).powi(2) + ((v - centre[1]) / radius[1]).powi(2)).sqrt();
        ((1. - d) / soft).clamp(0., 1.)
    }

    /// The cereal box's six faces in a 3 by 2 sheet (+x, -x, +y in the top row; -y, +z, -z below): on the front and
    /// back the box's own breakfast, "SUNNY FLAKES" in fat lozenges on a red band, a blue bowl heaped with gold flakes
    /// under a rising sun on orange card; on the sides a cream panel of small print; card on the flaps.
    pub fn cereal() -> Image {
        const ORANGE: [f32; 3] = [0.96, 0.56, 0.13];
        picture(768, 512, |x, y| {
            let (cell, u, v) = ((x * 3.).floor() as u32 + 3 * (y * 2.).floor() as u32, (x * 3.).fract(), (y * 2.).fract());
            match cell {
                // The sides: the height runs across these (u), the depth down (v).
                0 | 1 => {
                    let line = (u * 22.).fract() < 0.28 && v > 0.15 && v < 0.85 && u > 0.08 && u < 0.62;
                    if u > 0.66 { ORANGE } else if line { [0.45, 0.42, 0.38] } else { [0.95, 0.92, 0.82] }
                }
                2 | 3 => ORANGE,
                _ => {
                    // The front: across (u) and up (v; the picture's rows run down).
                    let up = 1. - v;
                    let mut c = mix(ORANGE, [0.99, 0.72, 0.2], up);
                    // The sun behind the bowl.
                    c = mix(c, [1., 0.88, 0.3], inside(u, up, [0.5, 0.42], [0.42, 0.29], 0.08));
                    for k in 0..9 {
                        let a = k as f32 / 8. * std::f32::consts::PI;
                        let (du, dv) = (u - 0.5 - a.cos() * 0.4, up - 0.42 - a.sin() * 0.27);
                        if du * du * 6. + dv * dv * 3. < 0.0012 {
                            c = [1., 0.85, 0.25];
                        }
                    }
                    // Flakes heaped in the bowl.
                    for k in 0..23 {
                        let (fu, fv) = (0.2 + ((k * 37) % 23) as f32 / 23. * 0.6, 0.27 + ((k * 11) % 7) as f32 / 7. * 0.12);
                        c = mix(c, if k % 3 == 0 { [0.86, 0.55, 0.12] } else { [0.97, 0.8, 0.32] }, inside(u, up, [fu, fv], [0.075, 0.045], 0.4));
                    }
                    // The bowl.
                    let bowl = inside(u, up, [0.5, 0.25], [0.36, 0.16], 0.1) * if up < 0.27 { 1. } else { 0. };
                    c = mix(c, mix([0.12, 0.3, 0.72], [0.3, 0.55, 0.95], (u - 0.2) * 1.4), bowl);
                    // The name on its band.
                    if up > 0.76 && up < 0.95 {
                        c = [0.82, 0.1, 0.12];
                        let row = if up > 0.855 { 0 } else { 1 };
                        let (letters, left, width) = if row == 0 { (5., 0.12, 0.76) } else { (6., 0.08, 0.84) };
                        let at = (u - left) / width * letters;
                        let centre = if row == 0 { 0.9 } else { 0.81 };
                        if at > 0. && at < letters && inside(at.fract(), up, [0.5, centre], [0.36, 0.035], 0.3) > 0.5 {
                            c = [1., 0.97, 0.9];
                        }
                    }
                    if up < 0.06 { [0.7, 0.3, 0.06] } else { c }
                }
            }
        })
    }
    /// A napkin's red and white gingham, four checks to a tile (a tile is 12.5 cm of cloth).
    pub fn check() -> Image {
        picture(128, 128, |u, v| {
            let (a, b) = ((u * 4.).floor() as u32 % 2, (v * 4.).floor() as u32 % 2);
            match a + b {
                0 => [0.97, 0.96, 0.93],
                1 => [0.93, 0.62, 0.62],
                _ => [0.8, 0.16, 0.18],
            }
        })
    }
    /// The cereal box's mesh with each face's texture co-ordinates in its own cell of `cereal`'s sheet.
    pub fn boxed(mut mesh: Mesh, half: [f32; 3]) -> Mesh {
        let (Some(VertexAttributeValues::Float32x3(positions)), Some(VertexAttributeValues::Float32x3(normals))) = (mesh.attribute(Mesh::ATTRIBUTE_POSITION), mesh.attribute(Mesh::ATTRIBUTE_NORMAL)) else {
            return mesh;
        };
        let uvs: Vec<[f32; 2]> = positions
            .iter()
            .zip(normals)
            .map(|(p, n)| {
                let axis = (0..3).max_by(|a, b| n[*a].abs().total_cmp(&n[*b].abs())).unwrap_or(1);
                let cell = axis as u32 * 2 + if n[axis] < 0. { 1 } else { 0 };
                let (a, b) = ((axis + 1) % 3, (axis + 2) % 3);
                // Across and up the face (the front's up is y, its across x).
                let (s, t) = if axis == 2 { ((p[0] / half[0] + 1.) / 2., 1. - (p[1] / half[1] + 1.) / 2.) } else { ((p[a] / half[a] + 1.) / 2., (p[b] / half[b] + 1.) / 2.) };
                [((cell % 3) as f32 + s.clamp(0.01, 0.99)) / 3., ((cell / 3) as f32 + t.clamp(0.01, 0.99)) / 2.]
            })
            .collect();
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
        mesh
    }
    /// Noise for a cell of a grid, 0 to 1: the same cell (and `seed`), the same number, and no pattern from one cell to the
    /// next (a hash of the cell's numbers, not a sine of them, whose ripples show as rows and arcs).
    fn cell_noise(cell: [i32; 3], seed: u32) -> f32 {
        let mut h = (cell[0] as u32).wrapping_mul(0x8da6_b343) ^ (cell[1] as u32).wrapping_mul(0xd816_3841) ^ (cell[2] as u32).wrapping_mul(0xcb1a_b31f) ^ seed.wrapping_mul(0x9e37_79b9);
        h ^= h >> 15;
        h = h.wrapping_mul(0x2c1b_3c6d);
        h ^= h >> 12;
        h = h.wrapping_mul(0x297a_2d39);
        h ^= h >> 15;
        (h >> 8) as f32 / (1 << 24) as f32
    }
    /// The noise of the cell `size` metres a side that a point is in: specks and grains that size.
    fn grain(p: [f32; 3], size: f32) -> f32 {
        cell_noise(p.map(|v| (v / size).floor() as i32), 0)
    }
    /// Whether a point of a turned skin is in a round speck: laid out on the skin itself (the distance round the axis, and
    /// up and out along the skin, which the distance from the axis plus the height follows within a half, flat or steep),
    /// `share` of its cells `size` metres a side hold one, anywhere in the cell (so that no rows show), three quarters of
    /// the cell's size across.
    fn dotted(p: [f32; 3], size: f32, share: f32) -> bool {
        let r = p[0].hypot(p[2]);
        let (round, up) = (p[2].atan2(p[0]) * r, r + p[1]);
        let (across, down) = ((round / size).floor() as i32, (up / size).floor() as i32);
        (-1..=1).flat_map(|i| (-1..=1).map(move |j| [across + i, down + j, 0])).any(|cell| {
            let centre = [(cell[0] as f32 + cell_noise(cell, 2)) * size, (cell[1] as f32 + cell_noise(cell, 3)) * size];
            cell_noise(cell, 1) < share && (round - centre[0]).hypot(up - centre[1]) < size * 0.375
        })
    }
    /// The colour (sRGB) of a turned piece at a point `p` of its skin, which faces `n`: by its distance from the axis, its
    /// height, and whether it faces out or in. A mug's glaze inside and out and its coffee, a plate's blue bands and
    /// unglazed foot, a cereal bowl's speckled glaze, the wound paper and the card core of a roll of paper towels, salt
    /// and pepper, a shaker's cap with its holes, a tin's label and rims.
    pub fn paint(kind: &str, p: [f32; 3], n: [f32; 3]) -> [f32; 3] {
        let shade = |c: [f32; 3], k: f32| [c[0] * k, c[1] * k, c[2] * k];
        const TEAL: [f32; 3] = [0.10, 0.40, 0.46];
        const CREAM: [f32; 3] = [0.95, 0.93, 0.87];
        const COBALT: [f32; 3] = [0.10, 0.22, 0.60];
        let (r, y) = (p[0].hypot(p[2]), p[1]);
        // Out from the axis (or down, under a foot), or in towards it (or up, on an inside floor).
        let radial = if r > 1e-5 { (n[0] * p[0] + n[2] * p[2]) / r } else { 0. };
        let inside = radial < -0.2 || (n[1] > 0.5 && radial < 0.2);
        match kind {
            // A teal mug, cream inside and round its lip, its foot left bare.
            "cup" if inside || y > 0.0965 => CREAM,
            "cup" if y < 0.004 => [0.80, 0.72, 0.60],
            "cup" | "glaze" => TEAL,
            "coffee" if r > 0.033 => [0.52, 0.33, 0.17],
            "coffee" => [0.20, 0.10, 0.05],
            "plate" if y > 0.009 && ((0.113..0.119).contains(&r) || (0.077..0.0795).contains(&r)) => COBALT,
            "plate" => [0.96, 0.96, 0.94],
            "foot" => [0.84, 0.80, 0.72],
            // A cereal bowl in a speckled sage glaze (round specks of about 2 mm), white inside, its rim the brown of the clay.
            "dish" if y > 0.0625 => [0.52, 0.36, 0.24],
            "dish" if inside => CREAM,
            "dish" => shade([0.46, 0.60, 0.50], if dotted(p, 0.003, 0.35) { 0.55 } else { 1. }),
            // Paper towel: quilted on the side, its wound layers in rings on the ends, the core's card at the middle.
            "roll" if n[1].abs() > 0.7 => shade([0.98, 0.98, 0.96], if (r / 0.0011).fract() < 0.35 { 0.9 } else { 1. } * if r < 0.024 { 0.93 } else { 1. }),
            "roll" => {
                let angle = p[2].atan2(p[0]);
                let quilt = ((angle * 26. + y * 90.).sin() * (angle * 26. - y * 90.).sin()).abs();
                shade([0.98, 0.98, 0.96], 0.9 + 0.1 * quilt.powf(0.3))
            }
            "core" if inside => [0.42, 0.29, 0.17],
            "core" => [0.66, 0.48, 0.30],
            // Salt, a little sparkle; ground black pepper, flecked (grains of a millimetre).
            "salt" => shade([0.97, 0.97, 0.98], 0.88 + 0.12 * grain(p, 0.001)),
            "pepper" => {
                let k = grain(p, 0.001);
                if k > 0.82 { [0.50, 0.44, 0.36] } else { shade([0.16, 0.15, 0.14], 0.7 + 0.5 * k) }
            }
            // The cap: satin steel, a screw thread round its skirt, and its holes in the top (3.6 mm across: big enough to read).
            "cap" if y > 0.0868 => {
                let holes = [[0., 0.], [0.0065, 0.], [-0.0065, 0.], [0., 0.0065], [0., -0.0065], [0.0046, 0.0046], [-0.0046, -0.0046]];
                if holes.iter().any(|h| (p[0] - h[0]).hypot(p[2] - h[1]) < 0.0018) { [0.05, 0.05, 0.05] } else { [0.92, 0.92, 0.94] }
            }
            "cap" if y < 0.072 && r > 0.021 => shade([0.92, 0.92, 0.94], if (y / 0.0016).fract() < 0.4 { 0.72 } else { 1. }),
            "cap" => [0.92, 0.92, 0.94],
            "knob" => [0.55, 0.36, 0.20],
            // A tin of beans: a green label with a red band and a cream stripe, steel rims with their ridges.
            "tin" if y > 0.014 && y < 0.094 && r > 0.034 && n[1].abs() < 0.5 => {
                if (0.047..0.063).contains(&y) {
                    [0.82, 0.14, 0.10]
                } else if (0.066..0.072).contains(&y) {
                    [0.96, 0.92, 0.80]
                } else {
                    [0.16, 0.46, 0.25]
                }
            }
            "tin" if n[1].abs() > 0.7 => shade([0.80, 0.82, 0.85], if (r / 0.005).fract() < 0.3 { 0.82 } else { 1. }),
            "tin" => shade([0.80, 0.82, 0.85], if (y / 0.003).fract() < 0.4 { 0.85 } else { 1. }),
            _ => [1., 1., 1.],
        }
    }
    /// A mesh painted in its corners' colours (`paint`): for the small pieces that are not turned shells (a mug's handle,
    /// the bread bin's knob), whose paint is plain enough for that.
    pub fn painted(mut mesh: Mesh, kind: &str) -> Mesh {
        let (Some(VertexAttributeValues::Float32x3(positions)), Some(VertexAttributeValues::Float32x3(normals))) = (mesh.attribute(Mesh::ATTRIBUTE_POSITION), mesh.attribute(Mesh::ATTRIBUTE_NORMAL)) else {
            return mesh;
        };
        let colours: Vec<[f32; 4]> = positions.iter().zip(normals).map(|(p, n)| { let c = paint(kind, *p, *n); Color::srgb(c[0], c[1], c[2]).to_linear().to_f32_array() }).collect();
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colours);
        mesh
    }
    /// The picture a turned piece is painted with (`paint`), laid over the skin that `outline` (its rows, loose.rs
    /// `outline`) turns: round the axis across, along the outline down, as `turn`'s texture co-ordinates run, which the
    /// transform returned with it brings to one picture over the whole skin. A texel is about 1.2 mm round and 0.8 mm
    /// along (finer than the rows and steps the paint was once put on the corners of), the mean of two points of the
    /// skin along it (where the fine rings and threads run), and the picture has its mipmaps, so that it neither shimmers
    /// far off nor blurs close up; paint that is the same all the way round (bands and rings) is a picture one texel across.
    pub fn baked(outline: &[(Vec2, Vec2)], kind: &str) -> (Image, Affine2) {
        let mut along = vec![0f32];
        for pair in outline.windows(2) {
            along.push(along[along.len() - 1] + pair[0].0.distance(pair[1].0));
        }
        let total = along[along.len() - 1].max(1e-6);
        let reach = outline.iter().fold(0f32, |reach, (point, _)| reach.max(point.x));
        // The point of the outline `t` metres along it, and the way the skin faces there (as the mesh shades it: the
        // normals of the rows either side, mixed).
        let at = |t: f32| {
            let k = along.partition_point(|a| *a <= t).clamp(1, outline.len() - 1);
            let s = ((t - along[k - 1]) / (along[k] - along[k - 1]).max(1e-9)).clamp(0., 1.);
            (outline[k - 1].0.lerp(outline[k].0, s), outline[k - 1].1.lerp(outline[k].1, s).normalize_or(outline[k].1))
        };
        // Linear light from sRGB, by table.
        let table: Vec<f32> = (0..1024).map(|i| Color::srgb(i as f32 / 1023., 0., 0.).to_linear().red).collect();
        let linear = |c: [f32; 3]| c.map(|v| table[(v.clamp(0., 1.) * 1023. + 0.5) as usize]);
        let colour = |(point, normal): (Vec2, Vec2), (sin, cos): (f32, f32)| linear(paint(kind, [point.x * cos, point.y, point.x * sin], [normal.x * cos, normal.y, normal.x * sin]));
        // The nearest power of two to a length over a texel's size.
        let texels = |length: f32, size: f32| 2u32.pow((length / size).max(1.).log2().round() as u32);
        let high = texels(total, 0.0008).clamp(64, 512);
        // The two points down each row of texels, a quarter and three quarters of the way.
        let rows: Vec<(Vec2, Vec2)> = (0..2 * high).map(|k| at((k as f32 + 0.5) / (2 * high) as f32 * total)).collect();
        // The same all the way round: every row of texels, at sixteen turns (none of them a whole number of eighths).
        let round = rows.iter().step_by(2).all(|row| {
            let first = colour(*row, 0.3f32.sin_cos());
            (1..16).all(|i| colour(*row, (0.3 + i as f32 / 16. * TAU).sin_cos()) == first)
        });
        let wide = if round { 1 } else { texels(TAU * reach, 0.0012).clamp(16, 512) };
        let turns: Vec<(f32, f32)> = (0..wide).map(|k| ((k as f32 + 0.5) / wide as f32 * TAU).sin_cos()).collect();
        let mut picture = Vec::with_capacity((wide * high) as usize);
        for j in 0..high as usize {
            for turned in &turns {
                let (a, b) = (colour(rows[2 * j], *turned), colour(rows[2 * j + 1], *turned));
                picture.push([(a[0] + b[0]) / 2., (a[1] + b[1]) / 2., (a[2] + b[2]) / 2.]);
            }
        }
        (mipped(wide, high, picture), Affine2::from_scale(Vec2::new(0.25, 1. / (8. * total))))
    }
    /// An sRGB picture from linear colours, `wide` by `high`, with every smaller mip after it (each texel the mean of the
    /// two or four it stands for), repeating across and held at its edges down.
    fn mipped(wide: u32, high: u32, mut level: Vec<[f32; 3]>) -> Image {
        let encode = |c: [f32; 3]| {
            let s = Color::LinearRgba(LinearRgba::rgb(c[0], c[1], c[2])).to_srgba();
            [s.red, s.green, s.blue].map(|v| (v.clamp(0., 1.) * 255.).round() as u8)
        };
        let (mut w, mut h, mut levels, mut data) = (wide as usize, high as usize, 1, Vec::new());
        loop {
            for c in &level {
                data.extend(encode(*c));
                data.push(255);
            }
            if w == 1 && h == 1 {
                break;
            }
            let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
            let mut next = Vec::with_capacity(nw * nh);
            for y in 0..nh {
                for x in 0..nw {
                    let (xs, ys) = (if w > 1 { [2 * x, 2 * x + 1] } else { [x, x] }, if h > 1 { [2 * y, 2 * y + 1] } else { [y, y] });
                    let mut sum = [0f32; 3];
                    for yy in ys {
                        for xx in xs {
                            let c = level[yy * w + xx];
                            sum = [sum[0] + c[0], sum[1] + c[1], sum[2] + c[2]];
                        }
                    }
                    next.push(sum.map(|v| v / 4.));
                }
            }
            (level, w, h, levels) = (next, nw, nh, levels + 1);
        }
        let mut image = Image::new_uninit(Extent3d { width: wide, height: high, depth_or_array_layers: 1 }, TextureDimension::D2, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::RENDER_WORLD);
        image.data = Some(data);
        image.texture_descriptor.mip_level_count = levels;
        image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: ImageAddressMode::Repeat,
            address_mode_v: ImageAddressMode::ClampToEdge,
            mag_filter: ImageFilterMode::Linear,
            min_filter: ImageFilterMode::Linear,
            mipmap_filter: ImageFilterMode::Linear,
            anisotropy_clamp: 8,
            ..default()
        });
        image
    }
    /// A chopping board's face: pale beech, its grain running along it, and the juice groove a couple of centimetres in
    /// from its edges, stopping short of the handle at its +x end. Across (u) is the board's length, down (v) its width.
    pub fn board() -> Image {
        picture(768, 512, |u, v| {
            let wave = (v * 23. + (u * std::f32::consts::TAU * 0.8).sin() * 0.45 + (u * 31.).sin() * 0.03 + (v * 7.).sin() * 0.2).fract();
            let ring = (wave - 0.5).abs() * 2.;
            let mut c = mix([0.70, 0.50, 0.30], [0.86, 0.69, 0.47], ring.powf(0.5));
            // Fine pores along the grain.
            let pore = ((u * 380. + (v * 61.).sin() * 3.).sin() * (v * 900.).sin()).abs();
            if pore > 0.985 {
                c = mix(c, [0.55, 0.38, 0.22], 0.5);
            }
            // The groove: a rounded rectangle 6 mm wide.
            let (x0, x1, z0, z1, round) = (0.044, 0.80, 0.067, 0.933, 0.05);
            let (cx, cz) = (u.clamp(x0 + round, x1 - round), v.clamp(z0 + round * 1.5, z1 - round * 1.5));
            let d = (((u - cx) / round).powi(2) + ((v - cz) / (round * 1.5)).powi(2)).sqrt() * round;
            let edge = if u > x0 + round && u < x1 - round { (v - z0).abs().min((v - z1).abs()) * 0.667 } else if v > z0 + round * 1.5 && v < z1 - round * 1.5 { (u - x0).abs().min((u - x1).abs()) } else { (d - round).abs() };
            let groove = (1. - edge / 0.0075).clamp(0., 1.);
            mix(c, [0.52, 0.35, 0.20], groove * 0.8)
        })
    }
    /// The bread bin's six faces in a 3 by 2 sheet as `boxed` lays them out: on the front its roll top, slat over slat,
    /// above a lip with BREAD on it in red; cream enamel elsewhere, a seam round the sides.
    pub fn bread() -> Image {
        const CREAM: [f32; 3] = [0.93, 0.89, 0.80];
        picture(768, 512, |x, y| {
            let (cell, u, v) = ((x * 3.).floor() as u32 + 3 * (y * 2.).floor() as u32, (x * 3.).fract(), (y * 2.).fract());
            let rim = (u.min(1. - u).min(v.min(1. - v)) / 0.03).clamp(0., 1.);
            let base = mix([0.78, 0.74, 0.66], CREAM, 0.6 + 0.4 * rim);
            match cell {
                // The front: across (u) and down (v).
                4 => {
                    if v < 0.72 {
                        // The roll top: slats, each shaded as a curve.
                        let slat = (v / 0.72 * 11.).fract();
                        let c = mix([0.80, 0.76, 0.68], CREAM, (slat * std::f32::consts::PI).sin().powf(0.4));
                        if slat < 0.06 { [0.55, 0.50, 0.44] } else { c }
                    } else {
                        let mut c = base;
                        if v < 0.745 {
                            c = [0.62, 0.57, 0.50];
                        }
                        // BREAD in fat red lozenges.
                        let (letters, left, width) = (5., 0.3, 0.4);
                        let at = (u - left) / width * letters;
                        if at > 0. && at < letters && inside(at.fract(), v, [0.5, 0.87], [0.33, 0.06], 0.3) > 0.5 {
                            c = [0.78, 0.12, 0.10];
                        }
                        c
                    }
                }
                // The sides: the roll top's quarter circle drawn as a seam.
                0 | 1 => {
                    let (a, b) = (if cell == 0 { u } else { 1. - u }, v);
                    let seam = (((a - 0.0).powi(2) + (b - 0.72).powi(2)).sqrt() - 0.72).abs();
                    if seam < 0.008 { [0.62, 0.57, 0.50] } else { base }
                }
                2 => base,
                3 => [0.45, 0.45, 0.45],
                _ => base,
            }
        })
    }
    /// A chopping board as it is drawn: a slab `half` (x along, y up, z across) with a handle slot cut through it near
    /// its +x end, [x0, z0, x1, z1]: its top and underside, its sides and the slot's walls, each face flat. The face is
    /// `board`'s picture, its sides a thin band of the same grain.
    pub fn slab(half: [f32; 3], slot: [f32; 4]) -> Mesh {
        let [hx, hy, hz] = half;
        let [sx0, sz0, sx1, sz1] = slot;
        let (mut positions, mut normals, mut uvs, mut indices): (Vec<[f32; 3]>, Vec<[f32; 3]>, Vec<[f32; 2]>, Vec<u32>) = (vec![], vec![], vec![], vec![]);
        let face_uv = |x: f32, z: f32| [(x + hx) / (2. * hx), (z + hz) / (2. * hz)];
        let mut quad = |corners: [[f32; 3]; 4], normal: [f32; 3], uv: [[f32; 2]; 4]| {
            let first = positions.len() as u32;
            for (c, t) in corners.iter().zip(uv) {
                positions.push(*c);
                normals.push(normal);
                uvs.push(t);
            }
            // Wound so that it faces along `normal`.
            let [a, b, c] = [corners[0], corners[1], corners[2]].map(Vec3::from_array);
            if (b - a).cross(c - a).dot(Vec3::from_array(normal)) > 0. {
                indices.extend([first, first + 1, first + 2, first, first + 2, first + 3]);
            } else {
                indices.extend([first, first + 2, first + 1, first, first + 3, first + 2]);
            }
        };
        // Top and underside: the slab round the slot, in four rectangles.
        for (y, ny) in [(hy, 1.), (-hy, -1.)] {
            for [x0, z0, x1, z1] in [[-hx, -hz, sx0, hz], [sx1, -hz, hx, hz], [sx0, -hz, sx1, sz0], [sx0, sz1, sx1, hz]] {
                quad([[x0, y, z0], [x1, y, z0], [x1, y, z1], [x0, y, z1]], [0., ny, 0.], [face_uv(x0, z0), face_uv(x1, z0), face_uv(x1, z1), face_uv(x0, z1)]);
            }
        }
        // Its long sides and its ends, and the slot's walls (facing into the slot).
        let side = |x: f32, y: f32| [(x + hx) / (2. * hx), 0.5 + y / hy * 0.015];
        let end = |z: f32, y: f32| [0.5 + y / hy * 0.008, (z + hz) / (2. * hz)];
        for (z, nz) in [(hz, 1.), (-hz, -1.)] {
            quad([[-hx, -hy, z], [hx, -hy, z], [hx, hy, z], [-hx, hy, z]], [0., 0., nz], [side(-hx, -hy), side(hx, -hy), side(hx, hy), side(-hx, hy)]);
        }
        for (x, nx) in [(hx, 1.), (-hx, -1.)] {
            quad([[x, -hy, -hz], [x, -hy, hz], [x, hy, hz], [x, hy, -hz]], [nx, 0., 0.], [end(-hz, -hy), end(hz, -hy), end(hz, hy), end(-hz, hy)]);
        }
        for (z, nz) in [(sz0, 1.), (sz1, -1.)] {
            quad([[sx0, -hy, z], [sx1, -hy, z], [sx1, hy, z], [sx0, hy, z]], [0., 0., nz], [side(sx0, -hy), side(sx1, -hy), side(sx1, hy), side(sx0, hy)]);
        }
        for (x, nx) in [(sx0, 1.), (sx1, -1.)] {
            quad([[x, -hy, sz0], [x, -hy, sz1], [x, hy, sz1], [x, hy, sz0]], [nx, 0., 0.], [end(sz0, -hy), end(sz1, -hy), end(sz1, hy), end(sz0, hy)]);
        }
        Mesh::new(bevy::mesh::PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
            .with_inserted_indices(bevy::mesh::Indices::U32(indices))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kitchen_sim::map::Loose as Placed;

    fn world() -> World {
        let mut world = World::new();
        world.init_resource::<Assets<Mesh>>();
        world.init_resource::<Assets<StandardMaterial>>();
        world
    }
    /// The bundled map with one more loose object of each kind in two rows along the counter.
    fn furnished() -> Map {
        let mut map = Map::bundled();
        // Each kind in a place of its own, clear of the map's blocks and of each other (the kitchen's pieces are big).
        let places = [
            [-1.45, 2.5], [-1.0, 2.45], [-0.7, 2.45], [-1.0, 2.85], [-1.6, 2.95], [0.0, 2.45], [-0.6, 3.3], [-0.25, 3.2],
            [0.1, 2.9], [0.45, 2.9], [0.2, 3.4], [0.8, 2.5], [0.75, 3.0], [0.95, 3.05], [1.5, 2.35], [1.0, 3.25],
        ];
        for (i, kind) in LOOSE.iter().enumerate() {
            let yaw = if *kind == "breadbox" || *kind == "board" || *kind == "napkin" { 0. } else { 0.3 * i as f64 };
            map.loose.push(Placed { kind: kind.to_string(), p: places[i], base: 0., yaw });
        }
        map
    }
    fn bodies(world: &mut World) -> Vec<(usize, Transform, Visibility)> {
        let mut found: Vec<_> = world.query::<(&Loose, &Transform, &Visibility)>().iter(world).map(|(loose, transform, visibility)| (loose.id, *transform, *visibility)).collect();
        found.sort_by_key(|body| body.0);
        found
    }
    /// Every triangle of a mesh faces the way its corners' normals point, and the normals are unit vectors.
    fn faces_out(mesh: &Mesh) -> bool {
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else { return false };
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(normals)) = mesh.attribute(Mesh::ATTRIBUTE_NORMAL) else { return false };
        let Some(Indices::U32(indices)) = mesh.indices() else { return false };
        normals.iter().all(|n| (Vec3::from_array(*n).length() - 1.).abs() < 1e-3)
            && indices.chunks(3).all(|t| {
                let [a, b, c] = [t[0], t[1], t[2]].map(|i| Vec3::from_array(positions[i as usize]));
                let face = (b - a).cross(c - a);
                let normal = [t[0], t[1], t[2]].iter().map(|i| Vec3::from_array(normals[*i as usize])).sum::<Vec3>();
                // A sliver on the axis has no area to face any way.
                face.length() < 1e-9 || face.dot(normal) > 0.
            })
    }
    #[test]
    fn the_meshes_are_closed_skins_of_the_physics_shapes() {
        let bowl = ShapeSpec::named("bowl").unwrap();
        let Part::Lathe { profile, radius, .. } = &bowl.parts[0] else { panic!("the bowl is turned") };
        let profile: Vec<Vec2> = profile.iter().map(|p| Vec2::new(p[0] as f32, p[1] as f32)).collect();
        let mesh = lathe(&profile, *radius as f32);
        assert!(faces_out(&mesh), "the bowl's skin faces out, inside and outside");
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else { panic!() };
        let (low, high) = positions.iter().fold((f32::MAX, f32::MIN), |(low, high), p| (low.min(p[1]), high.max(p[1])));
        let wide = positions.iter().fold(0f32, |wide, p| wide.max(p[0].hypot(p[2])));
        // The wall is 8 mm thick about the profile: 16 cm to the outside of the rim, 11.3 cm from underside to rim.
        assert!((low + 0.004).abs() < 1e-4 && (high - 0.109).abs() < 1e-4 && (wide - 0.159).abs() < 1.5e-3, "{low} {high} {wide}");
        let mesh = rounded_box([0.07, 0.0225, 0.05], 0.018);
        assert!(faces_out(&mesh), "a rounded box faces out");
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else { panic!() };
        let reach = positions.iter().fold(Vec3::ZERO, |reach, p| reach.max(Vec3::from_array(*p).abs()));
        assert!(reach.distance(Vec3::new(0.07, 0.0225, 0.05)) < 1e-5, "{reach}");
        // Its corners are rounded: none of it reaches the corner of the box it fills.
        assert!(positions.iter().all(|p| Vec3::from_array(*p).abs().distance(reach) > 0.005));
    }
    #[test]
    fn every_loose_object_of_a_map_gets_a_body_where_the_physics_has_it() {
        let mut world = world();
        let map = furnished();
        build(&mut world, &map);
        let mut feast = Feast::with_map(map.clone(), false);
        update(&mut world, &mut feast, false, 1. / 60., &[], &[], None);
        let found = bodies(&mut world);
        assert_eq!(found.len(), 1 + LOOSE.len());
        for (id, transform, visibility) in &found {
            let item = &map.loose[*id];
            assert!(transform.translation.distance(Vec3::new(item.p[0] as f32, FLOOR as f32, item.p[1] as f32)) < 1e-4, "{id}: {}", transform.translation);
            // A napkin's pad is never drawn: its sheet of cloth is.
            let napkin = item.kind == "napkin";
            assert!(transform.rotation.angle_between(Quat::from_rotation_y(item.yaw as f32)) < 1e-3 && (*visibility == Visibility::Inherited) != napkin);
        }
        let sheets = &world.resource::<Sheets>().by;
        assert_eq!(sheets.len(), 1, "one sheet, for the napkin");
        let napkin = map.loose.iter().position(|item| item.kind == "napkin").unwrap();
        let (low, high) = sheets[&napkin].2.bounds();
        assert!(low.y >= FLOOR as f32 && high.y < FLOOR as f32 + 0.05 && (high.x - low.x) > 0.38, "lying on the counter: {low} {high}");
        assert_eq!(world.query_filtered::<(), (With<Loose>, With<crate::level::MapPart>)>().iter(&world).count(), 1 + LOOSE.len());
        // Shoved, the bowl's body follows it: all the way at once when it is far, smoothly when near.
        let at = feast.props.position(0);
        feast.props.set_pose(0, [at[0] + 0.1, at[1], at[2]], feast.props.rotation(0));
        update(&mut world, &mut feast, false, 1. / 60., &[], &[], None);
        let x = bodies(&mut world)[0].1.translation.x;
        assert!(x > at[0] as f32 + 0.03 && x < at[0] as f32 + 0.07, "{x}");
        for _ in 0..30 {
            update(&mut world, &mut feast, false, 1. / 60., &[], &[], None);
        }
        assert!((bodies(&mut world)[0].1.translation.x - at[0] as f32 - 0.1).abs() < 1e-4);
        // A new map replaces them.
        crate::level::build(&mut world, &Map::bundled());
        assert_eq!(bodies(&mut world).len(), 1);
    }
    #[test]
    fn one_over_the_edge_is_seen_down_to_the_kitchen_floor_and_lies_there() {
        let mut world = world();
        let map = Map::bundled();
        build(&mut world, &map);
        let mut feast = Feast::with_map(map, false);
        let bounds = feast.level.map.bounds;
        feast.props.set_pose(0, [0., FLOOR + 0.2, bounds[1] - 0.3], [0.3, 0., 0., 0.954]);
        let floor = feast.bodies.config.kitchen_floor as f32;
        let mut seen = f32::MAX;
        for _ in 0..240 {
            feast.tick(1. / 60.);
            update(&mut world, &mut feast, false, 1. / 60., &[], &[], None);
            let (_, transform, visibility) = bodies(&mut world)[0];
            assert_eq!(visibility, Visibility::Inherited);
            assert!(transform.translation.y <= seen + 1e-4, "it never jumps back up");
            seen = transform.translation.y;
        }
        assert_eq!(feast.props.state(0), State::Lost);
        let (_, transform, _) = bodies(&mut world)[0];
        assert!((transform.translation.y - floor).abs() < 1e-3 && (transform.rotation * Vec3::Y).y > 0.999, "{} {}", transform.translation, transform.rotation * Vec3::Y);
    }
    #[test]
    fn metal_rings_china_chimes_wood_knocks_and_cloth_card_and_a_pillow_are_silent() {
        let (rings, pitch, soft) = voice(&["bowl"], 0.2).unwrap();
        let (_, _, loud) = voice(&["bowl"], 1.5).unwrap();
        assert!(rings && pitch < 0.7 && soft < 0.5 && loud == 1. && soft >= 0.25);
        assert!(voice(&["tray"], 0.5).unwrap().1 > pitch, "the tray rings higher than the big bowl");
        assert_eq!(voice(&["crate"], 0.5).map(|voice| voice.0), Some(false));
        assert!(voice(&["pillow"], 2.).is_none() && voice(&["napkin"], 2.).is_none() && voice(&["cereal"], 2.).is_none() && voice(&["roll"], 2.).is_none());
        // China and glass ring higher than any pan; a cutting board knocks.
        assert!(voice(&["plate"], 0.5).unwrap().1 > voice(&["tray"], 0.5).unwrap().1 && voice(&["salt"], 0.5).unwrap().0);
        assert_eq!(voice(&["board"], 0.5).map(|voice| voice.0), Some(false));
        assert_eq!(voice(&["pillow", "bowl"], 0.5).map(|voice| voice.0), Some(true), "a pillow landing in the bowl still rings it");
    }
    #[test]
    fn a_snapshot_moves_the_clients_copy_and_a_shove_in_step_with_the_server_is_left_alone() {
        let mut world = world();
        let map = Map::bundled();
        build(&mut world, &map);
        let mut feast = Feast::with_map(map, false);
        let rest = feast.props.get_state(0);
        let entry = |state: u8, p: [f64; 3], v: [f64; 3]| -> kitchen_sim::net::Frame {
            serde_json::from_value(serde_json::json!({"props": [{"id": 0, "state": state, "p": p, "q": [0., 0., 0., 1.], "v": v, "w": [0., 0., 0.]}], "knocks": [{"id": 1, "prop": 0, "other": -1, "p": p, "speed": 0.6, "time": 0.}]})).unwrap()
        };
        // Asleep here, moved there: the server's word is taken.
        let there = [rest.position[0] + 0.3, rest.position[1], rest.position[2]];
        apply(&mut world, &mut feast, &entry(1, there, [0.; 3]));
        assert!(feast.props.state(0) == State::Asleep && (feast.props.position(0)[0] - there[0]).abs() < 1e-9);
        // Sliding here, shoved by this player, the copy is ahead of the server's, which is where the copy
        // was a few frames ago: nothing to correct.
        let slide = |world: &mut World, feast: &mut Feast, x: f64| {
            let mut mine = feast.props.get_state(0);
            (mine.state, mine.position[0], mine.velocity) = (State::Awake, x, [0.3, 0., 0.]);
            feast.props.set_state(0, &mine);
            update(world, feast, true, 1. / 60., &[], &[], None);
        };
        for frame in 0..=10 {
            slide(&mut world, &mut feast, there[0] + 0.005 * frame as f64);
        }
        let ahead = feast.props.get_state(0);
        apply(&mut world, &mut feast, &entry(0, [there[0] + 0.02, there[1], there[2]], [0.3, 0., 0.]));
        assert_eq!(feast.props.get_state(0), ahead);
        // Off that trail by 3 cm, it keeps its own motion and gives way by three tenths of the difference.
        apply(&mut world, &mut feast, &entry(0, [there[0] + 0.02, there[1], there[2] + 0.03], [0.3, 0., 0.]));
        let now = feast.props.get_state(0);
        assert!((now.position[2] - there[2] - 0.009).abs() < 1e-9 && (now.position[0] - ahead.position[0]).abs() < 1e-9 && now.velocity == [0.3, 0., 0.] && now.state == State::Awake, "{now:?}");
        // Far from it, it takes the server's place.
        apply(&mut world, &mut feast, &entry(0, [there[0] - 0.2, there[1], there[2]], [0.3, 0., 0.]));
        assert!((feast.props.position(0)[0] - (there[0] - 0.2)).abs() < 1e-9);
        // The server has it at rest while the copy still moves nearby: left to come to rest, and then put
        // exactly where the server has it.
        let settled = [there[0] - 0.19, there[1], there[2]];
        apply(&mut world, &mut feast, &entry(1, settled, [0.; 3]));
        assert_eq!(feast.props.state(0), State::Awake);
        let mut mine = feast.props.get_state(0);
        (mine.state, mine.velocity) = (State::Asleep, [0.; 3]);
        feast.props.set_state(0, &mine);
        update(&mut world, &mut feast, true, 1. / 60., &[], &[], None);
        assert!(feast.props.state(0) == State::Asleep && (feast.props.position(0)[0] - settled[0]).abs() < 1e-9);
        // Lost on the server is lost here.
        apply(&mut world, &mut feast, &entry(3, [0., 0.12, 1.8], [0.; 3]));
        assert_eq!(feast.props.state(0), State::Lost);
    }

    /// Hotdog 1 of a furnished round, facing `yaw`, holding the loose object of `kind` (picked up beside it): the
    /// round, where its eye is, and where the simulation holds the object.
    fn holding(kind: usize, yaw: f32) -> (Feast, Transform, Transform) {
        let mut feast = Feast::with_map(furnished(), false);
        let prop = (0..feast.props.len()).rev().find(|i| feast.props.body(*i).shape == kind).unwrap();
        let p = feast.props.position(prop);
        let feet = Vec3::new(p[0] as f32 + 0.1, FLOOR as f32, p[2] as f32);
        assert_eq!(feast.players[0].id, 1);
        (feast.players[0].p, feast.players[0].yaw, feast.players[0].alive) = (feet.to_array(), yaw, true);
        assert!(feast.grab(1, Vec3::Y), "{kind}: picked up");
        assert_eq!(feast.players[0].carry as usize, prop);
        let (p, q) = (feast.props.position(prop), feast.props.rotation(prop));
        let held = Transform::from_xyz(p[0] as f32, p[1] as f32, p[2] as f32).with_rotation(Quat::from_xyzw(q[0] as f32, q[1] as f32, q[2] as f32, q[3] as f32));
        let eye = Transform::from_translation(feet + Vec3::Y * kitchen_sim::feast::EYE).with_rotation(Quat::from_rotation_y(yaw));
        (feast, eye, held)
    }

    #[test]
    fn a_carried_object_is_seen_in_first_person_at_its_true_size_where_it_is_held() {
        for kind in (0..LOOSE.len()).filter(|kind| *kind != NAPKIN) {
            for yaw in [0., 1.1, -2.4] {
                let (_, eye, held) = holding(kind, yaw);
                let local = held_place(kind, held, eye, Vec3::ZERO, 0.);
                // At its true size, turned as it is held, straight ahead of the hotdog where it is held.
                assert_eq!(local.scale, Vec3::ONE, "{kind}");
                let seen = eye.mul_transform(local);
                assert!(seen.rotation.angle_between(held.rotation) < 1e-4, "{kind}");
                assert!(seen.translation.xz().distance(held.translation.xz()) < 1e-5, "{kind}: {:?} {:?}", seen.translation, held.translation);
                // Looking up or down does not move it: it is held by the hotdog, not by the view.
                let pitched = eye.with_rotation(eye.rotation * Quat::from_rotation_x(-0.4));
                assert!(pitched.mul_transform(held_place(kind, held, pitched, Vec3::ZERO, 0.)).translation.distance(seen.translation) < 1e-5);
                // Its top keeps under the line of sight, looking level: the crosshair is clear.
                let ahead = (held.translation - eye.translation).with_y(0.).normalize();
                let over = tops(kind).into_iter().map(|c| {
                    let at = seen.transform_point(c) - eye.translation;
                    at.y + CLEAR.tan() * at.dot(ahead).max(0.)
                });
                let highest = over.fold(f32::MIN, f32::max);
                assert!(highest < 1e-5, "{kind}: {highest}");
                let lowered = held.translation.y - seen.translation.y;
                match LOOSE[kind] {
                    // Held low enough already: exactly where the simulation has it.
                    "pillow" | "tray" | "napkin" | "plate" | "board" => assert!(lowered.abs() < 1e-6, "{kind}: {lowered}"),
                    // Taller than the eye where it is held: let down only as far as keeps the crosshair clear.
                    _ => assert!(lowered > 0.005 && highest > -1e-5, "{kind}: {lowered} {highest}"),
                }
            }
        }
    }

    #[test]
    fn the_first_person_model_hangs_from_the_camera_at_a_scale_of_one() {
        let mut world = world();
        world.init_resource::<Kit>();
        let (_, eye, place) = holding(4, 0.7);
        let camera = world.spawn(eye).id();
        held(&mut world, camera, Some((4, place)), eye, 0., Vec3::ZERO);
        let models: Vec<(usize, Transform, Visibility, Entity)> = world.query::<(&Held, &Transform, &Visibility, &ChildOf)>().iter(&world).map(|(h, t, v, c)| (h.0, *t, *v, c.parent())).collect();
        assert_eq!(models.len(), LOOSE.len() - 1, "one for each kind but the napkin");
        let shown: Vec<_> = models.iter().filter(|m| m.2 != Visibility::Hidden).collect();
        assert_eq!(shown.len(), 1);
        let (kind, local, _, parent) = *shown[0];
        assert_eq!((kind, parent), (4, camera));
        assert_eq!(local.scale, Vec3::ONE, "the chair at its true size");
        assert_eq!(local, held_place(4, place, eye, Vec3::ZERO, 0.));
        // Put down, it is gone from the view.
        held(&mut world, camera, None, eye, 0., Vec3::ZERO);
        assert!(world.query::<(&Held, &Visibility)>().iter(&world).all(|(_, v)| *v == Visibility::Hidden));
    }

    /// The most triangles each kind is drawn with, all its parts together, in LOOSE's order. A Forts map puts out some
    /// sixty of the kitchen's pieces, and each was once drawn in tens of thousands (a roll of paper towels in 150,000, a
    /// salt shaker in 82,000): two million triangles, every frame, and again for every shadow.
    const BUDGET: [usize; 16] = [1700, 300, 300, 300, 1800, 1500, 300, 1400, 2000, 800, 40, 1300, 1300, 2100, 2100, 1000];
    fn triangles(world: &World, mesh: &Handle<Mesh>) -> usize {
        world.resource::<Assets<Mesh>>().get(mesh).map_or(0, |mesh| mesh.indices().map_or(mesh.count_vertices(), |i| i.len()) / 3)
    }
    #[test]
    fn every_kind_is_drawn_within_its_triangle_budget_and_a_forts_maps_pieces_within_theirs() {
        let mut world = world();
        world.init_resource::<Kit>();
        let kinds = world.resource::<Kit>().kinds.clone();
        for (kind, parts) in kinds.iter().enumerate() {
            let drawn: usize = parts.iter().map(|(mesh, _, _)| triangles(&world, mesh)).sum();
            println!("{:9} {drawn:5} triangles", LOOSE[kind]);
            assert!(drawn <= BUDGET[kind], "{}: {drawn} triangles, over its budget of {}", LOOSE[kind], BUDGET[kind]);
        }
        // A napkin's sheet, loose or carried.
        let sheet = crate::cloth::mesh(&Cloth::new([0.45, 0.45], Drive::Loose { at: Vec3::ZERO, turn: Quat::IDENTITY, resting: true }));
        let drawn = sheet.indices().unwrap().len() / 3;
        assert!(drawn <= 2200, "a napkin's sheet: {drawn} triangles");
        // Every loose object of each Forts map, as `level::build` puts them out: some sixty pieces, under a tenth of the
        // two million they once were.
        for name in ["pillow-fort", "jungle-fort", "moonlit-fort", "the-whole-kitchen-forts"] {
            let map = Map::parse(&std::fs::read_to_string(format!("assets/maps/{name}.json")).unwrap()).unwrap();
            crate::level::build(&mut world, &map);
            let parts: Vec<Handle<Mesh>> = world.query::<(&Mesh3d, &ChildOf)>().iter(&world).filter(|(_, c)| world.get::<Loose>(c.parent()).is_some()).map(|(m, _)| m.0.clone()).collect();
            let drawn: usize = parts.iter().map(|mesh| triangles(&world, mesh)).sum();
            println!("{name}: {} loose objects in {drawn} triangles", map.loose.len());
            assert!(map.loose.len() > 40 && drawn < 100_000, "{name}: {} loose objects in {drawn} triangles", map.loose.len());
        }
    }
    #[test]
    fn a_turned_piece_is_painted_in_a_picture_of_its_own_with_its_bands_and_its_specks() {
        let v = |x: f32, y: f32| Vec2::new(x, y);
        let texel = |image: &Image, x: u32, y: u32| {
            let at = ((y * image.width() + x) * 4) as usize;
            image.data.as_ref().unwrap()[at..at + 3].to_vec()
        };
        // A mug: the same all the way round, so one texel across, teal outside and cream inside, down its whole outline.
        let rows = outline(&[v(0., 0.0035), v(0.036, 0.0035), v(0.0395, 0.012), v(0.0395, 0.0975)], 0.0025);
        let (mug, uv) = art::baked(&rows, "cup");
        assert_eq!(mug.width(), 1);
        assert!(mug.texture_descriptor.mip_level_count > 5, "with its mipmaps");
        let down: Vec<Vec<u8>> = (0..mug.height()).map(|y| texel(&mug, 0, y)).collect();
        let (teal, cream) = (down.iter().filter(|c| c[2] > c[0] + 40).count(), down.iter().filter(|c| c[0] > 220 && c[1] > 220).count());
        assert!(teal > down.len() / 5 && cream > down.len() / 5, "{teal} teal and {cream} cream of {}", down.len());
        // The transform takes the mesh's texture co-ordinates (four times round, eight times a metre along) to the picture.
        let mesh = turn(&rows);
        let Some(bevy::mesh::VertexAttributeValues::Float32x2(uvs)) = mesh.attribute(Mesh::ATTRIBUTE_UV_0) else { panic!() };
        let last = uv.transform_point2(Vec2::from_array(uvs[uvs.len() - 1]));
        assert!((last - Vec2::ONE).length() < 1e-4, "{last}");
        // A cereal bowl's glaze is speckled: a picture across as well as down, with dark specks in its sage.
        let (dish, _) = art::baked(&outline(&[v(0., 0.0035), v(0.038, 0.0035), v(0.064, 0.0315), v(0.0715, 0.0615)], 0.0035), "dish");
        assert!(dish.width() >= 256);
        let row: Vec<Vec<u8>> = (0..dish.width()).map(|x| texel(&dish, x, dish.height() / 4)).collect();
        let dark = row.iter().filter(|c| (c[1] as f32) < 0.75 * row.iter().map(|c| c[1] as f32).fold(0., f32::max)).count();
        assert!(dark > 0 && dark < row.len() / 3, "{dark} of {} texels speckled", row.len());
    }
}
