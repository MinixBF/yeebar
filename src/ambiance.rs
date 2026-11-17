//! Time-of-day presets ("ambiances"): one brightness + colour temperature per part of the day.
//! Applied at start-up (the Libra 2 cannot report its real state, so this gives a known one)
//! and at each change of period, when the "Ambiance selon l'heure" option is ticked.

pub struct Preset {
    pub name: &'static str,
    /// Local hour from which the preset applies, until the next one.
    pub from: u8,
    pub bright: u8,
    pub ct: u32,
}

pub const PRESETS: [Preset; 4] = [
    Preset {
        name: "Matin",
        from: 6,
        bright: 70,
        ct: 4500,
    },
    Preset {
        name: "Journée",
        from: 10,
        bright: 100,
        ct: 5000,
    },
    Preset {
        name: "Soir",
        from: 18,
        bright: 60,
        ct: 3000,
    },
    Preset {
        name: "Nuit",
        from: 22,
        bright: 20,
        ct: 2700,
    },
];

/// Index in `PRESETS` of the preset in force at `hour` (0-23); before 6 h it is still "Nuit".
pub fn index_for_hour(hour: u8) -> usize {
    PRESETS
        .iter()
        .rposition(|p| hour >= p.from)
        .unwrap_or(PRESETS.len() - 1)
}

pub fn find(name: &str) -> Option<usize> {
    let norm = |s: &str| s.to_lowercase().replace('é', "e");
    PRESETS.iter().position(|p| norm(p.name) == norm(name))
}

pub fn label(i: usize) -> String {
    let p = &PRESETS[i];
    let to = PRESETS[(i + 1) % PRESETS.len()].from;
    format!(
        "{} ({} h – {} h) · {} % · {} K",
        p.name, p.from, to, p.bright, p.ct
    )
}

/// Current local hour.
#[cfg(windows)]
pub fn local_hour() -> u8 {
    let mut t = unsafe { std::mem::zeroed() };
    unsafe { windows_sys::Win32::System::SystemInformation::GetLocalTime(&mut t) };
    t.wHour as u8
}

#[cfg(not(windows))]
pub fn local_hour() -> u8 {
    // Only the Windows build is used for real; UTC is good enough elsewhere (tests, CI).
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    ((secs / 3600) % 24) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn periods_cover_the_whole_day() {
        let names: Vec<&str> = (0..24).map(|h| PRESETS[index_for_hour(h)].name).collect();
        assert_eq!(names[0], "Nuit");
        assert_eq!(names[5], "Nuit");
        assert_eq!(names[6], "Matin");
        assert_eq!(names[9], "Matin");
        assert_eq!(names[10], "Journée");
        assert_eq!(names[17], "Journée");
        assert_eq!(names[18], "Soir");
        assert_eq!(names[21], "Soir");
        assert_eq!(names[22], "Nuit");
        assert_eq!(names[23], "Nuit");
    }

    #[test]
    fn find_ignores_case_and_accent() {
        assert_eq!(find("journee"), Some(1));
        assert_eq!(find("SOIR"), Some(2));
        assert_eq!(find("aube"), None);
    }

    #[test]
    fn label_shows_range() {
        assert_eq!(label(3), "Nuit (22 h – 6 h) · 20 % · 2700 K");
    }
}
