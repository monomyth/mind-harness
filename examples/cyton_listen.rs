use std::io::{Read, Write};
use std::time::{Duration, Instant};

fn dump(label: &str, acc: &[u8]) {
    let text: String = acc.iter().map(|&b| {
        if (32..127).contains(&b) || b == b'\n' || b == b'\r' { b as char } else { '.' }
    }).collect();
    println!("{label} n={} text={:?} hex={:02x?}", label, text, acc.iter().take(64).cloned().collect::<Vec<_>>());
}

fn try_port(port: &str, baud: u32, dtr: bool) {
    println!("--- open {port} baud={baud} dtr={dtr}");
    let mut serial = match serialport::new(port, baud)
        .timeout(Duration::from_millis(150))
        .dtr_on_open(dtr)
        .open()
    {
        Ok(s) => s,
        Err(e) => {
            println!("open failed: {e}");
            return;
        }
    };
    let mut buf = [0u8; 512];
    let mut acc = Vec::new();
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_millis(400) {
        match serial.read(&mut buf) {
            Ok(n) if n > 0 => acc.extend_from_slice(&buf[..n]),
            _ => std::thread::sleep(Duration::from_millis(40)),
        }
    }
    dump("idle", &acc);

    for cmd in [b"s" as &[u8], b"v", b"?"] {
        acc.clear();
        let _ = serial.write_all(cmd);
        let _ = serial.flush();
        let t = Instant::now();
        while t.elapsed() < Duration::from_secs(3) {
            match serial.read(&mut buf) {
                Ok(n) if n > 0 => acc.extend_from_slice(&buf[..n]),
                _ => std::thread::sleep(Duration::from_millis(40)),
            }
            if acc.windows(3).any(|w| w == b"$$$") {
                break;
            }
        }
        dump(&format!("after {}", std::str::from_utf8(cmd).unwrap()), &acc);
        if let Ok(s) = std::str::from_utf8(&acc) {
            if s.to_ascii_lowercase().contains("batt") || s.contains('%') {
                println!("battery-like text: {s}");
            }
        }
    }
}

fn main() {
    let port = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: cyton_listen <port>");
        std::process::exit(2);
    });
    try_port(&port, 115200, false);
}
