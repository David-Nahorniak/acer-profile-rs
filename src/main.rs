//! acer-profile: řízení výkonnostních profilů Acer Swift SFG14-73 na Linuxu.
//!
//! Jeden binár s subcommandy: status / list / set / watch / measure / probe-pl2
//! / daemon. Nahrazuje power-profiles-daemon (rozbitý EBUSY) na DBus system bus.

mod controller;
mod daemon;
mod dbus;
mod levers;
mod measure;
mod probe;
mod profiles;

use std::thread;
use std::time::Duration;

use clap::{Parser, Subcommand};

use controller::VALID;
use profiles::{alias, load_profiles, PROFILE_ALIASES_LIST};

#[derive(Parser)]
#[command(
    name = "acer-profile",
    version,
    about = "Řízení výkonnostních profilů Acer Swift SFG14-73 (RAPL + EPP + i915 GPU) + DBus PPD náhrada"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Aktuální stav hardwarových pák
    Status,
    /// Seznam profilů
    List,
    /// Nastavit a aplikovat profil (eco/normal/performance + aliasy)
    Set { profile: String },
    /// Sledovat změny profilu (foreground)
    Watch {
        #[arg(short, long, default_value_t = 1.0)]
        interval: f64,
    },
    /// Srovnání profilů: zátěž + vzorkování CPU freq/teploty (vyžaduje root)
    Measure,
    /// Detekce stropu PL2 s teplotní ochranou (vyžaduje root)
    ProbePl2,
    /// DBus service mód (pro systemd)
    Daemon,
}

fn fmt_uw(uw: Option<i64>) -> String {
    match uw {
        Some(v) => format!("{:.1} W", v as f64 / 1_000_000.0),
        None => "-".to_string(),
    }
}

fn cmd_status() -> i32 {
    let s = levers::status();
    let cur = controller::current_profile();
    println!(
        "aktivní profil   : {}",
        cur.unwrap_or_else(|| "(žádný)".to_string())
    );
    if let Some(rapl) = &s.rapl {
        println!(
            "RAPL PL1 (long)  : {}  max {}",
            fmt_uw(rapl.pl1_uw),
            fmt_uw(rapl.pl1_max_uw)
        );
        println!(
            "RAPL PL2 (short) : {}  max {}",
            fmt_uw(rapl.pl2_uw),
            fmt_uw(rapl.pl2_max_uw)
        );
    }
    println!(
        "EPP              : {}",
        s.epp.unwrap_or_else(|| "-".to_string())
    );
    println!(
        "Governor         : {}",
        s.governor.unwrap_or_else(|| "-".to_string())
    );
    match s.cpu_freq_khz {
        Some(f) => println!("CPU max freq     : {:.0} MHz", f as f64 / 1000.0),
        None => println!("CPU max freq     : -"),
    }
    let g = &s.gpu;
    if g.max.is_some() || g.cur.is_some() {
        println!(
            "GPU freq         : cur {} MHz, max {} MHz, boost {} MHz (RP0 {} / RPn {})",
            g.cur
                .map(|x| x.to_string())
                .unwrap_or_else(|| "-".to_string()),
            g.max
                .map(|x| x.to_string())
                .unwrap_or_else(|| "-".to_string()),
            g.boost
                .map(|x| x.to_string())
                .unwrap_or_else(|| "-".to_string()),
            g.rp0
                .map(|x| x.to_string())
                .unwrap_or_else(|| "-".to_string()),
            g.rpn
                .map(|x| x.to_string())
                .unwrap_or_else(|| "-".to_string()),
        );
    }
    if !s.temps.is_empty() {
        let tstr: Vec<String> = s
            .temps
            .iter()
            .map(|(k, v)| format!("{}={:.0}°C", k, *v as f64 / 1000.0))
            .collect();
        println!("Teploty          : {}", tstr.join(", "));
    }
    0
}

