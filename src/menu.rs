// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! The in-game menu: Resume, Settings, Controls and leaving, over the game.
//!
//! * **Opening it.** Natively Esc opens it, anywhere: in a round, at the card table, dead, in the fridge between
//!   rounds online. In a browser Esc is the browser's own way out of the pointer lock, and the page never hears it,
//!   so the rule there is: **the menu opens when the pointer lock ends while the page still has the keyboard**. That is
//!   Esc (or the browser's other way out, such as its own shortcut), checked a moment later ([`SETTLE`]) so that a loss
//!   to another window or tab (the page loses the focus), to the page's chat box (it takes the keyboard), or because the
//!   game let go (a round over) does not open it; and Esc heard with the pointer already free opens it too. At the
//!   card table, where Esc frees the pointer to click the scoreboard, the first Esc (or the loss of the lock) only
//!   frees it, as it always has, and an Esc with the pointer free opens the menu. Esc in the menu goes back a page,
//!   cancels a rebinding, or closes it.
//! * **While it is open** the game hears nothing (controls.rs, the gate): no walking, no trigger, no look. Single
//!   player is paused (`arena::tick` stands still: the round's clocks, the simulation, the bots, the cats); online the
//!   round goes on underneath, and the hotdog stands where it is. Its pointer is free.
//! * **Resume** is a click (a browser takes the pointer lock only for one) and captures the pointer again; the click
//!   is not heard as a shot. Natively Esc also resumes, and captures it again; in a browser Esc closes it and the next
//!   click on the game captures the pointer, as at the start.
//! * **Settings** (settings.rs): mouse sensitivity, invert Y, the field of view, the volume, the hints (hints.rs), the
//!   damage indicators and the kill slow motion (damage.rs, slowmo.rs), the graphics quality (quality.rs), and in a
//!   browser the resolution and the renderer (a build of its own: a change reloads the page, and the page says so).
//!   Each applies at once and is kept for next time.
//! * **Controls** (controls.rs): every action in four tabs (movement, combat, weapons, carrying) with two inputs each.
//!   A click on one waits for a key, a mouse button or a roll of the wheel (Esc cancels); one in use for something heard
//!   at the same time says so and offers a swap; a right click clears one; each action can be reset, and everything
//!   at once. The wheel can be turned round.
//! * **Leaving**: natively QUIT GAME; in a browser online LEAVE MATCH, which reloads the page into the lobby (the
//!   fridge, until the next round), and offline RESTART, a new game of the same mode (the page's links choose others).
//!   Each asks once more first.
//! * **The page's links** (Play, the five-player demo, Licences and source: index.html's `.modes`) are the start page's:
//!   while the game (a round, the demo, a replay, a cut scene) is on the screen they would sit over its display, so the
//!   page hides them, and shows them again while this menu is open ([`tell_page`]: `<html data-screen>`). The menu has
//!   the licences of its own as well (THIRD-PARTY LICENCES).
//!
//! The look follows the head-up display (hud.rs): a dark translucent panel, white text, the mustard accent, the same
//! icons (icons.rs), sized from the window's height (720 pixels is 1x) and rebuilt when it changes. ASCII only.
use crate::controls::{Action, Bindings, Block, Conflict, Gate, Group, Input, Wheel};
use crate::icons::{Icon, Icons};
use crate::settings::Settings;
use bevy::input::mouse::{AccumulatedMouseScroll, MouseScrollUnit};
use bevy::prelude::*;
use bevy::ui::{FocusPolicy, RelativeCursorPosition};
use bevy::window::{CursorGrabMode, CursorOptions};

pub struct Plugin;
impl bevy::app::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((crate::controls::Plugin, crate::settings::Plugin))
            .init_resource::<Menu>()
            .init_resource::<Icons>()
            .init_resource::<View>()
            .add_systems(
                PreUpdate,
                run.after(bevy::input::InputSystems).after(bevy::ui::UiSystems::Focus).after(crate::touch::press).before(crate::controls::Shut),
            )
            .add_systems(Update, (draw.after(crate::arena::tick), tell_page));
    }
}

/// Seconds after the pointer lock is lost before the menu decides to open: long enough for the focus to have gone
/// where it was going (another window, the chat box).
pub const SETTLE: f32 = 0.12;
/// Seconds a lock asked for may take to come before the browser is taken to have refused it.
const PATIENCE: f32 = 1.;
/// Seconds after the menu opened on a lost lock in which an Esc is not heard (some browsers send the Esc that ended
/// the lock as well).
const DEAF: f32 = 0.35;
/// Seconds an "are you sure" waits.
const CONFIRM: f32 = 4.;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Page {
    #[default]
    Main,
    Settings,
    Controls,
}
/// What asks once more first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Confirm {
    Leave,
    ResetAll,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Toggle {
    InvertY,
    Hints,
    InvertWheel,
    /// The arcs round the crosshair that show where a hit came from (damage.rs).
    DamageIndicators,
    /// Slow motion on a killing shot (slowmo.rs).
    SlowMotion,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Slider {
    Sensitivity,
    Fov,
    Volume,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Pick {
    Quality,
    Resolution,
    Renderer,
    /// Whether the right button holds the scope up or toggles it (settings.rs `scope_toggle`).
    Scope,
}
/// What a click on a node does.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub enum Hit {
    Resume,
    Open(Page),
    Back,
    Leave,
    Tab(Group),
    Bind(Action, usize),
    Reset(Action),
    ResetAll,
    Toggle(Toggle),
    Step(Pick, i32),
    Slider(Slider),
    Swap,
    Cancel,
    Reload,
    /// The replays list (replay/list.rs), over the menu.
    Replays,
    /// The licences page, in a new tab (a browser only: natively the menu says where the file is).
    Licences,
}

/// The pointer lock as the menu follows it in a browser.
#[derive(Clone, Copy, Debug, Default)]
struct Lock {
    /// The game's grab last frame, and whether the page has had the lock since it was asked for, and when that was.
    grabbed: bool,
    seen: bool,
    asked: f32,
    /// When it was lost while the game had it (the menu may open SETTLE later).
    lost: Option<f32>,
}

/// The renderer as the page's graphics menu has it (a browser only, lobby.js).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Renderer {
    pub chosen: String,
    pub running: String,
    pub locked: String,
    /// value, name, disabled.
    pub items: Vec<(String, String, bool)>,
    /// What was chosen when the menu read it: a different one needs a reload.
    pub loaded: String,
}

#[derive(Resource, Default)]
pub struct Menu {
    pub open: bool,
    pub page: Page,
    pub tab: Option<Group>,
    /// A rebinding waits for an input for this action and slot; `armed` once the click that began it has passed.
    pub capture: Option<(Action, usize)>,
    armed: bool,
    /// A rebinding onto an input in use: the action, the slot, the input and who has it.
    pub clash: Option<(Action, usize, Input, Conflict)>,
    /// An "are you sure" and until when it waits.
    pub confirm: Option<(Confirm, f32)>,
    drag: Option<Slider>,
    /// Single player, open: the round stands still.
    pub paused: bool,
    lock: Lock,
    /// Esc is not heard until then.
    deaf: f32,
    /// After a click closed it: the mouse is kept from the game until its buttons are up.
    hold_mouse: bool,
    pub renderer: Option<Renderer>,
}
impl Menu {
    pub fn tab(&self) -> Group {
        self.tab.unwrap_or(Group::Movement)
    }
    /// Begins a rebinding of `action`'s `slot`.
    pub fn begin(&mut self, action: Action, slot: usize) {
        (self.capture, self.armed, self.clash) = (Some((action, slot)), false, None);
    }
    /// What a rebinding heard: given to the action, or, when another has it, held as a clash to settle.
    pub fn offer(&mut self, bindings: &mut Bindings, input: Input) {
        let Some((action, slot)) = self.capture.take() else { return };
        if !input.bindable() {
            self.capture = Some((action, slot));
            return;
        }
        if let Err(conflict) = bindings.assign(action, slot, input) {
            self.clash = Some((action, slot, input, conflict));
        }
    }
    /// The clash settled: swapped, or left as it was.
    pub fn settle(&mut self, bindings: &mut Bindings, swap: bool) {
        if let Some((action, slot, input, _)) = self.clash.take().filter(|_| swap) {
            bindings.swap(action, slot, input);
        }
    }
    /// Esc: the innermost thing open is closed. Returns whether the menu itself closes.
    pub fn escape(&mut self) -> bool {
        if self.capture.is_some() {
            self.capture = None;
        } else if self.clash.is_some() {
            self.clash = None;
        } else if self.confirm.is_some() {
            self.confirm = None;
        } else if self.page != Page::Main {
            self.page = Page::Main;
        } else {
            return true;
        }
        false
    }
    fn reset(&mut self) {
        (self.page, self.capture, self.clash, self.confirm, self.drag) = (Page::Main, None, None, None, None);
    }
}

/// Whether single player stands still for the menu (`arena::tick`).
pub fn paused(world: &World) -> bool {
    world.get_resource::<Menu>().is_some_and(|m| m.open && m.paused)
}

/// What an input heard while a rebinding waits is: Esc cancels; otherwise the first key, button or roll of the wheel
/// pressed this frame. (Rolled up is away from the player: a positive `y`.)
pub fn heard(keys: &ButtonInput<KeyCode>, mouse: &ButtonInput<MouseButton>, scroll: &AccumulatedMouseScroll) -> Option<Option<Input>> {
    if keys.just_pressed(KeyCode::Escape) {
        return Some(None);
    }
    if let Some(key) = keys.get_just_pressed().map(|k| crate::controls::plain(*k)).find(|k| Input::Key(*k).bindable()) {
        return Some(Some(Input::Key(key)));
    }
    if let Some(button) = mouse.get_just_pressed().next() {
        return Some(Some(Input::Mouse(*button)));
    }
    let enough = if scroll.unit == MouseScrollUnit::Pixel { 8. } else { 0.2 };
    if scroll.delta.y.abs() >= enough {
        return Some(Some(Input::Wheel(if scroll.delta.y > 0. { Wheel::Up } else { Wheel::Down })));
    }
    None
}

// ----- Every frame, before the game: opening, closing, clicks, rebinding, the gate.

/// What the menu needs to know of the game.
struct Situation {
    online: bool,
    table: bool,
    /// Whether the pointer is to be captured on resuming (not in the fridge, between rounds or in a cut scene).
    capture: bool,
}
fn situation(world: &World) -> Situation {
    let connect = world.get_resource::<crate::runtime::Options>().is_some_and(|o| !o.connect.is_empty());
    let holding = world.get_resource::<crate::lobby::Lobby>().is_some_and(|l| l.holding);
    match world.get_resource::<crate::arena::Game>() {
        Some(g) => Situation {
            online: g.network || connect,
            table: crate::cards::at_table(g),
            capture: !holding && g.playing() && g.phase_name() != "RoundOver" && !g.demo,
        },
        None => Situation { online: connect, table: false, capture: false },
    }
}

