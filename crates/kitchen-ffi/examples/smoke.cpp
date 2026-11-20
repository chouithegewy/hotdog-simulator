// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//
// smoke.cpp: proof that kitchen_sim.h and libkitchen_ffi.a work from C++. It plays a 600-step bot match of Kung Fu on
// the dojo and of Free For All on Counter Siege through the C interface alone, and prints where the hotdogs are, some of
// the events and the score. Built and run by crates/kitchen-ffi/examples/run-smoke.sh.
//
//   smoke [REPO_ROOT]     (the root holds assets/maps/*.json; default ".")
#include "kitchen_sim.h"

#include <algorithm>
#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <map>
#include <sstream>
#include <string>
#include <vector>

static KsStr str(const std::string& s) { return KsStr{reinterpret_cast<const uint8_t*>(s.data()), s.size()}; }
static std::string text(KsStr s) { return s.ptr ? std::string(reinterpret_cast<const char*>(s.ptr), s.len) : std::string(); }

#define CHECK(call)                                                                                    \
    do {                                                                                               \
        KsStatus status_ = (call);                                                                     \
        if (status_ != KS_OK) {                                                                        \
            std::fprintf(stderr, "%s failed (%d): %s\n", #call, (int)status_, text(ks_last_error()).c_str()); \
            std::exit(1);                                                                              \
        }                                                                                              \
    } while (0)

static std::string read_file(const std::string& path) {
    std::ifstream in(path, std::ios::binary);
    if (!in) {
        std::fprintf(stderr, "cannot read %s\n", path.c_str());
        std::exit(1);
    }
    std::ostringstream s;
    s << in.rdbuf();
    return s.str();
}

static const char* event_name(uint32_t kind) {
    switch (kind) {
        case KS_EVENT_SHOT: return "shot";
        case KS_EVENT_SPLAT: return "splat";
        case KS_EVENT_KNOCK: return "knock";
        case KS_EVENT_DAMAGE: return "damage";
        case KS_EVENT_KILL: return "kill";
        case KS_EVENT_SPAWN: return "spawn";
        case KS_EVENT_STAGE: return "stage";
        case KS_EVENT_ROUND: return "round";
        case KS_EVENT_WIN: return "win";
        case KS_EVENT_GRIDDLE: return "griddle";
        case KS_EVENT_MAP: return "map";
        case KS_EVENT_FU_MOVE: return "fu-move";
        case KS_EVENT_FU_HIT: return "fu-hit";
        case KS_EVENT_FU_BLOCKED: return "fu-blocked";
        case KS_EVENT_FU_PARRIED: return "fu-parried";
        case KS_EVENT_FU_GUARDBROKEN: return "fu-guardbroken";
        case KS_EVENT_FU_GRABBED: return "fu-grabbed";
        case KS_EVENT_FU_KNOCKDOWN: return "fu-knockdown";
        case KS_EVENT_FU_KO: return "fu-ko";
        case KS_EVENT_FU_DODGED: return "fu-dodged";
        case KS_EVENT_FU_ABSORBED: return "fu-absorbed";
        case KS_EVENT_FU_SPECIAL: return "fu-special";
        case KS_EVENT_FU_CHI_FULL: return "fu-chi-full";
        case KS_EVENT_FU_STYLE: return "fu-style";
        default: return "?";
    }
}

static std::vector<KsPlayer> players(KsGame* game) {
    uint32_t n = 0;
    CHECK(ks_game_players(game, nullptr, 0, &n));
    std::vector<KsPlayer> out(n);
    CHECK(ks_game_players(game, out.data(), n, &n));
    return out;
}

static void print_players(KsGame* game, const char* when) {
    std::printf("  %s:\n", when);
    for (const KsPlayer& p : players(game)) {
        std::printf("    #%u %-8s at (%7.3f, %6.3f, %7.3f) m  yaw %6.2f  hp %5.1f  %s%s\n", p.id, text(p.name).c_str(), p.p[0], p.p[1], p.p[2], p.yaw, p.hp,
                    (p.flags & KS_PLAYER_ALIVE) ? "alive" : "down", (p.flags & KS_PLAYER_BOT) ? " bot" : "");
    }
}

static void run(const char* title, const std::string& map_json, const char* rules) {
    KsGame* game = nullptr;
    CHECK(ks_game_create(str(map_json), str(rules), 42, KS_GAME_ALL_BOTS, &game));
    KsMapInfo info;
    CHECK(ks_map_info(game, &info));
    KsMatch m;
    CHECK(ks_game_match(game, &m));
    std::printf("== %s on \"%s\" (mode %s, %u blocks, %u decks, %u ramps, %u loose objects), rules \"%s\", stage %s\n", title, text(info.name).c_str(), text(info.mode).c_str(), info.blocks,
                info.decks, info.ramps, info.loose, text(m.rules).c_str(), text(ks_stage_name(m.stage)).c_str());
    print_players(game, "start");

    std::vector<KsEvent> batch(64);
    std::map<uint32_t, int> counts;
    std::vector<KsEvent> sample;
    for (int step = 0; step < 600; step++) {
        KsCommand c{};
        c.pickup = -1;
        CHECK(ks_game_set_command(game, &c));  // (ignored: every hotdog is a bot)
        CHECK(ks_game_step(game, 1.0f / 60.0f));
        uint32_t n = 0;
        do {
            CHECK(ks_game_events(game, batch.data(), (uint32_t)batch.size(), &n));
            for (uint32_t i = 0; i < n; i++) {
                const KsEvent& e = batch[i];
                counts[e.kind]++;
                bool interesting = e.kind == KS_EVENT_KILL || e.kind == KS_EVENT_FU_KO || e.kind == KS_EVENT_FU_HIT || e.kind == KS_EVENT_DAMAGE || e.kind == KS_EVENT_FU_PARRIED;
                if (interesting && sample.size() < 8) sample.push_back(e);
            }
        } while (n == batch.size());
        if (step == 299) print_players(game, "after 300 steps (5 s)");
    }
    print_players(game, "after 600 steps (10 s)");

    std::printf("  events by kind:");
    for (auto& [kind, count] : counts) std::printf(" %s=%d", event_name(kind), count);
    std::printf("\n  a few of them:\n");
    for (const KsEvent& e : sample) {
        std::printf("    t=%6.3f %-12s by #%u to #%u  amount %5.1f  move/weapon %u  sound \"%s\"\n", e.time, event_name(e.kind), e.by, e.to, e.amount, e.weapon,
                    text(ks_sound_file(e.sound)).c_str());
    }

    KsKungfu fu;
    if (ks_kungfu_state(game, &fu) == KS_OK) {
        std::vector<KsFighter> fighters(fu.fighters);
        uint32_t n = 0;
        CHECK(ks_kungfu_fighters(game, fighters.data(), (uint32_t)fighters.size(), &n));
        std::printf("  fighters at %.3f s:\n", fu.clock);
        for (const KsFighter& f : fighters) {
            KsMove mv;
            CHECK(ks_kungfu_move(f.mv, &mv));
            std::printf("    #%u %-7s move %2u %-22s t %.3f/%.3f phase %u chi %5.1f guard %5.1f%s%s\n", f.id, text(ks_kungfu_style_name(f.style)).c_str(), f.mv, text(mv.name).c_str(), f.t,
                        f.len, f.phase, f.chi, f.guard, (f.flags & KS_FIGHTER_IRON) ? " iron" : "", (f.flags & KS_FIGHTER_DOWN) ? " down" : "");
        }
    }

    CHECK(ks_game_match(game, &m));
    std::vector<KsPlayer> board = players(game);
    std::sort(board.begin(), board.end(), [](const KsPlayer& a, const KsPlayer& b) { return a.kills != b.kills ? a.kills > b.kills : a.deaths < b.deaths; });
    std::printf("  final score at %.2f s (%s, goal %u, %.0f s left, winner %s):", m.elapsed, text(ks_stage_name(m.stage)).c_str(), m.goal, m.time_left,
                m.winner ? ("#" + std::to_string(m.winner)).c_str() : "none yet");
    for (const KsPlayer& p : board) std::printf("  %s %u/%u (%u pts)", text(p.name).c_str(), p.kills, p.deaths, p.score);
    std::printf("\n");
    ks_game_destroy(game);
}

int main(int argc, char** argv) {
    std::string root = argc > 1 ? argv[1] : ".";
    KsVersion v = ks_version();
    std::printf("kitchen-ffi %s, ABI %u (header %u), protocol %u, %u steps / %u frames a second\n", text(v.version).c_str(), v.abi, KS_ABI_VERSION, v.protocol, v.tick_hz, v.send_hz);
    if (v.abi != KS_ABI_VERSION) return 1;

    // Errors come back as codes and a reason, never as a crash.
    KsGame* bad = nullptr;
    KsStatus status = ks_game_create(str("this is not a map"), str(""), 1, 0, &bad);
    std::printf("a bad map: status %d, \"%s\", handle %p\n", (int)status, text(ks_last_error()).c_str(), (void*)bad);
    KsMatch nothing;
    status = ks_game_match(nullptr, &nothing);
    std::printf("a null handle: status %d, \"%s\"\n", (int)status, text(ks_last_error()).c_str());

    run("Kung Fu", read_file(root + "/assets/maps/kung-fu-dojo.json"), "");
    run("Free For All", read_file(root + "/assets/maps/counter-siege.json"), "mode=ffa");

    KsMove mv;
    CHECK(ks_kungfu_move(24, &mv));
    std::printf("move table: %u moves; #24 %s \"%s\": %.3f + %.3f + %.3f s, %.1f HP, reach %.2f m\n", ks_kungfu_move_count(), text(mv.key).c_str(), text(mv.name).c_str(), mv.startup,
                mv.active, mv.recovery, mv.damage, mv.reach);
    return 0;
}
