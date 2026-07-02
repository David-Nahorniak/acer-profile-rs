//! Nízkoúrovňové páky (levers) pro řízení výkonu přes sysfs.
//!
//! Každá funkce objeví cesty za běhu (glob) a gracefully přeskočí to, co na
//! stroji neexistuje nebo není zapisovatelné. Vše běží jako root (systemd).
//!
//! Klíčové: zápis governor PŘED EPP. Na intel_pstate vrací zápis EPP EBUSY,
//! pokud je scaling_governor=performance (governor vlastní policy). Proto
//! nejdřív governor, pak EPP; při governor=performance EPP přeskočit.

use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

pub const RAPL_PKG_GLOB: &str = "/sys/class/powercap/intel-rapl:*";
pub const CPU_POLICY_GLOB: &str = "/sys/devices/system/cpu/cpufreq/policy*";
pub const I915_CARD_GLOB: &str = "/sys/bus/pci/drivers/i915/*:*/drm/card*/gt_max_freq_mhz";

/// Přečte soubor a vrátí ořezaný obsah, nebo None při chybě/absenci.
fn read_str(path: &Path) -> Option<String> {
    match fs::read_to_string(path) {
        Ok(s) => Some(s.trim().to_string()),
        Err(_) => None,
    }
}

fn as_int(s: Option<&str>) -> Option<i64> {
    s.filter(|v| !v.is_empty())
        .and_then(|v| v.parse::<i64>().ok())
}

/// Zapíše hodnotu do sysfs souboru (read+modify styl). Při EBUSY retry 3x
/// (sleep 150ms). Vrací true při úspěchu. Při EACCES (ro) nebo jiné chybě false.
fn write(path: &Path, value: &str, retry_ebusy: bool) -> bool {
    let attempts = if retry_ebusy { 3 } else { 1 };
    for attempt in 0..attempts {
        match fs::OpenOptions::new().read(true).write(true).open(path) {
            Ok(mut fh) => {
                let mut old = String::new();
                if fh.read_to_string(&mut old).is_ok() && old.trim() == value {
                    return true;
                }
                if fh.seek(SeekFrom::Start(0)).is_err() {
                    log::warn!("write seek failed: {}", path.display());
                    return false;
                }
                if let Err(e) = fh.write_all(format!("{}\n", value).as_bytes()) {
                    log::warn!("write failed {}: {}", path.display(), e);
                    return false;
                }
                let _ = fh.set_len(0);
                if let Err(e) = fh.flush() {
                    log::warn!("write flush failed {}: {}", path.display(), e);
                    return false;
                }
                return true;
            }
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                log::warn!("write denied (ro): {}", path.display());
                return false;
            }
            Err(e) => {
                let raw = e.raw_os_error();
                if retry_ebusy && raw == Some(16) && attempt < 2 {
                    thread::sleep(Duration::from_millis(150));
                    continue;
                }
                log::warn!("open failed {}: {}", path.display(), e);
                return false;
            }
        }
    }
    false
}

fn write_val<T: ToString>(path: &Path, value: T, retry_ebusy: bool) -> bool {
    write(path, &value.to_string(), retry_ebusy)
}

// ---------------------------------------------------------------- RAPL
#[derive(Debug, Clone, Default)]
pub struct RaplConstraints {
    pub pkg: Option<PathBuf>,
    pub pl1_uw: Option<i64>,
    pub pl2_uw: Option<i64>,
    pub pl1_max_uw: Option<i64>,
    pub pl2_max_uw: Option<i64>,
}

/// Minimal glob pro sysfs cesty: každý segment cesty může obsahovat `*`
/// (matchuje libovolný podřetězec). Přes std::fs, bez externí crate.
fn glob_paths(pattern: &str) -> Vec<PathBuf> {
    let segs: Vec<&str> = pattern.split('/').collect();
    let mut results: Vec<PathBuf> = vec![PathBuf::from("/")];
    let mut absolute = true;
    for seg in segs {
        if seg.is_empty() {
            absolute = true;
            continue;
        }
        if !absolute {
            results = vec![PathBuf::from("/")];
            absolute = true;
        }
        let mut next: Vec<PathBuf> = Vec::new();
        for base in &results {
            if seg.contains('*') {
                let mut matched: Vec<(String, PathBuf)> = Vec::new();
                if let Ok(entries) = fs::read_dir(base) {
                    for e in entries.flatten() {
                        let name = e.file_name().to_string_lossy().to_string();
                        if glob_match(seg, &name) {
                            matched.push((name, e.path()));
                        }
                    }
                }
                matched.sort_by(|a, b| a.0.cmp(&b.0));
                for (_, p) in matched {
                    next.push(p);
                }
            } else {
                let p = base.join(seg);
                if p.exists() {
                    next.push(p);
                }
            }
        }
        results = next;
        if results.is_empty() {
            break;
        }
    }
    results.sort();
    results
}