fn cmd_list() -> i32 {
    let profiles = load_profiles();
    for name in VALID {
        let Some(p) = profiles.get(name) else {
            continue;
        };
        let aliases: Vec<&str> = PROFILE_ALIASES_LIST
            .iter()
            .filter(|(a, c)| *c == name && *a != name)
            .map(|(a, _)| *a)
            .collect();
        println!("[{}]  (aliasy: {})", name, aliases.join(", "));
        println!("   PL1={}  PL2={}", fmt_uw(p.pl1_uw), fmt_uw(p.pl2_uw));
        println!(
            "   EPP={}  governor={}",
            p.epp.clone().unwrap_or_else(|| "-".to_string()),
            p.governor.clone().unwrap_or_else(|| "-".to_string())
        );
        println!(
            "   GPU max={} boost={} MHz",
            p.gpu_max_mhz
                .map(|x| x.to_string())
                .unwrap_or_else(|| "-".to_string()),
            p.gpu_boost_mhz
                .map(|x| x.to_string())
                .unwrap_or_else(|| "-".to_string())
        );
    }
    0
}

fn cmd_set(profile: &str) -> i32 {
    let Some(can) = alias(profile) else {
        eprintln!("Neznámý profil: {}", profile);
        let all: Vec<String> = PROFILE_ALIASES_LIST
            .iter()
            .map(|(a, _)| a.to_string())
            .collect();
        eprintln!(
            "Možnosti: {} (aliasy: {})",
            VALID.join(", "),
            all.join(", ")
        );
        return 2;
    };
    // sysfs zápisy (governor/EPP/RAPL/GPU) i state file vyžadují root.
    // Plasma slider to řeší přes DBus (daemon běží jako root), ale CLI set
    // musí běžet pod rootem, jinak se profil reálně neaplikuje.
    if !is_root() {
        eprintln!("acer-profile set vyžaduje root (zápis sysfs + state file).");
        eprintln!("Použij: sudo acer-profile set {}", profile);
        eprintln!("  nebo Plasma slider (přes DBus daemon, bez sudo).");
        return 126;
    }
    if !controller::set_profile(can) {
        eprintln!("Chyba při aplikaci profilu {}", can);
        return 1;
    }
    println!("Nastaven a aplikován profil: {}", can);
    0
}

fn cmd_watch(interval: f64) -> i32 {
    let mut last = controller::current_profile();
    loop {
        let cur = controller::current_profile();
        if cur != last {
            let now = std::process::Command::new("date")
                .arg("+%H:%M:%S")
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .map(|s| s.trim().to_string())
                .unwrap_or_else(|| "?".to_string());
            println!("[{}] profil={}", now, cur.clone().unwrap_or_default());
            last = cur;
            cmd_status();
            println!("{}", "-".repeat(60));
        }
        thread::sleep(Duration::from_secs_f64(interval));
    }
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn"))
        .format_timestamp_secs()
        .init();

    let cli = Cli::parse();
    let code = match cli.cmd {
        Cmd::Status => cmd_status(),
        Cmd::List => cmd_list(),
        Cmd::Set { profile } => cmd_set(&profile),
        Cmd::Watch { interval } => cmd_watch(interval),
        Cmd::Measure => {
            require_root_or_exit("measure");
            measure::run();
            0
        }
        Cmd::ProbePl2 => {
            require_root_or_exit("probe-pl2");
            probe::run();
            0
        }
        Cmd::Daemon => match daemon::run() {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("daemon chyba: {:#}", e);
                1
            }
        },
    };
    std::process::exit(code);
}

/// true pokud běžíme pod rootem (euid 0).
fn is_root() -> bool {
    extern "C" {
        fn geteuid() -> u32;
    }
    unsafe { geteuid() == 0 }
}

/// Pro subcommandy vyžadující root: vypíše hlášku a skončí, pokud ne-root.
fn require_root_or_exit(cmd: &str) {
    if !is_root() {
        eprintln!("acer-profile {} vyžaduje root (zápis sysfs + zátěž).", cmd);
        eprintln!("Použij: sudo acer-profile {}", cmd);
        std::process::exit(126);
    }
}
