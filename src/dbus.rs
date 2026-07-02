//! DBus interface `net.hadess.PowerProfiles` na system bus.
//!
//! Nahrazuje power-profiles-daemon (PPD), který je na tomto stroji rozbitý
//! (EBUSY při zápisu EPP). Plasma posuvník mluví přímo s naším daemonem.
//!
//! Mapování profilů (Plasma ↔ naše):
//!   power-saver  ↔ eco
//!   balanced     ↔ normal
//!   performance  ↔ performance

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use zbus::interface;
use zbus::zvariant::{OwnedValue, Str};

use crate::controller;
use crate::profiles::{canonical, to_ppd_name};

pub const BUS_NAME: &str = "net.hadess.PowerProfiles";
/// Alias bus jméno — Plasma/PowerDevil kontroluje toto pro detekci "PPD nainstalován"
/// a PowerDevil proxy jde přes toto jméno. PPD registruje obě jména, my také.
pub const BUS_NAME_ALIAS: &str = "org.freedesktop.UPower.PowerProfiles";
/// Hlavní cesta (PPD historická, pro net.hadess).
pub const OBJECT_PATH: &str = "/net/hadess/PowerProfiles";
/// Alias cesta — PowerDevil proxy volá GetAll na této cestě
/// (ppdPath = "/org/freedesktop/UPower/PowerProfiles" v powerprofile.cpp).
pub const OBJECT_PATH_ALIAS: &str = "/org/freedesktop/UPower/PowerProfiles";

/// Stav sdílený mezi DBus interface a polling loopem daemonu.
#[derive(Clone)]
pub struct SharedState {
    /// Kanonický název aktivního profilu (eco/normal/performance).
    pub active: Arc<Mutex<String>>,
}

impl SharedState {
    pub fn new(initial: &str) -> Self {
        Self {
            active: Arc::new(Mutex::new(initial.to_string())),
        }
    }

    pub fn get(&self) -> String {
        self.active.lock().unwrap().clone()
    }

    pub fn set(&self, name: &str) {
        *self.active.lock().unwrap() = name.to_string();
    }
}

pub struct PowerProfiles {
    state: SharedState,
    next_hold: AtomicU32,
}

impl PowerProfiles {
    pub fn new(state: SharedState) -> Self {
        Self {
            state,
            next_hold: AtomicU32::new(1),
        }
    }
}

fn profile_dict(name: &str, cpu_driver: &str) -> HashMap<String, OwnedValue> {
    let mut d = HashMap::new();
    d.insert("Profile".to_string(), OwnedValue::from(Str::from(name)));
    d.insert(
        "CpuDriver".to_string(),
        OwnedValue::from(Str::from(cpu_driver)),
    );
    d.insert(
        "PlatformDriver".to_string(),
        OwnedValue::from(Str::from("placeholder")),
    );
    d.insert(
        "Driver".to_string(),
        OwnedValue::from(Str::from(cpu_driver)),
    );
    d
}

#[interface(name = "net.hadess.PowerProfiles")]
impl PowerProfiles {
    // ---- Properties ----

    /// ActiveProfile: s (writable) - vrací PPD-style název (power-saver/balanced/performance).
    #[zbus(property)]
    fn active_profile(&self) -> String {
        to_ppd_name(&self.state.get()).to_string()
    }

    /// Setter: mapuje PPD název -> kanonický, aplikuje profil, persistuje stav.
    /// PropertiesChanged se emituje automaticky (emits_changed_signal = true).
    #[zbus(property)]
    fn set_active_profile(&mut self, value: String) -> zbus::fdo::Result<()> {
        let Some(can) = canonical(&value) else {
            return Err(zbus::fdo::Error::InvalidArgs(format!(
                "neznámý profil: {} (power-saver/balanced/performance)",
                value
            )));
        };
        log::info!("DBus setActiveProfile: {} -> {}", value, can);
        if !controller::set_profile(can) {
            return Err(zbus::fdo::Error::Failed(format!(
                "chyba při aplikaci profilu {}",
                can
            )));
        }
        self.state.set(can);
        Ok(())
    }

    /// Profiles: aa{sv} - 3 profily (power-saver/balanced/performance) s klíči
    /// Profile/CpuDriver/PlatformDriver/Driver (shodný tvar s PPD pro Plasma).
    #[zbus(property(emits_changed_signal = "const"))]
    fn profiles(&self) -> Vec<HashMap<String, OwnedValue>> {
        vec![
            profile_dict("power-saver", "intel_pstate"),
            profile_dict("balanced", "intel_pstate"),
            profile_dict("performance", "intel_pstate"),
        ]
    }