fn glob_match(pattern: &str, name: &str) -> bool {
    // Podpora `*` (libovolný podřetězec, i prázdný). Ostatní znaky doslova.
    let pat: Vec<char> = pattern.chars().collect();
    let nam: Vec<char> = name.chars().collect();
    glob_match_impl(&pat, 0, &nam, 0)
}

fn glob_match_impl(pat: &[char], mut pi: usize, nam: &[char], mut ni: usize) -> bool {
    while pi < pat.len() {
        if pat[pi] == '*' {
            // přeskoč více `*`
            while pi < pat.len() && pat[pi] == '*' {
                pi += 1;
            }
            if pi == pat.len() {
                return true;
            }
            while ni <= nam.len() {
                if glob_match_impl(pat, pi, nam, ni) {
                    return true;
                }
                if ni == nam.len() {
                    break;
                }
                ni += 1;
            }
            return false;
        } else {
            if ni >= nam.len() || pat[pi] != nam[ni] {
                return false;
            }
            pi += 1;
            ni += 1;
        }
    }
    ni == nam.len()
}

pub fn rapl_discover() -> Option<PathBuf> {
    let pkgs = glob_paths(RAPL_PKG_GLOB);
    for p in &pkgs {
        if read_str(&p.join("name")).as_deref() == Some("package-0") {
            return Some(p.clone());
        }
    }
    pkgs.into_iter().next()
}

pub fn rapl_read(pkg: Option<&Path>) -> RaplConstraints {
    let pkg = pkg.map(|p| p.to_path_buf()).or_else(rapl_discover);
    let Some(pkg) = pkg else {
        return RaplConstraints::default();
    };
    RaplConstraints {
        pkg: Some(pkg.clone()),
        pl1_uw: as_int(read_str(&pkg.join("constraint_0_power_limit_uw")).as_deref()),
        pl2_uw: as_int(read_str(&pkg.join("constraint_1_power_limit_uw")).as_deref()),
        pl1_max_uw: as_int(read_str(&pkg.join("constraint_0_max_power_uw")).as_deref()),
        pl2_max_uw: as_int(read_str(&pkg.join("constraint_1_max_power_uw")).as_deref()),
    }
}

pub fn rapl_apply(pl1_uw: Option<i64>, pl2_uw: Option<i64>) {
    let Some(pkg) = rapl_discover() else {
        log::warn!("RAPL package nenalezen, preskakuji");
        return;
    };
    if let Some(pl1) = pl1_uw {
        write_val(&pkg.join("constraint_0_power_limit_uw"), pl1, true);
    }
    if let Some(pl2) = pl2_uw {
        write_val(&pkg.join("constraint_1_power_limit_uw"), pl2, true);
    }
}

// ----------------------------------------------- EPP/governor (pořadí!)
pub fn cpu_policies() -> Vec<PathBuf> {
    glob_paths(CPU_POLICY_GLOB)
}

/// governor PŘED EPP - jinak EBUSY při governor=performance.
/// Při governor=performance intel_pstate VLASTNÍ EPP -> EPP přeskočit.
pub fn epp_apply(epp: Option<&str>, governor: Option<&str>) {
    let pols = cpu_policies();
    if pols.is_empty() {
        log::warn!("žádné cpufreq policy, EPP preskakuji");
        return;
    }
    if let Some(gov) = governor {
        for p in &pols {
            write_val(&p.join("scaling_governor"), gov, true);
        }
    }
    if let Some(e) = epp {
        if governor != Some("performance") {
            for p in &pols {
                write_val(&p.join("energy_performance_preference"), e, true);
            }
        } else {
            log::debug!("governor=performance -> EPP preskočeno (driver ho vlastní)");
        }
    }
}

pub fn epp_read() -> (Option<String>, Option<String>) {
    let pols = cpu_policies();
    if pols.is_empty() {
        return (None, None);
    }
    let p = &pols[0];
    (
        read_str(&p.join("energy_performance_preference")),
        read_str(&p.join("scaling_governor")),
    )
}

// ----------------------------------------------------------------- GPU
pub fn i915_card_dir() -> Option<PathBuf> {
    glob_paths(I915_CARD_GLOB)
        .into_iter()
        .next()
        .and_then(|f| f.parent().map(|p| p.to_path_buf()))
}

