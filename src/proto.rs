//! Yeelight LAN protocol: SSDP-like discovery (UDP 1982) and JSON-over-TCP commands (port 55443).
//! Platform independent so it can be unit-tested anywhere.

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream, UdpSocket};
use std::time::{Duration, Instant};

pub const CMD_PORT: u16 = 55443;
const SSDP_ADDR: &str = "239.255.255.250:1982";
const SSDP_SEARCH: &str = "M-SEARCH * HTTP/1.1\r\nHOST: 239.255.255.250:1982\r\nMAN: \"ssdp:discover\"\r\nST: wifi_bulb\r\n";

/// What we know about the device. `None` = unknown (the Libra 2 does not answer every query).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    pub power: Option<bool>,
    pub bright: Option<u8>,
    pub ct: Option<u32>,
    pub bg_power: Option<bool>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub ip: IpAddr,
    pub model: String,
    pub name: String,
    pub fw: String,
    pub state: State,
}

/// Parse one discovery answer (`HTTP/1.1 200 OK` + `Key: value` lines).
pub fn parse_ssdp(text: &str) -> Option<Found> {
    let mut ip = None;
    let (mut model, mut name, mut fw) = (String::new(), String::new(), String::new());
    let mut st = State::default();
    for line in text.lines() {
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        let (k, v) = (k.trim().to_ascii_lowercase(), v.trim());
        match k.as_str() {
            "location" => {
                // yeelight://192.168.1.71:55443
                let host = v.trim_start_matches("yeelight://");
                let host = host.rsplit_once(':').map(|(h, _)| h).unwrap_or(host);
                ip = host.parse().ok();
            }
            "model" => model = v.to_string(),
            "name" => name = v.to_string(),
            "fw_ver" => fw = v.to_string(),
            "power" => st.power = parse_onoff(v),
            "bright" => st.bright = v.parse().ok(),
            "ct" => st.ct = v.parse().ok(),
            _ => {}
        }
    }
    Some(Found {
        ip: ip?,
        model,
        name,
        fw,
        state: st,
    })
}

fn parse_onoff(v: &str) -> Option<bool> {
    match v {
        "on" => Some(true),
        "off" => Some(false),
        _ => None,
    }
}

/// Discover devices. `hint` (a known IP) also gets a unicast probe, which survives
/// multicast being routed to the wrong adapter (Tailscale, VPN, Hyper-V…).
pub fn discover(hint: Option<IpAddr>, wait: Duration) -> Vec<Found> {
    let mut sockets = Vec::new();
    // Socket bound to the adapter that carries the default route (usually the home LAN):
    // Windows then sends the multicast out of that adapter.
    if let Some(lan) = default_lan_ip()
        && let Ok(s) = UdpSocket::bind(SocketAddr::new(lan, 0))
    {
        sockets.push(s);
    }
    if let Ok(s) = UdpSocket::bind("0.0.0.0:0") {
        sockets.push(s);
    }
    for s in &sockets {
        let _ = s.send_to(SSDP_SEARCH.as_bytes(), SSDP_ADDR);
        if let Some(h) = hint {
            let _ = s.send_to(SSDP_SEARCH.as_bytes(), SocketAddr::new(h, 1982));
        }
        let _ = s.set_read_timeout(Some(Duration::from_millis(100)));
    }
    let deadline = Instant::now() + wait;
    let mut found: Vec<Found> = Vec::new();
    let mut buf = [0u8; 2048];
    while Instant::now() < deadline {
        for s in &sockets {
            if let Ok((n, _)) = s.recv_from(&mut buf)
                && let Some(f) = parse_ssdp(&String::from_utf8_lossy(&buf[..n]))
                && !found.iter().any(|x| x.ip == f.ip)
            {
                found.push(f);
            }
        }
        // Stop early once the hinted device answered.
        if hint.is_some_and(|h| found.iter().any(|f| f.ip == h)) {
            break;
        }
    }
    found
}