    #[zbus(property(emits_changed_signal = "const"))]
    fn performance_degraded(&self) -> String {
        String::new()
    }

    #[zbus(property(emits_changed_signal = "const"))]
    fn performance_inhibited(&self) -> String {
        String::new()
    }

    #[zbus(property(emits_changed_signal = "const"))]
    fn actions(&self) -> Vec<String> {
        Vec::new()
    }

    #[zbus(property(emits_changed_signal = "false"))]
    fn active_profile_holds(&self) -> Vec<HashMap<String, OwnedValue>> {
        Vec::new()
    }

    #[zbus(property(emits_changed_signal = "const"))]
    fn version(&self) -> String {
        "0.30".to_string()
    }

    // ---- Methods ----

    /// HoldProfile(sss -> u): stub. Vrátí inkrementující cookie, nepřepíná profil.
    fn hold_profile(&mut self, _profile: String, _reason: String, _app_id: String) -> u32 {
        self.next_hold.fetch_add(1, Ordering::SeqCst)
    }

    /// ReleaseProfile(u -> ): stub.
    fn release_profile(&mut self, _cookie: u32) {}

    // ---- Signals ----

    #[zbus(signal)]
    async fn profile_released(
        signal_ctxt: &zbus::object_server::SignalContext<'_>,
        cookie: u32,
    ) -> zbus::Result<()>;
}

// ---------------------------------------------------------------------
// Alias interface `org.freedesktop.UPower.PowerProfiles` — stejné vlastnosti
// a metody jako net.hadess.PowerProfiles, ale pod jménem, které PowerDevil
// (Plasma battery applet) používá pro proxy volání. PPD registruje obě jména.
// ---------------------------------------------------------------------
pub struct PowerProfilesAlias {
    state: SharedState,
    next_hold: AtomicU32,
}

impl PowerProfilesAlias {
    pub fn new(state: SharedState) -> Self {
        Self {
            state,
            next_hold: AtomicU32::new(1),
        }
    }
}

#[interface(name = "org.freedesktop.UPower.PowerProfiles")]
impl PowerProfilesAlias {
    #[zbus(property)]
    fn active_profile(&self) -> String {
        to_ppd_name(&self.state.get()).to_string()
    }

    #[zbus(property)]
    fn set_active_profile(&mut self, value: String) -> zbus::fdo::Result<()> {
        let Some(can) = canonical(&value) else {
            return Err(zbus::fdo::Error::InvalidArgs(format!(
                "neznámý profil: {} (power-saver/balanced/performance)",
                value
            )));
        };
        log::info!("DBus(alias) setActiveProfile: {} -> {}", value, can);
        if !controller::set_profile(can) {
            return Err(zbus::fdo::Error::Failed(format!(
                "chyba při aplikaci profilu {}",
                can
            )));
        }
        self.state.set(can);
        Ok(())
    }

    #[zbus(property(emits_changed_signal = "const"))]
    fn profiles(&self) -> Vec<HashMap<String, OwnedValue>> {
        vec![
            profile_dict("power-saver", "intel_pstate"),
            profile_dict("balanced", "intel_pstate"),
            profile_dict("performance", "intel_pstate"),
        ]
    }

    #[zbus(property(emits_changed_signal = "const"))]
    fn performance_degraded(&self) -> String {
        String::new()
    }

    #[zbus(property(emits_changed_signal = "const"))]
    fn performance_inhibited(&self) -> String {
        String::new()
    }

    #[zbus(property(emits_changed_signal = "const"))]
    fn actions(&self) -> Vec<String> {
        Vec::new()
    }

    #[zbus(property(emits_changed_signal = "false"))]
    fn active_profile_holds(&self) -> Vec<HashMap<String, OwnedValue>> {
        Vec::new()
    }

    #[zbus(property(emits_changed_signal = "const"))]
    fn version(&self) -> String {
        "0.30".to_string()
    }

    fn hold_profile(&mut self, _profile: String, _reason: String, _app_id: String) -> u32 {
        self.next_hold.fetch_add(1, Ordering::SeqCst)
    }

    fn release_profile(&mut self, _cookie: u32) {}

    #[zbus(signal)]
    async fn profile_released(
        signal_ctxt: &zbus::object_server::SignalContext<'_>,
        cookie: u32,
    ) -> zbus::Result<()>;
}