fn set_grab(world: &mut World, grab: bool) {
    // (A headset has no pointer to hold, and Android says so every time it is asked.)
    if cfg!(target_os = "android") {
        return;
    }
    for mut cursor in world.query::<&mut CursorOptions>().iter_mut(world) {
        cursor.grab_mode = if grab { CursorGrabMode::Locked } else { CursorGrabMode::None };
        cursor.visible = !grab;
    }
}
fn grabbed(world: &mut World) -> bool {
    world.query::<&CursorOptions>().iter(world).any(|c| c.grab_mode == CursorGrabMode::Locked)
}

fn open(world: &mut World, menu: &mut Menu, now: f32) {
    menu.reset();
    menu.open = true;
    menu.lock.lost = None;
    menu.renderer = read_renderer();
    set_grab(world, false);
    let _ = now;
}
/// Closes it; `grab` captures the pointer again.
fn close(world: &mut World, menu: &mut Menu, grab: bool) {
    menu.reset();
    menu.open = false;
    if grab {
        set_grab(world, true);
        menu.lock = Lock { grabbed: true, seen: false, asked: world.get_resource::<Time<Real>>().map_or(0., |t| t.elapsed_secs()), lost: None };
    }
}

/// PreUpdate, after the input and the UI's interactions are read and before the game's systems run.
pub fn run(world: &mut World) {
    let now = world.get_resource::<Time<Real>>().map_or(0., |t| t.elapsed_secs());
    let keys = world.get_resource::<ButtonInput<KeyCode>>().cloned().unwrap_or_default();
    let mouse = world.get_resource::<ButtonInput<MouseButton>>().cloned().unwrap_or_default();
    let scroll = world.get_resource::<AccumulatedMouseScroll>().copied().unwrap_or_default();
    let Some(mut menu) = world.remove_resource::<Menu>() else { return };
    let was_open = menu.open;
    let at = situation(world);
    pointer(world, &mut menu, now, &at);
    // Esc: a rebinding's is its own (it cancels), handled with what it hears below.
    let escape = keys.just_pressed(KeyCode::Escape) && now >= menu.deaf && !(menu.capture.is_some() && menu.armed);
    if escape {
        if !menu.open {
            // At the card table a captured pointer is freed first (cards.rs), as it always has been.
            if at.table && grabbed(world) {
                set_grab(world, false);
            } else {
                open(world, &mut menu, now);
            }
        } else if menu.escape() {
            // Natively Esc resumes and captures again; a browser takes the lock only for a click.
            close(world, &mut menu, at.capture && !cfg!(target_arch = "wasm32"));
        }
    }
    if menu.open && !escape {
        if let Some(confirm) = menu.confirm.filter(|(_, until)| now > *until) {
            let _ = confirm;
            menu.confirm = None;
        }
        // A rebinding hears the input before anything else does.
        if menu.capture.is_some() && menu.armed {
            if let Some(input) = heard(&keys, &mouse, &scroll) {
                world.resource_scope(|_, mut bindings: Mut<Bindings>| match input {
                    Some(input) => menu.offer(&mut bindings, input),
                    None => menu.capture = None,
                });
            }
        } else {
            clicks(world, &mut menu, &mouse, now, &at);
        }
        menu.armed = menu.capture.is_some();
        // A slider follows the pointer while its button is held.
        if let Some(slider) = menu.drag {
            if mouse.pressed(MouseButton::Left) {
                let x = world.query::<(&Hit, &RelativeCursorPosition)>().iter(world).find(|(hit, _)| **hit == Hit::Slider(slider)).and_then(|(_, p)| p.normalized).map(|p| p.x + 0.5);
                if let Some(x) = x {
                    slide(world, slider, x.clamp(0., 1.));
                }
            } else {
                menu.drag = None;
            }
        }
    }
    menu.paused = menu.open && !at.online;
    // The gate: shut while open, and on the frame it closes; the mouse after a click closed it, until it is let go.
    let closed = was_open && !menu.open;
    if closed && mouse.get_pressed().len() > 0 {
        menu.hold_mouse = true;
    }
    if menu.hold_mouse && mouse.get_pressed().len() == 0 && !closed {
        menu.hold_mouse = false;
    }
    let block = if menu.open || closed { Block::All } else if menu.hold_mouse { Block::Mouse } else { Block::Nothing };
    if let Some(mut gate) = world.get_resource_mut::<Gate>() {
        gate.block = block;
    }
    world.insert_resource(menu);
}

/// The browser's pointer lock (`pointer`).
fn pointer(world: &mut World, menu: &mut Menu, now: f32, at: &Situation) {
    let grab = grabbed(world);
    let really = web::locked(grab);
    let (release, open_it) = follow(&mut menu.lock, grab, really, web::has_keyboard, menu.open, at.table, now);
    if release {
        set_grab(world, false);
    }
    if open_it {
        open(world, menu, now);
        menu.deaf = now + DEAF;
    }
}
/// The rule for a browser, from the game's grab (`grab`), whether the page really has the pointer lock (`really`),
/// whether it has the keyboard (asked only when it matters), whether the menu is `open`, whether this is the card
/// table, and the time. Returns whether to let the grab go and whether to open the menu.
///
/// A lock the page lost while the game had it lets the grab go (so that the next click asks for it again), and
/// opens the menu SETTLE later if by then the page still has the keyboard: not if it went to another window or tab
/// (the page lost the focus) or the chat box (it took the keyboard), nor at the card table, where losing the lock only
/// frees the pointer. A lock asked for that never comes within PATIENCE (the browser refused it: too soon after an
/// Esc, say) lets the grab go too, and opens nothing. Natively `really` is the grab, and nothing is ever lost.
fn follow(lock: &mut Lock, grab: bool, really: bool, keyboard: impl Fn() -> bool, open: bool, table: bool, now: f32) -> (bool, bool) {
    let mut release = false;
    if grab && !lock.grabbed {
        (lock.asked, lock.seen) = (now, false);
    }
    if grab && really {
        lock.seen = true;
    } else if grab && (lock.seen || now - lock.asked > PATIENCE) {
        let lost = lock.seen;
        release = true;
        lock.seen = false;
        if lost && !open && !table {
            lock.lost = Some(now);
        }
    }
    lock.grabbed = grab && !release;
    let mut open_it = false;
    if let Some(when) = lock.lost {
        if open || lock.grabbed {
            lock.lost = None;
        } else if now - when >= SETTLE {
            lock.lost = None;
            open_it = keyboard();
        }
    }
    (release, open_it)
}

/// A click on the menu (the left button: what Bevy's UI found pressed this frame), or a right click on a binding.
fn clicks(world: &mut World, menu: &mut Menu, mouse: &ButtonInput<MouseButton>, now: f32, at: &Situation) {
    let pressed: Option<Hit> = if mouse.just_pressed(MouseButton::Left) {
        world.query::<(&Hit, &Interaction)>().iter(world).find(|(_, i)| **i == Interaction::Pressed).map(|(h, _)| *h)
    } else {
        None
    };
    if mouse.just_pressed(MouseButton::Right) && menu.clash.is_none() {
        let over = world.query::<(&Hit, &Interaction)>().iter(world).find(|(h, i)| matches!(h, Hit::Bind(..)) && **i != Interaction::None).map(|(h, _)| *h);
        if let Some(Hit::Bind(action, slot)) = over {
            world.resource_mut::<Bindings>().clear(action, slot);
        }
    }
    let Some(hit) = pressed else { return };
    if !matches!(hit, Hit::Leave | Hit::ResetAll) {
        menu.confirm = None;
    }
    match hit {
        Hit::Resume => close(world, menu, at.capture),
        Hit::Open(page) => menu.page = page,
        Hit::Back => menu.page = Page::Main,
        Hit::Leave => {
            if menu.confirm.is_some_and(|(c, _)| c == Confirm::Leave) {
                leave(world, at.online);
            } else {
                menu.confirm = Some((Confirm::Leave, now + CONFIRM));
            }
        }
        Hit::Tab(group) => menu.tab = Some(group),
        Hit::Bind(action, slot) => menu.begin(action, slot),
        Hit::Reset(action) => world.resource_mut::<Bindings>().reset(action),
        Hit::ResetAll => {
            if menu.confirm.is_some_and(|(c, _)| c == Confirm::ResetAll) {
                world.resource_mut::<Bindings>().reset_all();
                menu.confirm = None;
            } else {
                menu.confirm = Some((Confirm::ResetAll, now + CONFIRM));
            }
        }
        Hit::Toggle(which) => toggle(world, which),
        Hit::Step(pick, by) => step(world, menu, pick, by),
        Hit::Slider(slider) => menu.drag = Some(slider),
        Hit::Swap => world.resource_scope(|_, mut b: Mut<Bindings>| menu.settle(&mut b, true)),
        Hit::Cancel => {
            menu.settle(&mut Bindings::default(), false);
            menu.capture = None;
        }
        Hit::Reload => web::reload(),
        // (Over the menu, which stays open beneath it: single player stays paused, and the game hears nothing.)
        Hit::Replays => crate::replay::list::open(world),
        Hit::Licences => web::licences(),
    }
}

/// A switch on the settings page flipped (each is kept for next time: settings.rs, hints.rs, controls.rs).
pub(crate) fn toggle(world: &mut World, which: Toggle) {
    match which {
        Toggle::InvertY => {
            let mut s = world.resource_mut::<Settings>();
            s.invert_y = !s.invert_y;
        }
        Toggle::Hints => {
            if let Some(mut hints) = world.get_resource_mut::<crate::hints::Hints>() {
                hints.toggle();
            }
        }
        Toggle::InvertWheel => {
            let mut b = world.resource_mut::<Bindings>();
            b.invert_wheel = !b.invert_wheel;
        }
        Toggle::DamageIndicators => {
            let mut s = world.resource_mut::<Settings>();
            s.damage_indicators = !s.damage_indicators;
        }
        Toggle::SlowMotion => {
            let mut s = world.resource_mut::<Settings>();
            s.kill_slowmo = !s.kill_slowmo;
        }
    }
}

/// The scope's chooser stepped: from holding it up to toggling it, or back (settings.rs `scope_toggle`).
pub(crate) fn scope(world: &mut World) {
    if let Some(mut s) = world.get_resource_mut::<Settings>() {
        s.scope_toggle = !s.scope_toggle;
    }
}

/// A slider set to `x` of its way along.
fn slide(world: &mut World, slider: Slider, x: f32) {
    let mut s = world.resource_mut::<Settings>();
    let (lo, hi, snap) = range(slider);
    let v = ((lo + (hi - lo) * x) / snap).round() * snap;
    match slider {
        Slider::Sensitivity => s.sensitivity = v,
        Slider::Fov => s.fov = v,
        Slider::Volume => s.volume = v,
    }
}
fn range(slider: Slider) -> (f32, f32, f32) {
    match slider {
        Slider::Sensitivity => (crate::settings::SENSITIVITY.0, crate::settings::SENSITIVITY.1, 0.05),
        Slider::Fov => (crate::settings::FOV.0, crate::settings::FOV.1, 1.),
        Slider::Volume => (crate::settings::VOLUME.0, crate::settings::VOLUME.1, 1.),
    }
}
fn slider_value(s: &Settings, slider: Slider) -> f32 {
    match slider {
        Slider::Sensitivity => s.sensitivity,
        Slider::Fov => s.fov,
        Slider::Volume => s.volume,
    }
}
fn slider_text(s: &Settings, slider: Slider) -> String {
    match slider {
        Slider::Sensitivity => format!("{:.2}", s.sensitivity),
        Slider::Fov => format!("{:.0}", s.fov),
        Slider::Volume => format!("{:.0}%", s.volume),
    }
}

