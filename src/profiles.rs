//! Definice profilů a načítání konfigurace (TOML).
//!
//! Standalone režim: acer-profile vlastní VŠECHNY páky (RAPL + EPP + governor +
//! GPU), protože power-profiles-daemon je na Swift SFG14-73 rozbitý (EBUSY).
//!
//! Profily: eco (20W), normal (35W), performance (45/64W).

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde::Deserialize;

pub const CONFIG_PATH: &str = "/etc/acer-profile/profiles.toml";

/// Akceptované názvy profilů (včetně powerprofilesctl aliasů pro pohodlí).
/// Mapuje vstupní název -> kanonický (eco/normal/performance).
pub fn alias(name: &str) -> Option<&'static str> {
    match name {
        "power-saver" | "eco" => Some("eco"),
        "balanced" | "normal" => Some("normal"),
        "performance" | "perf" => Some("performance"),
        _ => None,
    }
}

/// Všechny aliasy (vstupní název -> kanonický) pro nápovědu CLI.
pub const PROFILE_ALIASES_LIST: &[(&str, &str)] = &[
    ("power-saver", "eco"),
    ("balanced", "normal"),
    ("performance", "performance"),
    ("eco", "eco"),
    ("normal", "normal"),
    ("perf", "performance"),
];

/// PPD-style název pro DBus (Plasma očekává power-saver/balanced/performance).
pub fn to_ppd_name(canonical: &str) -> &'static str {
    match canonical {
        "eco" => "power-saver",
        "normal" => "balanced",
        "performance" => "performance",
        _ => "balanced",
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Profile {
    #[serde(default)]
    pub pl1_uw: Option<i64>, // long_term sustained (W * 1_000_000)
    #[serde(default)]
    pub pl2_uw: Option<i64>, // short_term turbo
    #[serde(default)]
    pub epp: Option<String>, // intel_pstate energy_performance_preference
    #[serde(default)]
    pub governor: Option<String>, // performance / powersave
    #[serde(default)]
    pub gpu_max_mhz: Option<i64>, // i915 gt_max_freq_mhz
    #[serde(default)]
    pub gpu_boost_mhz: Option<i64>,
}

// Defaulty kalibrované pro Swift SFG14-73 (Meteor Lake-H / Core Ultra 9 185H):
//   RAPL PL1 max hlášen 45 W; PL2 (short) bez max, BIOS default 64 W.
//   i915 GPU: RPn/RP1=800 MHz, RP0/boost=2350 MHz.
fn defaults() -> HashMap<&'static str, Profile> {
    let mut m = HashMap::new();
    m.insert(
        "eco",
        Profile {
            pl1_uw: Some(20_000_000),
            pl2_uw: Some(25_000_000),
            epp: Some("power".to_string()),
            governor: Some("powersave".to_string()),
            gpu_max_mhz: Some(800),
            gpu_boost_mhz: Some(800),
        },
    );
    m.insert(
        "normal",
        Profile {
            pl1_uw: Some(35_000_000),
            pl2_uw: Some(45_000_000),
            epp: Some("balance_power".to_string()),
            governor: Some("powersave".to_string()),
            gpu_max_mhz: Some(1600),
            gpu_boost_mhz: Some(1600),
        },
    );
    m.insert(
        "performance",
        Profile {
            pl1_uw: Some(45_000_000),
            pl2_uw: Some(64_000_000),
            epp: Some("performance".to_string()),
            governor: Some("performance".to_string()),
            gpu_max_mhz: Some(2350),
            gpu_boost_mhz: Some(2350),
        },
    );
    m
}

/// Načte profily: defaulty + override z CONFIG_PATH (zachová neznámé ignoruje).
pub fn load_profiles() -> HashMap<String, Profile> {
    let mut out: HashMap<String, Profile> = HashMap::new();
    for (k, v) in defaults() {
        out.insert(k.to_string(), v);
    }
    let path = Path::new(CONFIG_PATH);
    if !path.exists() {
        return out;
    }
    match fs::read_to_string(path) {
        Ok(content) => match toml::from_str::<HashMap<String, Profile>>(&content) {
            Ok(cfg) => {
                for (name, section) in cfg {
                    if !out.contains_key(&name) {
                        log::warn!("ignoruji neznámý profil {} v konfigu", name);
                        continue;
                    }
                    out.insert(name, section);
                }
                log::info!("profily načteny z {}", CONFIG_PATH);
            }
            Err(e) => log::error!("neplatný TOML {}: {}", CONFIG_PATH, e),
        },
        Err(e) => log::error!("nelze číst {}: {}", CONFIG_PATH, e),
    }
    out
}

/// Resolve alias -> profil z načtené mapy.
pub fn resolve<'a>(name: &str, profiles: &'a HashMap<String, Profile>) -> Option<&'a Profile> {
    alias(name).and_then(|key| profiles.get(key))
}

/// Kanonický název, nebo None pro neznámý.
pub fn canonical(name: &str) -> Option<&'static str> {
    alias(name)
}
