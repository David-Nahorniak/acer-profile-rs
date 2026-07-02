//! Daemon: blocking DBus service `net.hadess.PowerProfiles` na system bus.
//!
//! Start: načte state file, aplikuje profil, zaregistruje DBus jméno a interface,
//! vstoupí do loopu. Při změně ActiveProfile z Plasmy (DBus setter) se profil
//! aplikuje a PropertiesChanged emituje automaticky. Navíc polling (1s) sleduje
//! state file - když CLI `acer-profile set` změní stav, daemon profil aplikuje a
//! emituje PropertiesChanged, aby Plasma slider zůstal synchronizovaný.

use std::thread;
use std::time::Duration;

use anyhow::Result;
use async_io::block_on;
use zbus::blocking::Connection;

use crate::controller;
use crate::dbus::{
    PowerProfiles, PowerProfilesAlias, SharedState, BUS_NAME, BUS_NAME_ALIAS, OBJECT_PATH,
    OBJECT_PATH_ALIAS,
};

const POLL_SECS: u64 = 1;

pub fn run() -> Result<()> {
    // 1. Startovní profil z state file (nebo bezpečný default normal).
    let last = controller::current_profile().unwrap_or_else(|| {
        log::info!("žádný state file -> startuji na normal");
        "normal".to_string()
    });
    log::info!("startovní profil z state: {}", last);
    controller::set_profile(&last);

    let state = SharedState::new(&last);

    // 2. DBus: system bus, registrace interface a jména.
    let connection = Connection::system()?;
    // Hlavní cesta /net/hadess/PowerProfiles (historická PPD).
    connection
        .object_server()
        .at(OBJECT_PATH, PowerProfiles::new(state.clone()))?;
    connection
        .object_server()
        .at(OBJECT_PATH, PowerProfilesAlias::new(state.clone()))?;
    // Alias cesta /org/freedesktop/UPower/PowerProfiles — PowerDevil proxy volá
    // GetAll na této cestě (ppdPath v powerprofile.cpp). Bez ní profileChoices=0.
    connection
        .object_server()
        .at(OBJECT_PATH_ALIAS, PowerProfiles::new(state.clone()))?;
    connection
        .object_server()
        .at(OBJECT_PATH_ALIAS, PowerProfilesAlias::new(state.clone()))?;
    connection.request_name(BUS_NAME)?;
    connection.request_name(BUS_NAME_ALIAS)?;
    log::info!(
        "DBus: {} + {} (alias) na {} + {} (poll={}s, standalone, bez PPD)",
        BUS_NAME,
        BUS_NAME_ALIAS,
        OBJECT_PATH,
        OBJECT_PATH_ALIAS,
        POLL_SECS
    );

    // 3. Polling loop: sleduje state file (změny z CLI) + emituje PropertiesChanged.
    let mut last_seen = last;
    loop {
        thread::sleep(Duration::from_secs(POLL_SECS));
        let Some(cur) = controller::current_profile() else {
            continue;
        };
        if cur == last_seen {
            continue;
        }
        log::info!("změna profilu {} -> {}", last_seen, cur);
        // Aplikuj (CLI už aplikoval, ale pro jistotu při ruční editaci state file).
        controller::set_profile(&cur);
        last_seen = cur.clone();
        state.set(&cur);

        // Emit PropertiesChanged pro ActiveProfile na obou cestách + obou interfacech,
        // aby Plasma slider (PowerDevil proxy) refrešnul bez ohledu na to, kterou
        // cestu/interface sleduje.
        emit_active_profile_changed(&connection, OBJECT_PATH);
        emit_active_profile_changed(&connection, OBJECT_PATH_ALIAS);
    }
}

// Zastavení zastará systemd SIGTERM (Type=simple, Restart=on-failure).

/// Emit PropertiesChanged pro ActiveProfile na dané cestě (oba interface).
fn emit_active_profile_changed(connection: &Connection, path: &str) {
    // net.hadess.PowerProfiles interface
    if let Ok(iface_ref) = connection
        .object_server()
        .interface::<_, PowerProfiles>(path)
    {
        let inst = iface_ref.get();
        let sc = iface_ref.signal_context();
        if let Err(e) = block_on(PowerProfiles::active_profile_changed(&inst, sc)) {
            log::warn!("PropertiesChanged (net.hadess @ {}) selhal: {}", path, e);
        }
    }
    // org.freedesktop.UPower.PowerProfiles interface (PowerDevil proxy)
    if let Ok(iface_ref) = connection
        .object_server()
        .interface::<_, PowerProfilesAlias>(path)
    {
        let inst = iface_ref.get();
        let sc = iface_ref.signal_context();
        if let Err(e) = block_on(PowerProfilesAlias::active_profile_changed(&inst, sc)) {
            log::warn!("PropertiesChanged (UPower @ {}) selhal: {}", path, e);
        }
    }
}