/// The quality choices in the page's order.
const QUALITIES: [crate::quality::Choice; 5] = [
    crate::quality::Choice::Auto,
    crate::quality::Choice::Fixed(crate::quality::Level::High),
    crate::quality::Choice::Fixed(crate::quality::Level::Medium),
    crate::quality::Choice::Fixed(crate::quality::Level::Low),
    crate::quality::Choice::Fixed(crate::quality::Level::Lowest),
];
const SCALES: [f32; 6] = [1., 0.9, 0.8, 0.7, 0.6, 0.5];
fn step(world: &mut World, menu: &mut Menu, pick: Pick, by: i32) {
    use crate::quality::Resolution;
    let turn = |at: usize, n: usize| (at as i32 + by).rem_euclid(n as i32) as usize;
    match pick {
        Pick::Quality => {
            if let Some(mut g) = world.get_resource_mut::<crate::quality::Graphics>().filter(|g| g.forced.is_none()) {
                let at = QUALITIES.iter().position(|c| *c == g.choice).unwrap_or(0);
                let next = QUALITIES[turn(at, QUALITIES.len())];
                g.ask(next);
            }
        }
        Pick::Resolution => {
            if let Some(mut g) = world.get_resource_mut::<crate::quality::Graphics>().filter(|g| g.forced_resolution.is_none() && g.scalable()) {
                let all: Vec<Resolution> = std::iter::once(Resolution::Auto).chain(SCALES.map(Resolution::Fixed)).collect();
                let at = all.iter().position(|r| match (r, g.resolution) {
                    (Resolution::Auto, Resolution::Auto) => true,
                    (Resolution::Fixed(a), Resolution::Fixed(b)) => (a - b).abs() < 0.01,
                    _ => false,
                });
                let next = all[turn(at.unwrap_or(0), all.len())];
                g.ask_resolution(next);
            }
        }
        Pick::Scope => scope(world),
        Pick::Renderer => {
            if let Some(r) = menu.renderer.as_mut().filter(|r| r.locked.is_empty()) {
                let open: Vec<&(String, String, bool)> = r.items.iter().filter(|i| !i.2).collect();
                if open.is_empty() {
                    return;
                }
                let at = open.iter().position(|i| i.0 == r.chosen).unwrap_or(0);
                let next = open[turn(at, open.len())].0.clone();
                web::store_renderer(&next);
                r.chosen = next;
            }
        }
    }
}

/// Leaves: natively the game quits; in a browser the page reloads (online into the lobby, offline a new game).
fn leave(world: &mut World, online: bool) {
    let _ = online;
    #[cfg(target_arch = "wasm32")]
    web::reload();
    #[cfg(not(target_arch = "wasm32"))]
    world.write_message(AppExit::Success);
    #[cfg(target_arch = "wasm32")]
    let _ = world;
}
/// The leaving button's words.
fn leave_words(online: bool) -> &'static str {
    if !cfg!(target_arch = "wasm32") {
        "QUIT GAME"
    } else if online {
        "LEAVE MATCH"
    } else {
        "RESTART"
    }
}

fn read_renderer() -> Option<Renderer> {
    let text = web::renderer();
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    let s = |key: &str| v[key].as_str().unwrap_or("").to_string();
    let items = v["items"].as_array()?.iter().map(|i| (i["value"].as_str().unwrap_or("").to_string(), i["text"].as_str().unwrap_or("").to_ascii_uppercase(), i["disabled"].as_bool().unwrap_or(false))).collect();
    Some(Renderer { chosen: s("chosen"), running: s("running"), locked: s("locked"), items, loaded: s("chosen") })
}
impl Renderer {
    /// Whether what is chosen now needs the page reloaded: not what it loaded with, nor the build running.
    pub fn reload(&self) -> bool {
        self.chosen != self.loaded && self.chosen != self.running
    }
    fn name(&self, value: &str) -> String {
        let name = self.items.iter().find(|i| i.0 == value).map_or_else(|| value.to_ascii_uppercase(), |i| i.1.clone());
        let running = self.items.iter().find(|i| i.0 == self.running).map_or_else(|| self.running.to_ascii_uppercase(), |i| i.1.clone());
        if value == "auto" { format!("{name} ({running})") } else { name }
    }
}

/// What the page is told is on the screen ([`web::screen`]): the game, or this menu over it.
pub fn screen(menu: &Menu) -> &'static str {
    if menu.open { "menu" } else { "game" }
}
/// Every frame the game runs: the page told what is on the screen when it changes, so that it hides its links over the
/// game and shows them with the menu (index.html).
fn tell_page(menu: Res<Menu>, mut said: Local<&'static str>) {
    let now = screen(&menu);
    if *said != now {
        *said = now;
        web::screen(now);
    }
}

/// The page's side, in a browser; natively what it would say.
mod web {
    #[cfg(target_arch = "wasm32")]
    mod js {
        #[wasm_bindgen::prelude::wasm_bindgen(inline_js = "
export function menu_locked(){return !!document.pointerLockElement}
export function menu_keyboard(){try{const a=document.activeElement;return document.hasFocus()&&(!a||a===document.body||a===document.documentElement||a.tagName==='CANVAS')}catch{return true}}
export function menu_renderer(){try{const m=window.hotdogGraphicsMenus?.state?.()?.model?.renderer;if(!m)return '';return JSON.stringify({chosen:m.chosen,running:m.running,locked:m.locked||'',items:m.items.map(i=>({value:i.value,text:i.text,disabled:!!i.disabled}))})}catch{return ''}}
export function menu_store_renderer(v){try{localStorage.setItem('hotdog-renderer',v)}catch{}}
export function menu_reload(){location.reload()}
export function menu_licences(){try{window.open('THIRD_PARTY_LICENSES.html','_blank','noopener')}catch{}}
export function menu_screen(s){try{const d=document.documentElement.dataset;if(s)d.screen=s;else delete d.screen}catch{}}
")]
        extern "C" {
            pub fn menu_locked() -> bool;
            pub fn menu_keyboard() -> bool;
            pub fn menu_renderer() -> String;
            pub fn menu_store_renderer(value: &str);
            pub fn menu_reload();
            pub fn menu_licences();
            pub fn menu_screen(state: &str);
        }
    }
    /// Tells the page what is on the screen: "game", "menu" (this menu over it), or "" (nothing of the game's).
    pub fn screen(state: &str) {
        #[cfg(target_arch = "wasm32")]
        js::menu_screen(state);
        #[cfg(not(target_arch = "wasm32"))]
        let _ = state;
    }
    /// Whether the page really has the pointer lock (natively the grab is the truth).
    pub fn locked(grab: bool) -> bool {
        #[cfg(target_arch = "wasm32")]
        {
            let _ = grab;
            js::menu_locked()
        }
        #[cfg(not(target_arch = "wasm32"))]
        grab
    }
    /// Whether the page has the keyboard: it has the focus, and no box of its own (the chat) has it.
    pub fn has_keyboard() -> bool {
        #[cfg(target_arch = "wasm32")]
        return js::menu_keyboard();
        #[cfg(not(target_arch = "wasm32"))]
        true
    }
    pub fn renderer() -> String {
        #[cfg(target_arch = "wasm32")]
        return js::menu_renderer();
        #[cfg(not(target_arch = "wasm32"))]
        String::new()
    }
    pub fn store_renderer(value: &str) {
        #[cfg(target_arch = "wasm32")]
        js::menu_store_renderer(value);
        #[cfg(not(target_arch = "wasm32"))]
        let _ = value;
    }
    pub fn reload() {
        #[cfg(target_arch = "wasm32")]
        js::menu_reload();
    }
    /// Opens the licences page (THIRD_PARTY_LICENSES.html, at the page's own root) in a new tab.
    pub fn licences() {
        #[cfg(target_arch = "wasm32")]
        js::menu_licences();
    }
}

/// The licences page, which every package carries and the browser game's root has (tools/build-licenses.sh makes it, tools/dist-common.sh
/// puts it in): the licence of every library and font in the game, and the game's own.
const LICENCES_FILE: &str = "THIRD_PARTY_LICENSES.html";
/// Where the source code is published: the `repository` line of Cargo.toml, the one place the project writes it (HOTDOG_SOURCE_URL, when it is
/// set while the game is compiled, takes its place: tools/dist-common.sh, `dist_source_url`, reads the same two).
const SOURCE_URL: &str = match option_env!("HOTDOG_SOURCE_URL") {
    Some(url) => url,
    None => env!("CARGO_PKG_REPOSITORY"),
};
/// What the menu shows of the source's address: without the scheme.
fn source_words() -> &'static str {
    SOURCE_URL.trim_start_matches("https://").trim_start_matches("http://").trim_end_matches('/')
}
/// Where the licences page is in a package, said as a path: beside the assets folder (in a package's top folder, in a Mac app's
/// Contents/Resources), or in the working directory when the game runs from a checkout. Printable ASCII only, as the menu's font has.
fn licences_path(assets: &str) -> String {
    let beside = std::path::Path::new(assets).parent().filter(|dir| !dir.as_os_str().is_empty());
    let path = beside.map_or_else(|| LICENCES_FILE.to_string(), |dir| dir.join(LICENCES_FILE).to_string_lossy().into_owned());
    path.chars().map(|c| if c.is_ascii_graphic() || c == ' ' { c } else { '?' }).collect()
}

// ----- The look.

const WHITE: Color = Color::srgb(0.96, 0.97, 0.93);
const DIM: Color = Color::srgb(0.62, 0.67, 0.66);
const FAINT: Color = Color::srgb(0.40, 0.44, 0.44);
const ACCENT: Color = Color::srgb(1.0, 0.80, 0.30);
const ON_ACCENT: Color = Color::srgb(0.10, 0.08, 0.03);
const RED: Color = Color::srgb(1.0, 0.42, 0.36);
const GREEN: Color = Color::srgb(0.45, 0.92, 0.55);
const PANEL: Color = Color::srgba(0.035, 0.042, 0.052, 0.985);
const EDGE: Color = Color::srgba(1., 1., 1., 0.09);
const WELL: Color = Color::srgba(1., 1., 1., 0.045);
const HOVER: Color = Color::srgba(1., 1., 1., 0.085);
const CLEAR: Color = Color::NONE;
fn accent(a: f32) -> Color {
    ACCENT.with_alpha(a)
}

