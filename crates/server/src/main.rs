// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! `hotdog-server`: see config.rs for the command line and lib.rs for what it serves.
fn main() {
    check_cpu();
    serve();
}
/// Stops first thing, with a message instead of an illegal instruction, on a processor without the x86-64 level the build
/// was compiled for (tools/build-linux.sh and build-windows.sh, `CPU=v2` or `v3`), as the game does (src/runtime.rs,
/// check_cpu, which says why cpuid is read directly). Plain integer code, so that it runs on any x86-64.
#[cfg(target_arch = "x86_64")]
#[inline(never)]
fn check_cpu() {
    use std::arch::x86_64::{__cpuid, __cpuid_count};
    let (v2, v3) = (cfg!(target_feature = "sse4.2"), cfg!(target_feature = "avx2"));
    if !v2 && !v3 {
        return;
    }
    const V2: u32 = 1 | 1 << 9 | 1 << 13 | 1 << 19 | 1 << 20 | 1 << 23;
    const V3: u32 = V2 | 1 << 12 | 1 << 22 | 1 << 26 | 1 << 27 | 1 << 28 | 1 << 29;
    const V3_LEAF7: u32 = 1 << 3 | 1 << 5 | 1 << 8;
    #[allow(unused_unsafe)]
    let ok = unsafe {
        let (max, extended, ecx) = (__cpuid(0).eax, __cpuid(0x8000_0000).eax, __cpuid(1).ecx);
        let lahf = extended >= 0x8000_0001 && __cpuid(0x8000_0001).ecx & 1 != 0;
        if v3 {
            ecx & V3 == V3 && lahf && max >= 7 && __cpuid_count(7, 0).ebx & V3_LEAF7 == V3_LEAF7 && __cpuid(0x8000_0001).ecx & 1 << 5 != 0 && {
                let xcr0: u32;
                std::arch::asm!("xgetbv", in("ecx") 0, out("eax") xcr0, out("edx") _, options(nomem, nostack, preserves_flags));
                xcr0 & 6 == 6
            }
        } else {
            ecx & V2 == V2 && lahf
        }
    };
    if !ok {
        cpu_fail(v3);
    }
}
#[cfg(not(target_arch = "x86_64"))]
fn check_cpu() {}
#[cfg(target_arch = "x86_64")]
#[cold]
#[inline(never)]
fn cpu_fail(v3: bool) -> ! {
    use std::io::Write;
    let _ = std::io::stderr().write_all(if v3 {
        b"This build of hotdog-server is for x86-64-v3 processors (AVX2, FMA and BMI2: Intel from 2013, AMD from 2015), which this one is not: start hotdog-server beside the bin folder, which picks the build this processor runs.\n"
    } else {
        b"This build of hotdog-server is for x86-64-v2 processors (SSE4.2, SSSE3 and POPCNT: from about 2009), which this one is not: build it with CPU=baseline.\n"
    });
    std::process::exit(1);
}
#[tokio::main]
async fn serve() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let config = match hotdog_server::config::parse(&args) {
        Ok(config) => config,
        Err(reason) => {
            eprintln!("{reason}");
            std::process::exit(1);
        }
    };
    let port = config.port;
    let running = match hotdog_server::start(config, true).await {
        Ok(running) => running,
        Err(error) => {
            eprintln!("{}", if error.kind() == std::io::ErrorKind::AddrInUse { format!("Port {port} is already in use; pick another with --port") } else { error.to_string() });
            std::process::exit(1);
        }
    };
    tokio::select! {
        _ = running.task => {}
        _ = tokio::signal::ctrl_c() => {}
        _ = terminate() => {}
    }
}
/// systemd stops the service with SIGTERM.
async fn terminate() {
    #[cfg(unix)]
    if let Ok(mut signal) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
        signal.recv().await;
        return;
    }
    std::future::pending::<()>().await
}