/// Local IP of the adapter used for the default route (no packet is sent).
fn default_lan_ip() -> Option<IpAddr> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("8.8.8.8:80").ok()?;
    let ip = s.local_addr().ok()?.ip();
    (ip != IpAddr::V4(Ipv4Addr::UNSPECIFIED)).then_some(ip)
}

pub struct Device {
    pub ip: IpAddr,
    pub port: u16,
    pub timeout: Duration,
}

impl Device {
    pub fn new(ip: IpAddr) -> Self {
        Device {
            ip,
            port: CMD_PORT,
            timeout: Duration::from_secs(3),
        }
    }

    /// Send one command and wait for its answer, skipping `props` notifications.
    pub fn call(&self, method: &str, params: Value) -> Result<Value, String> {
        let addr = SocketAddr::new(self.ip, self.port);
        let mut s = TcpStream::connect_timeout(&addr, self.timeout)
            .map_err(|e| format!("connexion impossible à {addr} ({e})"))?;
        s.set_read_timeout(Some(self.timeout)).ok();
        s.set_write_timeout(Some(self.timeout)).ok();
        let id = 1u64;
        let line = json!({ "id": id, "method": method, "params": params }).to_string() + "\r\n";
        s.write_all(line.as_bytes())
            .map_err(|e| format!("envoi impossible ({e})"))?;
        let deadline = Instant::now() + self.timeout;
        let mut r = BufReader::new(s);
        loop {
            let mut buf = String::new();
            match r.read_line(&mut buf) {
                Ok(0) => return Err(format!("{method} : connexion fermée par l'appareil")),
                Ok(_) => {}
                Err(_) => return Err(format!("{method} : pas de réponse de l'appareil")),
            }
            if let Ok(v) = serde_json::from_str::<Value>(buf.trim())
                && v.get("id").and_then(Value::as_u64) == Some(id)
            {
                if let Some(e) = v.get("error") {
                    let msg = e.get("message").and_then(Value::as_str).unwrap_or("erreur");
                    return Err(format!("{method} refusé par l'appareil : {msg}"));
                }
                return Ok(v.get("result").cloned().unwrap_or(Value::Null));
            }
            if Instant::now() > deadline {
                return Err(format!("{method} : pas de réponse de l'appareil"));
            }
        }
    }

    pub fn toggle(&self) -> Result<Value, String> {
        self.call("toggle", json!([]))
    }
    pub fn power(&self, on: bool) -> Result<Value, String> {
        self.call("set_power", json!([onoff(on), "smooth", 300]))
    }
    pub fn bright(&self, pct: u8) -> Result<Value, String> {
        self.call("set_bright", json!([pct.clamp(1, 100), "smooth", 300]))
    }
    pub fn ct(&self, kelvin: u32) -> Result<Value, String> {
        self.call(
            "set_ct_abx",
            json!([kelvin.clamp(2700, 6500), "smooth", 300]),
        )
    }
    pub fn bg_power(&self, on: bool) -> Result<Value, String> {
        self.call("bg_set_power", json!([onoff(on), "smooth", 300]))
    }
    pub fn bg_rgb(&self, rgb: u32) -> Result<Value, String> {
        self.call("bg_set_rgb", json!([rgb & 0xFF_FFFF, "smooth", 300]))
    }
    pub fn bg_bright(&self, pct: u8) -> Result<Value, String> {
        self.call("bg_set_bright", json!([pct.clamp(1, 100), "smooth", 300]))
    }

    /// Read the state with `get_prop`; fields the device leaves empty stay `None`.
    /// Unused: the Libra 2 (fw 1.0.0) never answers it and may drop off Wi-Fi afterwards.
    #[allow(dead_code)]
    pub fn get_state(&self) -> Result<State, String> {
        let v = self.call("get_prop", json!(["power", "bright", "ct", "bg_power"]))?;
        let a = v.as_array().ok_or("get_prop : réponse inattendue")?;
        let s = |i: usize| a.get(i).and_then(Value::as_str).unwrap_or("");
        Ok(State {
            power: parse_onoff(s(0)),
            bright: s(1).parse().ok(),
            ct: s(2).parse().ok(),
            bg_power: parse_onoff(s(3)),
        })
    }
}

