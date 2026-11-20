// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! Placement previews: where what this player carries would land if Q put it down now, drawn every frame from the
//! simulation's own answer (kitchen_sim `Feast::placement`, by which `Feast::put` lands it, so the preview is the
//! landing).
//!
//! * **The ghost.** A see-through copy of the object, made of the same meshes (`loose::meshes`), unlit, its rim and
//!   rounded edges glowing (`assets/shaders/ghost.wgsl`) and casting no shadow, where it would land and turned as it
//!   would be, with a thin box drawn round it ([`corners`]). Its tint ([`Tint`]) says what Q would do: white, a plain
//!   put (on the move it is let go with the hotdog's pace and settles where the physics takes it, locking there);
//!   cyan, it snaps onto something or flush beside it but is not locked at once (it settles first); green, it is
//!   locked where it lands. Whatever it snaps to (the object it lands on, the one it is drawn flush against) glows
//!   cyan.
//! * **A napkin.** Its ghost is green where it would drape into a wall and grey where it would only lie flat,
//!   which is also where the drape would be refused once it landed: `Feast::refusal` is asked here too, so that the
//!   preview never shows a wall that the landing then refuses. Draping, the wall it would make is outlined in green
//!   (its top at the napkin's top, its corners down to the counter, a faint pane between them), and each loose object
//!   it would cover glows green. Lying flat, its own outline is drawn in grey where it would lie.
//! * **Refused.** In Forts nothing may be put down in a team's base (`Placement::refused`): the ghost is grey there,
//!   nothing glows, and Q leaves it in the hands.
//! * **Turned and tipped.** It is drawn as it is held (the wheel turns it, X tips it: kitchen_sim build.rs `Pose`), which
//!   is how it lands.
//! * **The tooltip** (hints.rs) says the same in words ([`Spot::put`]): "Q: put down (snaps, locks)", "Q: drape over
//!   3 items".
//! * It is drawn in the first-person pass (viewmodel.rs `VIEW_LAYER`) with the depth test off: over the world, and
//!   over the object in the player's hands, which would otherwise hide the spot just under it. It is shown while this
//!   player's living hotdog carries something in a round that takes Q ([`wanted`]): not dead (the view camera then
//!   draws nothing at all), not in the demo, not at the card table, not between rounds. It stays when H hides the
//!   hints: it is the put's own feedback, as the crosshair is the gun's, not a tip about the game.
//! * **Cost.** The preview is asked once a frame. The refusal, which in Forts builds a navigation grid, is asked again
//!   only when the drape it is asked about, the walls or the bodies near them change. The ghost, its box, the
//!   outline's edges and pane and the glows are made once (the glows as many as have been needed at once) and kept:
//!   each frame moves them, shows or hides them and swaps their materials, which are made once too.
//! * **Online** the client's own copy of the round is asked (`arena::Game::placement`), about this player's hotdog as
//!   its prediction has it, holding what the server's frames say it holds. Its pace, which the server keeps and does
//!   not send, is kept here from where the hotdog is drawn, as the simulation keeps it.
use crate::arena::Game;
use crate::hints::Put;
use crate::viewmodel::VIEW_LAYER;
use bevy::{
    camera::visibility::RenderLayers,
    light::NotShadowCaster,
    mesh::MeshVertexBufferLayoutRef,
    pbr::{MaterialPipeline, MaterialPipelineKey},
    prelude::*,
    render::render_resource::{AsBindGroup, CompareFunction, RenderPipelineDescriptor, SpecializedMeshPipelineError},
    shader::ShaderRef,
};
use kitchen_sim::drape::{Canopy, Refusal, NAPKIN};
use kitchen_sim::feast::{Feast, Placement, KEEP, PACE};

/// How much larger than the object a glow is drawn, so that it wraps the object rather than lying on its faces.
const GLOW: f32 = 1.06;
/// How thick the edges of the box round a ghost are, and those of a napkin's outline, which is larger and further
/// off (metres).
const EDGE: f32 = 0.0015;
const OUTLINE: f32 = 0.003;
/// The most glows shown at once (a napkin over a pile covers a dozen or two).
const GLOWS: usize = 40;

pub struct Plugin;
impl bevy::app::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<GhostMaterial>::default()).init_resource::<Preview>().add_systems(Update, tidy.after(crate::arena::tick));
    }
}

/// The preview's material (`assets/shaders/ghost.wgsl`): a tint seen through, more opaque and paler at its rim and
/// where it bends sharply, unlit, casting no shadow, and drawn whatever stands in front of it.
#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct GhostMaterial {
    /// The tint (linear) and how opaque it is where it faces the eye.
    #[uniform(0)]
    pub colour: Vec4,
    /// How much more opaque the rim is, how sharply it falls off, how far the whole breathes, and how much paler the
    /// rim is.
    #[uniform(0)]
    pub rim: Vec4,
    /// How much more opaque it is where its surface bends sharply (a rounded edge or corner), and how fast its normal
    /// must turn across a pixel to count fully as a bend.
    #[uniform(0)]
    pub crease: Vec4,
}
impl Material for GhostMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/ghost.wgsl".into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }
    fn enable_prepass() -> bool {
        false
    }
    fn enable_shadows() -> bool {
        false
    }
    fn specialize(_pipeline: &MaterialPipeline, descriptor: &mut RenderPipelineDescriptor, _layout: &MeshVertexBufferLayoutRef, _key: MaterialPipelineKey<Self>) -> Result<(), SpecializedMeshPipelineError> {
        // Drawn over whatever is in front of it: in the first-person pass that is only the object in the hands.
        if let Some(depth) = descriptor.depth_stencil.as_mut() {
            depth.depth_compare = Some(CompareFunction::Always);
            depth.depth_write_enabled = Some(false);
        }
        Ok(())
    }
}

/// What the preview's colour says about a landing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tint {
    /// White: a plain put, let go to settle where the physics takes it (and lock there).
    Plain,
    /// Cyan: it snaps onto or beside something, and settles before it locks.
    Snap,
    /// Green: it is locked where it lands.
    Lock,
    /// Green: a napkin that drapes into a wall.
    Drape,
    /// Grey: a napkin that lies flat.
    Flat,
}
impl Tint {
    const ALL: [Tint; 5] = [Tint::Plain, Tint::Snap, Tint::Lock, Tint::Drape, Tint::Flat];
    /// Its colour (linear).
    pub fn colour(self) -> [f32; 3] {
        match self {
            Tint::Plain => [0.86, 0.89, 0.93],
            Tint::Snap => [0.02, 0.72, 1.0],
            Tint::Lock | Tint::Drape => [0.08, 0.95, 0.22],
            Tint::Flat => [0.58, 0.60, 0.63],
        }
    }
    fn index(self) -> usize {
        self as usize
    }
}