/// How a clickable node looks, resting and under the pointer.
#[derive(Component, Clone, Copy)]
struct Look {
    bg: Color,
    hover_bg: Color,
    edge: Color,
    hover_edge: Color,
}
impl Look {
    fn plain() -> Self {
        Look { bg: CLEAR, hover_bg: HOVER, edge: CLEAR, hover_edge: CLEAR }
    }
    fn well() -> Self {
        Look { bg: WELL, hover_bg: HOVER, edge: Color::srgba(1., 1., 1., 0.11), hover_edge: accent(0.9) }
    }
    fn primary() -> Self {
        Look { bg: ACCENT, hover_bg: Color::srgb(1.0, 0.87, 0.48), edge: CLEAR, hover_edge: CLEAR }
    }
}
/// The slot a rebinding waits on: it pulses.
#[derive(Component)]
struct Waiting;
#[derive(Component)]
struct SliderFill(Slider);
#[derive(Component)]
struct SliderKnob(Slider);
#[derive(Component)]
struct SliderText(Slider);
#[derive(Component)]
struct MenuRoot;

/// The menu as built: its root, and what it was built from.
#[derive(Resource, Default)]
struct View {
    root: Option<Entity>,
    built: String,
}

#[derive(Clone, Copy)]
struct Size {
    s: f32,
    sf: f32,
}
impl Size {
    fn px(&self, base: f32) -> Val {
        Val::Px((base * self.s * self.sf).round() / self.sf)
    }
    fn font(&self, base: f32) -> TextFont {
        TextFont { font_size: FontSize::Px((base * self.s).round()), ..default() }
    }
}

/// Everything the menu shows that is not a slider's value: when it changes the menu is built again.
fn signature(world: &World, menu: &Menu, z: Size) -> String {
    let b = world.get_resource::<Bindings>().cloned().unwrap_or_default();
    let s = world.get_resource::<Settings>().copied().unwrap_or_default();
    let hints = world.get_resource::<crate::hints::Hints>().is_some_and(|h| h.on);
    let graphics = world.get_resource::<crate::quality::Graphics>().map(|g| format!("{:?}{:?}{:?}{:?}", g.choice, g.forced, g.resolution, g.forced_resolution));
    let detail = world.get_resource::<crate::quality::Detail>().map(|d| (d.level, (d.scale * 100.).round() as i32));
    let at = situation(world);
    format!(
        "{:?}{:?}{:?}{:?}{:?}{}{}|{}{}{}{}{}{}|{:?}{:?}|{:?}|{}{}|{:.3}{:.3}|{}",
        menu.page,
        menu.tab,
        menu.capture,
        menu.clash,
        menu.confirm.map(|c| c.0),
        b.to_text(),
        b.invert_wheel,
        s.invert_y,
        s.damage_indicators,
        s.kill_slowmo,
        s.scope_toggle,
        hints,
        menu.paused,
        graphics,
        detail,
        menu.renderer,
        at.online,
        mode_line(world),
        z.s,
        z.sf,
        crate::touch::on(),
    )
}

/// Every frame after the round: the menu built (again when what it shows changes), the pointer's highlights, the
/// sliders' values.
pub fn draw(world: &mut World) {
    let (s, sf) = world.query::<&Window>().iter(world).next().map_or((1., 1.), |w| ((w.height() / 720.).clamp(0.8, 4.), w.scale_factor()));
    let z = Size { s, sf };
    let Some(menu) = world.remove_resource::<Menu>() else { return };
    let mut view = world.remove_resource::<View>().unwrap_or_default();
    if !menu.open {
        if let Some(root) = view.root.take()
            && let Ok(e) = world.get_entity_mut(root)
        {
            e.despawn();
        }
        view.built.clear();
    } else {
        let sig = signature(world, &menu, z);
        if view.root.is_none_or(|r| world.get_entity(r).is_err()) || sig != view.built {
            if let Some(root) = view.root.take()
                && let Ok(e) = world.get_entity_mut(root)
            {
                e.despawn();
            }
            view.root = Some(build(world, &menu, z));
            view.built = sig;
        }
        live(world, z);
    }
    world.insert_resource(view);
    world.insert_resource(menu);
}

/// The pointer's highlights, the waiting slot's pulse and the sliders, each frame.
fn live(world: &mut World, _z: Size) {
    let now = world.get_resource::<Time<Real>>().map_or(0., |t| t.elapsed_secs());
    for (look, interaction, mut bg, mut edge, waiting) in world.query::<(&Look, &Interaction, &mut BackgroundColor, &mut BorderColor, Option<&Waiting>)>().iter_mut(world) {
        let over = *interaction != Interaction::None;
        let (want_bg, want_edge) = if waiting.is_some() {
            let pulse = 0.55 + 0.45 * (now * 5.).sin().abs();
            (accent(0.16), accent(pulse))
        } else if over {
            (look.hover_bg, look.hover_edge)
        } else {
            (look.bg, look.edge)
        };
        if bg.0 != want_bg {
            bg.0 = want_bg;
        }
        let want = BorderColor::all(want_edge);
        if *edge != want {
            *edge = want;
        }
    }
    let s = world.get_resource::<Settings>().copied().unwrap_or_default();
    let share = |slider: Slider| {
        let (lo, hi, _) = range(slider);
        ((slider_value(&s, slider) - lo) / (hi - lo)).clamp(0., 1.) * 100.
    };
    for (fill, mut node) in world.query::<(&SliderFill, &mut Node)>().iter_mut(world) {
        let want = Val::Percent(share(fill.0));
        if node.width != want {
            node.width = want;
        }
    }
    for (knob, mut node) in world.query::<(&SliderKnob, &mut Node)>().iter_mut(world) {
        let want = Val::Percent(share(knob.0));
        if node.left != want {
            node.left = want;
        }
    }
    for (label, mut text) in world.query::<(&SliderText, &mut Text)>().iter_mut(world) {
        let want = slider_text(&s, label.0);
        if text.0 != want {
            text.0 = want;
        }
    }
}

/// The mode and the map, for the main page.
fn mode_line(world: &World) -> String {
    let Some(g) = world.get_resource::<crate::arena::Game>() else { return String::new() };
    let holding = world.get_resource::<crate::lobby::Lobby>().is_some_and(|l| l.holding);
    let mode = if holding {
        "LOBBY"
    } else if g.demo {
        "DEMO"
    } else if g.feast.rules.cards {
        "FULLY LOADED"
    } else if g.feast.rules.sandbox {
        "SANDBOX"
    } else if g.feast.rules.ffa {
        "FREE FOR ALL"
    } else if g.forts() {
        "FORTS"
    } else {
        "COUNTER SIEGE"
    };
    let map: String = g.feast.level.map.name.chars().filter(|c| c.is_ascii() && !c.is_ascii_control()).collect::<String>().to_ascii_uppercase();
    if map.trim().is_empty() || holding { mode.into() } else { format!("{mode}  /  {}", map.trim()) }
}

/// A builder of the menu's nodes at its size.
struct B<'w> {
    world: &'w mut World,
    z: Size,
}
impl B<'_> {
    fn node(&mut self, node: Node) -> Entity {
        self.world.spawn(node).id()
    }
    fn with(&mut self, parent: Entity, kids: &[Entity]) -> Entity {
        self.world.entity_mut(parent).add_children(kids);
        parent
    }
    fn text(&mut self, text: &str, size: f32, color: Color) -> Entity {
        self.world.spawn((Text::new(text), self.z.font(size), TextColor(color), TextLayout::no_wrap())).id()
    }
    fn icon(&mut self, icon: Icon, size: f32, tint: Color) -> Entity {
        let height = (size * self.z.s * self.z.sf).round().max(8.) as u32;
        let handle = self.world.resource_scope(|world, mut icons: Mut<Icons>| {
            let mut images = world.resource_mut::<Assets<Image>>();
            icons.get(&mut images, icon, height)
        });
        let (w, h) = crate::icons::size(icon, height);
        self.world
            .spawn((ImageNode { color: tint, ..ImageNode::new(handle) }, Node { width: Val::Px(w as f32 / self.z.sf), height: Val::Px(h as f32 / self.z.sf), flex_shrink: 0., ..default() }))
            .id()
    }
    /// A clickable node.
    fn button(&mut self, hit: Hit, mut node: Node, look: Look, kids: &[Entity]) -> Entity {
        if node.border == UiRect::default() {
            node.border = UiRect::all(self.z.px(1.));
        }
        let e = self.world.spawn((hit, Button, node, look, BackgroundColor(look.bg), BorderColor::all(look.edge))).id();
        self.with(e, kids)
    }
    /// A key cap, as the head-up display draws it.
    fn cap(&mut self, label: &str, size: f32) -> Entity {
        let z = self.z;
        let text = self.text(label, size, WHITE);
        let e = self
            .world
            .spawn((
                Node {
                    min_width: z.px(size * 1.7),
                    height: z.px(size * 1.7),
                    padding: UiRect::axes(z.px(5.), Val::Px(0.)),
                    border: UiRect::all(z.px(1.5)),
                    border_radius: BorderRadius::all(z.px(4.)),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    flex_shrink: 0.,
                    ..default()
                },
                BackgroundColor(Color::srgba(0.03, 0.04, 0.05, 0.72)),
                BorderColor::all(Color::srgba(0.9, 0.93, 0.9, 0.85)),
            ))
            .id();
        self.with(e, &[text])
    }
    /// An input as it is shown in a slot: the mouse's left and right buttons as their icons, the rest by name.
    fn input(&mut self, input: Option<Input>, size: f32, color: Color) -> Vec<Entity> {
        match input {
            None => vec![self.text("-", size, FAINT)],
            Some(Input::Mouse(MouseButton::Left)) => vec![self.icon(Icon::MouseLeft, size * 1.25, color), self.text("LEFT CLICK", size, color)],
            Some(Input::Mouse(MouseButton::Right)) => vec![self.icon(Icon::MouseRight, size * 1.25, color), self.text("RIGHT CLICK", size, color)],
            Some(input) => vec![self.text(input.label(), size, color)],
        }
    }
    fn row(&mut self, gap: f32, kids: &[Entity]) -> Entity {
        let z = self.z;
        let e = self.node(Node { align_items: AlignItems::Center, column_gap: z.px(gap), ..default() });
        self.with(e, kids)
    }
    fn rule(&mut self, top: f32, bottom: f32) -> Entity {
        let z = self.z;
        self.world.spawn((Node { height: z.px(1.), width: Val::Percent(100.), margin: UiRect { top: z.px(top), bottom: z.px(bottom), ..default() }, ..default() }, BackgroundColor(EDGE))).id()
    }
    fn spacer(&mut self) -> Entity {
        self.node(Node { flex_grow: 1., ..default() })
    }
}

/// The whole menu: the dim over the game, the page's panel, and a rebinding's or a clash's card over it.
fn build(world: &mut World, menu: &Menu, z: Size) -> Entity {
    let root = world
        .spawn((
            MenuRoot,
            Name::new("Menu"),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.),
                top: Val::Px(0.),
                width: Val::Percent(100.),
                height: Val::Percent(100.),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.008, 0.01, 0.014, 0.74)),
            FocusPolicy::Block,
            GlobalZIndex(100),
        ))
        .id();
    let mut b = B { world, z };
    let panel = match menu.page {
        Page::Main => main_page(&mut b, menu),
        Page::Settings => settings_page(&mut b, menu),
        Page::Controls => controls_page(&mut b, menu),
    };
    b.with(root, &[panel]);
    if let Some((action, slot)) = menu.capture {
        let card = capture_card(&mut b, action, slot);
        b.with(root, &[card]);
    } else if let Some(clash) = menu.clash {
        let card = clash_card(&mut b, clash);
        b.with(root, &[card]);
    }
    root
}