fn onoff(on: bool) -> &'static str {
    if on { "on" } else { "off" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;

    // Real discovery answer of a Libra 2 (firmware 1.0.0), device id anonymised.
    const LIBRA2: &str = "HTTP/1.1 200 OK\r\nCache-Control: max-age=3600\r\nDate: \r\nExt: \r\nLocation: yeelight://192.168.1.71:55443\r\nServer: POSIX UPnP/1.0 YGLC/1\r\nid: 0x0000000012ab3c4d\r\nmodel: libra2\r\nfw_ver: 1.0.0\r\nsupport: \r\npower: on\r\nbright: 100\r\ncolor_mode: 1\r\nct: 4000\r\nrgb: 16777215\r\nhue: 250\r\nsat: 250\r\nname: libra2\r\n";

    #[test]
    fn parses_real_libra2_answer() {
        let f = parse_ssdp(LIBRA2).unwrap();
        assert_eq!(f.ip, "192.168.1.71".parse::<IpAddr>().unwrap());
        assert_eq!(f.model, "libra2");
        assert_eq!(f.fw, "1.0.0");
        assert_eq!(f.state.power, Some(true));
        assert_eq!(f.state.bright, Some(100));
        assert_eq!(f.state.ct, Some(4000));
    }

    #[test]
    fn rejects_answer_without_location() {
        assert!(parse_ssdp("HTTP/1.1 200 OK\r\nmodel: x\r\n").is_none());
    }

    /// Fake device: reads one request line, replies with the given lines, returns the request.
    fn fake(replies: &'static [&'static str]) -> (Device, thread::JoinHandle<String>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let h = thread::spawn(move || {
            let (s, _) = l.accept().unwrap();
            let mut r = BufReader::new(s.try_clone().unwrap());
            let mut req = String::new();
            r.read_line(&mut req).unwrap();
            let mut w = s;
            for rep in replies {
                w.write_all(rep.as_bytes()).unwrap();
            }
            // silent device: keep the socket open past the client timeout
            thread::sleep(Duration::from_millis(if replies.is_empty() {
                1500
            } else {
                300
            }));
            req
        });
        let mut d = Device::new("127.0.0.1".parse().unwrap());
        d.port = port;
        d.timeout = Duration::from_millis(800);
        (d, h)
    }

    #[test]
    fn command_skips_notifications_and_returns_result() {
        let (d, h) = fake(&[
            "{\"method\":\"props\",\"params\":{\"power\":\"off\"}}\r\n",
            "{\"id\":1,\"result\":[\"ok\"]}\r\n",
        ]);
        assert_eq!(d.power(false).unwrap(), json!(["ok"]));
        let req: Value = serde_json::from_str(h.join().unwrap().trim()).unwrap();
        assert_eq!(
            req,
            json!({"id":1,"method":"set_power","params":["off","smooth",300]})
        );
    }

    #[test]
    fn device_error_is_reported() {
        let (d, _h) =
            fake(&["{\"id\":1,\"error\":{\"code\":-1,\"message\":\"method not supported\"}}\r\n"]);
        let e = d.bg_rgb(0xFF0000).unwrap_err();
        assert!(e.contains("method not supported"), "{e}");
    }

    #[test]
    fn silence_is_a_timeout_not_a_hang() {
        let (d, _h) = fake(&[]);
        let t = Instant::now();
        assert!(d.toggle().unwrap_err().contains("pas de réponse"));
        assert!(t.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn get_state_maps_empty_fields_to_none() {
        let (d, _h) = fake(&["{\"id\":1,\"result\":[\"on\",\"60\",\"\",\"\"]}\r\n"]);
        let s = d.get_state().unwrap();
        assert_eq!(
            s,
            State {
                power: Some(true),
                bright: Some(60),
                ct: None,
                bg_power: None
            }
        );
    }
}
