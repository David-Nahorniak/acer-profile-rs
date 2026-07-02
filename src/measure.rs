//! Subcommand `measure`: srovnání reálných rozdílů mezi profily.
//!
//! Pro každý profil: set, snapshot (idle), 15s stress-ng CPU + vzorkování
//! scaling_cur_freq (max, průměr), snapshot po zátěži, 20s cooldown.
//! Lunar Lake nemá RAPL energy counter -> měříme CPU frekvenci + teplotu.

use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use crate::controller;
use crate::levers;
use crate::probe::read_package_temp;

const PROFILES: [&str; 3] = ["eco", "normal", "performance"];
const STRESS_SECS: u64 = 15;
const SAMPLE_SECS: u64 = 14;
const COOLDOWN_SECS: u64 = 20;

fn read_policy0(path: &str) -> String {
    std::fs::read_to_string(path)
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "-".to_string())
}

fn snapshot(label: &str) {
    let now = chrono_like();
    println!("--- [{}] {} ---", label, now);
    let cur = controller::current_profile().unwrap_or_else(|| "(žádný)".to_string());
    println!("profil   = {}", cur);
    println!(
        "EPP      = {}",
        read_policy0("/sys/devices/system/cpu/cpufreq/policy0/energy_performance_preference")
    );
    println!(
        "gov      = {}",
        read_policy0("/sys/devices/system/cpu/cpufreq/policy0/scaling_governor")
    );
    println!(
        "PL1      = {}",
        read_policy0("/sys/class/powercap/intel-rapl:0/constraint_0_power_limit_uw")
    );
    println!(
        "PL2      = {}",
        read_policy0("/sys/class/powercap/intel-rapl:0/constraint_1_power_limit_uw")
    );
    if let Some(d) = levers::i915_card_dir() {
        let max = read_policy0(&format!("{}/gt_max_freq_mhz", d.display()));
        let boost = read_policy0(&format!("{}/gt_boost_freq_mhz", d.display()));
        println!("GPU max  = {} MHz, boost={} MHz", max, boost);
    }
    let t = read_package_temp()
        .map(|x| x.to_string())
        .unwrap_or_else(|| "-".to_string());
    println!("teplota  = {}°C", t);
}

fn chrono_like() -> String {
    // RFC3339-like bez externí crate: použijeme `date` pro shodu s původním skriptem.
    Command::new("date")
        .arg("-Is")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "?".to_string())
}

fn sample_freq(dur_secs: u64) -> i64 {
    let end = Instant::now() + Duration::from_secs(dur_secs);
    let mut sum: i64 = 0;
    let mut n: i64 = 0;
    while Instant::now() < end {
        let mx = levers::cpu_max_freq_khz().unwrap_or(0);
        sum += mx;
        n += 1;
        thread::sleep(Duration::from_millis(500));
    }
    if n > 0 {
        sum / n / 1000
    } else {
        0
    }
}

fn nproc() -> String {
    Command::new("nproc")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "1".to_string())
}

pub fn run() {
    println!("=== daemon ===");
    let active = Command::new("systemctl")
        .args(["is-active", "acer-profile"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    println!(
        "{}",
        if active {
            "daemon aktivní"
        } else {
            "daemon NEaktivní"
        }
    );

    for p in PROFILES {
        measure(p);
    }

    println!();
    println!(">>> návrat na normal");
    controller::set_profile("normal");
    println!();
    println!("Hotovo. Porovnej prům. CPU freq a PL1/PL2 mezi profily:");
    println!("  eco (~20W, nízká frekv) < normal (~35W) < performance (~45W+, vyšší frekv).");
}

fn measure(prof: &str) {
    println!();
    println!(">>> acer-profile set {}", prof);
    controller::set_profile(prof);
    thread::sleep(Duration::from_secs(2));
    snapshot(&format!("{} (idle)", prof));
    println!(
        ">>> zátěž {}s (stress-ng CPU), vzorkuji CPU freq...",
        STRESS_SECS
    );

    let np = nproc();
    let mut stress = Command::new("stress-ng")
        .args(["--cpu", &np, "--timeout", &format!("{}s", STRESS_SECS)])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("stress-ng není nainstalován");

    let avg = sample_freq(SAMPLE_SECS);
    let _ = stress.wait();
    println!("  prům. max CPU freq pod zátěží: {} MHz", avg);

    thread::sleep(Duration::from_secs(1));
    snapshot(&format!("{} (po zátěži)", prof));
    println!(">>> cooldown {}s před dalším profilem...", COOLDOWN_SECS);
    thread::sleep(Duration::from_secs(COOLDOWN_SECS));
}
