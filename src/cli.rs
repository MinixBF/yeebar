//! Command line: `yeebar on|off|toggle|bright 60|ct 4000|bg on|bg off|bg rgb ff0000|bg bright 50|status|discover|test [--ip X]`.
//! Handy for Windows shortcuts, Stream Deck, AutoHotkey…

use crate::{ambiance, config, proto, update};
use std::net::IpAddr;
use std::time::Duration;

const USAGE: &str = "yeebar — pilote une Yeelight (Libra 2) sur le réseau local

  yeebar                      icône dans la barre des tâches
  yeebar on | off | toggle    allumer / éteindre / basculer
  yeebar bright <1-100>       luminosité
  yeebar ct <2700-6500>       température (K)
  yeebar ambiance [nom]       ambiance de l'heure (ou matin|journee|soir|nuit)
  yeebar bg on|off            rétroéclairage
  yeebar bg rgb <RRGGBB>      couleur du rétroéclairage
  yeebar bg bright <1-100>    luminosité du rétroéclairage
  yeebar status               état
  yeebar discover             chercher les appareils
  yeebar test                 tester les commandes de base (diagnostic)
  yeebar version              version installée
  yeebar update               installer la dernière version publiée
  option : --ip <adresse>     forcer l'adresse (sinon mémorisée / recherchée)";

pub fn run(args: &[String]) -> i32 {
    let mut ip: Option<IpAddr> = None;
    let mut rest = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--ip" {
            match it.next().and_then(|v| v.parse().ok()) {
                Some(v) => ip = Some(v),
                None => return fail("--ip attend une adresse, ex. --ip 192.168.1.71"),
            }
        } else {
            rest.push(a.as_str());
        }
    }
    if matches!(
        rest.first(),
        None | Some(&"help") | Some(&"-h") | Some(&"--help")
    ) {
        println!("{USAGE}");
        return 0;
    }
    if rest[0] == "version" {
        println!("yeebar {}", update::VERSION);
        return 0;
    }
    if rest[0] == "update" {
        return self_update();
    }
    if rest[0] == "discover" {
        let found = proto::discover(ip.or_else(config::load_ip), Duration::from_secs(3));
        if found.is_empty() {
            return fail("aucun appareil trouvé (contrôle LAN activé ? même réseau ? pare-feu ?)");
        }
        for f in &found {
            println!(
                "{}  modèle {}  firmware {}  nom {}",
                f.ip, f.model, f.fw, f.name
            );
        }
        config::save_ip(found[0].ip);
        return 0;
    }
    let Some(ip) = ip.or_else(config::load_ip).or_else(|| {
        let f = proto::discover(None, Duration::from_secs(3));
        f.first().map(|f| {
            config::save_ip(f.ip);
            f.ip
        })
    }) else {
        return fail("aucun appareil trouvé ; essaie `yeebar discover` ou `--ip`");
    };
    let d = proto::Device::new(ip);
    let num = |i: usize| rest.get(i).and_then(|v| v.parse::<u32>().ok());
    let res = match rest.as_slice() {
        ["on"] => d.power(true),
        ["off"] => d.power(false),
        ["toggle"] => d.toggle(),
        ["bright", _] => match num(1) {
            Some(n) => d.bright(n.min(100) as u8),
            None => return fail("bright attend un nombre de 1 à 100"),
        },
        ["ct", _] => match num(1) {
            Some(n) => d.ct(n),
            None => return fail("ct attend une température, ex. 4000"),
        },
        ["bg", "on"] => d.bg_power(true),
        ["bg", "off"] => d.bg_power(false),
        ["bg", "rgb", hex] => match u32::from_str_radix(hex.trim_start_matches('#'), 16) {
            Ok(c) => d.bg_rgb(c),
            Err(_) => return fail("bg rgb attend une couleur hexadécimale, ex. ff0000"),
        },
        ["bg", "bright", _] => match num(2) {
            Some(n) => d.bg_bright(n.min(100) as u8),
            None => return fail("bg bright attend un nombre de 1 à 100"),
        },
        ["ambiance"] => apply_preset(&d, ambiance::index_for_hour(ambiance::local_hour())),
        ["ambiance", name] => match ambiance::find(name) {
            Some(i) => apply_preset(&d, i),
            None => return fail("ambiance inconnue : matin, journee, soir ou nuit"),
        },
        ["status"] => return status(&d),
        ["test"] => return selftest(&d),
        _ => {
            return fail(&format!(
                "commande inconnue : {}\n\n{USAGE}",
                rest.join(" ")
            ));
        }
    };
    match res {
        Ok(_) => {
            println!("ok");
            0
        }
        Err(e) => fail(&e),
    }
}

/// Power on, then brightness and temperature, with a short pause between commands.
pub fn apply_preset(d: &proto::Device, i: usize) -> Result<serde_json::Value, String> {
    let p = &ambiance::PRESETS[i];
    d.power(true)?;
    std::thread::sleep(Duration::from_millis(400));
    d.bright(p.bright)?;
    std::thread::sleep(Duration::from_millis(400));
    d.ct(p.ct)
}

#[cfg(windows)]
fn self_update() -> i32 {
    match update::check() {
        Err(e) => fail(&format!("vérification impossible : {e}")),
        Ok(None) => {
            println!("déjà à jour (version {})", update::VERSION);
            0
        }
        Ok(Some(tag)) => match update::install(&tag) {
            Ok(_) => {
                println!(
                    "mis à jour vers {tag} ; relance l'icône (Quitter puis yeebar.exe) si elle tourne"
                );
                0
            }
            Err(e) => fail(&format!("mise à jour impossible : {e}")),
        },
    }
}

#[cfg(not(windows))]
fn self_update() -> i32 {
    fail("mise à jour automatique : Windows uniquement")
}

/// One diagnostic step: label + command to run.
type Step<'a> = Box<dyn Fn() -> Result<serde_json::Value, String> + 'a>;

fn status(d: &proto::Device) -> i32 {
    // The Libra 2 never answers get_prop and its discovery answer is frozen on the boot values,
    // so only reachability can be reported.
    match proto::discover(Some(d.ip), Duration::from_secs(2))
        .into_iter()
        .find(|f| f.ip == d.ip)
    {
        Some(f) => println!(
            "{} : en ligne ({}, firmware {}) — la barre ne donne pas son état réel",
            f.ip, f.model, f.fw
        ),
        None => return fail("l'appareil ne répond pas"),
    }
    0
}

/// Runs every command once and reports which ones the firmware accepts.
fn selftest(d: &proto::Device) -> i32 {
    let steps: Vec<(&str, Step)> = vec![
        ("set_power off", Box::new(|| d.power(false))),
        ("set_power on", Box::new(|| d.power(true))),
        ("toggle", Box::new(|| d.toggle())),
        ("toggle", Box::new(|| d.toggle())),
        ("set_bright 50", Box::new(|| d.bright(50))),
        ("set_ct_abx 3000", Box::new(|| d.ct(3000))),
    ];
    let mut ok = 0;
    for (name, f) in &steps {
        match f() {
            Ok(v) => {
                ok += 1;
                println!("OK      {name:<18} {v}");
            }
            Err(e) => println!("ÉCHEC   {name:<18} {e}"),
        }
        std::thread::sleep(Duration::from_millis(700));
    }
    println!("{ok}/{} commandes acceptées par {}", steps.len(), d.ip);
    if ok == 0 { 1 } else { 0 }
}

fn fail(msg: &str) -> i32 {
    eprintln!("erreur : {msg}");
    1
}
