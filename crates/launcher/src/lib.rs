// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! What a Linux or Windows package's `hotdog-simulator` and `hotdog-server` are (tools/build-linux.sh,
//! build-windows.sh): each starts one of two builds of the real program in `bin/` beside it, `NAME-v3` (compiled for
//! x86-64-v3: AVX2, FMA, BMI2) on a processor that has those, and otherwise `NAME-v2` (x86-64-v2: SSE4.2, SSSE3,
//! POPCNT), with the same arguments, and ends with its exit code. Nobody has to choose; `--cpu v2|v3` on the command
//! line (taken out before the rest is passed on) or `HOTDOG_CPU=v2|v3` in the environment forces one. The choice and
//! why is passed on in `HOTDOG_CPU_CHOSEN`, which the game writes in its log, and said on standard error.
//!
//! On Linux (and any Unix) the launcher becomes the program (exec), so signals, the process id and the exit status
//! are the program's own. On Windows it starts the program and waits for it; the game's launcher is a window program
//! (no console flashes up on a double-click), the server's a console program sharing its console, which lets Ctrl+C
//! reach the server and waits for it to finish.
//!
//! This crate is compiled for plain x86-64 whatever level the programs are, so `is_x86_feature_detected!` asks the
//! processor (in a program compiled with a feature it would answer true unasked).
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Command;

/// The features of x86-64-v2, and those x86-64-v3 adds (as rustc's target-cpu names them).
fn level() -> Option<&'static str> {
    #[cfg(target_arch = "x86_64")]
    {
        use std::arch::is_x86_feature_detected as has;
        let v2 = has!("sse3") && has!("ssse3") && has!("sse4.1") && has!("sse4.2") && has!("popcnt") && has!("cmpxchg16b");
        let v3 = has!("avx") && has!("avx2") && has!("fma") && has!("bmi1") && has!("bmi2") && has!("lzcnt") && has!("movbe") && has!("f16c") && has!("xsave");
        if v2 && v3 {
            return Some("v3");
        }
        if v2 {
            return Some("v2");
        }
    }
    None
}

/// Starts `bin/PROGRAM-v3` or `bin/PROGRAM-v2` (`.exe` on Windows) beside this launcher; never returns.
pub fn launch(program: &str, window: bool) -> ! {
    let mut args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let mut forced = std::env::var("HOTDOG_CPU").ok().filter(|v| !v.is_empty()).map(|v| (v, "HOTDOG_CPU"));
    if let Some(i) = args.iter().position(|a| a == "--cpu") {
        let value = args.get(i + 1).map(|v| v.to_string_lossy().into_owned()).unwrap_or_default();
        args.drain(i..(i + 2).min(args.len()));
        forced = Some((value, "--cpu"));
    }
    let best = level();
    let (level, why) = match &forced {
        Some((value, how)) => match value.as_str() {
            "v2" | "x86-64-v2" => ("v2", format!("as {how} says")),
            "v3" | "x86-64-v3" => ("v3", format!("as {how} says")),
            _ => fail(program, window, &format!("{how} takes v2 or v3, not \"{value}\".")),
        },
        None => match best {
            Some(level) => (level, "the best this processor runs".to_string()),
            None => fail(
                program,
                window,
                "This processor is older than x86-64-v2 (SSE4.2, SSSE3, POPCNT: about 2009), which the game needs. Build it for this one with CPU=baseline (README.md, \"Building and packaging\").",
            ),
        },
    };
    if level == "v3" && best != Some("v3") {
        eprintln!("{program}: warning: this processor lacks x86-64-v3 (AVX2, FMA, BMI2); the v3 build will stop");
    }
    let here = std::env::current_exe().ok().and_then(|exe| exe.parent().map(PathBuf::from)).unwrap_or_default();
    let exe = here.join("bin").join(format!("{program}-{level}{}", std::env::consts::EXE_SUFFIX));
    let chosen = format!("x86-64-{level}, {why}");
    eprintln!("{program}: the x86-64-{level} build ({why})");
    let mut command = Command::new(&exe);
    command.args(&args).env("HOTDOG_CPU_CHOSEN", &chosen);
    run(program, window, command, &exe)
}

#[cfg(unix)]
fn run(program: &str, window: bool, mut command: Command, exe: &std::path::Path) -> ! {
    use std::os::unix::process::CommandExt;
    let error = command.exec();
    fail(program, window, &format!("Cannot start {}: {error}", exe.display()))
}

#[cfg(not(unix))]
fn run(program: &str, window: bool, mut command: Command, exe: &std::path::Path) -> ! {
    #[cfg(windows)]
    if !window {
        // Ctrl+C reaches every program on the console: the server stops on it, and this waits for that.
        unsafe extern "system" {
            fn SetConsoleCtrlHandler(handler: Option<unsafe extern "system" fn(u32) -> i32>, add: i32) -> i32;
        }
        unsafe extern "system" fn wait_for_the_server(_event: u32) -> i32 {
            1
        }
        unsafe { SetConsoleCtrlHandler(Some(wait_for_the_server), 1) };
    }
    match command.status() {
        Ok(status) => std::process::exit(status.code().unwrap_or(1)),
        Err(error) => fail(program, window, &format!("Cannot start {}: {error}", exe.display())),
    }
}

/// Says why on standard error (and, from the game's launcher on Windows, in a message box) and exits with 1.
fn fail(program: &str, window: bool, text: &str) -> ! {
    eprintln!("{program}: {text}");
    #[cfg(windows)]
    if window {
        #[link(name = "user32")]
        unsafe extern "system" {
            fn MessageBoxW(window: *mut std::ffi::c_void, text: *const u16, caption: *const u16, kind: u32) -> i32;
        }
        let wide = |s: &str| s.encode_utf16().chain([0]).collect::<Vec<u16>>();
        const MB_ICONERROR: u32 = 0x10;
        unsafe { MessageBoxW(std::ptr::null_mut(), wide(text).as_ptr(), wide("Hotdog Simulator").as_ptr(), MB_ICONERROR) };
    }
    let _ = window;
    std::process::exit(1)
}
