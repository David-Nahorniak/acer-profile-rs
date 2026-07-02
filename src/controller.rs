//! Aplikace profilu na hardwarové páky + správa stavu (state file).
//!
//! Pořadí zápisu (důležité pro intel_pstate - jinak EBUSY):
//!   1. governor (powersave/performance)
//!   2. EPP
//!   3. RAPL PL1/PL2
//!   4. GPU max/boost

use std::fs;

use crate::levers;
use crate::profiles::{canonical, load_profiles, resolve, Profile};

pub const STATE_DIR: &str = "/var/lib/acer-profile";
pub const STATE_FILE: &str = "/var/lib/acer-profile/current";
pub const VALID: [&str; 3] = ["eco", "normal", "performance"];

pub fn current_profile() -> Option<String> {
    match fs::read_to_string(STATE_FILE) {
        Ok(s) => {
            let t = s.trim();
            if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            }
        }
        Err(_) => None,
    }
}

pub fn save_profile(name: &str) {
    let _ = fs::create_dir_all(STATE_DIR);
    if let Err(e) = fs::write(STATE_FILE, format!("{}\n", name)) {
        log::error!("nelze zapsat state {}: {}", STATE_FILE, e);
    }
}

pub fn apply_profile(profile: &Profile, label: &str) {
    log::info!("aplikuji profil {}", label);
    // 1. governor PŘED EPP (intel_pstate EBUSY fix)
    levers::epp_apply(profile.epp.as_deref(), profile.governor.as_deref());
    // 2. RAPL
    levers::rapl_apply(profile.pl1_uw, profile.pl2_uw);
    // 3. GPU
    levers::gpu_apply(profile.gpu_max_mhz, profile.gpu_boost_mhz);
}

/// Najde profil, aplikuje ho, persistuje stav. True pokud OK.
pub fn set_profile(name: &str) -> bool {
    let Some(can) = canonical(name) else {
        log::warn!("neznámý profil: {}", name);
        return false;
    };
    if !VALID.contains(&can) {
        log::warn!("neznámý profil: {}", name);
        return false;
    }
    let profiles = load_profiles();
    let Some(prof) = resolve(can, &profiles) else {
        log::warn!("profil {} nenalezen v konfigu", can);
        return false;
    };
    apply_profile(prof, can);
    save_profile(can);
    true
}