/// A panel `width` wide.
fn panel(b: &mut B, width: f32, kids: &[Entity]) -> Entity {
    let z = b.z;
    let e = b
        .world
        .spawn((
            Node {
                width: z.px(width),
                max_width: Val::Percent(94.),
                max_height: Val::Percent(94.),
                flex_direction: FlexDirection::Column,
                padding: UiRect::axes(z.px(30.), z.px(26.)),
                border: UiRect::all(z.px(1.)),
                border_radius: BorderRadius::all(z.px(12.)),
                ..default()
            },
            BackgroundColor(PANEL),
            BorderColor::all(EDGE),
            BoxShadow::new(Color::srgba(0., 0., 0., 0.55), Val::Px(0.), z.px(10.), Val::Px(0.), z.px(36.)),
        ))
        .id();
    b.with(e, kids)
}

fn main_page(b: &mut B, menu: &Menu) -> Entity {
    let z = b.z;
    let at = situation(b.world);
    let mode = mode_line(b.world);
    let mark = b.icon(Icon::Hotdog, 16., ACCENT);
    let name = b.text("HOTDOG SIMULATOR", 12., ACCENT);
    let eyebrow = b.row(8., &[mark, name]);
    let title = b.text(if menu.paused { "PAUSED" } else { "MENU" }, 34., WHITE);
    b.world.entity_mut(title).insert(Node { margin: UiRect::top(z.px(8.)), ..default() });
    let dot = b.world.spawn((Node { width: z.px(7.), height: z.px(7.), border_radius: BorderRadius::all(z.px(4.)), ..default() }, BackgroundColor(if at.online { GREEN } else { ACCENT }))).id();
    let status = b.text(if at.online { "ONLINE - THE GAME GOES ON" } else { "SINGLE PLAYER - GAME PAUSED" }, 11., DIM);
    let status = b.row(8., &[dot, status]);
    b.world.entity_mut(status).insert(Node { align_items: AlignItems::Center, column_gap: z.px(8.), margin: UiRect::top(z.px(6.)), ..default() });
    let mut head = vec![eyebrow, title, status];
    if !mode.is_empty() {
        let m = b.text(&mode, 11., FAINT);
        b.world.entity_mut(m).insert(Node { margin: UiRect::top(z.px(3.)), ..default() });
        head.push(m);
    }
    let rule = b.rule(20., 12.);
    head.push(rule);
    let item = |b: &mut B, hit: Hit, label: &str, look: Look, color: Color, key: Option<&str>| {
        let text = b.text(label, 18., color);
        let mut kids = vec![text];
        if let Some(key) = key {
            let gap = b.spacer();
            let cap = b.cap(key, 11.);
            kids.extend([gap, cap]);
        }
        b.button(
            hit,
            Node {
                height: z.px(46.),
                padding: UiRect::axes(z.px(16.), Val::Px(0.)),
                align_items: AlignItems::Center,
                border_radius: BorderRadius::all(z.px(8.)),
                ..default()
            },
            look,
            &kids,
        )
    };
    let resume = item(b, Hit::Resume, "RESUME", Look::primary(), ON_ACCENT, None);
    let settings = item(b, Hit::Open(Page::Settings), "SETTINGS", Look::plain(), WHITE, None);
    let controls = item(b, Hit::Open(Page::Controls), "CONTROLS", Look::plain(), WHITE, None);
    let replays = item(b, Hit::Replays, "REPLAYS", Look::plain(), WHITE, None);
    let sure = menu.confirm.is_some_and(|(c, _)| c == Confirm::Leave);
    let words = if sure { format!("{}? CLICK AGAIN", leave_words(at.online)) } else { leave_words(at.online).to_string() };
    let quit_look = Look { hover_bg: Color::srgba(1., 0.42, 0.36, 0.14), ..Look::plain() };
    let quit = item(b, Hit::Leave, &words, quit_look, if sure { RED } else { DIM }, None);
    let parting = b.rule(6., 6.);
    let list = b.node(Node { flex_direction: FlexDirection::Column, row_gap: z.px(4.), ..default() });
    b.with(list, &[resume, settings, controls, replays, parting, quit]);
    head.push(list);
    // How to get back to the game without the mouse.
    let foot = if cfg!(target_arch = "wasm32") {
        let cap = b.cap("ESC", 10.);
        let text = b.text("CLOSE   CLICK RESUME TO CAPTURE THE MOUSE", 10., FAINT);
        b.row(8., &[cap, text])
    } else {
        let cap = b.cap("ESC", 10.);
        let text = b.text("RESUME", 10., FAINT);
        b.row(8., &[cap, text])
    };
    b.world.entity_mut(foot).insert(Node { align_items: AlignItems::Center, column_gap: z.px(8.), margin: UiRect::top(z.px(18.)), ..default() });
    head.push(foot);
    let legal = legal(b);
    head.push(legal);
    panel(b, 400., &head)
}

/// Text that wraps at the panel's width, at a word where it can and anywhere where a word (a path) is too long.
fn wrapped(b: &mut B, text: &str, size: f32, color: Color) -> Entity {
    b.world.spawn((Text::new(text), b.z.font(size), TextColor(color), TextLayout::linebreak(LineBreak::WordOrCharacter), Node { width: Val::Percent(100.), ..default() })).id()
}

/// The licence and the source, tiny, under the menu: the game is GPL-3.0-only and says where its source is; the licences page lists
/// everything in it that is not the game's own. In a browser a click opens the page in a new tab; natively the menu says where the file is.
fn legal(b: &mut B) -> Entity {
    let z = b.z;
    let mut kids = vec![];
    let source = source_words();
    let words = if source.is_empty() { "GPL-3.0-ONLY. FREE SOFTWARE, WITHOUT WARRANTY.".to_string() } else { format!("GPL-3.0-ONLY. SOURCE CODE: {source}") };
    kids.push(wrapped(b, &words, 10., FAINT));
    if cfg!(target_arch = "wasm32") {
        let text = b.text("THIRD-PARTY LICENCES (NEW TAB)", 10., DIM);
        let node = Node { height: z.px(24.), padding: UiRect::axes(z.px(8.), Val::Px(0.)), margin: UiRect::left(z.px(-8.)), align_items: AlignItems::Center, border_radius: BorderRadius::all(z.px(6.)), ..default() };
        kids.push(b.button(Hit::Licences, node, Look::plain(), &[text]));
    } else {
        let path = licences_path(&crate::runtime::asset_directory());
        kids.push(wrapped(b, &format!("THIRD-PARTY LICENCES: {path}"), 10., FAINT));
    }
    let e = b.node(Node { flex_direction: FlexDirection::Column, row_gap: z.px(4.), margin: UiRect::top(z.px(8.)), width: Val::Percent(100.), ..default() });
    b.with(e, &kids)
}

/// A sub-page's head: back, the title, and anything on the right.
fn header(b: &mut B, title: &str, right: Vec<Entity>) -> Entity {
    let z = b.z;
    let chevron = b.text("<", 15., DIM);
    let back_text = b.text("BACK", 12., DIM);
    let back = b.button(
        Hit::Back,
        Node { height: z.px(30.), padding: UiRect::axes(z.px(10.), Val::Px(0.)), align_items: AlignItems::Center, column_gap: z.px(6.), border_radius: BorderRadius::all(z.px(6.)), ..default() },
        Look::well(),
        &[chevron, back_text],
    );
    let title = b.text(title, 24., WHITE);
    let gap = b.spacer();
    let mut kids = vec![back, title, gap];
    kids.extend(right);
    let e = b.row(18., &kids);
    b.world.entity_mut(e).insert(Node { align_items: AlignItems::Center, column_gap: z.px(18.), width: Val::Percent(100.), ..default() });
    e
}
fn section(b: &mut B, title: &str) -> Entity {
    let z = b.z;
    let t = b.text(title, 11., ACCENT);
    b.world.entity_mut(t).insert(Node { margin: UiRect { top: z.px(14.), bottom: z.px(4.), left: z.px(12.), ..default() }, ..default() });
    t
}
/// A settings row: its name, an optional note under it, and its control on the right.
fn setting(b: &mut B, label: &str, note: Option<(&str, Color)>, control: Entity) -> Entity {
    let z = b.z;
    let name = b.text(label, 15., WHITE);
    let mut left = vec![name];
    if let Some((note, color)) = note {
        left.push(b.text(note, 10., color));
    }
    let words = b.node(Node { flex_direction: FlexDirection::Column, row_gap: z.px(3.), ..default() });
    b.with(words, &left);
    let gap = b.spacer();
    let e = b.world.spawn((Node { min_height: z.px(42.), padding: UiRect::axes(z.px(12.), z.px(4.)), align_items: AlignItems::Center, border_radius: BorderRadius::all(z.px(6.)), ..default() }, BackgroundColor(Color::srgba(1., 1., 1., 0.022)))).id();
    b.with(e, &[words, gap, control])
}
/// An OFF / ON pair; a click anywhere on it turns it over.
fn switch(b: &mut B, hit: Hit, on: bool) -> Entity {
    let z = b.z;
    let cell = |b: &mut B, label: &str, lit: bool| {
        let t = b.text(label, 12., if lit { ON_ACCENT } else { DIM });
        let e = b.world.spawn((Node { width: z.px(52.), height: z.px(26.), justify_content: JustifyContent::Center, align_items: AlignItems::Center, border_radius: BorderRadius::all(z.px(5.)), ..default() }, BackgroundColor(if lit { ACCENT } else { CLEAR }))).id();
        b.with(e, &[t])
    };
    let off = cell(b, "OFF", !on);
    let on = cell(b, "ON", on);
    b.button(hit, Node { padding: UiRect::all(z.px(2.)), column_gap: z.px(2.), border_radius: BorderRadius::all(z.px(7.)), ..default() }, Look::well(), &[off, on])
}
/// A track with a fill and a knob, and the value; dragged along.
fn slider(b: &mut B, slider: Slider, settings: &Settings) -> Entity {
    let z = b.z;
    let (lo, hi, _) = range(slider);
    let share = ((slider_value(settings, slider) - lo) / (hi - lo)).clamp(0., 1.) * 100.;
    let fill = b.world.spawn((SliderFill(slider), Node { width: Val::Percent(share), height: Val::Percent(100.), border_radius: BorderRadius::all(z.px(3.)), ..default() }, BackgroundColor(ACCENT))).id();
    let track = b.world.spawn((Node { width: Val::Percent(100.), height: z.px(5.), border_radius: BorderRadius::all(z.px(3.)), ..default() }, BackgroundColor(Color::srgba(1., 1., 1., 0.13)))).id();
    b.with(track, &[fill]);
    let knob = b
        .world
        .spawn((
            SliderKnob(slider),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(share),
                width: z.px(15.),
                height: z.px(15.),
                margin: UiRect::left(z.px(-7.5)),
                border: UiRect::all(z.px(2.)),
                border_radius: BorderRadius::all(z.px(8.)),
                ..default()
            },
            BackgroundColor(WHITE),
            BorderColor::all(ACCENT),
        ))
        .id();
    let hit = b.world.spawn((Hit::Slider(slider), Button, RelativeCursorPosition::default(), Node { width: z.px(250.), height: z.px(26.), align_items: AlignItems::Center, ..default() })).id();
    b.with(hit, &[track, knob]);
    let value = b.world.spawn((SliderText(slider), Text::new(slider_text(settings, slider)), z.font(14.), TextColor(WHITE), TextLayout::no_wrap())).id();
    let holder = b.node(Node { width: z.px(54.), justify_content: JustifyContent::FlexEnd, ..default() });
    b.with(holder, &[value]);
    b.row(16., &[hit, holder])
}
/// `<  VALUE  >`, or the value alone when it cannot be changed here.
fn chooser(b: &mut B, pick: Pick, value: &str, enabled: bool) -> Entity {
    let z = b.z;
    let arrow = |b: &mut B, by: i32| {
        let t = b.text(if by < 0 { "<" } else { ">" }, 15., if enabled { WHITE } else { FAINT });
        let node = Node { width: z.px(30.), height: z.px(28.), justify_content: JustifyContent::Center, align_items: AlignItems::Center, border_radius: BorderRadius::all(z.px(6.)), ..default() };
        if enabled {
            b.button(Hit::Step(pick, by), node, Look::well(), &[t])
        } else {
            let e = b.node(node);
            b.with(e, &[t])
        }
    };
    let left = arrow(b, -1);
    let text = b.text(value, 13., if enabled { WHITE } else { DIM });
    let middle = b.node(Node { width: z.px(176.), justify_content: JustifyContent::Center, ..default() });
    b.with(middle, &[text]);
    let right = arrow(b, 1);
    b.row(6., &[left, middle, right])
}