/// What the preview shows this frame: the simulation's placement, and for a napkin that would drape why the drape
/// would be refused once it landed (None: it would stand).
#[derive(Clone, Debug, PartialEq)]
pub struct Spot {
    pub placement: Placement,
    pub refused: Option<Refusal>,
}
impl Spot {
    pub fn napkin(&self) -> bool {
        self.placement.kind == NAPKIN
    }
    /// A napkin: the wall it would drape into, unless the drape would be refused (it then lies flat).
    pub fn wall(&self) -> Option<&Canopy> {
        self.placement.drape.as_ref().filter(|_| self.refused.is_none())
    }
    /// The ghost's tint: for a napkin whether it drapes into a wall or lies flat; for anything else whether it
    /// locks where it lands, or failing that snaps to something, or is a plain put.
    pub fn tint(&self) -> Tint {
        // Grey where it may not be put down at all (Forts: in a base).
        if self.placement.refused.is_some() {
            return Tint::Flat;
        }
        match (self.napkin(), self.placement.lock, self.placement.snapped) {
            (true, _, _) if self.wall().is_some() => Tint::Drape,
            (true, _, _) => Tint::Flat,
            (false, true, _) => Tint::Lock,
            (false, false, true) => Tint::Snap,
            (false, false, false) => Tint::Plain,
        }
    }
    /// Where the ghost is drawn: the object's origin (the middle of its underside) where it would land, turned as it
    /// would be.
    pub fn ghost(&self) -> Transform {
        let (p, q) = (self.placement.p, self.placement.q);
        Transform::from_xyz(p[0] as f32, p[1] as f32, p[2] as f32).with_rotation(Quat::from_xyzw(q[0] as f32, q[1] as f32, q[2] as f32, q[3] as f32).normalize())
    }
    /// The loose objects that glow, and in what: those a napkin would drape over, in green; or what anything else
    /// snaps to (the object it lands on, the one it is drawn flush against), in cyan.
    pub fn lit(&self) -> Vec<(usize, Tint)> {
        if self.placement.refused.is_some() {
            return vec![];
        }
        if self.napkin() {
            return self.wall().map_or(vec![], |c| c.props.iter().map(|p| (*p, Tint::Drape)).collect());
        }
        let mut lit: Vec<(usize, Tint)> = self.placement.onto.into_iter().map(|p| (p, Tint::Snap)).collect();
        if let Some(p) = self.placement.beside.filter(|p| Some(*p) != self.placement.onto) {
            lit.push((p, Tint::Snap));
        }
        lit
    }
    /// What Q would do, for the tooltip.
    pub fn put(&self) -> Put {
        let p = &self.placement;
        if let Some(why) = p.refused {
            return Put::Refused(why);
        }
        if self.napkin() {
            return match (&p.drape, self.refused) {
                (Some(c), None) => Put::Drape(c.props.len()),
                (_, refused) => Put::Flat(refused),
            };
        }
        if p.velocity.iter().any(|v| v.abs() > 1e-9) {
            return Put::Moving;
        }
        Put::Still { snaps: p.snapped, locks: p.lock }
    }
}

/// The outline drawn round a napkin's landing: the four corners of its top, in order round it, and the height its
/// corners come down to (the counter under a wall; the top itself for a napkin lying flat, whose outline has no
/// sides).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Outline {
    pub corners: [Vec3; 4],
    pub foot: f32,
    pub tint: Tint,
}
impl Outline {
    /// Whether it outlines a wall (a box down to the counter) rather than a napkin lying flat (a rectangle).
    pub fn wall(&self) -> bool {
        self.tint == Tint::Drape
    }
}
/// The outline for a napkin's landing (None for anything else): draping, the wall it would make (`Canopy`: over
/// what it covers, up to the napkin's top, down to the counter); lying flat, its own outline where it would lie.
pub fn outline(spot: &Spot, feast: &Feast) -> Option<Outline> {
    if !spot.napkin() {
        return None;
    }
    let round = |[x0, z0, x1, z1]: [f32; 4]| [(x0, z0), (x1, z0), (x1, z1), (x0, z1)];
    if let Some(c) = spot.wall() {
        let ([x0, z0, x1, z1], top) = (c.rect.map(|v| v as f32), c.top as f32);
        let foot = feast.level.ground(c.rect[0] / 2. + c.rect[2] / 2., c.rect[1] / 2. + c.rect[3] / 2.) as f32;
        return Some(Outline { corners: round([x0, z0, x1, z1]).map(|(x, z)| Vec3::new(x, top, z)), foot, tint: Tint::Drape });
    }
    let shape = feast.props.shape(spot.placement.kind);
    let (a, o) = (shape.aabb, shape.origin);
    let ([x0, z0, x1, z1], top) = ([a[0] - o[0], a[2] - o[2], a[3] - o[0], a[5] - o[2]].map(|v| v as f32), (a[4] - o[1]) as f32);
    let at = spot.ghost();
    let corners = round([x0, z0, x1, z1]).map(|(x, z)| at.transform_point(Vec3::new(x, top, z)));
    Some(Outline { corners, foot: corners[0].y, tint: Tint::Flat })
}

/// The corners of the box round the ghost of anything but a napkin (whose outline is `outline`'s), where it would
/// land and turned as it would be: corner `k` is at the low or high end of the object's x, y and z as bits 1, 2 and 4
/// of `k` say. Its edges are drawn in the ghost's tint, a crisp line round the landing that reads even over the
/// object in the hands, which stands just above the spot.
pub fn corners(spot: &Spot, feast: &Feast) -> Option<[Vec3; 8]> {
    if spot.napkin() {
        return None;
    }
    let shape = feast.props.shape(spot.placement.kind);
    let (a, o) = (shape.aabb, shape.origin);
    let (low, high) = (Vec3::new((a[0] - o[0]) as f32, (a[1] - o[1]) as f32, (a[2] - o[2]) as f32), Vec3::new((a[3] - o[0]) as f32, (a[4] - o[1]) as f32, (a[5] - o[2]) as f32));
    let at = spot.ghost();
    Some(std::array::from_fn(|k| at.transform_point(Vec3::new(if k & 1 == 0 { low.x } else { high.x }, if k & 2 == 0 { low.y } else { high.y }, if k & 4 == 0 { low.z } else { high.z }))))
}

