// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
// The loose objects' shapes (a map's props of the kinds in map.mjs's LOOSE), as the simulation builds them: crates/kitchen-sim/src/props.rs
// (`ShapeSpec::named`) has the same parts, and the game draws them from them (src/loose.rs). The map editor draws its loose objects from these
// (editor/solid.js), and map.test.mjs measures their outlines; fixtures/presets-golden.json holds them, for map.test.mjs and props.rs's tests to keep
// the two in step. Lengths in metres, masses in kilograms; each shape stands on its origin (the middle of its underside).
//   box   {type:'box', center, half, radius, mass}: a rounded box, `half` its half-sizes, `radius` the rounding
//   lathe {type:'lathe', center, profile, radius, mass}: a shell turned round the vertical axis, `profile` its [radius, height] outline
export const PRESETS = {
  /** The big silver bowl: 32 cm across the rim, 13 cm across the base, 11 cm deep. */
  bowl: () => ({name: 'bowl', friction: 0.25, restitution: 0.3, linearDamping: 0.05, angularDamping: 0.4,
    parts: [{type: 'lathe', center: [0, 0.004, 0], profile: [[0, 0], [0.065, 0], [0.125, 0.045], [0.155, 0.105]], radius: 0.004, mass: 0.35}]}),
  crate: (size = 0.1, mass = 0.3) => ({name: 'crate', friction: 0.5, restitution: 0.1, linearDamping: 0.05, angularDamping: 0.1,
    parts: [{type: 'box', center: [0, size / 2, 0], half: [size / 2, size / 2, size / 2], radius: 0.004, mass}]}),
  /** A pillow, 14 cm by 10 cm and 4 cm thick: flat and nearly square-edged, so that pillows pile up. */
  pillow: () => ({name: 'pillow', friction: 1, restitution: 0, linearDamping: 0.6, angularDamping: 3,
    parts: [{type: 'box', center: [0, 0.02, 0], half: [0.07, 0.02, 0.05], radius: 0.008, mass: 0.04}]}),
  /** A cloth napkin, or a tea towel: 45 cm square and 90 g, a thin flat pad here (the game draws it as cloth). Over other props the game drapes it (drape.rs). */
  napkin: () => ({name: 'napkin', friction: 0.9, restitution: 0, linearDamping: 0.3, angularDamping: 1.5,
    parts: [{type: 'box', center: [0, 0.004, 0], half: [0.225, 0.004, 0.225], radius: 0.003, mass: 0.09}]}),
  chair: () => ({name: 'chair', friction: 0.5, restitution: 0.1, linearDamping: 0.05, angularDamping: 0.1,
    parts: [
      {type: 'box', center: [0, 0.066, 0], half: [0.045, 0.006, 0.045], radius: 0.004, mass: 0.1},
      {type: 'box', center: [0, 0.115, -0.039], half: [0.045, 0.045, 0.006], radius: 0.004, mass: 0.06},
      {type: 'box', center: [0.039, 0.031, 0.039], half: [0.006, 0.031, 0.006], radius: 0.004, mass: 0.0225},
      {type: 'box', center: [-0.039, 0.031, 0.039], half: [0.006, 0.031, 0.006], radius: 0.004, mass: 0.0225},
      {type: 'box', center: [0.039, 0.031, -0.039], half: [0.006, 0.031, 0.006], radius: 0.004, mass: 0.0225},
      {type: 'box', center: [-0.039, 0.031, -0.039], half: [0.006, 0.031, 0.006], radius: 0.004, mass: 0.0225},
    ]}),
  /** A rectangular baking tray: a base and four low walls, open on top. */
  tray: () => ({name: 'tray', friction: 0.25, restitution: 0.25, linearDamping: 0.05, angularDamping: 0.3,
    parts: [
      {type: 'box', center: [0, 0.004, 0], half: [0.15, 0.004, 0.1], radius: 0.003, mass: 0.3},
      {type: 'box', center: [0.146, 0.026, 0], half: [0.004, 0.022, 0.1], radius: 0.003, mass: 0.05},
      {type: 'box', center: [-0.146, 0.026, 0], half: [0.004, 0.022, 0.1], radius: 0.003, mass: 0.05},
      {type: 'box', center: [0, 0.026, 0.096], half: [0.142, 0.022, 0.004], radius: 0.003, mass: 0.05},
      {type: 'box', center: [0, 0.026, -0.096], half: [0.142, 0.022, 0.004], radius: 0.003, mass: 0.05},
    ]}),
  /** A family-size box of breakfast cereal, 23 cm wide, 8 cm deep and 33 cm tall, 650 g full. */
  cereal: () => ({name: 'cereal', friction: 0.6, restitution: 0.05, linearDamping: 0.1, angularDamping: 0.4,
    parts: [{type: 'box', center: [0, 0.165, 0], half: [0.115, 0.165, 0.04], radius: 0.003, mass: 0.65}]}),
  /** A china mug, 10 cm tall and 8.4 cm across, its handle on its +x side, 350 g. */
  cup: () => ({name: 'cup', friction: 0.5, restitution: 0.15, linearDamping: 0.2, angularDamping: 0.6,
    parts: [
      {type: 'lathe', center: [0, 0.002, 0], profile: [[0, 0], [0.04, 0], [0.04, 0.096], [0, 0.096]], radius: 0.002, mass: 0.32},
      {type: 'box', center: [0.054, 0.052, 0], half: [0.013, 0.03, 0.005], radius: 0.004, mass: 0.03},
    ]}),
  /** A stoneware dinner plate, 26 cm across and 2 cm deep, 550 g. */
  plate: () => ({name: 'plate', friction: 0.4, restitution: 0.15, linearDamping: 0.1, angularDamping: 0.4,
    parts: [{type: 'lathe', center: [0, 0.003, 0], profile: [[0, 0], [0.127, 0], [0.127, 0.014], [0, 0.014]], radius: 0.003, mass: 0.55}]}),
  /** A tin of beans, 7.5 cm across and 11 cm tall, 450 g. */
  tin: () => ({name: 'tin', friction: 0.4, restitution: 0.15, linearDamping: 0.1, angularDamping: 0.3,
    parts: [{type: 'lathe', center: [0, 0.002, 0], profile: [[0, 0], [0.0355, 0], [0.0355, 0.106], [0, 0.106]], radius: 0.002, mass: 0.45}]}),
  /** A wooden cutting board, 45 cm by 30 cm and 2.5 cm thick, 2.2 kg. */
  board: () => ({name: 'board', friction: 0.6, restitution: 0.1, linearDamping: 0.05, angularDamping: 0.2,
    parts: [{type: 'box', center: [0, 0.0125, 0], half: [0.225, 0.0125, 0.15], radius: 0.003, mass: 2.2}]}),
  /** A china cereal bowl, 15 cm across the rim and 6.5 cm deep, 350 g. */
  dish: () => ({name: 'dish', friction: 0.35, restitution: 0.15, linearDamping: 0.1, angularDamping: 0.4,
    parts: [{type: 'lathe', center: [0, 0.0035, 0], profile: [[0, 0], [0.038, 0], [0.064, 0.028], [0.0715, 0.058]], radius: 0.0035, mass: 0.35}]}),
  /** A roll of paper towels, 28 cm tall and 12 cm across, 180 g. */
  roll: () => ({name: 'roll', friction: 0.7, restitution: 0.05, linearDamping: 0.2, angularDamping: 0.6,
    parts: [{type: 'lathe', center: [0, 0.002, 0], profile: [[0, 0], [0.058, 0], [0.058, 0.276], [0, 0.276]], radius: 0.002, mass: 0.18}]}),
  /** A glass salt shaker with a steel cap, 9 cm tall and 4.5 cm across, 120 g. */
  salt: () => shaker('salt'),
  /** The pepper shaker beside it, the same glass and cap. */
  pepper: () => shaker('pepper'),
  /** An enamel bread bin with a roll top, 40 cm wide, 25 cm deep and 20 cm tall, 1.8 kg. */
  breadbox: () => ({name: 'breadbox', friction: 0.55, restitution: 0.1, linearDamping: 0.05, angularDamping: 0.2,
    parts: [{type: 'box', center: [0, 0.1, 0], half: [0.2, 0.1, 0.125], radius: 0.012, mass: 1.8}]}),
};
/** A glass shaker with a steel cap (salt or pepper), 9 cm tall and 4.5 cm across, 120 g. */
function shaker(name){
  return {name, friction: 0.4, restitution: 0.2, linearDamping: 0.1, angularDamping: 0.3,
    parts: [{type: 'lathe', center: [0, 0.0012, 0], profile: [[0, 0], [0.021, 0], [0.0215, 0.064], [0.0195, 0.07], [0.0185, 0.0876], [0, 0.0876]], radius: 0.0012, mass: 0.12}]};
}