fn settings_page(b: &mut B, menu: &Menu) -> Entity {
    let z = b.z;
    let s = b.world.get_resource::<Settings>().copied().unwrap_or_default();
    let hints = b.world.get_resource::<crate::hints::Hints>().is_some_and(|h| h.on);
    let head = header(b, "SETTINGS", vec![]);
    let rule = b.rule(16., 2.);
    let mut rows = vec![head, rule];
    rows.push(section(b, "MOUSE AND VIEW"));
    let c = slider(b, Slider::Sensitivity, &s);
    rows.push(setting(b, "MOUSE SENSITIVITY", None, c));
    let c = switch(b, Hit::Toggle(Toggle::InvertY), s.invert_y);
    rows.push(setting(b, "INVERT MOUSE Y", None, c));
    let c = slider(b, Slider::Fov, &s);
    rows.push(setting(b, "FIELD OF VIEW", Some(("DEGREES, TOP TO BOTTOM OF THE SCREEN", FAINT)), c));
    let c = switch(b, Hit::Toggle(Toggle::Hints), hints);
    rows.push(setting(b, "HINTS", Some(("TIPS AND TOOLTIPS IN WORDS", FAINT)), c));
    rows.push(section(b, "AUDIO"));
    let c = slider(b, Slider::Volume, &s);
    rows.push(setting(b, "VOLUME", None, c));
    rows.push(section(b, "COMBAT"));
    let c = switch(b, Hit::Toggle(Toggle::DamageIndicators), s.damage_indicators);
    rows.push(setting(b, "DAMAGE INDICATORS", Some(("ARCS ROUND THE CROSSHAIR POINT WHERE A HIT CAME FROM", FAINT)), c));
    let c = switch(b, Hit::Toggle(Toggle::SlowMotion), s.kill_slowmo);
    rows.push(setting(b, "KILL SLOW MOTION", Some(("A MOMENT OF SLOW MOTION WHEN YOUR SHOT KILLS", FAINT)), c));
    let c = chooser(b, Pick::Scope, if s.scope_toggle { "TOGGLE" } else { "HOLD" }, true);
    rows.push(setting(b, "SCOPE", Some(("HOLD THE RIGHT BUTTON TO AIM, OR CLICK ONCE UP AND ONCE DOWN", FAINT)), c));
    rows.push(section(b, "GRAPHICS"));
    // The quality: as the graphics have it (quality.rs); ?quality= in the address holds it.
    let graphics = b.world.get_resource::<crate::quality::Graphics>().map(|g| (g.choice, g.forced.is_some(), g.resolution, g.forced_resolution.is_some(), g.scalable()));
    let detail = b.world.get_resource::<crate::quality::Detail>().map(|d| (d.level, d.scale));
    match graphics {
        Some((choice, forced, resolution, forced_resolution, scalable)) => {
            let value = match (choice, detail) {
                (crate::quality::Choice::Auto, Some((level, _))) => format!("AUTO ({})", level.name().to_ascii_uppercase()),
                (crate::quality::Choice::Auto, None) => "AUTO".into(),
                (crate::quality::Choice::Fixed(level), _) => level.name().to_ascii_uppercase(),
            };
            let note = if forced { Some(("SET BY THE ADDRESS (?QUALITY=)", FAINT)) } else { Some(("AUTO FOLLOWS THE FRAME RATE", FAINT)) };
            let c = chooser(b, Pick::Quality, &value, !forced);
            rows.push(setting(b, "QUALITY", note, c));
            if scalable {
                let value = match (resolution, detail) {
                    (crate::quality::Resolution::Auto, Some((_, scale))) => format!("AUTO ({:.0}%)", scale * 100.),
                    (crate::quality::Resolution::Auto, None) => "AUTO".into(),
                    (crate::quality::Resolution::Fixed(share), _) => format!("{:.0}%", share * 100.),
                };
                let note = if forced_resolution { ("SET BY THE ADDRESS (?SCALE=)", FAINT) } else { ("THE SHARE OF THE SCREEN'S PIXELS DRAWN", FAINT) };
                let c = chooser(b, Pick::Resolution, &value, !forced_resolution);
                rows.push(setting(b, "RESOLUTION", Some(note), c));
            }
        }
        None => {
            let c = chooser(b, Pick::Quality, "-", false);
            rows.push(setting(b, "QUALITY", None, c));
        }
    }
    if let Some(r) = menu.renderer.as_ref() {
        let enabled = r.locked.is_empty() && r.items.iter().filter(|i| !i.2).count() > 1;
        let c = chooser(b, Pick::Renderer, &r.name(&r.chosen), enabled);
        let control = if r.reload() {
            let t = b.text("RELOAD NOW", 12., ON_ACCENT);
            let reload = b.button(
                Hit::Reload,
                Node { height: z.px(28.), padding: UiRect::axes(z.px(12.), Val::Px(0.)), align_items: AlignItems::Center, border_radius: BorderRadius::all(z.px(6.)), ..default() },
                Look::primary(),
                &[t],
            );
            b.row(10., &[reload, c])
        } else {
            c
        };
        let note = if !r.locked.is_empty() {
            ("SET BY THE ADDRESS", FAINT)
        } else if r.reload() {
            ("TAKES EFFECT WHEN THE PAGE RELOADS", ACCENT)
        } else {
            ("A CHANGE RELOADS THE PAGE", FAINT)
        };
        rows.push(setting(b, "RENDERER", Some(note), control));
    }
    let foot = b.text("CHANGES APPLY AT ONCE AND ARE KEPT FOR NEXT TIME", 10., FAINT);
    b.world.entity_mut(foot).insert(Node { margin: UiRect { top: z.px(16.), left: z.px(12.), ..default() }, ..default() });
    rows.push(foot);
    panel(b, 760., &rows)
}

fn controls_page(b: &mut B, menu: &Menu) -> Entity {
    let z = b.z;
    let bindings = b.world.get_resource::<Bindings>().cloned().unwrap_or_default();
    let sure = menu.confirm.is_some_and(|(c, _)| c == Confirm::ResetAll);
    let pristine = bindings.all_default();
    let reset_text = b.text(if sure { "RESET EVERYTHING? CLICK AGAIN" } else { "RESET ALL" }, 12., if sure { RED } else if pristine { FAINT } else { DIM });
    let node = Node { height: z.px(30.), padding: UiRect::axes(z.px(12.), Val::Px(0.)), align_items: AlignItems::Center, border: UiRect::all(z.px(1.)), border_radius: BorderRadius::all(z.px(6.)), ..default() };
    // (Nothing to reset while everything is as it was: shown, out of reach.)
    let reset_all = if pristine {
        let e = b.world.spawn((node, BorderColor::all(Color::srgba(1., 1., 1., 0.06)))).id();
        b.with(e, &[reset_text])
    } else {
        b.button(Hit::ResetAll, node, Look::well(), &[reset_text])
    };
    let head = header(b, "CONTROLS", vec![reset_all]);
    // The tabs.
    let tab = menu.tab();
    let mut tabs = vec![];
    for group in Group::ALL {
        let lit = group == tab;
        let t = b.text(group.name(), 13., if lit { WHITE } else { DIM });
        let look = Look { bg: CLEAR, hover_bg: HOVER, edge: if lit { ACCENT } else { CLEAR }, hover_edge: if lit { ACCENT } else { CLEAR } };
        let node = Node { height: z.px(34.), padding: UiRect::axes(z.px(14.), Val::Px(0.)), align_items: AlignItems::Center, border: UiRect::bottom(z.px(2.)), border_radius: BorderRadius::top(z.px(6.)), ..default() };
        tabs.push(b.button(Hit::Tab(group), node, look, &[t]));
    }
    let tabs = b.row(4., &tabs);
    b.world.entity_mut(tabs).insert(Node { align_items: AlignItems::FlexEnd, column_gap: z.px(4.), margin: UiRect::top(z.px(14.)), ..default() });
    let rule = b.rule(0., 8.);
    // The columns' heads.
    let name_head = b.text("ACTION", 10., FAINT);
    let name_col = b.node(Node { flex_grow: 1., ..default() });
    b.with(name_col, &[name_head]);
    let key_head = b.text("KEY", 10., FAINT);
    let key_col = b.node(Node { width: z.px(170.), justify_content: JustifyContent::Center, ..default() });
    b.with(key_col, &[key_head]);
    let alt_head = b.text("ALTERNATE", 10., FAINT);
    let alt_col = b.node(Node { width: z.px(170.), justify_content: JustifyContent::Center, ..default() });
    b.with(alt_col, &[alt_head]);
    let pad = b.node(Node { width: z.px(64.), ..default() });
    let heads = b.row(10., &[name_col, key_col, alt_col, pad]);
    b.world.entity_mut(heads).insert(Node { align_items: AlignItems::Center, column_gap: z.px(10.), padding: UiRect::axes(z.px(12.), Val::Px(0.)), margin: UiRect::bottom(z.px(4.)), ..default() });
    let mut rows = vec![head, tabs, rule, heads];
    for action in Action::ALL.into_iter().filter(|a| a.group() == tab) {
        rows.push(binding_row(b, &bindings, menu, action));
    }
    if tab.wheels() {
        let c = switch(b, Hit::Toggle(Toggle::InvertWheel), bindings.invert_wheel);
        let row = setting(b, "INVERT MOUSE WHEEL", Some(("FOR THE GUNS, TURNING WHAT IS CARRIED AND THE STYLES", FAINT)), c);
        b.world.entity_mut(row).insert(Node { min_height: z.px(40.), padding: UiRect::axes(z.px(12.), z.px(4.)), margin: UiRect::top(z.px(8.)), align_items: AlignItems::Center, border_radius: BorderRadius::all(z.px(6.)), ..default() });
        rows.push(row);
    }
    let gap = b.spacer();
    rows.push(gap);
    let a = b.text("CLICK A KEY TO CHANGE IT", 10., FAINT);
    let c = b.text("RIGHT-CLICK TO CLEAR IT", 10., FAINT);
    let d = b.text("THE WHEEL CAN BE BOUND TOO", 10., FAINT);
    let foot = b.row(26., &[a, c, d]);
    b.world.entity_mut(foot).insert(Node { align_items: AlignItems::Center, column_gap: z.px(26.), margin: UiRect { top: z.px(14.), left: z.px(12.), ..default() }, ..default() });
    rows.push(foot);
    let p = panel(b, 820., &rows);
    // The tallest tab (the weapons) sets the height, so that the panel does not jump between tabs.
    b.world.entity_mut(p).entry::<Node>().and_modify(|mut n| n.min_height = z.px(626.));
    p
}