/// Whether this player is shown the preview: playing its own living hotdog in a round that takes Q (not the demo, a
/// cut scene, the fridge, the card table, the death camera, nor the end of a round or the pause between two). Not
/// whether the hints are on: H leaves it.
pub fn wanted(g: &Game) -> bool {
    crate::hints::wanted(g) && !matches!(g.phase_name(), "RoundOver" | "Rest")
}

/// A hotdog's pace as the simulation keeps it (`Feast`'s `stride`): its walk averaged over PACE seconds and at most
/// KEEP m/s, from where it was `dt` seconds ago (`last`) and where it is now; a jump further than any walk is no pace
/// at all. Returns the new pace.
pub fn pace(pace: [f32; 2], last: Option<Vec3>, now: Vec3, dt: f32) -> [f32; 2] {
    let Some(was) = last.filter(|_| dt > 0.) else {
        return pace;
    };
    let (mut vx, mut vz) = ((now.x - was.x) / dt, (now.z - was.z) / dt);
    let fast = vx.hypot(vz);
    if fast > KEEP * 2.5 {
        return pace;
    }
    if fast > KEEP {
        (vx, vz) = (vx * KEEP / fast, vz * KEEP / fast);
    }
    let k = (dt / PACE).min(1.);
    [pace[0] + (vx - pace[0]) * k, pace[1] + (vz - pace[1]) * k]
}

/// What the preview shows, and what it keeps from frame to frame.
#[derive(Resource, Default)]
pub struct Preview {
    /// What is shown, and the frame (`arena::Game::frame`) it was worked out on: it is current only on that frame.
    spot: Option<Spot>,
    frame: u32,
    /// Online: this player's pace, and where its hotdog was drawn on the last frame.
    pace: [f32; 2],
    last: Option<Vec3>,
    /// The last drape the refusal was asked about, and its answer.
    asked: Option<(Asked, Option<Refusal>)>,
    /// How often it has been asked, for the tests.
    pub refusals: u32,
    kit: Option<Kit>,
    /// How many entities have been made, for the tests: they are made once and reused.
    pub spawned: usize,
}
impl Preview {
    /// What the preview shows on frame `frame` (None when it was not worked out on that frame).
    pub fn spot(&self, frame: u32) -> Option<&Spot> {
        self.spot.as_ref().filter(|_| self.frame == frame)
    }
}

/// What a refusal depends on: the drape's wall (to a tenth of a millimetre), the walls standing, the forcefield and
/// where the bodies near the new wall stand (to a millimetre).
#[derive(Clone, Debug, PartialEq, Eq)]
struct Asked {
    rect: [i64; 4],
    top: i64,
    drapes: Vec<u32>,
    field: bool,
    bodies: Vec<(u32, [i64; 3])>,
}
impl Asked {
    fn of(canopy: &Canopy, feast: &Feast, bodies: &kitchen_sim::actor::World) -> Self {
        let (tenth, millimetre) = (|v: f64| (v * 1e4).round() as i64, |v: f64| (v * 1e3).round() as i64);
        let r = canopy.rect;
        let near = |p: [f64; 3]| p[0] > r[0] - 0.1 && p[0] < r[2] + 0.1 && p[2] > r[1] - 0.1 && p[2] < r[3] + 0.1;
        Self {
            rect: r.map(tenth),
            top: tenth(canopy.top),
            drapes: feast.drapes.iter().map(|d| d.id).collect(),
            field: feast.fort.as_ref().is_some_and(|f| f.up),
            bodies: bodies.actors.iter().filter(|a| near(a.p)).map(|a| (a.id, a.p.map(millimetre))).collect(),
        }
    }
}

/// Marks everything the preview draws.
#[derive(Component)]
struct Mark;
/// A shape drawn with a loose object kind's meshes: its root, its parts (as many as any kind it has worn needed) and
/// the kind it wears.
struct Shape {
    root: Entity,
    parts: Vec<Entity>,
    kind: Option<usize>,
    tint: Option<Tint>,
}
/// The entities and the materials, made the first time they are needed.
struct Kit {
    ghost: Shape,
    glows: Vec<Shape>,
    /// The napkin's outline's twelve edges (the top's four, the corners' four, the foot's four) and the pane between
    /// them; the twelve edges of the box round anything else's ghost (`corners`).
    edges: [Entity; 12],
    pane: Entity,
    boxed: [Entity; 12],
    /// The materials by tint (`Tint::index`): the ghost's (a napkin's, which is large, fainter), the glows', the
    /// edges', the pane's.
    ghosts: [Handle<GhostMaterial>; 5],
    sheets: [Handle<GhostMaterial>; 5],
    glows_by: [Handle<GhostMaterial>; 5],
    lines: [Handle<GhostMaterial>; 5],
    panes: [Handle<GhostMaterial>; 5],
    /// The tints the outline and the box wear now.
    outline: Option<Tint>,
    boxing: Option<Tint>,
}

