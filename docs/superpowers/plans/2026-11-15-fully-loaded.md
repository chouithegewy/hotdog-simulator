# Fully Loaded Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A fourth lobby mode, Fully Loaded: the hotdogs play a press-your-luck topping card game at a table on the Counter Siege microwave, online (server-dealt) and offline (with bots).

**Architecture:** The card game is a pure rules engine in the shared simulation crate (`kitchen_sim::cards::Table`). It is a *rule of the game*, like Free For All: `Rules.cards` (`mode=cards`) makes `Feast` hold a `Table`, stand its five hotdogs round the microwave and play the table instead of the fight, ending the round through the existing `RoundOver` path. So the server's seats, ready, countdown, 9 s opening, bots-take-over and back-to-the-fridge all work unchanged; the server only adds a `cards` message each way. The client reads the table's `View` (from its own `Feast` offline, from the server's `cards` message online) and draws the table, cards, scoreboard and first-person seat in a new `src/cards.rs`, while the round's camera, movement, guns and HUD stand down.

**Tech Stack:** Rust 2024, Bevy 0.19 (client), axum/tokio (server, `crates/server`), serde/serde_json, Node `node:test` for the page scripts.

**Spec:** `docs/superpowers/specs/2026-11-15-fully-loaded-design.md` (read it first; the card list, rules and pacing are there).

## Global Constraints

- Mode id `cards`; display name **Fully Loaded** everywhere a mode is named.
- Card names and values exactly as the spec's table: 0 Plain Bun (1 copy), 1 Celery Salt, 2 Sport Peppers, 3 Jalapeños, 4 Bacon, 5 Cheese, 6 Chili, 7 Sauerkraut, 8 Pickles, 9 Onions, 10 Relish, 11 Mustard, 12 Ketchup (copies = value); Fries +2, Chips +4, Pickle Spear +6, Soda +8, Onion Rings +10, Foot-Long (×2 toppings) one each; Back in the Fridge, Pile It On, Napkin three each. 94 cards.
- First to **100**; fully loaded = **7** different toppings, **+15**; Pile It On = **3** flips.
- Pacing: a card shows **0.7 s**; a person's decision **15 s** online (offline: no limit); bot thinks **0.8–1.6 s**; scores between hands **3 s**; winner **8 s** (the existing RoundOver wait).
- Protocol 12's binary frame format and `net::STAGES` are untouched; the table travels only as JSON text `{type:'cards', ...}`. Siege, Forts and Free For All behave exactly as before.
- Nothing from the original card game's name, art, layout or rulebook wording.
- New files start with `// SPDX-License-Identifier: GPL-3.0-only` and `// Copyright (C) 2026 David Nunes-Childs`; follow the repo's prose doc-comment style and British spelling (colour).
- **Do not run `git commit`** (the user has two days of uncommitted work in this tree and has not asked for commits). Each task ends with a checkpoint (its tests green) instead. The pre-work snapshot is git tree `b9445481dbf26a341f96e443285f038c22725b9e` (`git diff b944548 -- <path>` shows this feature's changes).

## Review Focus

1. **A person leaves mid-decision** (turn or target pick): the bot that takes the seat acts within its think time, not after the 15 s limit. Test in Task 1.
2. **The deck runs out mid-hand**: the discards are shuffled into a new deck; cards in front of the seats stay there. Test in Task 1.
3. **Pile It On's target busts or is fully loaded part-way**: the remaining flips stop, and action cards it drew meanwhile are discarded rather than resolved. Test in Task 1.
4. **A client sends a card command that is out of turn, for another seat, or malformed**: refused, nothing changes, nothing panics. Test in Task 3.
5. **Switching back from Fully Loaded to Siege**: the next round is an ordinary siege (no table, rules without `cards`) and the client's HUD, camera, guns and tags come back. Tests in Task 3 (server) and Task 5 (client).

---

### Task 1: The card table rules engine

**Files:**
- Create: `crates/kitchen-sim/src/cards.rs`
- Modify: `crates/kitchen-sim/src/lib.rs` (add `pub mod cards;`)
- Test: `#[cfg(test)] mod tests` in `crates/kitchen-sim/src/cards.rs`

**Interfaces:**
- Produces (all `pub`, serde `Serialize + Deserialize` on the data types, `Clone, Debug, PartialEq`):
  - consts `TARGET: u32 = 100`, `BONUS: u32 = 15`, `LOADED: usize = 7`, `PILE: usize = 3`, `SHOW: f32 = 0.7`, `TURN: f32 = 15.`, `BETWEEN: f32 = 3.`, `THINK: [f32; 2] = [0.8, 1.6]`, `TOPPINGS: [&str; 13]` (index = value).
  - `enum Card { Topping(u8), Side(u8), FootLong, Fridge, PileOn, Napkin }` (+ `Copy, Eq, Hash`); `Card::name(self) -> String` ("Ketchup 12", "Fries +2", "Foot-Long ×2", "Back in the Fridge", "Pile It On", "Napkin"); `Card::is_action(self) -> bool` (Fridge | PileOn).
  - `fn deck() -> Vec<Card>` (the 94, unshuffled).
  - `enum Status { In, Stopped, Fridged, Bust, Loaded, Out }` (`Out`: not in this hand, e.g. a tie-break).
  - `enum Command { Flip, Stop, Target(usize) }`.
  - `enum Kind { Dealt, Flipped, Bust, Saved, Loaded, Stopped, Fridged, PiledOn, Passed, Hand, Won }`; `struct Event { seq: u64, seat: usize, kind: Kind, card: Option<Card>, target: Option<usize> }`.
  - `struct SeatView { cards: Vec<Card>, status: Status, points: u32, total: u32, napkin: bool, bot: bool }`.
  - `struct View { seats: Vec<SeatView>, dealer: usize, hand: u32, turn: Option<usize>, choosing: Option<(usize, Card)>, clock: Option<f32>, deck: usize, between: bool, winner: Option<usize>, events: Vec<Event>, version: u64 }` (`events`: the last 16; `clock`: seconds left on the current person's decision, None when unlimited or a bot's).
  - `struct Table` with:
    - `Table::new(seats: usize, seed: u64) -> Table` — seat 0 is the first dealer; nothing is dealt until the first `tick`.
    - `tick(&mut self, dt: f32)` — advances shows, bots' thinking, decision clocks; handles any `dt` (loops in slices like `Feast::tick`).
    - `command(&mut self, seat: usize, command: Command) -> Result<(), &'static str>` — only the seat whose turn (Flip/Stop) or pick (Target) it is; Target must name a seat whose status is `In`.
    - `set_bot(&mut self, seat: usize, bot: bool)`; `set_limit(&mut self, limit: Option<f32>)` (a person's decision time; default None).
    - `view(&self) -> View`; `version(&self) -> u64` (raised on every change of what `view` shows except `clock`); `winner(&self) -> Option<usize>`.
    - `Table::points(cards: &[Card], loaded: bool) -> u32` — toppings summed, ×2 with a Foot-Long, + sides, + BONUS if loaded.
    - `set_total(&mut self, seat: usize, total: u32)` — for tests and the server's tests (doc it so).
    - `#[cfg(test)] fn stack(&mut self, top: Vec<Card>)` — the next cards drawn, in order (tests only).

- [ ] **Step 1: Write the failing tests** (in `cards.rs`; all with fixed seeds and `stack` where a test needs particular cards):
  - `the_deck_is_94_cards_with_as_many_copies_of_a_topping_as_its_value` — 79 toppings, `Topping(0)` once, `Topping(12)` 12 times; 5 sides + Foot-Long; 3 each of Fridge, PileOn, Napkin.
  - `a_hands_points_double_toppings_with_a_foot_long_then_add_sides_and_the_bonus` — `[Topping(12), Topping(5), FootLong, Side(4)]` → 38; toppings 0..=6 loaded → 36.
  - `the_deal_goes_round_from_the_seat_after_the_dealer_and_the_dealer_moves_each_hand` — first hand's `Dealt` events in seat order 1,2,3,4,0; second hand starts at seat 2.
  - `a_duplicate_topping_busts_and_scores_nothing`.
  - `a_napkin_wipes_one_duplicate_and_both_go_to_the_discards` — status stays `In`, `napkin` false after, event `Saved`.
  - `a_second_napkin_goes_to_another_hotdog_still_in_without_one_else_it_is_discarded` — event `Passed` with `target`.
  - `seven_different_toppings_are_fully_loaded_and_end_the_hand_for_everyone` — +15 for the loaded seat; others keep their points; busts 0.
  - `back_in_the_fridge_banks_the_target_now` — target `Fridged`, its points added at the hand's end.
  - `pile_it_on_flips_three_then_resolves_actions_drawn_meanwhile` — target gets exactly 3 `Flipped` events then the deferred action's pick.
  - `pile_it_on_stops_at_a_bust_and_discards_actions_drawn_meanwhile` (Review Focus 3).
  - `turns_skip_hotdogs_that_stopped_busted_or_were_fridged`.
  - `a_command_out_of_turn_or_at_a_seat_not_in_is_refused` — `Err`, `version` unchanged.
  - `at_100_the_highest_total_wins_and_a_tie_plays_one_more_hand_between_the_tied` — via `set_total`; tie-break hand deals only the tied seats (others `Out`).
  - `a_person_out_of_time_stops_and_a_pick_out_of_time_is_made_for_them` — `set_limit(Some(TURN))`; `tick(15.1)`.
  - `a_bot_that_takes_a_persons_seat_mid_decision_acts_within_its_think_time` (Review Focus 1) — `set_bot(seat, true)` while it is that person's turn; within `THINK[1] + SHOW` seconds the turn has moved on.
  - `when_the_deck_runs_out_the_discards_are_shuffled_in_and_the_table_keeps_its_cards` (Review Focus 2).
  - `a_bot_stops_when_busting_is_likely_and_flips_when_it_is_not` — bot holding toppings whose remaining copies make up > 40 % of the deck stops; with no toppings it flips.
  - `bots_play_whole_games_to_one_winner` — for seeds 1..=40, five bots, `tick(0.1)` until `winner()`; finishes within 3600 simulated seconds; winner's total ≥ 100 and strictly highest.
  - `the_version_changes_with_the_table_not_with_idle_time` — waiting on a person (no limit) for 30 s leaves `version` unchanged.
  - `a_view_round_trips_through_json`.

- [ ] **Step 2: Run them to verify they fail**
  Run: `cargo test --manifest-path crates/kitchen-sim/Cargo.toml cards::`
  Expected: compile errors (module missing).

- [ ] **Step 3: Implement `cards.rs`**
  State: deck `Vec<Card>` (draw from the end), discards, seats, dealer, hand, a job queue (`VecDeque` of `Deal(seat)`, `Draw { seat, pile: bool }`, `Action(seat, Card)`, `Turn(seat)`), the current wait (`Show(f32)`, `Decide { seat, left }`, `Pick { seat, card, left }`, `Think { left }` before a bot's move, `Between(f32)`, `Over`), a `cursor` (the last seat to have had a turn), events, version, own RNG (xorshift64* seeded from `seed | 1`; no new dependency). Rules as the spec says. Notes the tests do not determine:
  - Actions drawn during a Pile It On are inserted after the remaining `Draw { pile: true }` jobs of that seat; an `Action` job whose drawer is no longer `In` discards the card.
  - When the job queue empties: a `Loaded` seat or no seat `In` ends the hand; otherwise push `Turn(next seat In after cursor)`.
  - Reshuffle: when `deck` is empty, move all discards into it and shuffle (Fisher–Yates with the table's RNG).
  - Bot decision, with `risk` = (cards in the deck that are toppings the bot already holds) / deck length:
    ```
    stop if points + total >= TARGET and points > 0
    limit = 0.30 + (0.25 if napkin) - (0.10 if points >= 30)
    flip if risk <= limit else stop
    ```
    Fridge pick: the opponent `In` with the most `total + points`; but itself if it is `In`, its `risk > 0.4` and its points ≥ 20; itself if it is the only one. Pile It On pick: the opponent `In` with the most toppings (ties: higher total); itself if none.
  - A person's decision out of time: Turn → Stop; Pick → the bot's choice.

- [ ] **Step 4: Run the tests to verify they pass**
  Run: `cargo test --manifest-path crates/kitchen-sim/Cargo.toml cards::`
  Expected: all pass. Then the whole crate: `cargo test --manifest-path crates/kitchen-sim/Cargo.toml` — all pass (200 + new).

- [ ] **Step 5: Checkpoint** — no commit (Global Constraints).

### Task 2: Fully Loaded as a rule of the game in `Feast`

**Files:**
- Modify: `crates/kitchen-sim/src/rules.rs` (`Rules`, `from_pairs`, `text`, module doc)
- Modify: `crates/kitchen-sim/src/feast.rs` (`Feast` fields, `set_rules`, `reset`, `step`, new fns)
- Test: `rules.rs` tests module and `feast.rs` tests module

**Interfaces:**
- Consumes: Task 1's `Table`, `Command`, `TURN`.
- Produces:
  - `Rules.cards: bool`; `Rules::parse("mode=cards").cards == true`; `text()` writes `mode=cards`. (`mode=cards` is honoured with or without `sandbox`, like `mode=ffa`.)
  - `Feast.cards: Option<cards::Table>` (None unless `rules.cards`), `Feast.card_seed: u64` (default 0; kept across `reset`).
  - `Feast::cards_command(&mut self, id: u32, command: cards::Command) -> bool` — the seat is the player's index in `players`.
  - `pub fn table_seats(level: &Level, seats: usize) -> Vec<([f32; 3], f32)>` in `feast.rs` — each seat's feet and yaw (facing the table's middle).

- [ ] **Step 1: Write the failing tests**
  - rules.rs `mode_cards_is_read_and_written` — parse/text round trip; `mode=ffa` still gives `ffa` only.
  - feast.rs `a_cards_round_stands_the_hotdogs_round_the_microwave` — `Feast::with_map(Map::bundled(), true)` + `set_rules(Rules { cards: true, ..Default::default() })`: five bodies at y = 0.25 inside the microwave's rect `[-1.46, 3.1, -0.96, 3.43]`, each at least 0.07 m from the others, each facing the rect's centre (yaw within 0.05 rad).
  - feast.rs `a_cards_round_has_no_wave_hunger_or_fight_and_ends_with_a_winner` — demo, tick 1/30 s up to 3600 s: `enemies` and `wave_spawned` stay 0, every `hp` stays at its start, `stage` reaches `RoundOver` with `winner == Some(players[t.winner()].id)`; 8 s later `round` has risen and a fresh table is dealt (`cards` is Some and its `hand` ≤ 1).
  - feast.rs `walking_and_firing_do_nothing_at_the_table` — `walk(id, ..)` returns None and leaves the body; `fire`-style entry points used by the client/server leave `projectiles` empty.
  - feast.rs `a_person_at_the_table_has_no_time_limit_offline_and_fifteen_seconds_when_following` — `follow = true` before `set_rules` → `view().clock == Some(15.)` on a person's turn; otherwise None.

- [ ] **Step 2: Run to verify they fail** — `cargo test --manifest-path crates/kitchen-sim/Cargo.toml cards` → compile errors.

- [ ] **Step 3: Implement**
  - `set_rules`: when `rules.cards`, build `Table::new(players.len(), card_seed ^ (round as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15))`, `set_limit(follow.then_some(TURN))`, mark every player's bot flag into it, and place bodies with `table_seats` (stage stays `Warmup`, alert empty).
  - `table_seats`: the first block of `level.map.blocks` with style `microwave` (fallback: a 0.5 × 0.33 rect centred on the first start, at its ground height): an oval of seats round the top's centre, radii `rx = min(0.17, w/2 - 0.08)`, `rz = min(0.125, d/2 - 0.04)`, five at equal angles from the microwave's front (the front seat first).
  - `step`: right after the `flight` call, when `self.cards.is_some()` and the stage is not `RoundOver`: keep bodies at their seats, copy each player's `bot` into the table, `tick` it, and when it has a winner set `stage = RoundOver`, `winner`, `over_at = elapsed`, `alert = "<NAME> IS FULLY LOADED: <total> POINTS"`; then return. (The existing RoundOver branch does the 8 s wait and `reset`.)
  - `reset`: carry `card_seed`; `set_rules` (already called) rebuilds the table.
  - `walk` and the other player-action entry points: return early when `self.cards.is_some()`.

- [ ] **Step 4: Run** `cargo test --manifest-path crates/kitchen-sim/Cargo.toml` → all pass.
- [ ] **Step 5: Checkpoint.**

### Task 3: The server plays Fully Loaded

**Files:**
- Modify: `crates/server/src/lobby.rs` (`CARDS`, `PLAYS`, `mode_name`, `default_map`, `rules_for`, `choose`, `start`, module doc)
- Modify: `crates/server/src/game.rs` (`with_rules` seed, `cards`, command guard)
- Modify: `crates/server/src/hub.rs` (`receive` branch, broadcast on change, send on join, `status`)
- Modify: `crates/server/src/config.rs` (`--mode cards`)
- Test: `crates/server/src/tests.rs`

**Interfaces:**
- Consumes: `Rules.cards`, `Feast.cards`, `Feast.card_seed`, `Feast::cards_command`, `Table::version/view/set_total/winner`, `cards::Command`.
- Produces:
  - `lobby::CARDS = "cards"`; `PLAYS = ["siege", "forts", FFA, CARDS]`; `mode_name(CARDS) == "Fully Loaded"`; `default_map(CARDS) == "counter-siege"`.
  - `Game::cards(&mut self, seat: usize, command: kitchen_sim::cards::Command) -> bool`.
  - Client → server (game socket): `{type:'cards', action:'flip'|'stop'|'target', seat?:number}`.
  - Server → client (game socket): `{type:'cards', table: View}` — sent to every joined game socket whenever `Table::version` changes (checked on frame steps), and once to a socket when it joins while a table exists.

- [ ] **Step 1: Write the failing tests** (follow the FFA test at tests.rs:1850 and the `Table` helper; the hub tests use `hub_with`/`socket`/`texts`):
  - `the_leader_chooses_fully_loaded_and_it_plays_on_counter_siege` — `choose(Some("cards"), None)` true; roster mode `cards`; after ready + opening the game's map is counter-siege and `feast.cards` is Some; choosing another map while Fully Loaded is chosen is refused with notice "Fully Loaded is played on Counter Siege.".
  - `a_person_flips_and_stops_on_their_own_turn_only` — drive `game.cards(seat, Flip)` when `view().turn == Some(seat)` → true; another seat → false; malformed JSON over a socket (`{type:'cards', action:'dance'}`, `seat: 99`, missing fields) changes nothing and the socket stays open (Review Focus 4).
  - `every_game_socket_is_sent_the_table_when_it_changes` — hub test: a joined socket receives a text `{"type":"cards","table":{...}}` after the round starts, and another after a `flip`.
  - `a_person_who_leaves_is_replaced_by_a_bot_that_keeps_their_score` — `set_total` on their seat, `leave`, table `bot` true and total unchanged.
  - `after_a_winner_everyone_is_back_in_the_fridge` — `set_total(seat, 99)` for one seat, run until `stage() == "Lobby"` (≤ 600 s).
  - `choosing_siege_after_fully_loaded_plays_an_ordinary_siege` — next round: `feast.cards` None, `rules.cards` false (Review Focus 5).
  - `walking_at_the_table_goes_nowhere` — binary/JSON movement commands during a cards round leave the bodies at their seats.
  - config test: `--mode cards` starts the lobby on Fully Loaded.

- [ ] **Step 2: Run to verify they fail** — `cargo test --release --manifest-path crates/server/Cargo.toml` → failures/compile errors in the new tests.

- [ ] **Step 3: Implement** — `rules_for` sets `cards: entry.mode == CARDS`; `choose`/`start` resolve any CARDS choice (mode, Random, or counter-siege by name) to the counter-siege entry `played_as(CARDS)`; `Game::with_rules` sets `feast.card_seed` from `SystemTime` nanos before `set_rules`; `Game::command`/`drive` skip movement and actions while `feast.cards.is_some()`; `Hub` keeps `cards_sent: Option<u64>` (cleared when `lobby.tick()` builds a new game) and broadcasts with `peer.outbox.send_text` like `tuning`; `status` reports mode `cards`.

- [ ] **Step 4: Run** `cargo test --release --manifest-path crates/server/Cargo.toml` → all pass (67 + new).
- [ ] **Step 5: Checkpoint.**

### Task 4: The lobby page and the native lobby name the mode

**Files:**
- Modify: `lobby.js` (`MODES`, `MODE_NAME`, `modeText`, `options`)
- Modify: `src/lobby.rs` (choice naming ~214-220, native "Next" line ~664-667)
- Modify: `devlink.js`, `dev.html` (mode list includes `cards`)
- Test: `tools/lobby.test.mjs`, `tools/devlink.test.mjs`, `src/lobby.rs` tests

**Interfaces:**
- Consumes: roster `mode: 'cards'`, `maps` entries.
- Produces: `MODES` includes `'cards'` after `'ffa'`; `MODE_NAME.cards === 'Fully Loaded'`; `modeText({mode:'cards'})` is the line "Cards at a little table on the microwave: flip toppings, stop before you double up. First to 100."; `options(roster, 'cards')` lists only the counter-siege entry (no Random).

- [ ] **Step 1: Write the failing tests** — lobby.test.mjs: `MODES`/`MODE_NAME`/`modeText`/`options` as above; `nextGame` names "Fully Loaded · Counter Siege". devlink.test.mjs: a `mode=cards` link. src/lobby.rs: the native next line reads `Next: Fully Loaded · Counter Siege`.
- [ ] **Step 2: Run to verify they fail** — `node --test tools/lobby.test.mjs tools/devlink.test.mjs`; `cargo test --release --target x86_64-unknown-linux-gnu --target-dir target-native lobby::` → failures.
- [ ] **Step 3: Implement** the four lobby.js changes, the two `src/lobby.rs` names, and the dev page's mode option.
- [ ] **Step 4: Run** the same commands → pass; plus all node tests in README's Checks list.
- [ ] **Step 5: Checkpoint.**

### Task 5: The client hands the round to the table

**Files:**
- Create: `src/cards.rs` (resource + plumbing only in this task: `CardsPlugin`, `Seen`, `Wishes`)
- Modify: `src/main.rs` (`mod cards;`, add the plugin)
- Modify: `src/arena.rs` (stand the round down at the table: tick early return after `started` when `g.rules.cards`; `update_hud` hides all `Hud`, `ScopeMask`, `FoodAlert`; no `build_viewmodels`; pickups skipped)
- Modify: `src/tags.rs` (gate `place`/`own` off at the table)
- Modify: `src/network.rs` (receive arm `"cards"` → `Seen`; send drains `Wishes` as JSON)
- Modify: `src/runtime.rs` only if `?mode=cards` / `--rules mode=cards` do not already reach `Rules` (they should, via `from_pairs`)
- Test: `src/cards.rs` tests; `src/arena.rs` tests

**Interfaces:**
- Consumes: `Feast.cards`, `Feast::cards_command`, `Rules.cards`, `View`, `Command`.
- Produces (in `src/cards.rs`):
  - `#[derive(Resource, Default)] pub struct Seen { pub view: Option<View>, pub me: Option<usize> }` — what the table looks like now and which seat is this player's (offline: index of player id 1; online: index of the welcome's id in the roster/feast players). Refreshed every frame offline from `g.feast.cards`; online from the latest `cards` message; cleared (`view = None`) whenever `g.rules.cards` is false.
  - `#[derive(Resource, Default)] pub struct Wishes(pub Vec<Command>)` — commands the table view wants sent; offline drained into `g.feast.cards_command(own id, c)`; online drained by `network::send` as `{type:'cards', action, seat}`.
  - `pub fn at_table(g: &arena::Game) -> bool` — `g.rules.cards` and the round is under way (not holding in the lobby).

- [ ] **Step 1: Write the failing tests**
  - cards.rs `seen_follows_the_offline_table_and_clears_when_the_rules_change` (App with the plugin and a minimal Game/Feast as arena's tests build them).
  - cards.rs `wishes_reach_the_offline_table` — a `Flip` wish on this player's turn advances the view.
  - network.rs `a_cards_message_fills_seen` and `wishes_go_out_as_json` (follow the existing native-only network tests).
  - arena.rs `at_the_table_the_round_hud_guns_and_camera_stand_down` and `back_in_a_siege_they_come_back` (Review Focus 5).
- [ ] **Step 2: Run to verify they fail** — `cargo test --release --target x86_64-unknown-linux-gnu --target-dir target-native cards` → failures.
- [ ] **Step 3: Implement** the plumbing above.
- [ ] **Step 4: Run** the full client suite: `cargo test --release --target x86_64-unknown-linux-gnu --target-dir target-native` → all pass (162 + new).
- [ ] **Step 5: Checkpoint.**

### Task 6: The table on the microwave: scene, scoreboard, seat and input

**Files:**
- Modify: `src/cards.rs` (scene, cards, UI, camera, input, reactions)
- Test: `src/cards.rs` tests

**Interfaces:**
- Consumes: Task 5's `Seen`, `Wishes`, `at_table`; `feast::table_seats` (same seat positions as the simulation); `player_model::{PlayerModels, paint, animate, Cues}`; `maya::HotdogCamera`; the roster's names and colours (`g.roster`).
- Produces: `pub fn card_face(card: Card) -> Face` where `struct Face { colour: Color, label: String, value: Option<u8>, glyph: Glyph }` (tested; drives both the 3D card and the UI card); `pub fn seat_in_sight(eye: Vec3, forward: Vec3, seats: &[Vec3], max_angle: f32) -> Option<usize>` (tested).

- [ ] **Step 1: Write the failing tests**
  - `every_card_has_a_face` — all 25 kinds: distinct colour per topping; `label` = `Card::name`.
  - `the_seat_in_sight_is_the_nearest_within_the_angle` — straight at a seat → it; 40° off with `max_angle` 25° → None.
  - `the_scoreboard_rows_follow_the_view` — after a `View` with a bust and a turn: one row per seat in seat order, the turn's row highlighted, "BUST" on the busted row.
  - `name_tags_keep_clear_of_the_scoreboard` — a tag whose spot falls inside the panel's rect is moved below it.
- [ ] **Step 2: Run to verify they fail.**
- [ ] **Step 3: Implement** (systems run after `arena::tick`'s chain; despawn everything when `at_table` turns false):
  - **Table:** oval felt top on one leg on the microwave's top, `rx = 0.11, rz = 0.08`, top 0.075 m up (bigger than the spike's 0.078 m round table, as the user asked).
  - **Cards in 3D:** per seat, its `cards` face up in a row in front of its hotdog (≈1.6 × 2.2 cm), each a white card with the face's colour and its value as seven-segment digits (or the glyph for sides/actions); a Napkin held lies apart; the draw pile in the middle (height from `deck`). Rebuilt when `view.version` changes.
  - **Camera:** this player's seat, eye 0.15 m above the microwave top, pulled back 0.04 m from the seat away from the table, looking at the table's middle; free look with the mouse (pointer lock as the round does) within ±70° yaw and ±35° pitch; fov 70°.
  - **Hotdogs:** the bodies the round already draws stay at their seats; override their yaw to turn smoothly towards the seat whose turn or pick it is; `Cues { cheer: true }` for a `Loaded` seat and the winner; a bust gives a short slump (a `Cues { crouch: true }` for 1 s). This player's body hidden.
  - **UI:** scoreboard top-left (name in its colour, card names, `points / total`, "BUST", "STOPPED", "FRIDGED", "FULLY LOADED"; the turn's row highlighted); name tags over heads clear of the panel; this player's cards big along the bottom; the prompt line: on your turn `[LMB] Flip   [RMB] Stop   12 s`, picking `Pick who gets Pile It On: look at a hotdog and click, or click a name`, otherwise `Relish is thinking…`; between hands each seat's points this hand; Esc frees the pointer so rows can be clicked.
  - **Input:** on this player's turn LMB → `Wishes.push(Flip)`, RMB → `Stop`; picking: LMB with a seat in sight (`seat_in_sight`, 25°) or a click on a scoreboard row → `Target(seat)`; nothing while the chat box is open (the round's existing check).
- [ ] **Step 4: Run** the full client suite → pass. Then look at it: run natively offline (`--skip-intro --rules mode=cards`) with a temporary screenshot hook in a scratch copy (as the spike did) and check the picture: table bigger, five seats, cards readable, nothing covering the name tags.
- [ ] **Step 5: Checkpoint.**

### Task 6b: Friendly hotdog faces (added 2026-11-15 at the user's request)

**Why:** looking at the table screenshots, the user said: "hotdogs look like a part of gang. make friendly looking hotdog faces." What gives the gang look is the painted oval **patch** over each player's face, which reads as a mask, together with the thick, pouting, tube-shaped **lips**. When asked, the user chose:
- **Everywhere:** one friendly face for every hotdog in every mode (the lobby figures, Siege, Forts, Free For All and the card table), not only at the table.
- **The player's colour moves to the ring and the name tag:** the face is the same friendly face for everyone. A player's chosen colour shows on the ring round its waist (the `badge` torus) and on its name tag, not on the face.

**Files:**
- Modify: `src/player_model.rs` (`Face`, `PlayerModels` meshes/materials, `paint`, `speak`, `spawn`'s face parts, tests)
- Modify, only if the colour cannot reach the ring and tag otherwise: `src/tags.rs` (tag colour), `src/lobby.rs:577` and `src/network.rs:416` (the `paint` call sites)
- Test: the `player_model.rs` tests module; tags/lobby tests if those change

**Interfaces:**
- Keep `pub fn paint(world, root, color: &str)` and `pub fn speak(...)` with their current signatures and callers. `paint` now colours the waist ring (and the tag, if the tag reads its colour from the body). It no longer colours the face. An unknown or empty colour, as for a bot, keeps the ring's current per-id colour (`colors[(id-1) % 5]`).
- `PAINTS` is unchanged (`["red","blue","green","yellow","black"]`). The lobby still offers those colours.

**The friendly face** (the look is the implementer's call; these are the requirements):
- **No mask:** the patch and its edge are gone, for everyone.
- **A smile:** a closed mouth whose corners curve up, slimmer than the current lips, in one warm dark colour (a soft dark red-brown, not lipstick red) for everyone. It still opens with speech the way `speak` opens the mouth now: talking must still visibly move it.
- **Eyes that read as friendly:** keep the big white eyes and their shine. Add soft, slightly raised eyebrows (small dark arcs above each eye).
- **Rosy cheeks:** a soft pink oval on the casing below and outside each eye.
- Cartoon-cute rather than menacing. No frown, no heavy outlines, no colour block over the face.
- Cheap: a handful of extra small parts per hotdog that share mesh and material handles (built once in `PlayerModels`), respecting the existing `level(..)` detail choices.

**Steps:**
- [ ] **Step 1: Write the failing tests** in `player_model.rs`:
  - `a_painted_hotdog_has_no_mask_and_wears_its_colour_on_its_ring`: after `paint(world, root, "blue")`, there is no patch entity, or it is hidden or despawned, and the badge's material is the blue paint. `paint(world, root, "")` leaves the badge at the id colour.
  - `the_mouth_smiles`: the mouth's corners sit higher than its middle, by at least 1 mm in the hotdog's local y.
  - `the_face_has_brows_and_cheeks`: two brows above the eyes and two cheeks below and outside them, all on the front of the casing.
  - `talking_still_opens_the_mouth`: after `speak(.., level 1.)` over a few frames, the mouth's dark part is taller than at rest.
  - Update the existing `the_mouth_lies_on_the_face_under_the_eyes_over_the_mustard_and_its_paint_is_one_of_five` to the new face: keep its placement checks and drop the patch checks.
- [ ] **Step 2: Run them to verify they fail:** `cargo test --release --target x86_64-unknown-linux-gnu --target-dir target-native player_model::`
- [ ] **Step 3: Implement** the face, `paint` → ring, and, if needed, the tag colour.
- [ ] **Step 4: Run** the full client suite and the wasm check. Everything passes.
- [ ] **Step 5: Look at it.** In a scratch copy, as Task 6 did, take two screenshots:
  1. a close-up of two or three hotdogs' faces, e.g. the lobby's figures or a photo camera on Counter Siege;
  2. the card table from your seat.

  Save both to `.superpowers/sdd/2026-11-15-fully-loaded/task-6b-*.png`. Read them back and iterate until the faces read as friendly and cute at the table's distance. Leave no screenshot hook in the product code.
- [ ] **Step 6: Checkpoint.** No commit (Global Constraints).

### Task 7: Touch, documentation and the whole check

**Files:**
- Modify: `src/touch.rs` (`state()` reports the table; FIRE = flip, SCOPE = stop at the table), `touch.js` (show only Flip and Stop buttons, so labelled, at the table)
- Modify: `src/cards.rs` (read `touch` FIRE press and SCOPE as flip/stop)
- Modify: `README.md` (a "Fully Loaded" section after Free For All: what it is, the cards table, the rules, offline `?play&mode=cards` / `--rules mode=cards`, `--mode cards`, the `cards` messages; add any new test files to Checks)
- Test: `tools/touch.test.mjs`, `src/touch.rs` tests

- [ ] **Step 1: Write the failing tests** — touch.test.mjs `at_the_table_only_flip_and_stop_show`; touch.rs `the_table_asks_for_flip_and_stop`.
- [ ] **Step 2: Run to verify they fail** — `node --test tools/touch.test.mjs`; `cargo test --release --target x86_64-unknown-linux-gnu --target-dir target-native touch::`.
- [ ] **Step 3: Implement** the touch state/buttons and the README section.
- [ ] **Step 4: Run every check in README's Checks section** and the WebGL build (`tools/build-web.sh` or the README's command): all green, build succeeds.
- [ ] **Step 5: Live check** — rebuild the server, start a second server on a spare port (`--port 8150 --countdown 1 --opening 1 --mode cards`; the user's service on 8137 is left alone), join with the native client (`--connect 127.0.0.1:8150`), ready up, and confirm with `--stats`/log output and a screenshot that the table plays a hand with four bots. Stop the test server.
- [ ] **Step 6: Checkpoint** and report: what changed (`git diff --stat b944548`), test counts, and what was not verified.

### Task 8: The mouse wheel changes weapons (added 2026-11-15 at the user's request)

**Why:** the user asked: "let middle scroll button change weapons in relevant modes (with weapons)". No code uses the mouse wheel today: `grep MouseWheel|AccumulatedMouseScroll|scroll src/` finds nothing outside the editor.

**What:** in a round where you hold guns (Siege, Forts, Free For All, the sandbox), the mouse wheel changes the gun in hand. It changes guns exactly when and how the keys `1`, `2` and `Y` already do (src/arena.rs ~1766-1789):
- **Scroll down:** the next gun.
- **Scroll up:** the previous gun.
- **Order:** the order `Y` cycles in (`held_guns(&g.owned)` in slot order when more than two are held, else between `slots(&g.owned)`'s primary and secondary).
- **Gates:** only under the same conditions as those keys (`alive && !laden && !table`), and only when the keys would act, so not while the chat box is open if the keys are blocked then.
- **On a change:** `g.scoped = false`, as the keys do.
- **Not at the card table:** the `!table` gate covers it.
- **Not elsewhere:** not in the lobby or fridge, and not while dead.

**Files:**
- Modify: `src/arena.rs` (read the wheel next to the weapon keys; a small pure helper for the step logic)
- Modify: `README.md` (the controls list, where `1`, `2` and `Y` are described)
- Test: `src/arena.rs` tests

**Decisions:**
- **Read the wheel.** Use Bevy 0.19's `AccumulatedMouseScroll` resource (or `MessageReader<MouseWheel>`, whichever arena's `tick(world)` reaches cleanly), with its unit:
  - `MouseScrollUnit::Line`: one line is one step.
  - `MouseScrollUnit::Pixel` (trackpads, browsers): accumulate and step once per 50 px, keeping the remainder, so a trackpad's stream of small deltas does not race through the guns. Clear the remainder when the direction flips, or after 0.3 s with no scroll.
  - At most one switch per frame, so a fast flick moves one gun per frame, not several at once.
- **Pure helper.** Make it `fn wheel_step(accumulated: &mut f32, delta: f32, unit_is_pixel: bool) -> i32` (or similar), returning -1, 0 or +1, so it can be tested without an app.
- **Cycling.** With two guns either direction swaps. With more than two, down goes forward and up goes back through `held_guns`, wrapping round.
- **Online.** Nothing new on the wire: `g.weapon` already reaches the server the way the keys' changes do.

**Steps:**
- [ ] **Step 1: Write the failing tests:**
  - `a_wheel_notch_steps_once_and_a_trackpad_needs_a_full_notch`: Line +1 gives +1, Line -1 gives -1; Pixel 10 px five times gives exactly one step; the remainder is kept; a flip of direction clears it.
  - `the_wheel_changes_guns_like_y`: an arena test app holding two guns; scrolling down then up toggles `g.weapon` and clears `g.scoped`; with three sandbox guns, down goes forward and up goes back in `held_guns` order.
  - `the_wheel_does_nothing_at_the_card_table_dead_or_laden`.
- [ ] **Step 2: Run them to verify they fail:** `cargo test --release --target x86_64-unknown-linux-gnu --target-dir target-native wheel`
- [ ] **Step 3: Implement** the helper, the wheel reading and the README line.
- [ ] **Step 4: Run** the full client suite and the wasm compile check. All must pass.
- [ ] **Step 5: Checkpoint.** No commit.