#[derive(Debug, Clone, Default)]
pub struct GpuBounds {
    pub rp0: Option<i64>,
    #[allow(dead_code)]
    pub rp1: Option<i64>,
    pub rpn: Option<i64>,
    pub max: Option<i64>,
    pub boost: Option<i64>,
    pub cur: Option<i64>,
}

pub fn gpu_read_bounds() -> GpuBounds {
    let Some(d) = i915_card_dir() else {
        return GpuBounds::default();
    };
    GpuBounds {
        rp0: as_int(read_str(&d.join("gt_RP0_freq_mhz")).as_deref()),
        rp1: as_int(read_str(&d.join("gt_RP1_freq_mhz")).as_deref()),
        rpn: as_int(read_str(&d.join("gt_RPn_freq_mhz")).as_deref()),
        max: as_int(read_str(&d.join("gt_max_freq_mhz")).as_deref()),
        boost: as_int(read_str(&d.join("gt_boost_freq_mhz")).as_deref()),
        cur: as_int(read_str(&d.join("gt_cur_freq_mhz")).as_deref()),
    }
}

/// max před boost (boost nesmí > max), EINVAL na i915 tolerujeme (race).
pub fn gpu_apply(max_mhz: Option<i64>, boost_mhz: Option<i64>) {
    let Some(d) = i915_card_dir() else {
        log::warn!("i915 card nenalezen, GPU preskakuji");
        return;
    };
    if let Some(m) = max_mhz {
        write_gpu_freq(&d.join("gt_max_freq_mhz"), m);
    }
    if let Some(b) = boost_mhz {
        write_gpu_freq(&d.join("gt_boost_freq_mhz"), b);
    }
}

fn write_gpu_freq(path: &Path, value: i64) {
    let val = value.to_string();
    match fs::OpenOptions::new().read(true).write(true).open(path) {
        Ok(mut fh) => {
            let mut old = String::new();
            if fh.read_to_string(&mut old).is_ok() && old.trim() == val {
                return;
            }
            if fh.seek(SeekFrom::Start(0)).is_err() {
                log::warn!("GPU seek failed: {}", path.display());
                return;
            }
            let _ = fh.write_all(format!("{}\n", val).as_bytes());
            let _ = fh.set_len(0);
            let _ = fh.flush();
        }
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            log::warn!("GPU write denied (ro): {}", path.display());
        }
        Err(e) => {
            if e.raw_os_error() == Some(22) {
                log::debug!("GPU write EINVAL (tolerováno) {}={}", path.display(), value);
            } else {
                log::warn!("GPU write failed {}: {}", path.display(), e);
            }
        }
    }
}

// ------------------------------------------------------------- teploty
pub fn temps_read() -> Vec<(String, i64)> {
    let mut out: Vec<(String, i64)> = Vec::new();
    for f in glob_paths("/sys/devices/platform/acer-wmi/hwmon/*/temp*_input") {
        let key = format!(
            "acer_{}",
            f.file_name().unwrap_or_default().to_string_lossy()
        );
        let t = as_int(read_str(&f).as_deref()).unwrap_or(0);
        out.push((key, t));
    }
    for f in glob_paths("/sys/class/hwmon/hwmon*/temp1_input") {
        let parent = f.parent().unwrap_or_else(|| Path::new("/"));
        let name = read_str(&parent.join("name")).unwrap_or_else(|| "hwmon".to_string());
        if let Some(t) = as_int(read_str(&f).as_deref()) {
            out.push((name, t));
        }
    }
    out
}

pub fn cpu_max_freq_khz() -> Option<i64> {
    let mut best: i64 = 0;
    for p in cpu_policies() {
        if let Some(v) = as_int(read_str(&p.join("scaling_cur_freq")).as_deref()) {
            if v > best {
                best = v;
            }
        }
    }
    (best > 0).then_some(best)
}

// --------------------------------------------------- kompletní status
#[derive(Debug, Default)]
pub struct Status {
    pub rapl: Option<RaplConstraints>,
    pub epp: Option<String>,
    pub governor: Option<String>,
    pub gpu: GpuBounds,
    pub temps: Vec<(String, i64)>,
    pub cpu_freq_khz: Option<i64>,
}

pub fn status() -> Status {
    let (epp, gov) = epp_read();
    let r = rapl_read(None);
    let rapl = if r.pkg.is_none() { None } else { Some(r) };
    Status {
        rapl,
        epp,
        governor: gov,
        gpu: gpu_read_bounds(),
        temps: temps_read(),
        cpu_freq_khz: cpu_max_freq_khz(),
    }
}