/// Something of the preview, on the first-person layer, hidden until it is shown.
fn drawn() -> impl Bundle {
    (Mark, RenderLayers::layer(VIEW_LAYER), NotShadowCaster, Transform::default(), Visibility::Hidden)
}
fn make(world: &mut World) -> Kit {
    let mut materials = world.resource_mut::<Assets<GhostMaterial>>();
    let mut each = |alpha: f32, rim: [f32; 4], crease: f32| Tint::ALL.map(|t| {
        let [r, g, b] = t.colour();
        materials.add(GhostMaterial { colour: Vec4::new(r, g, b, alpha), rim: Vec4::from_array(rim), crease: Vec4::new(crease, 0.12, 0., 0.) })
    });
    // The ghost: a fill that tints what is behind it, a stronger rim and edges a little paler, breathing a little (a
    // napkin's, half a metre across, fainter); a glow: a tint, its rim and edges strong, so that each object it lights
    // reads by its own outline; an edge: a line of colour; the pane: barely there, a little more edge on.
    let (ghosts, sheets, glows_by) = (each(0.30, [0.5, 2.5, 0.15, 0.15], 0.5), each(0.12, [0.25, 2.5, 0.15, 0.1], 0.), each(0.55, [0.35, 2., 0., 0.1], 0.8));
    let (lines, panes) = (each(0.9, [0., 1., 0., 0.], 0.), each(0.07, [0.25, 2.5, 0., 0.], 0.));
    let cube = world.resource_mut::<Assets<Mesh>>().add(Cuboid::new(1., 1., 1.));
    let ghost = Shape { root: world.spawn((Name::new("Placement preview"), Mark, Transform::default(), Visibility::Hidden)).id(), parts: vec![], kind: None, tint: None };
    let mut line = |name: &'static str| world.spawn((Name::new(name), Mesh3d(cube.clone()), MeshMaterial3d(lines[0].clone()), drawn())).id();
    let (edges, boxed) = (std::array::from_fn(|_| line("Placement outline")), std::array::from_fn(|_| line("Placement box")));
    let pane = world.spawn((Name::new("Placement wall"), Mesh3d(cube), MeshMaterial3d(panes[0].clone()), drawn())).id();
    world.resource_mut::<Preview>().spawned += 26;
    Kit { ghost, glows: vec![], edges, pane, boxed, ghosts, sheets, glows_by, lines, panes, outline: None, boxing: None }
}
/// Dresses a shape as loose object kind `kind`, tinted with `material`: its parts take the kind's meshes (more parts
/// are made the first time a kind needs them, and those it does not need are hidden).
fn dress(world: &mut World, shape: &mut Shape, kind: usize, tint: Tint, material: &Handle<GhostMaterial>) {
    if shape.kind != Some(kind) {
        let meshes = crate::loose::meshes(world, kind);
        while shape.parts.len() < meshes.len() {
            let part = world.spawn((Mesh3d(meshes[0].0.clone()), MeshMaterial3d(material.clone()), drawn())).id();
            world.entity_mut(shape.root).add_child(part);
            shape.parts.push(part);
            world.resource_mut::<Preview>().spawned += 1;
        }
        for (i, part) in shape.parts.iter().enumerate() {
            let mut e = world.entity_mut(*part);
            match meshes.get(i) {
                Some((mesh, centre)) => {
                    e.insert((Mesh3d(mesh.clone()), Transform::from_translation(*centre), Visibility::Inherited));
                }
                None => {
                    e.insert(Visibility::Hidden);
                }
            }
        }
        (shape.kind, shape.tint) = (Some(kind), None);
    }
    if shape.tint != Some(tint) {
        for part in &shape.parts {
            world.entity_mut(*part).insert(MeshMaterial3d(material.clone()));
        }
        shape.tint = Some(tint);
    }
}
fn set(world: &mut World, entity: Entity, at: Option<Transform>) {
    let Ok(mut e) = world.get_entity_mut(entity) else {
        return;
    };
    let shown = if at.is_some() { Visibility::Inherited } else { Visibility::Hidden };
    if let Some(at) = at {
        if e.get::<Transform>().is_none_or(|t| *t != at) {
            e.insert(at);
        }
    }
    if e.get::<Visibility>().is_none_or(|v| *v != shown) {
        e.insert(shown);
    }
}
/// A box from `a` to `b`, `thick` thick.
fn edge(a: Vec3, b: Vec3, thick: f32) -> Transform {
    let along = b - a;
    let length = along.length();
    Transform { translation: (a + b) / 2., rotation: Quat::from_rotation_arc(Vec3::X, along / length.max(1e-6)), scale: Vec3::new(length + thick, thick, thick) }
}

