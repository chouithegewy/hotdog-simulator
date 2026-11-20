# Fully Loaded: a hotdog card game at a table on the microwave

Status: approved in conversation 2026-11-15 ("build it"). Calls marked *(Claude's call)* were not discussed and can be changed.

## Intent

A turn-based press-your-luck card mini-game inside Hotdog Simulator: the players' own hotdogs stand round a little
card table on top of the microwave on Counter Siege and flip topping cards. It is an original game in the same genre
as the push-your-luck card game the user pointed to (keep flipping or stop and bank; a duplicate busts you): it
borrows the mechanics only. Its name, card names, art, look and rules text are this project's own; nothing from that
game's name, art or rulebook is used.

## Decisions (the user's)

- **A mode of its own.** The lobby leader picks it from the mode list, beside Siege, Forts and Free For All. When the
  fridge opens, everyone goes straight to the table. **No cats**, no guns, no hunger, no HP.
- **Where:** a little card table on top of the microwave on **Counter Siege**. The table should be **bigger** than the
  spike's (the spike used a 7.8 cm radius; see Look).
- **Who:** the lobby's real players; bots fill empty seats, as in a round.
- **Cards:** condiments and toppings. Each topping is worth points. The same topping twice ruins your hotdog (bust,
  nothing that hand). Seven different toppings is **fully loaded**: +15, and the hand ends.
- **Special cards like the original's**, renamed (below).
- **First to 100** wins.
- **View C:** first person from your seat, your cards big in front of you, a scoreboard panel in the corner with
  everyone's cards and points. **Real 3D hotdogs** (the game's player models), not stand-ins.
- Played back and approved: leader picks it in the fridge; five seats; no guns/hunger/HP; the server deals and scores
  online; it also works offline with bots; back to the fridge after a game; each player's cards lie face up in 3D in
  front of their hotdog; free look from a fixed seat; turns go round the table, the current player highlighted, the
  other hotdogs turn to look at them; faces react to bust / fully loaded; LMB flip, RMB stop, two touch buttons; pick
  a special card's target by looking at a hotdog and clicking, or clicking its scoreboard row; chat and voice work at
  the table.
- Played back and approved ("build it"): deal one card each to start; then flip or stop in turn until everyone has
  stopped or bust, or someone is fully loaded (ends the hand for all; the others keep what they have; a bust scores
  nothing); the dealer moves one seat each hand; at 100 the hand is finished and the highest total wins (a tie: one
  more hand between the tied); the winner shown ~8 s then the fridge; 15 s turn timer online (then you stop); bots take
  a second or two and play sensibly (stop when busting gets likely; aim Back in the Fridge and Pile It On at the
  leader); someone leaving mid-game is replaced by a bot that keeps their score; late arrivals wait in the fridge.

## The cards (94)

