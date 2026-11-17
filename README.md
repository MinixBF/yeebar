# yeebar

[![build](https://github.com/MinixBF/yeebar/actions/workflows/build.yml/badge.svg)](https://github.com/MinixBF/yeebar/actions/workflows/build.yml)
[![release](https://img.shields.io/github/v/release/MinixBF/yeebar)](https://github.com/MinixBF/yeebar/releases/latest)
[![licence MIT](https://img.shields.io/badge/licence-MIT-blue)](LICENSE)

**Pilotez votre barre d'écran Yeelight Libra 2 depuis Windows, en local, sans compte ni cloud.**

Une icône près de l'horloge pour allumer, éteindre et régler la lumière, une ligne de commande pour vos raccourcis, et une ambiance qui suit l'heure de la journée. Un seul `.exe` d'environ 600 Ko, sans installation.

> 🇬🇧 *Tiny Windows tray app + CLI to control a Yeelight Libra 2 monitor light bar over the local network (Yeelight LAN protocol). No account, no cloud, one small exe, self-updating from GitHub releases. The UI is in French.*

## Fonctions

- **Icône dans la zone de notification** : un clic gauche allume ou éteint ; un clic droit ouvre la luminosité, la température de couleur, les ambiances, la recherche de la barre et le démarrage avec Windows.
- **Ambiance selon l'heure** : au lancement, la barre prend l'ambiance du moment, puis change à chaque période de la journée.
- **Ligne de commande** pour un raccourci Windows, un Stream Deck ou AutoHotkey.
- **Mises à jour** proposées dans le menu, vérifiées par empreinte SHA-256.
- **100 % local** : l'app parle directement à la barre sur votre réseau. Rien ne passe par un compte ou un serveur Yeelight. Le seul accès Internet est la vérification de mise à jour auprès de GitHub.

## Installation

1. Téléchargez `yeebar.exe` depuis la [dernière version](https://github.com/MinixBF/yeebar/releases/latest) et rangez-le dans un dossier à vous, par exemple `C:\Outils\`. Évitez *Program Files*, où l'app ne pourrait pas se mettre à jour.
   *Ou avec winget :* `winget install MinixBF.yeebar`, puis lancez `yeebar` (et `winget upgrade MinixBF.yeebar` pour mettre à jour).
2. Dans l'app **Yeelight** du téléphone : ouvrez la barre → ⚙️ → activez **Contrôle LAN** (*LAN Control*).
3. Lancez `yeebar.exe`. L'icône apparaît près de l'horloge et la barre est trouvée automatiquement.
4. Clic droit → **Démarrer avec Windows**.

> **SmartScreen** peut avertir au premier lancement, car l'exe n'est pas signé : *Informations complémentaires* → *Exécuter quand même*. L'exe est construit par GitHub Actions à partir de ce dépôt, et son empreinte SHA-256 est publiée avec chaque version.

**Prérequis** : Windows 10 ou 11 ; le PC et la barre sur le même réseau local ; le profil réseau Windows en **Privé**, sinon le pare-feu bloque la recherche (*Paramètres → Réseau et Internet → votre connexion → Type de profil réseau*).

## Ambiances

Avec **« Ambiance selon l'heure »** cochée :

| Ambiance | Heures | Luminosité | Température |
|---|---|---|---|
| Matin | 6 h – 10 h | 70 % | 4500 K |
| Journée | 10 h – 18 h | 100 % | 5000 K |
| Soir | 18 h – 22 h | 60 % | 3000 K |
| Nuit | 22 h – 6 h | 20 % | 2700 K |

- Au lancement, l'app applique l'ambiance du moment, puis la suivante à chaque nouvelle période.
- Une barre éteinte depuis l'app reste éteinte.
- Un réglage manuel tient jusqu'à la période suivante.
- Le sous-menu **Ambiances** en applique une à la main.

Les valeurs sont définies dans [`src/ambiance.rs`](src/ambiance.rs).

## Ligne de commande

Dans PowerShell, depuis le dossier de l'exe. Le `| Out-Host` fait attendre PowerShell que la commande se termine.

```powershell
.\yeebar.exe on | Out-Host
.\yeebar.exe off | Out-Host
.\yeebar.exe toggle | Out-Host
.\yeebar.exe bright 60 | Out-Host            # luminosité 1-100
.\yeebar.exe ct 4000 | Out-Host              # température 2700-6500 K
.\yeebar.exe ambiance | Out-Host             # ambiance de l'heure
.\yeebar.exe ambiance soir | Out-Host        # matin | journee | soir | nuit
.\yeebar.exe status | Out-Host
.\yeebar.exe discover | Out-Host             # chercher les appareils
.\yeebar.exe test | Out-Host                 # diagnostic des commandes de base
.\yeebar.exe version | Out-Host
.\yeebar.exe update | Out-Host               # installer la dernière version
.\yeebar.exe --ip 192.168.1.50 on | Out-Host # forcer l'adresse
```

## Mises à jour

Peu après son lancement, puis une fois par jour, l'icône vérifie s'il existe une nouvelle version sur GitHub. Si c'est le cas, le menu affiche **« ⬆ Mettre à jour vers vX.Y.Z »**. Un clic télécharge la nouvelle version, vérifie son empreinte SHA-256, remplace l'exe et relance l'app. Rien n'est installé sans votre clic.

## Compatibilité et limites

Mesuré sur une Libra 2, firmware 1.0.0 :

| Fonction | État |
|---|---|
| Allumer / éteindre / basculer | ✅ |
| Luminosité, température (2700–6500 K) | ✅ |
| Lire l'état réel de la barre (`get_prop`) | ❌ aucune réponse |
| État dans la réponse de découverte | ❌ figé sur les valeurs du démarrage |
| Rétroéclairage RGB (`bg_set_*`) | ❌ refusé (`invalid params`) |

- **L'état affiché est la dernière commande envoyée par l'app.** Un changement fait depuis le téléphone ou le bouton de la barre n'y apparaît pas.
- **Le rétroéclairage RGB est désactivé** : le code reste dans le dépôt, mais le menu est masqué.
- **Ces commandes ne sont jamais envoyées** : sur ce firmware, la barre a parfois décroché du Wi-Fi juste après les avoir reçues.

Votre barre a un firmware plus récent qui les gère ? Ouvrez une [issue](https://github.com/MinixBF/yeebar/issues) avec le résultat de `yeebar discover` et `yeebar test`. Voir aussi [home-assistant/core#181762](https://github.com/home-assistant/core/issues/181762).

Les autres lampes Yeelight dotées du *Contrôle LAN* parlent le même protocole et devraient accepter allumage, luminosité et température. Seule la Libra 2 a été testée.

## Dépannage

| Symptôme | Piste |
|---|---|
| « barre introuvable » | Contrôle LAN activé ? Même réseau ? Profil réseau **Privé** ? Puis clic droit → *Rechercher la barre*. |
| « ne répond pas » alors qu'elle est allumée | La barre a perdu le Wi-Fi : débranchez-la et rebranchez-la. Préférez un chargeur **5 V 2 A** à un port USB de PC. |
| VPN actif (Tailscale, etc.) | Si la recherche échoue, forcez l'adresse : `.\yeebar.exe --ip <adresse> discover`. Elle sera mémorisée. |
| `yeebar` n'est pas reconnu dans PowerShell | Lancez `.\yeebar.exe …` depuis son dossier. |
| La mise à jour échoue | L'exe est-il dans un dossier protégé (*Program Files*) ? Sinon `.\yeebar.exe update` affiche l'erreur. |

Les réglages sont rangés dans `%APPDATA%\yeebar\` : `ip.txt` pour l'adresse et `ambiance.txt` pour la case *Ambiance*.

## Fonctionnement

L'app utilise le protocole LAN Yeelight (*Yeelight Inter-Operation Specification*) :

- **Découverte** : requête SSDP `M-SEARCH` en multicast vers `239.255.255.250:1982`, plus une requête directe à la dernière adresse connue.
- **Commandes** : JSON sur TCP, port `55443` (`set_power`, `set_bright`, `set_ct_abx`…).

Toutes les opérations réseau passent par un seul thread, pour que l'icône ne se fige jamais.

```
src/
├── main.rs      point d'entrée : icône sans argument, ligne de commande sinon
├── tray.rs      icône, menu, thread réseau (Windows)
├── cli.rs       ligne de commande
├── proto.rs     protocole Yeelight : découverte SSDP + commandes JSON/TCP
├── ambiance.rs  ambiances selon l'heure
├── update.rs    mise à jour depuis les Releases GitHub
└── config.rs    réglages dans %APPDATA%\yeebar
```

## Développement

```sh
cargo test                       # tests (protocole, ambiances, versions), sur tout OS
cargo clippy -- -D warnings
cargo build --release            # sous Windows : target/release/yeebar.exe
```

La CI (GitHub Actions, `windows-latest`) lance `fmt`, `clippy`, les tests et le build à chaque push et pull request.

**Publier une version** : mettez à jour `version` dans `Cargo.toml` et une section `## X.Y.Z` dans [CHANGELOG.md](CHANGELOG.md), puis :

```sh
git tag vX.Y.Z && git push --tags
```

La CI publie alors `yeebar.exe` et son `.sha256` dans les Releases, et les copies installées proposent la mise à jour.

## Licence

[MIT](LICENSE). Projet indépendant, sans lien avec Yeelight ni Xiaomi.