/// Every frame of a round, once the hotdogs and the loose objects stand where the frame has them (arena::tick): asks
/// where what this player carries would land, keeps it for the tooltip, and draws it.
pub fn show(world: &mut World, g: &mut Game, dt: f32) {
    world.init_resource::<Preview>();
    let wanted = wanted(g);
    let spot = {
        let mut preview = world.resource_mut::<Preview>();
        // (Online: the pace the server keeps, from where the hotdog is drawn.)
        if g.network && wanted {
            preview.pace = pace(preview.pace, preview.last, g.position, dt);
            preview.last = Some(g.position);
        } else {
            (preview.pace, preview.last) = ([0.; 2], None);
        }
        let placement = if wanted { g.placement(preview.pace) } else { None };
        let spot = placement.map(|placement| {
            let refused = placement.refused.or_else(|| placement.drape.as_ref().and_then(|canopy| {
                let asked = Asked::of(canopy, &g.feast, g.bodies());
                match &preview.asked {
                    Some((before, answer)) if *before == asked => *answer,
                    _ => {
                        let answer = g.feast.refusal(canopy, g.bodies());
                        preview.refusals += 1;
                        preview.asked = Some((asked, answer));
                        answer
                    }
                }
            }));
            Spot { placement, refused }
        });
        (preview.spot, preview.frame) = (spot.clone(), g.frame());
        spot
    };
    // Nothing is made until there is something to show, and the loose objects' meshes are there to show it with.
    if world.resource::<Preview>().kit.is_none() {
        if spot.as_ref().is_none_or(|s| crate::loose::meshes(world, s.placement.kind).is_empty()) {
            return;
        }
        let kit = make(world);
        world.resource_mut::<Preview>().kit = Some(kit);
    }
    let mut kit = world.resource_mut::<Preview>().kit.take().expect("made above");
    draw(world, &mut kit, spot.as_ref().map(|s| (s, &g.feast)));
    world.resource_mut::<Preview>().kit = Some(kit);
}
/// Shows `shown` (a spot, in the round it is in), or hides everything.
fn draw(world: &mut World, kit: &mut Kit, shown: Option<(&Spot, &Feast)>) {
    // The ghost.
    match shown {
        Some((spot, _)) => {
            let tint = spot.tint();
            let material = if spot.napkin() { kit.sheets[tint.index()].clone() } else { kit.ghosts[tint.index()].clone() };
            dress(world, &mut kit.ghost, spot.placement.kind, tint, &material);
            set(world, kit.ghost.root, Some(spot.ghost()));
        }
        None => set(world, kit.ghost.root, None),
    }
    // The glows: each round its object, centred on it, a little larger.
    let lit = shown.map_or(vec![], |(s, f)| s.lit().into_iter().filter(|(p, _)| *p < f.props.len()).collect());
    for (i, (prop, tint)) in lit.iter().take(GLOWS).enumerate() {
        let Some((_, feast)) = shown else {
            break;
        };
        if i == kit.glows.len() {
            let root = world.spawn((Name::new("Placement glow"), Mark, Transform::default(), Visibility::Hidden)).id();
            world.resource_mut::<Preview>().spawned += 1;
            kit.glows.push(Shape { root, parts: vec![], kind: None, tint: None });
        }
        let kind = feast.props.body(*prop).shape;
        let material = kit.glows_by[tint.index()].clone();
        dress(world, &mut kit.glows[i], kind, *tint, &material);
        let (p, q, shape) = (feast.props.position(*prop), feast.props.rotation(*prop), feast.props.shape(kind));
        let (a, o) = (shape.aabb, shape.origin);
        let middle = Vec3::new((a[0] + a[3]) as f32 / 2. - o[0] as f32, (a[1] + a[4]) as f32 / 2. - o[1] as f32, (a[2] + a[5]) as f32 / 2. - o[2] as f32);
        let turn = Quat::from_xyzw(q[0] as f32, q[1] as f32, q[2] as f32, q[3] as f32).normalize();
        let at = Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32) + turn * (middle * (1. - GLOW));
        set(world, kit.glows[i].root, Some(Transform { translation: at, rotation: turn, scale: Vec3::splat(GLOW) }));
    }
    for shape in kit.glows.iter().skip(lit.len().min(GLOWS)) {
        set(world, shape.root, None);
    }
    // A napkin's outline: the top's edges, and for a wall its corners down to the counter, its foot and its pane.
    let outline = shown.and_then(|(s, f)| outline(s, f));
    let tint = outline.map(|o| o.tint);
    if tint.is_some() && kit.outline != tint {
        let (line, pane) = (kit.lines[tint.unwrap().index()].clone(), kit.panes[tint.unwrap().index()].clone());
        for e in kit.edges {
            world.entity_mut(e).insert(MeshMaterial3d(line.clone()));
        }
        world.entity_mut(kit.pane).insert(MeshMaterial3d(pane));
        kit.outline = tint;
    }
    let mut edges: [Option<Transform>; 12] = [None; 12];
    let mut pane = None;
    if let Some(o) = outline {
        let c = o.corners;
        let down = |v: Vec3| Vec3::new(v.x, o.foot, v.z);
        for k in 0..4 {
            edges[k] = Some(edge(c[k], c[(k + 1) % 4], OUTLINE));
            if o.wall() {
                edges[4 + k] = Some(edge(c[k], down(c[k]), OUTLINE));
                edges[8 + k] = Some(edge(down(c[k]), down(c[(k + 1) % 4]), OUTLINE));
            }
        }
        if o.wall() {
            let (low, high) = (c.iter().fold(Vec3::MAX, |m, v| m.min(*v)), c.iter().fold(Vec3::MIN, |m, v| m.max(*v)));
            let (low, high) = (Vec3::new(low.x, o.foot, low.z), Vec3::new(high.x, high.y, high.z));
            pane = Some(Transform::from_translation((low + high) / 2.).with_scale((high - low).max(Vec3::splat(1e-4))));
        }
    }
    for (e, at) in kit.edges.into_iter().zip(edges) {
        set(world, e, at);
    }
    set(world, kit.pane, pane);
    // The box round anything else's ghost.
    let corners = shown.and_then(|(s, f)| self::corners(s, f).map(|c| (c, s.tint())));
    if let Some((_, tint)) = corners.filter(|(_, t)| kit.boxing != Some(*t)) {
        for e in kit.boxed {
            world.entity_mut(e).insert(MeshMaterial3d(kit.lines[tint.index()].clone()));
        }
        kit.boxing = Some(tint);
    }
    let pairs = (0..8usize).flat_map(|k| [1usize, 2, 4].into_iter().filter(move |b| k & b == 0).map(move |b| (k, k | b)));
    for (e, (a, b)) in kit.boxed.into_iter().zip(pairs) {
        set(world, e, corners.map(|(c, _)| edge(c[a], c[b], EDGE)));
    }
}

/// After arena::tick: anything the preview still shows is hidden when this frame did not work it out (a cut scene, the
/// end of a siege or the fridge returns before it is asked).
fn tidy(world: &mut World) {
    let Some(frame) = world.get_resource::<Game>().map(|g| g.frame()) else {
        return;
    };
    let Some(mut preview) = world.get_resource_mut::<Preview>().filter(|p| p.frame != frame) else {
        return;
    };
    preview.spot = None;
    let Some(mut kit) = preview.kit.take() else {
        return;
    };
    draw(world, &mut kit, None);
    world.resource_mut::<Preview>().kit = Some(kit);
}

#[cfg(test)]
mod tests {
    use super::*;
    use kitchen_sim::map::{Map, FLOOR};
    use std::f32::consts::FRAC_PI_2;