**Toppings (79)** — a topping's value is also how many copies of it are in the deck, so the valuable ones are common
and therefore risky *(Claude's call: names and order)*:

| Value | Topping | | Value | Topping |
|---|---|---|---|---|
| 0 | Plain Bun (1 copy) | | 7 | Sauerkraut |
| 1 | Celery Salt | | 8 | Pickles |
| 2 | Sport Peppers | | 9 | Onions |
| 3 | Jalapeños | | 10 | Relish |
| 4 | Bacon | | 11 | Mustard |
| 5 | Cheese | | 12 | Ketchup |
| 6 | Chili | | | |

**Sides (6)** — never bust you: Fries +2, Chips +4, Pickle Spear +6, Soda +8, Onion Rings +10, and **Foot-Long**
(doubles your topping points this hand). One of each.

**Action cards (9)**, three of each:
- **Back in the Fridge** — the hotdog you pick (any hotdog still in the hand, yourself included) stops now and banks.
- **Pile It On** — the hotdog you pick must flip three cards, one at a time (stopping early on a bust or fully
  loaded). Action cards it flips meanwhile are dealt with after the three, if it is still in; a Napkin it flips is
  kept at once.
- **Napkin** — keep it; it wipes away one duplicate topping (the duplicate and the Napkin are discarded) so you don't
  bust. You hold at most one: a second goes to another hotdog still in the hand that has none, else it is discarded.

## Rules

- **Seats:** five, in seat order round the table. A hand involves every seat; a tie-break hand only the tied seats.
- **Deck:** shuffled at the start of a game. Cards played go to a discard pile at the end of each hand; when the deck
  runs out the discards are shuffled into a new deck.
- **A hand:** the dealer button moves one seat each hand (seat 0 deals the first). The deal goes round from the seat
  after the dealer, one card face up each. An action card dealt is resolved at once (its drawer picks the target).
  Then turns go round from the seat after the dealer: the hotdog whose turn it is **flips** (takes one card) or
  **stops** (banks). Stopped, busted and fridged hotdogs are out of the hand and skipped. A hotdog that has had its
  turn with cards and is still in plays again when the turn comes round.
- **Bust:** a topping already in front of you, without a Napkin, busts you: you score nothing this hand.
- **Fully loaded:** seven different toppings (Plain Bun counts as one) ends the hand at once for everyone.
- **Hand score:** toppings' values summed, ×2 with a Foot-Long, plus sides, plus 15 if fully loaded. A bust scores 0.
- **End of the game:** after a hand, if any total is 100 or more, the highest total wins; a tie at the top plays one
  more hand among the tied (repeat until one wins).
- **Picking a target:** whoever drew the action picks among the hotdogs still in the hand (itself included; if it is
  the only one, it is the target).

## Pacing *(Claude's call)*

The table runs on its own clock (server-side online, locally offline): a dealt or flipped card shows for 0.7 s before
play moves on; a person has 15 s for a turn (then they stop) and 15 s to pick a target (then the bots' choice is made
for them); a bot thinks 0.8–1.6 s; the scores between hands show 3 s; the winner 8 s, then back to the fridge.

## Bots *(Claude's call)*

A bot stops when its chance of busting on the next flip (copies left of the toppings it holds ÷ cards left, as it can
count them from the table and discards) exceeds a threshold that rises with a Napkin and falls with its hand's points,
or when its hand alone would win the game. Back in the Fridge goes to the opponent still in the hand with the most at
stake (hand points + total), unless the bot itself is the one most at risk of busting and has a good hand, when it
fridges itself. Pile It On goes to the opponent still in with the most toppings (most likely to bust), preferring
the leader.

## Online

The server runs the table instead of the kitchen simulation when the mode is Fully Loaded, keeps the lobby stages and
the 9 s fridge-door start, seats people and bots as in a round (a person who leaves is replaced by a bot keeping their
score; arrivals during a game wait in the fridge), and sends the table's state to every client; clients send `flip`,
`stop` and `target` commands. Only the hotdog whose turn (or choice) it is may act; the server refuses the rest.
*(Claude's call: the table state goes as JSON text messages on the game socket, sent when it changes; the binary
frame format, protocol 12, is untouched.)*

## Look

- Counter Siege's microwave top is 0.50 × 0.33 m at 0.25 m. *(Claude's call:)* table radius ~0.10 m, top ~0.075 m
  up, on one leg; the hotdogs stand at ~0.15 m from its middle, spread in an arc so the two beside you are not right at
  your shoulders (the spike showed evenly spaced seats put them at the screen's edges). No sitting pose: they stand.
- Cards are small 3D cards face up in front of each hotdog with printed faces (topping name and value, side, action);
  the draw pile is in the middle.
- The scoreboard (top-left): each hotdog's name in its colour, its cards this hand, hand points and total; the current
  player highlighted; busts and stops marked. It must not cover name tags (the spike's did).
- Your cards also big along the bottom; the prompt: flip / stop, and the turn timer.
- No gun, no HP, no round HUD, no cats.
- **Friendly faces, everywhere** (the user, 2026-11-15, after the first screenshots: "hotdogs look like a part of
  gang. make friendly looking hotdog faces"): every hotdog in every mode gets one friendly face: a smile, bright eyes,
  soft brows, rosy cheeks, and no painted mask. A player's chosen colour moves to the ring round its waist and its name
  tag.

## Non-goals

- No betting, no chips, no other card games.
- No new map; the table is only on Counter Siege.
- No sitting animation in this version.

## Anti-goals

- Anything recognisably the original game's name, card art, layout or rulebook wording.
- A card game that blocks or breaks Siege, Forts or Free For All, the lobby, or protocol 12 clients.

## Left to the builder

Module layout, message field names, the card-face rendering technique, sounds, exactly how the camera's free look is
limited, and tests (rules tests in the simulation crate, server tests for the mode, the lobby page's tests).