fn binding_row(b: &mut B, bindings: &Bindings, menu: &Menu, action: Action) -> Entity {
    let z = b.z;
    let mut label = vec![];
    if let Some(gun) = action.gun() {
        let icon = b.icon(Icon::Gun(gun as u8), 16., DIM);
        let slot = b.node(Node { width: z.px(44.), justify_content: JustifyContent::Center, ..default() });
        label.push(b.with(slot, &[icon]));
    }
    label.push(b.text(action.name(), 14., WHITE));
    let name = b.row(10., &label);
    let name_col = b.node(Node { flex_grow: 1., ..default() });
    b.with(name_col, &[name]);
    let mut kids = vec![name_col];
    for slot in 0..2 {
        let input = bindings.get(action)[slot];
        let waiting = menu.capture == Some((action, slot));
        let clashing = bindings.clashing(action, slot);
        let content = if waiting { vec![b.text("PRESS A KEY", 12., ACCENT)] } else { b.input(input, 12., if clashing { RED } else { WHITE }) };
        let look = if clashing { Look { edge: RED, hover_edge: RED, ..Look::well() } } else { Look::well() };
        let e = b.button(
            Hit::Bind(action, slot),
            Node { width: z.px(170.), height: z.px(27.), justify_content: JustifyContent::Center, align_items: AlignItems::Center, column_gap: z.px(6.), border_radius: BorderRadius::all(z.px(5.)), ..default() },
            look,
            &content,
        );
        if waiting {
            b.world.entity_mut(e).insert(Waiting);
        }
        kids.push(e);
    }
    let reset = if bindings.is_default(action) {
        b.node(Node { width: z.px(64.), ..default() })
    } else {
        let t = b.text("RESET", 10., DIM);
        b.button(
            Hit::Reset(action),
            Node { width: z.px(64.), height: z.px(24.), justify_content: JustifyContent::Center, align_items: AlignItems::Center, border_radius: BorderRadius::all(z.px(5.)), ..default() },
            Look::plain(),
            &[t],
        )
    };
    kids.push(reset);
    // (A row changed from the defaults is marked with the accent down its left edge.)
    let changed = !bindings.is_default(action);
    let e = b
        .world
        .spawn((
            Node { min_height: z.px(31.), padding: UiRect::axes(z.px(12.), Val::Px(0.)), align_items: AlignItems::Center, column_gap: z.px(10.), border: UiRect::left(z.px(3.)), border_radius: BorderRadius::all(z.px(4.)), ..default() },
            BackgroundColor(if changed { Color::srgba(1., 1., 1., 0.03) } else { CLEAR }),
            BorderColor::all(if changed { ACCENT } else { CLEAR }),
        ))
        .id();
    b.with(e, &kids)
}