    /// The layout of pillow-fort with the old stock the Forts tests play with (tools/network/fixtures/long-fort.json).
    fn map() -> Map {
        Map::parse(&std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tools/network/fixtures/long-fort.json")).unwrap()).unwrap()
    }
    /// That round in its build, nobody a bot. Its first loose objects: chairs 0 to 2, crates 3 to 6 (at x = -1.75),
    /// pillows 7 to 11 (at x = -1.5, from z = 2.7), napkins 12 to 14.
    fn fort() -> Feast {
        let mut f = Feast::with_map(map(), false);
        f.set_build(600.);
        for p in &mut f.players {
            p.bot = false;
        }
        f
    }
    /// Hotdog 1 standing still at [x, z], facing +x, with `prop` in its hands.
    fn hold(f: &mut Feast, prop: usize, x: f32, z: f32) {
        let p = &mut f.players[0];
        (p.p, p.yaw, p.pace, p.last, p.grounded, p.alive) = ([x, FLOOR as f32, z], -FRAC_PI_2, [0.; 2], None, true, true);
        assert!(f.props.pick_up(prop, 1));
        f.players[0].carry = prop as i32;
    }
    fn spot(f: &Feast) -> Spot {
        let placement = f.placement(1).expect("it carries something");
        let refused = placement.refused.or_else(|| placement.drape.as_ref().and_then(|c| f.refusal(c, &f.bodies)));
        Spot { placement, refused }
    }
    /// A placement of loose object kind `kind`, snapped or not, locked or not, let go on the move or not.
    fn placed(kind: usize, snapped: bool, lock: bool, moving: bool) -> Placement {
        Placement {
            prop: 0,
            kind,
            p: [0.3, FLOOR, 0.2],
            yaw: 0.5,
            q: kitchen_sim::props::yaw_rotation(0.5),
            onto: snapped.then_some(3),
            beside: None,
            snapped,
            lock,
            velocity: if moving { [0.4, 0., 0.] } else { [0.; 3] },
            drape: None,
            refused: None,
        }
    }

    #[test]
    fn the_ghost_stands_where_the_simulation_says_the_object_lands_turned_as_it_lands() {
        let mut f = fort();
        assert_eq!(f.placement(1), None, "nothing in its hands, nothing to show");
        // A pillow held over the first pillow of the old stock, and a chair in the open, from a slant; and the chair
        // turned a quarter and tipped over onto its back (it lands as it is held).
        let tipped = kitchen_sim::feast::build::Pose { turn: 2, tip: 1 };
        for (prop, x, z, yaw, pose) in [(8, -1.61, 2.7, -FRAC_PI_2, Default::default()), (0, -2.6, 2.5, -FRAC_PI_2 + 0.3, Default::default()), (0, -1.3, 2.45, -FRAC_PI_2, tipped)] {
            let mut f = fort();
            hold(&mut f, prop, x, z);
            f.players[0].yaw = yaw;
            f.set_held_pose(1, pose);
            let ghost = spot(&f).ghost();
            let s = spot(&f);
            // Its box: the object's own, turned as it is, its underside on the spot.
            let round = corners(&s, &f).unwrap();
            let low = round.iter().map(|c| c.y).fold(f32::MAX, f32::min);
            let floor = if pose == tipped { FLOOR as f32 + 0.0005 } else { s.placement.p[1] as f32 };
            assert!((low - floor).abs() < 0.003, "{round:?}");
            if pose == tipped {
                assert_eq!(s.placement.q, tipped.rotation(s.placement.yaw));
            }
            assert!(round.iter().all(|c| ghost.translation.distance(*c) < 0.3));
            assert!((ghost.translation - Vec3::new(s.placement.p[0] as f32, s.placement.p[1] as f32, s.placement.p[2] as f32)).length() < 1e-6);
            assert!(f.put(1, Vec3::X));
            let (p, q) = (f.props.position(prop), f.props.rotation(prop));
            assert!((ghost.translation - Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32)).length() < 1e-5, "{prop}: the ghost at {ghost:?}, landed at {p:?}");
            let landed = Quat::from_xyzw(q[0] as f32, q[1] as f32, q[2] as f32, q[3] as f32);
            assert!(ghost.rotation.angle_between(landed) < 1e-4, "{prop}: turned as it lands");
        }
        f.players[0].carry = -1;
        assert_eq!(f.placement(1), None);
    }

    #[test]
    fn the_tint_is_white_for_a_plain_put_cyan_for_a_snap_and_green_for_a_lock() {
        let tint = |kind, snapped, lock, moving| Spot { placement: placed(kind, snapped, lock, moving), refused: None }.tint();
        for kind in [0, 1, 2, 4, 6, 11] {
            assert_eq!(tint(kind, false, false, true), Tint::Plain, "on the move");
            assert_eq!(tint(kind, false, false, false), Tint::Plain, "standing, settling");
            assert_eq!(tint(kind, true, false, false), Tint::Snap);
            assert_eq!(tint(kind, true, true, false), Tint::Lock);
            assert_eq!(tint(kind, false, true, false), Tint::Lock);
        }
        let [white, cyan, green] = [Tint::Plain, Tint::Snap, Tint::Lock].map(Tint::colour);
        assert!(white.iter().all(|c| *c > 0.85), "white: {white:?}");
        assert!(cyan[2] > 0.9 && cyan[1] > 0.7 && cyan[0] < 0.3, "cyan: {cyan:?}");
        assert!(green[1] > 0.9 && green[0] < 0.4 && green[2] < 0.5, "green: {green:?}");
        // From the simulation: a pillow put down standing still on a pillow snaps onto it and locks, and that pillow
        // glows cyan; on the move it is a plain put.
        let mut f = fort();
        hold(&mut f, 8, -1.61, 2.7);
        let still = spot(&f);
        assert_eq!((still.placement.onto, still.tint(), still.lit()), (Some(7), Tint::Lock, vec![(7, Tint::Snap)]));
        f.players[0].pace = [0.5, 0.];
        let moving = spot(&f);
        assert_eq!((moving.tint(), moving.lit()), (Tint::Plain, vec![]));
        assert_eq!(moving.put(), Put::Moving);
        assert_eq!(still.put(), Put::Still { snaps: true, locks: true });
    }

