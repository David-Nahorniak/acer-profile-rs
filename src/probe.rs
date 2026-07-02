//! Subcommand `probe-pl2`: BEZPEČNÁ inkrementální detekce stropu PL2.
//!
//! Bezpečnostní principy (shodné s původním probe-pl2.sh):
//!   1. Nikdy nepřesáhne Intel MTP (115W) - spec čipu, ne overclock.
//!   2. Inkrementální kroky (64->80->100->115), ne skok na maximum.
//!   3. Krátké zátěže (8s) s teplotním limitem: Package >95°C -> ZASTAVÍ.
//!   4. Mezi pokusy 20s cooldown.
//!   5. Po skriptu návrat na normal profil.

use std::fs;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::controller;

const PKG: &str = "/sys/class/powercap/intel-rapl:0";
const MAXTEMP: i64 = 95;
const STEPS: [i64; 4] = [64, 80, 100, 115];

fn read_int(path: &str) -> Option<i64> {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse::<i64>().ok())
}

pub fn read_package_temp() -> Option<i64> {
    // sensors výstup: `Package id 0:  +XX.X°C (high = ...)` -> první float, celá část.
    let out = Command::new("sensors").output().ok()?;
    let txt = String::from_utf8_lossy(&out.stdout);
    for line in txt.lines() {
        if line.contains("Package") {
            // najdi první číslo ve tvaru N.N nebo -N.N
            let bytes = line.as_bytes();
            let mut i = 0;
            while i < bytes.len() {
                let b = bytes[i];
                if (b == b'+' || b == b'-') && i + 1 < bytes.len() && bytes[i + 1].is_ascii_digit()
                {
                    let mut j = i + 1;
                    while j < bytes.len() && (bytes[j].is_ascii_digit() || bytes[j] == b'.') {
                        j += 1;
                    }
                    let num: String = bytes[i + 1..j].iter().map(|&c| c as char).collect();
                    if let Some(int_part) = num.split('.').next() {
                        if let Ok(v) = int_part.parse::<i64>() {
                            return Some(v);
                        }
                    }
                    i = j;
                    continue;
                }
                i += 1;
            }
        }
    }
    None
}

fn read_pl1() -> Option<i64> {
    read_int(&format!("{}/constraint_0_power_limit_uw", PKG))
}
fn read_pl2() -> Option<i64> {
    read_int(&format!("{}/constraint_1_power_limit_uw", PKG))
}
fn write_pl2(uw: i64) {
    let p = format!("{}/constraint_1_power_limit_uw", PKG);
    if let Err(e) = fs::write(&p, format!("{}\n", uw)) {
        log::warn!("write PL2 selhal: {}", e);
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
    println!("=== probe-pl2 (bezpečná verze, MAXTEMP={}°C) ===", MAXTEMP);
    println!("Intel 185H MTP=115W. Testuji inkrementálně: {:?}", STEPS);
    println!();

    println!("[0] baseline: performance profil");
    controller::set_profile("performance");
    println!(
        "    PL1={} PL2={} teplota={}°C",
        read_pl1().unwrap_or(0),
        read_pl2().unwrap_or(0),
        read_package_temp()
            .map(|x| x.to_string())
            .unwrap_or_else(|| "-".to_string())
    );
    println!();

    let mut best_w: i64 = 64;

    for &w in &STEPS {
        let t = read_package_temp();
        if let Some(ct) = t {
            if ct > MAXTEMP {
                println!(
                    "!!! teplota {}°C > {}°C - ZASTAVUJI probe (ochrana)",
                    ct, MAXTEMP
                );
                break;
            }
        }

        println!(">>> test PL2={}W (předchozí OK: {}W)", w, best_w);
        write_pl2(w * 1_000_000);
        let got = read_pl2().unwrap_or(0);
        let got_w = got / 1_000_000;

        if got_w != w {
            println!(
                "    VRM/EC tunul zápis: požadováno {}W, přijato {}W",
                w, got_w
            );
            println!("    -> reálný strop je ~{}W. Končím.", got_w);
            break;
        }
        println!("    zápis přijat: {}W", got_w);

        println!("    zátěž 8s + teplotní dohled...");
        let np = nproc();
        let mut stress = Command::new("stress-ng")
            .args(["--cpu", &np, "--timeout", "8s"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("stress-ng není nainstalován");

        let end = Instant::now() + Duration::from_secs(7);
        let mut mx: i64 = 0;
        let mut overheat = false;
        while Instant::now() < end {
            if let Some(v) = crate::levers::cpu_max_freq_khz() {
                if v > mx {
                    mx = v;
                }
            }
            if let Some(ct) = read_package_temp() {
                if ct > MAXTEMP {
                    overheat = true;
                    println!("    !!! {}°C > {}°C pod zátěží - ruším zátěž", ct, MAXTEMP);
                    let _ = stress.kill();
                    break;
                }
            }
            thread::sleep(Duration::from_millis(500));
        }
        let _ = stress.wait();
        let ct = read_package_temp()
            .map(|x| x.to_string())
            .unwrap_or_else(|| "-".to_string());
        println!("    max freq={}MHz  teplota={}°C", mx / 1000, ct);

        if overheat {
            println!("    -> {}W přehřívá. Bezpečný strop = {}W.", w, best_w);
            break;
        }
        best_w = w;
        println!("    OK, cooldown 20s...");
        thread::sleep(Duration::from_secs(20));
    }

    println!();
    println!("=== výsledek ===");
    println!("Bezpečný PL2 (žádný throttle <{}°C): {}W", MAXTEMP, best_w);
    println!();
    println!("=== návrat na normal ===");
    controller::set_profile("normal");
    println!(
        "PL1={} PL2={}",
        read_pl1().unwrap_or(0),
        read_pl2().unwrap_or(0)
    );
    println!();
    println!("Doporučení: nastav v /etc/acer-profile/profiles.toml:");
    println!("  [performance]");
    println!("  pl2_uw = {}", best_w * 1_000_000);
    println!("Pak: sudo systemctl restart acer-profile");
}
