//! Windows tray app. Left click = toggle, right click = menu.
//! All network I/O runs on one worker thread so the tray never freezes.

use crate::{ambiance, cli, config, proto, update};
use std::net::IpAddr;
use std::os::windows::process::CommandExt;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::Duration;
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, TranslateMessage, WM_QUIT,
};

const APP: &str = "Libra 2";
const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

enum Action {
    Toggle,
    Power(bool),
    Bright(u8),
    Ct(u32),
    BgPower(bool),
    BgRgb(u32),
    BgBright(u8),
    Refresh,
    Rediscover,
    /// Apply one preset now (menu "Ambiances").
    Preset(usize),
    /// "Ambiance selon l'heure" ticked / unticked.
    Auto(bool),
}

/// What the worker reports back to the UI thread.
struct Report {
    ip: Option<IpAddr>,
    state: proto::State,
    error: Option<String>,
    /// Name of the ambiance last applied, cleared by a manual brightness/temperature change.
    preset: Option<&'static str>,
}

pub fn run() {
    let (act_tx, act_rx) = mpsc::channel::<Action>();
    let (rep_tx, rep_rx) = mpsc::channel::<Report>();
    thread::spawn(move || worker(act_rx, rep_tx));
    update::cleanup();
    // Update check: shortly after start, then once a day. Sends the newer tag, if any.
    let (upd_tx, upd_rx) = mpsc::channel::<String>();
    thread::spawn(move || {
        loop {
            thread::sleep(Duration::from_secs(15));
            if let Ok(Some(tag)) = update::check() {
                let _ = upd_tx.send(tag);
                return;
            }
            thread::sleep(Duration::from_secs(24 * 3600));
        }
    });
    let (inst_tx, inst_rx) = mpsc::channel::<Result<std::path::PathBuf, String>>();
    let mut pending: Option<String> = None;

    // --- menu -------------------------------------------------------------
    let status = MenuItem::with_id("status", "Recherche de la barre…", false, None);
    let on = MenuItem::with_id("on", "Allumer", true, None);
    let off = MenuItem::with_id("off", "Éteindre", true, None);
    let bright = Submenu::with_id("bright_menu", "Luminosité", true);
    for p in [10u8, 25, 50, 75, 100] {
        bright
            .append(&MenuItem::with_id(
                format!("bright:{p}"),
                format!("{p} %"),
                true,
                None,
            ))
            .ok();
    }
    let ct = Submenu::with_id("ct_menu", "Température", true);
    for (k, label) in [
        (2700u32, "Très chaud · 2700 K"),
        (3500, "Chaud · 3500 K"),
        (4000, "Neutre · 4000 K"),
        (5000, "Froid · 5000 K"),
        (6500, "Très froid · 6500 K"),
    ] {
        ct.append(&MenuItem::with_id(format!("ct:{k}"), label, true, None))
            .ok();
    }
    let bg = Submenu::with_id("bg_menu", "Rétroéclairage", true);
    bg.append(&MenuItem::with_id("bg:on", "Allumer", true, None))
        .ok();
    bg.append(&MenuItem::with_id("bg:off", "Éteindre", true, None))
        .ok();
    bg.append(&PredefinedMenuItem::separator()).ok();
    for (hex, label) in [
        ("ff0000", "Rouge"),
        ("ff6a00", "Orange"),
        ("ffd200", "Jaune"),
        ("00d26a", "Vert"),
        ("00b4ff", "Cyan"),
        ("2050ff", "Bleu"),
        ("9b30ff", "Violet"),
        ("ff2bd6", "Rose"),
        ("ffffff", "Blanc"),
    ] {
        bg.append(&MenuItem::with_id(format!("rgb:{hex}"), label, true, None))
            .ok();
    }
    bg.append(&PredefinedMenuItem::separator()).ok();
    for p in [10u8, 50, 100] {
        bg.append(&MenuItem::with_id(
            format!("bgbright:{p}"),
            format!("Luminosité {p} %"),
            true,
            None,
        ))
        .ok();
    }
    let auto = CheckMenuItem::with_id(
        "auto",
        "Ambiance selon l'heure",
        true,
        config::load_auto(),
        None,
    );
    let presets = Submenu::with_id("preset_menu", "Ambiances", true);
    for i in 0..ambiance::PRESETS.len() {
        presets
            .append(&MenuItem::with_id(
                format!("preset:{i}"),
                ambiance::label(i),
                true,
                None,
            ))
            .ok();
    }
    let rediscover = MenuItem::with_id("rediscover", "Rechercher la barre", true, None);
    let autostart = CheckMenuItem::with_id(
        "autostart",
        "Démarrer avec Windows",
        true,
        autostart_enabled(),
        None,
    );
    let upd = MenuItem::with_id("update", format!("yeebar {}", update::VERSION), false, None);
    let quit = MenuItem::with_id("quit", "Quitter", true, None);

    let menu = Menu::new();
    menu.append(&status).ok();
    menu.append(&PredefinedMenuItem::separator()).ok();
    menu.append(&on).ok();
    menu.append(&off).ok();
    menu.append(&bright).ok();
    menu.append(&ct).ok();
    menu.append(&PredefinedMenuItem::separator()).ok();
    menu.append(&auto).ok();
    menu.append(&presets).ok();
    // Ambient RGB hidden until the Libra 2 parameter format is known (bg_* → "invalid params").
    let _ = &bg;
    menu.append(&PredefinedMenuItem::separator()).ok();
    menu.append(&rediscover).ok();
    menu.append(&autostart).ok();
    menu.append(&upd).ok();
    menu.append(&quit).ok();

    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .with_tooltip(format!("{APP} — recherche…"))
        .with_icon(icon(Look::Unknown))
        .build()
        .expect("création de l'icône impossible");

    // --- message loop -------------------------------------------------------
    let mut msg: MSG = unsafe { std::mem::zeroed() };
    loop {
        unsafe {
            while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                if msg.message == WM_QUIT {
                    return;
                }
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        while let Ok(ev) = TrayIconEvent::receiver().try_recv() {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = ev
            {
                let _ = act_tx.send(Action::Toggle);
            }
        }
        while let Ok(ev) = MenuEvent::receiver().try_recv() {
            let id = ev.id.0.as_str();
            let action = match id {
                "on" => Some(Action::Power(true)),
                "off" => Some(Action::Power(false)),
                "bg:on" => Some(Action::BgPower(true)),
                "bg:off" => Some(Action::BgPower(false)),
                "rediscover" => Some(Action::Rediscover),
                "auto" => Some(Action::Auto(auto.is_checked())),
                "autostart" => {
                    set_autostart(autostart.is_checked());
                    autostart.set_checked(autostart_enabled());
                    None
                }
                "update" => {
                    if let Some(tag) = pending.clone() {
                        upd.set_text(format!("Mise à jour vers {tag}…"));
                        upd.set_enabled(false);
                        let tx = inst_tx.clone();
                        thread::spawn(move || {
                            let _ = tx.send(update::install(&tag));
                        });
                    }
                    None
                }
                "quit" => return,
                _ => parse_param(id),
            };
            if let Some(a) = action {
                let _ = act_tx.send(a);
            }
        }
        while let Ok(tag) = upd_rx.try_recv() {
            upd.set_text(format!("⬆ Mettre à jour vers {tag}"));
            upd.set_enabled(true);
            pending = Some(tag);
        }
        while let Ok(res) = inst_rx.try_recv() {
            match res {
                Ok(exe) => {
                    update::restart_later(&exe);
                    return;
                }
                Err(e) => {
                    let tag = pending.clone().unwrap_or_default();
                    upd.set_text(truncate(
                        &format!("⬆ Réessayer vers {tag} — échec : {e}"),
                        90,
                    ));
                    upd.set_enabled(true);
                }
            }
        }
        while let Ok(r) = rep_rx.try_recv() {
            let (look, text) = describe(&r);
            let _ = tray.set_icon(Some(icon(look)));
            let _ = tray.set_tooltip(Some(truncate(&format!("{APP} — {text}"), 120)));
            status.set_text(text);
        }
        thread::sleep(Duration::from_millis(15));
    }
}

fn parse_param(id: &str) -> Option<Action> {
    let (k, v) = id.split_once(':')?;
    match k {
        "bright" => v.parse().ok().map(Action::Bright),
        "ct" => v.parse().ok().map(Action::Ct),
        "rgb" => u32::from_str_radix(v, 16).ok().map(Action::BgRgb),
        "bgbright" => v.parse().ok().map(Action::BgBright),
        "preset" => v
            .parse()
            .ok()
            .filter(|i: &usize| *i < ambiance::PRESETS.len())
            .map(Action::Preset),
        _ => None,
    }
}

fn worker(rx: Receiver<Action>, tx: Sender<Report>) {
    let mut ip = config::load_ip();
    let mut state = proto::State::default();
    let mut need_lookup = true;
    let mut auto = config::load_auto();
    let mut preset: Option<&'static str> = None;
    // Period already handled by the automatic mode; None = apply at the first chance (start-up).
    let mut period: Option<usize> = None;
    loop {
        if need_lookup {
            let found = proto::discover(ip, Duration::from_secs(3));
            let hit = ip
                .and_then(|i| found.iter().find(|f| f.ip == i))
                .or(found.first());
            let error = match hit {
                Some(f) => {
                    ip = Some(f.ip);
                    config::save_ip(f.ip);
                    // Start-up: the bar cannot tell its real state, so set a known one.
                    if auto && period.is_none() {
                        let i = ambiance::index_for_hour(ambiance::local_hour());
                        period = Some(i);
                        match cli::apply_preset(&proto::Device::new(f.ip), i) {
                            Ok(_) => {
                                set_preset_state(&mut state, i);
                                preset = Some(ambiance::PRESETS[i].name);
                                None
                            }
                            Err(e) => Some(e),
                        }
                    } else {
                        None
                    }
                }
                None if ip.is_some() => Some("ne répond pas à la recherche".to_string()),
                None => Some("barre introuvable".to_string()),
            };
            let _ = tx.send(Report {
                ip,
                state: state.clone(),
                error,
                preset,
            });
            need_lookup = false;
        }
        let action = match rx.recv_timeout(Duration::from_secs(60)) {
            Ok(a) => a,
            Err(RecvTimeoutError::Timeout) => Action::Refresh, // pick up knob/remote changes
            Err(RecvTimeoutError::Disconnected) => return,
        };
        let Some(addr) = ip else {
            need_lookup = true;
            continue;
        };
        let d = proto::Device::new(addr);
        let action_is_refresh = matches!(action, Action::Refresh);
        if matches!(action, Action::Bright(_) | Action::Ct(_)) {
            preset = None;
        }
        // Automatic mode: new part of the day → apply its ambiance, unless the bar was switched off
        // from here (then it stays off). A manual change keeps until the next period.
        let now = ambiance::index_for_hour(ambiance::local_hour());
        let action = match action {
            Action::Refresh if auto && period != Some(now) && state.power != Some(false) => {
                Action::Preset(now)
            }
            Action::Refresh if auto => {
                period = Some(now);
                Action::Refresh
            }
            Action::Auto(on) => {
                auto = on;
                config::save_auto(on);
                if on {
                    Action::Preset(now)
                } else {
                    Action::Refresh
                }
            }
            a => a,
        };
        if let Action::Preset(i) = action
            && i == now
        {
            period = Some(now);
        }
        let res = match action {
            Action::Toggle => d.toggle().map(|_| state.power = state.power.map(|p| !p)),
            Action::Power(on) => d.power(on).map(|_| state.power = Some(on)),
            Action::Bright(p) => d.bright(p).map(|_| {
                state.bright = Some(p);
                state.power = Some(true)
            }),
            Action::Ct(k) => d.ct(k).map(|_| {
                state.ct = Some(k);
                state.power = Some(true)
            }),
            Action::BgPower(on) => d.bg_power(on).map(|_| state.bg_power = Some(on)),
            Action::BgRgb(c) => d.bg_rgb(c).map(|_| state.bg_power = Some(true)),
            Action::BgBright(p) => d.bg_bright(p).map(|_| state.bg_power = Some(true)),
            Action::Refresh | Action::Auto(_) => Ok(()),
            Action::Preset(i) => cli::apply_preset(&d, i).map(|_| {
                set_preset_state(&mut state, i);
                preset = Some(ambiance::PRESETS[i].name);
            }),
            Action::Rediscover => {
                need_lookup = true;
                continue;
            }
        };
        let error = match (action_is_refresh, res) {
            (_, Err(e)) => Some(e),
            (true, Ok(())) if !reachable(addr) => Some("ne répond pas".to_string()),
            _ => None,
        };
        let _ = tx.send(Report {
            ip,
            state: state.clone(),
            error,
            preset,
        });
    }
}

fn set_preset_state(state: &mut proto::State, i: usize) {
    let p = &ambiance::PRESETS[i];
    state.power = Some(true);
    state.bright = Some(p.bright);
    state.ct = Some(p.ct);
}

/// The Libra 2 (fw 1.0.0) never answers get_prop, and its discovery answer is frozen on the
/// boot values (verified: still "on, 100 %" after `off`). So the shown state is the last
/// command sent from this app; discovery is only used to check that the bar is reachable.
fn reachable(ip: IpAddr) -> bool {
    proto::discover(Some(ip), Duration::from_millis(1500))
        .iter()
        .any(|f| f.ip == ip)
}

#[derive(Clone, Copy)]
enum Look {
    On,
    Off,
    Unknown,
    Error,
}

fn describe(r: &Report) -> (Look, String) {
    if r.ip.is_none() {
        return (Look::Error, "introuvable · clic droit → Rechercher".into());
    }
    let mut parts = Vec::new();
    let look = match r.state.power {
        Some(true) => {
            parts.push("allumée".to_string());
            Look::On
        }
        Some(false) => {
            parts.push("éteinte".to_string());
            Look::Off
        }
        None => {
            parts.push("en ligne · état inconnu jusqu'à la 1re commande".to_string());
            Look::Unknown
        }
    };
    if r.state.power == Some(true) {
        if let Some(b) = r.state.bright {
            parts.push(format!("{b} %"));
        }
        if let Some(k) = r.state.ct {
            parts.push(format!("{k} K"));
        }
        if let Some(p) = r.preset {
            parts.push(format!("ambiance {p}"));
        }
    }
    let mut text = parts.join(" · ");
    if let Some(e) = &r.error {
        text = format!("{text} — {e}");
        return (Look::Error, text);
    }
    (look, text)
}

fn truncate(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

/// 32×32 icon drawn in code: a monitor light bar, lit or not.
fn icon(look: Look) -> Icon {
    const N: usize = 32;
    let mut px = vec![0u8; N * N * 4];
    let mut put = |x: usize, y: usize, c: [u8; 4]| {
        let i = (y * N + x) * 4;
        px[i..i + 4].copy_from_slice(&c);
    };
    let bar = match look {
        Look::On => [255, 196, 64, 255],
        Look::Off => [150, 150, 150, 255],
        Look::Unknown => [200, 200, 200, 255],
        Look::Error => [150, 150, 150, 255],
    };
    // bar
    for y in 6..12 {
        for x in 2..30 {
            put(x, y, bar);
        }
    }
    // light cone when on
    if let Look::On = look {
        for y in 12..28 {
            let spread = (y - 12) / 2;
            for x in (6 - spread.min(5))..(26 + spread.min(5)) {
                let a = (200 - (y - 12) * 11) as u8;
                put(x, y, [255, 214, 110, a]);
            }
        }
    }
    // red dot on error
    if let Look::Error = look {
        for y in 18..30 {
            for x in 18..30 {
                let (dx, dy) = (x as i32 - 24, y as i32 - 24);
                if dx * dx + dy * dy <= 30 {
                    put(x, y, [220, 40, 40, 255]);
                }
            }
        }
    }
    Icon::from_rgba(px, N as u32, N as u32).expect("icône invalide")
}

fn autostart_enabled() -> bool {
    std::process::Command::new("reg")
        .args(["query", RUN_KEY, "/v", "yeebar"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn set_autostart(on: bool) {
    let mut c = std::process::Command::new("reg");
    if on {
        let exe = std::env::current_exe()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        c.args([
            "add",
            RUN_KEY,
            "/v",
            "yeebar",
            "/t",
            "REG_SZ",
            "/d",
            &format!("\"{exe}\""),
            "/f",
        ]);
    } else {
        c.args(["delete", RUN_KEY, "/v", "yeebar", "/f"]);
    }
    let _ = c.creation_flags(CREATE_NO_WINDOW).output();
}