    #[test]
    fn a_napkin_is_green_over_a_pile_with_what_it_covers_glowing_and_grey_lying_flat_or_where_its_drape_would_be_refused() {
        // Over the crates: a wall, green, outlined from the napkin's top down to the counter, every object under it
        // glowing green.
        let mut f = fort();
        hold(&mut f, 12, -2.04, 3.05);
        let over = spot(&f);
        let canopy = over.placement.drape.clone().expect("over the pile it drapes");
        assert!(canopy.props.len() >= 2 && (4..=5).all(|c| canopy.props.contains(&c)), "{:?}", canopy.props);
        assert_eq!((over.refused, over.tint(), over.put()), (None, Tint::Drape, Put::Drape(canopy.props.len())));
        assert_eq!(over.lit(), canopy.props.iter().map(|p| (*p, Tint::Drape)).collect::<Vec<_>>());
        let wall = outline(&over, &f).unwrap();
        assert!(wall.wall() && wall.tint == Tint::Drape);
        assert!(wall.corners.iter().all(|c| (c.y - canopy.top as f32).abs() < 1e-6));
        assert_eq!(wall.corners[0], Vec3::new(canopy.rect[0] as f32, canopy.top as f32, canopy.rect[1] as f32));
        assert_eq!(wall.corners[2], Vec3::new(canopy.rect[2] as f32, canopy.top as f32, canopy.rect[3] as f32));
        assert!((wall.foot - FLOOR as f32).abs() < 1e-6, "down to the counter");
        assert_eq!(Tint::Drape.colour(), Tint::Lock.colour());
        // The same drape refused once it landed (here: it would seal a base) lies flat: grey, nothing glows.
        let refused = Spot { refused: Some(Refusal::Sealed), ..over.clone() };
        assert_eq!((refused.tint(), refused.lit(), refused.put()), (Tint::Flat, vec![], Put::Flat(Some(Refusal::Sealed))));
        assert!(!outline(&refused, &f).unwrap().wall());
        // On the bare counter: it lies flat, grey, outlined where it lies.
        let mut f = fort();
        hold(&mut f, 12, -1.3, 2.45);
        let flat = spot(&f);
        assert_eq!(flat.placement.drape, None);
        assert_eq!((flat.tint(), flat.lit(), flat.put()), (Tint::Flat, vec![], Put::Flat(None)));
        let line = outline(&flat, &f).unwrap();
        assert!(!line.wall() && line.tint == Tint::Flat);
        let ghost = flat.ghost();
        let around = line.corners.map(|c| ghost.transform_point(Vec3::ZERO).distance(Vec3::new(c.x, ghost.translation.y, c.z)));
        assert!(around.iter().all(|d| (d - around[0]).abs() < 1e-4 && *d > 0.3), "its own corners: {around:?}");
        let [r, g, b] = Tint::Flat.colour();
        assert!((r - g).abs() < 0.05 && (g - b).abs() < 0.05 && r < 0.7, "grey");
        // Anything else has no outline.
        assert_eq!(outline(&Spot { placement: placed(2, true, true, false), refused: None }, &f), None);
    }

    /// Forts: held so that it would reach into its team's base (where the bacon is scored), it may not be put down: the
    /// ghost is grey where it would be, nothing glows, Q would leave it in the hands and the tooltip says why. A napkin
    /// there is refused the same way. A step out of the base and it lands.
    #[test]
    fn in_a_base_the_ghost_is_grey_and_q_is_refused() {
        let mut f = fort();
        hold(&mut f, 8, -2.75, 3.1);
        let s = spot(&f);
        assert_eq!((s.placement.refused, s.refused), (Some(Refusal::Base), Some(Refusal::Base)), "{:?}", s.placement);
        assert_eq!((s.tint(), s.lit(), s.put()), (Tint::Flat, vec![], Put::Refused(Refusal::Base)));
        assert_eq!(crate::hints::putting(s.put()).0, "can't put down in a base");
        assert!(corners(&s, &f).is_some(), "its box is drawn, grey");
        assert!(!f.put(1, Vec3::X) && f.players[0].carry == 8);
        let mut f = fort();
        hold(&mut f, 12, -2.75, 2.9);
        assert_eq!(spot(&f).put(), Put::Refused(Refusal::Base));
        let mut f = fort();
        hold(&mut f, 8, -2.4, 3.1);
        assert_eq!(spot(&f).placement.refused, None);
        assert!(f.put(1, Vec3::X));
    }

    #[test]
    fn the_pace_is_kept_as_the_simulation_keeps_it() {
        let mut pace_now = [0.; 2];
        let mut at = Vec3::new(1., FLOOR as f32, 2.);
        let mut last = None;
        for _ in 0..120 {
            pace_now = pace(pace_now, last, at, 1. / 60.);
            last = Some(at);
            at.x += 0.5 / 60.;
        }
        assert!((pace_now[0] - 0.5).abs() < 1e-3 && pace_now[1].abs() < 1e-6, "{pace_now:?}");
        // No faster than KEEP, and a jump further than any walk is no pace at all.
        assert!((pace([0.; 2], Some(Vec3::ZERO), Vec3::new(2. / 60., 0., 0.), 1. / 60.)[0] - KEEP / 6.).abs() < 1e-4);
        assert_eq!(pace([0.2, 0.], Some(Vec3::ZERO), Vec3::new(1., 0., 0.), 1. / 60.), [0.2, 0.]);
        assert_eq!(pace([0.2, 0.], None, Vec3::new(1., 0., 0.), 1. / 60.), [0.2, 0.]);
    }

    /// A round of pillow-fort in the game, as the arena has it (single player), with the loose objects' bodies.
    fn forts_app() -> App {
        let mut app = crate::arena::test_app("");
        app.init_asset::<GhostMaterial>().init_resource::<crate::hints::Hints>();
        let map = map();
        crate::loose::build(app.world_mut(), &map);
        let mut g = app.world_mut().resource_mut::<Game>();
        g.feast = Feast::with_map(map, false);
        g.begin();
        assert_eq!(g.phase_name(), "Build");
        for p in &mut g.feast.players {
            p.bot = false;
        }
        app
    }
    /// One frame of the preview (as `arena::tick` asks it), and what it shows: the ghost's place (None: hidden), the
    /// glows', and how many of the napkin outline's edges.
    fn frame(app: &mut App) -> (Option<Transform>, Vec<Transform>, usize) {
        let world = app.world_mut();
        world.resource_scope(|world, mut g: Mut<Game>| {
            g.roster = g.feast.players.clone();
            show(world, &mut g, 1. / 60.);
        });
        let shown = |world: &mut World, e: Entity| world.get::<Visibility>(e).is_some_and(|v| *v != Visibility::Hidden).then(|| *world.get::<Transform>(e).unwrap());
        let preview = world.remove_resource::<Preview>().unwrap();
        let out = match &preview.kit {
            None => (None, vec![], 0),
            Some(kit) => (shown(world, kit.ghost.root), kit.glows.iter().filter_map(|s| shown(world, s.root)).collect(), kit.edges.iter().filter(|e| shown(world, **e).is_some()).count()),
        };
        world.insert_resource(preview);
        out
    }
    fn carry(app: &mut App, prop: usize, x: f32, z: f32) {
        let mut g = app.world_mut().resource_mut::<Game>();
        hold(&mut g.feast, prop, x, z);
    }