/// A card over the page, with everything under it out of reach.
fn card(b: &mut B, kids: &[Entity]) -> Entity {
    let z = b.z;
    let inner = b
        .world
        .spawn((
            Node {
                width: z.px(460.),
                max_width: Val::Percent(90.),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                padding: UiRect::axes(z.px(30.), z.px(26.)),
                border: UiRect::all(z.px(1.)),
                border_radius: BorderRadius::all(z.px(12.)),
                row_gap: z.px(6.),
                ..default()
            },
            BackgroundColor(Color::srgba(0.05, 0.058, 0.07, 0.98)),
            BorderColor::all(accent(0.35)),
            BoxShadow::new(Color::srgba(0., 0., 0., 0.6), Val::Px(0.), z.px(12.), Val::Px(0.), z.px(40.)),
        ))
        .id();
    b.with(inner, kids);
    let back = b
        .world
        .spawn((
            Node { position_type: PositionType::Absolute, left: Val::Px(0.), top: Val::Px(0.), width: Val::Percent(100.), height: Val::Percent(100.), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() },
            BackgroundColor(Color::srgba(0., 0., 0., 0.5)),
            FocusPolicy::Block,
            Interaction::default(),
        ))
        .id();
    b.with(back, &[inner])
}
fn capture_card(b: &mut B, action: Action, slot: usize) -> Entity {
    let z = b.z;
    let ask = b.text("PRESS A KEY, A MOUSE BUTTON OR ROLL THE WHEEL", 11., ACCENT);
    let name = b.text(action.name(), 26., WHITE);
    b.world.entity_mut(name).insert(Node { margin: UiRect::axes(Val::Px(0.), z.px(6.)), ..default() });
    let which = b.text(if slot == 0 { "MAIN KEY" } else { "ALTERNATE KEY" }, 11., DIM);
    let rule = b.rule(14., 10.);
    let esc = b.cap("ESC", 10.);
    let cancel = b.text("CANCEL", 10., FAINT);
    let foot = b.row(8., &[esc, cancel]);
    card(b, &[ask, name, which, rule, foot])
}
fn clash_card(b: &mut B, (action, slot, input, conflict): (Action, usize, Input, Conflict)) -> Entity {
    let z = b.z;
    let bindings = b.world.get_resource::<Bindings>().cloned().unwrap_or_default();
    let warn = b.text("ALREADY IN USE", 11., RED);
    let key = b.cap(input.label(), 13.);
    let is = b.text("IS BOUND TO", 14., DIM);
    let other = b.text(conflict.action.name(), 14., WHITE);
    let line = b.row(10., &[key, is, other]);
    b.world.entity_mut(line).insert(Node { align_items: AlignItems::Center, column_gap: z.px(10.), margin: UiRect::axes(Val::Px(0.), z.px(8.)), ..default() });
    let replaced = bindings.get(action)[slot];
    let then = match replaced {
        Some(r) => format!("SWAP THEM: {} GETS {}", conflict.action.name(), r.label()),
        None => format!("SWAP THEM: {} IS LEFT WITHOUT IT", conflict.action.name()),
    };
    let then = b.text(&then, 11., DIM);
    let rule = b.rule(14., 12.);
    let swap_text = b.text("SWAP", 14., ON_ACCENT);
    let swap = b.button(
        Hit::Swap,
        Node { width: z.px(150.), height: z.px(38.), justify_content: JustifyContent::Center, align_items: AlignItems::Center, border_radius: BorderRadius::all(z.px(8.)), ..default() },
        Look::primary(),
        &[swap_text],
    );
    let cancel_text = b.text("CANCEL", 14., WHITE);
    let cancel = b.button(
        Hit::Cancel,
        Node { width: z.px(150.), height: z.px(38.), justify_content: JustifyContent::Center, align_items: AlignItems::Center, border_radius: BorderRadius::all(z.px(8.)), ..default() },
        Look::well(),
        &[cancel_text],
    );
    let buttons = b.row(12., &[swap, cancel]);
    card(b, &[warn, line, then, rule, buttons])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arena::{test_app, test_frame, Game, Weapon};
    use bevy::input::mouse::AccumulatedMouseMotion;

    /// The page is told the game is on the screen (it hides its links over it) and the menu once it opens (they come back).
    #[test]
    fn the_page_hides_its_links_over_the_game_and_shows_them_with_the_menu() {
        let mut menu = Menu::default();
        assert_eq!(screen(&menu), "game");
        menu.open = true;
        assert_eq!(screen(&menu), "menu");
        let page = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/index.html")).unwrap();
        assert!(page.contains("html[data-screen=game] .modes{display:none}"));
    }

    /// A round with the menu's systems as `main` has them (the gate, the menu), and the round's own.
    fn round(rules: &str) -> App {
        let mut app = test_app(rules);
        app.init_resource::<crate::prefs::Prefs>()
            .init_resource::<Bindings>()
            .init_resource::<Settings>()
            .init_resource::<Gate>()
            .init_resource::<Menu>()
            .add_systems(PreUpdate, (run, crate::controls::shut).chain())
            .add_systems(Update, crate::arena::tick);
        test_frame(&mut app, 0.05);
        test_frame(&mut app, 0.05);
        app
    }
    fn key(app: &mut App, key: KeyCode) {
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(key);
    }

    /// Open, the menu keeps the game from every key, button and motion of the mouse: single player stands still, and
    /// online (where it does not) the hotdog neither walks nor fires nor turns. Esc opens it natively and closes it.
    #[test]
    fn the_open_menu_blocks_the_trigger_the_walk_and_the_look() {
        let mut app = round("");
        let capture = |app: &mut App| {
            let world = app.world_mut();
            for mut c in world.query::<&mut CursorOptions>().iter_mut(world) {
                c.grab_mode = CursorGrabMode::Locked;
            }
        };
        // (The window has no cursor options in the test world until given them.)
        let window = app.world_mut().query_filtered::<Entity, With<Window>>().iter(app.world()).next().unwrap();
        app.world_mut().entity_mut(window).insert(CursorOptions::default());
        capture(&mut app);
        let before = |app: &App| {
            let g = app.world().resource::<Game>();
            (g.position, g.yaw, g.ammo[Weapon::Rifle as usize])
        };
        // Esc opens it.
        key(&mut app, KeyCode::Escape);
        test_frame(&mut app, 0.05);
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().release_all();
        assert!(app.world().resource::<Menu>().open && paused(app.world()), "open, and single player is paused");
        // Both ways: paused, and as online is (not paused: the gate alone).
        for online in [false, true] {
            if online {
                app.world_mut().resource_mut::<crate::runtime::Options>().connect = "ws://test".into();
            }
            let was = before(&app);
            for _ in 0..10 {
                key(&mut app, KeyCode::KeyW);
                app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
                app.world_mut().resource_mut::<AccumulatedMouseMotion>().delta = Vec2::new(40., 0.);
                test_frame(&mut app, 0.05);
            }
            assert_eq!(paused(app.world()), !online);
            let now = before(&app);
            assert_eq!(now.0, was.0, "no walk (online: {online})");
            assert_eq!(now.1, was.1, "no look");
            assert_eq!(now.2, was.2, "no shot");
            assert!(!app.world().resource::<Game>().pending_shot);
            app.world_mut().resource_mut::<ButtonInput<KeyCode>>().release_all();
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release_all();
            test_frame(&mut app, 0.05);
        }
        // Esc again closes it (natively capturing the pointer), and the keys reach the game again.
        key(&mut app, KeyCode::Escape);
        test_frame(&mut app, 0.05);
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().release_all();
        assert!(!app.world().resource::<Menu>().open);
        let was = before(&app);
        for _ in 0..6 {
            key(&mut app, KeyCode::KeyW);
            test_frame(&mut app, 0.05);
        }
        assert_ne!(before(&app).0, was.0, "walks once it is closed");
    }

    /// A rebinding hears the next input; one in use is a clash, which a swap settles by trading; Esc cancels; a key
    /// with no name (Esc) cannot be bound.
    #[test]
    fn a_rebinding_waits_for_an_input_and_a_clash_offers_a_swap() {
        let mut b = Bindings::default();
        let mut menu = Menu::default();
        menu.begin(Action::Fire, 0);
        menu.offer(&mut b, Input::Key(KeyCode::KeyG));
        assert_eq!(b.get(Action::Fire)[0], Some(Input::Key(KeyCode::KeyG)));
        assert!(menu.capture.is_none() && menu.clash.is_none());
        // Space is the jump's.
        menu.begin(Action::Hints, 0);
        menu.offer(&mut b, Input::Key(KeyCode::Space));
        let clash = menu.clash.expect("a clash");
        assert_eq!(clash.3.action, Action::Jump);
        assert_eq!(b.get(Action::Hints)[0], Some(Input::Key(KeyCode::KeyH)), "nothing changes until it is settled");
        menu.settle(&mut b, true);
        assert_eq!((b.get(Action::Hints)[0], b.get(Action::Jump)[0]), (Some(Input::Key(KeyCode::Space)), Some(Input::Key(KeyCode::KeyH))));
        // A trade the other way would clash in turn (F is Kung Fu's kick, which is heard with the jump): it is left without.
        let mut c = Bindings::default();
        menu.begin(Action::Melee, 0);
        menu.offer(&mut c, Input::Key(KeyCode::Space));
        menu.settle(&mut c, true);
        assert_eq!((c.get(Action::Melee)[0], c.get(Action::Jump)[0]), (Some(Input::Key(KeyCode::Space)), None));
        // Cancelled: left as it was.
        menu.begin(Action::Use, 0);
        menu.offer(&mut b, Input::Key(KeyCode::KeyQ));
        menu.settle(&mut b, false);
        assert_eq!(b.get(Action::Use)[0], Some(Input::Key(KeyCode::KeyE)));
        // Esc: what is heard, and how a menu unwinds.
        let (mut keys, mouse, scroll) = (ButtonInput::default(), ButtonInput::default(), AccumulatedMouseScroll::default());
        keys.press(KeyCode::Escape);
        assert_eq!(heard(&keys, &mouse, &scroll), Some(None), "Esc cancels");
        let mut keys = ButtonInput::default();
        keys.press(KeyCode::ControlRight);
        assert_eq!(heard(&keys, &mouse, &scroll), Some(Some(Input::Key(KeyCode::ControlLeft))), "either Ctrl is Ctrl");
        let wheel = AccumulatedMouseScroll { unit: MouseScrollUnit::Line, delta: Vec2::new(0., -1.) };
        assert_eq!(heard(&ButtonInput::default(), &mouse, &wheel), Some(Some(Input::Wheel(Wheel::Down))));
        menu.page = Page::Controls;
        menu.begin(Action::Jump, 1);
        assert!(!menu.escape() && menu.capture.is_none() && menu.page == Page::Controls, "first the rebinding");
        assert!(!menu.escape() && menu.page == Page::Main, "then the page");
        assert!(menu.escape(), "then the menu");
    }

    /// The browser's rule: a lock lost with the page still holding the keyboard opens the menu a moment later; lost to
    /// another window, or at the card table, it only lets the grab go; a lock the browser never gave lets it go too.
    #[test]
    fn in_a_browser_a_lost_pointer_lock_opens_the_menu_unless_the_focus_went_elsewhere() {
        // (grab, really, keyboard, at the table) each frame, 0.05 s apart: what was let go and opened, over them all.
        let run = |frames: &[(bool, bool, bool, bool)]| {
            let mut lock = Lock::default();
            let (mut released, mut opened) = (false, false);
            let mut grab = false;
            for (i, (asked, really, keyboard, table)) in frames.iter().enumerate() {
                grab = grab || *asked;
                let (r, o) = follow(&mut lock, grab, *really, || *keyboard, opened, *table, i as f32 * 0.05);
                if r {
                    grab = false;
                }
                (released, opened) = (released || r, opened || o);
            }
            (released, opened)
        };
        let had = (true, true, true, false);
        let lost = (false, false, true, false);
        // Esc: lost with the keyboard, and open once SETTLE has passed (and not before).
        assert_eq!(run(&[had, had, lost, lost, lost, lost]), (true, true));
        assert_eq!(run(&[had, had, lost, lost]), (true, false), "not before SETTLE");
        // Another window or tab: the focus went with it.
        let away = (false, false, false, false);
        assert_eq!(run(&[had, had, away, away, away, away]), (true, false));
        // At the card table the first Esc only frees the pointer.
        let table = (true, true, true, true);
        let freed = (false, false, true, true);
        assert_eq!(run(&[table, table, freed, freed, freed, freed]), (true, false));
        // Asked for and never given: let go after PATIENCE, nothing opens.
        let refused = [(true, false, true, false)].into_iter().chain([(false, false, true, false); 30]).collect::<Vec<_>>();
        assert_eq!(run(&refused), (true, false));
        // Given a moment after it was asked for: nothing is lost.
        let slow = [(true, false, true, false), (false, false, true, false), (false, true, true, false), (false, true, true, false)];
        assert_eq!(run(&slow), (false, false));
    }

    /// Rebound, the slot and gun keys take the right guns: 1 no longer does, G takes the primary; a gun's own key takes
    /// it when it is held; the wheel inverted goes the other way round.
    #[test]
    fn rebound_weapon_keys_take_the_right_guns_and_the_wheel_turns_round() {
        let mut app = round("sandbox god weapons=mustard,sniper,hotsauce");
        let held = crate::arena::held_guns(&app.world().resource::<Game>().owned);
        assert_eq!(held.len(), 3, "{held:?}");
        let in_hand = |app: &App| app.world().resource::<Game>().weapon;
        let press = |app: &mut App, k: KeyCode| {
            key(app, k);
            test_frame(app, 0.05);
            app.world_mut().resource_mut::<ButtonInput<KeyCode>>().release_all();
            test_frame(app, 0.05);
        };
        {
            let mut b = app.world_mut().resource_mut::<Bindings>();
            b.assign(Action::Primary, 0, Input::Key(KeyCode::KeyG)).unwrap();
            b.assign(Action::Secondary, 0, Input::Key(KeyCode::KeyV)).unwrap();
            b.assign(Action::Gun3, 0, Input::Key(KeyCode::Digit3)).unwrap();
        }
        let [primary, secondary] = crate::arena::slots(&app.world().resource::<Game>().owned);
        press(&mut app, KeyCode::KeyV);
        assert_eq!(Some(in_hand(&app)), secondary);
        press(&mut app, KeyCode::Digit1);
        assert_eq!(Some(in_hand(&app)), secondary, "1 is no longer the primary's");
        press(&mut app, KeyCode::KeyG);
        assert_eq!(Some(in_hand(&app)), primary);
        press(&mut app, KeyCode::Digit3);
        assert_eq!(in_hand(&app), Weapon::Flame, "the hot sauce thrower by its own key");
        // The last gun used: back to the primary.
        app.world_mut().resource_mut::<Bindings>().assign(Action::LastWeapon, 0, Input::Key(KeyCode::KeyB)).unwrap();
        press(&mut app, KeyCode::KeyB);
        assert_eq!(Some(in_hand(&app)), primary, "the last gun");
        // The wheel: rolled down is the next; inverted, the one before.
        let notch = |app: &mut App, y: f32| {
            app.world_mut().resource_mut::<AccumulatedMouseScroll>().delta = Vec2::new(0., y);
            app.world_mut().resource_mut::<AccumulatedMouseScroll>().unit = MouseScrollUnit::Line;
            test_frame(app, 0.2);
        };
        let at = held.iter().position(|w| *w == in_hand(&app)).unwrap();
        notch(&mut app, -1.);
        assert_eq!(in_hand(&app), held[(at + 1) % 3], "down: the next");
        app.world_mut().resource_mut::<Bindings>().invert_wheel = true;
        notch(&mut app, -1.);
        assert_eq!(in_hand(&app), held[at], "inverted, down is the one before");
    }

    /// The menu says the game is GPL-3.0-only and where its source is, and names (a browser links to) the licences page, which
    /// is there to be opened: the page is in the repository, where the packaging script and the server both take it from.
    #[test]
    fn the_menu_names_the_licence_the_source_and_the_licences_page() {
        assert_eq!(licences_path("/opt/hotdog/assets"), "/opt/hotdog/THIRD_PARTY_LICENSES.html");
        assert_eq!(licences_path("assets"), "THIRD_PARTY_LICENSES.html", "run from a checkout: in the working directory");
        assert_eq!(licences_path("/home/Jos\u{e9}/game/assets"), "/home/Jos?/game/THIRD_PARTY_LICENSES.html", "the menu's font has no accents");
        assert!(SOURCE_URL.is_empty() || SOURCE_URL.starts_with("https://"), "{SOURCE_URL}");
        assert!(!source_words().starts_with("http") && !source_words().ends_with('/'));
        let page = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(LICENCES_FILE);
        let text = std::fs::read_to_string(&page).unwrap_or_else(|e| panic!("{}: {e} (tools/build-licenses.sh makes it)", page.display()));
        assert!(text.contains("GPL-3.0-only") && text.contains("Fira Mono"), "the licences page lists the game's licence and its fonts");

        let mut world = World::new();
        let root = legal(&mut B { world: &mut world, z: Size { s: 1., sf: 1. } });
        let words: Vec<String> = world.query::<&Text>().iter(&world).map(|t| t.0.clone()).collect();
        assert!(words.iter().any(|w| w.starts_with("GPL-3.0-ONLY.")), "{words:?}");
        assert!(words.iter().any(|w| w.starts_with("THIRD-PARTY LICENCES")), "{words:?}");
        assert!(world.get::<Children>(root).is_some_and(|c| !c.is_empty()));
    }
}