    #[test]
    fn the_ghost_shows_while_carrying_with_h_too_and_goes_while_dead_or_at_the_card_table_and_is_made_once() {
        let mut app = forts_app();
        assert_eq!(frame(&mut app), (None, vec![], 0), "nothing carried, nothing shown");
        carry(&mut app, 8, -1.61, 2.7);
        let (ghost, glows, edges) = frame(&mut app);
        let g = app.world().resource::<Game>();
        let spot = app.world().resource::<Preview>().spot(g.frame()).cloned().expect("the preview of this frame");
        assert_eq!(ghost, Some(spot.ghost()), "where it lands");
        assert_eq!((spot.tint(), glows.len(), edges), (Tint::Lock, 1, 0), "it locks onto pillow 7, which glows");
        // Its box: twelve edges round where it lands, green.
        let kit = app.world().resource::<Preview>().kit.as_ref().unwrap();
        let round = corners(&spot, &g.feast).unwrap();
        assert!(kit.boxed.iter().all(|e| app.world().get::<Visibility>(*e) == Some(&Visibility::Inherited)));
        assert!(kit.boxed.iter().all(|e| app.world().get::<MeshMaterial3d<GhostMaterial>>(*e).is_some_and(|m| m.0 == kit.lines[Tint::Lock.index()])));
        let ends: Vec<Vec3> = kit.boxed.iter().flat_map(|e| {
            let t = app.world().get::<Transform>(*e).unwrap();
            [t.transform_point(Vec3::new(-0.5, 0., 0.)), t.transform_point(Vec3::new(0.5, 0., 0.))]
        }).collect();
        assert!(round.iter().all(|c| ends.iter().any(|e| e.distance(*c) < EDGE)), "every corner of the box ends three of its edges");
        let under = g.feast.props.position(7);
        assert!((glows[0].translation - Vec3::new(under[0] as f32, under[1] as f32, under[2] as f32)).length() < 0.01);
        // Its parts wear the pillow's meshes, in the green of a lock.
        let kit = app.world().resource::<Preview>().kit.as_ref().unwrap();
        let green = kit.ghosts[Tint::Lock.index()].clone();
        assert!(kit.ghost.parts.iter().all(|p| app.world().get::<MeshMaterial3d<GhostMaterial>>(*p).is_some_and(|m| m.0 == green)));
        assert!(kit.ghost.parts.iter().all(|p| app.world().get::<RenderLayers>(*p) == Some(&RenderLayers::layer(VIEW_LAYER))));
        let made = app.world().resource::<Preview>().spawned;
        // H hides the hints, not the ghost.
        app.world_mut().resource_mut::<crate::hints::Hints>().on = false;
        assert_eq!(frame(&mut app).0, ghost);
        // Dead: gone, and back with the next life.
        let me = |app: &mut App, alive: bool| app.world_mut().resource_mut::<Game>().feast.players[0].alive = alive;
        me(&mut app, false);
        assert_eq!(frame(&mut app), (None, vec![], 0), "dead");
        me(&mut app, true);
        assert_eq!(frame(&mut app).0, ghost);
        // At the card table: gone.
        app.world_mut().resource_mut::<Game>().feast.rules.cards = true;
        assert_eq!(frame(&mut app), (None, vec![], 0), "at the card table");
        app.world_mut().resource_mut::<Game>().feast.rules.cards = false;
        // A chair instead: the ghost changes its meshes, and nothing more is made than it needs.
        {
            let mut g = app.world_mut().resource_mut::<Game>();
            g.feast.players[0].carry = -1;
            g.feast.props.drop_prop(8, [0.; 3], [0.; 3]);
        }
        carry(&mut app, 0, -2.6, 2.5);
        let chair = frame(&mut app).0.expect("the chair's ghost");
        let parts = crate::loose::meshes(app.world(), 4).len();
        let kit = app.world().resource::<Preview>().kit.as_ref().unwrap();
        assert_eq!(kit.ghost.kind, Some(4));
        assert_eq!(kit.ghost.parts.iter().filter(|p| app.world().get::<Visibility>(**p) == Some(&Visibility::Inherited)).count(), parts);
        let made_chair = app.world().resource::<Preview>().spawned;
        for _ in 0..5 {
            assert_eq!(frame(&mut app).0, Some(chair));
        }
        assert_eq!(app.world().resource::<Preview>().spawned, made_chair, "every frame after the first reuses what is there");
        assert!(made_chair <= made + parts, "{made} then {made_chair}");
    }

    #[test]
    fn a_napkins_drape_is_outlined_its_covered_objects_glow_and_its_refusal_is_asked_once() {
        let mut app = forts_app();
        carry(&mut app, 12, -2.04, 3.05);
        let (ghost, glows, edges) = frame(&mut app);
        let g = app.world().resource::<Game>();
        let spot = app.world().resource::<Preview>().spot(g.frame()).cloned().unwrap();
        let covered = spot.wall().expect("a wall").props.clone();
        assert_eq!((ghost, glows.len(), edges), (Some(spot.ghost()), covered.len(), 12), "the wall's twelve edges");
        let kit = app.world().resource::<Preview>().kit.as_ref().unwrap();
        assert!(app.world().get::<Visibility>(kit.pane).is_some_and(|v| *v != Visibility::Hidden), "and its pane");
        assert_eq!(app.world().resource::<Preview>().refusals, 1);
        for _ in 0..3 {
            frame(&mut app);
        }
        assert_eq!(app.world().resource::<Preview>().refusals, 1, "the same drape is not asked about again");
        // On the bare counter: flat, the outline its own four edges, nothing glowing.
        {
            let mut g = app.world_mut().resource_mut::<Game>();
            let p = &mut g.feast.players[0];
            p.p = [-1.3, FLOOR as f32, 2.45];
        }
        let (ghost, glows, edges) = frame(&mut app);
        assert_eq!((ghost.is_some(), glows.len(), edges), (true, 0, 4));
        let kit = app.world().resource::<Preview>().kit.as_ref().unwrap();
        assert!(kit.boxed.iter().all(|e| app.world().get::<Visibility>(*e) == Some(&Visibility::Hidden)), "a napkin has its outline, not a box");
        assert!(app.world().get::<Visibility>(kit.pane) == Some(&Visibility::Hidden));
        assert!(app.world().get::<MeshMaterial3d<GhostMaterial>>(kit.edges[0]).is_some_and(|m| m.0 == kit.lines[Tint::Flat.index()]), "grey");
    }
}
