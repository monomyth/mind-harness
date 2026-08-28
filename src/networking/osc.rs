//! OSC output support using the `rosc` crate.

use rosc::{OscMessage, OscPacket, OscType};
use std::net::UdpSocket;

pub struct OscSender {
    socket: UdpSocket,
    target: String,
}

impl OscSender {
    pub fn new(target: &str) -> Result<Self, String> {
        let target = target.trim();
        if target.parse::<std::net::SocketAddr>().is_err() {
            return Err(format!(
                "invalid host:port {:?}. Example: 127.0.0.1:9000",
                target
            ));
        }
        let socket = UdpSocket::bind("0.0.0.0:0").map_err(|e| e.to_string())?;
        Ok(Self {
            socket,
            target: target.to_string(),
        })
    }

    pub fn send_eeg_sample(&mut self, sample: &[f64]) -> Result<(), String> {
        let args: Vec<OscType> = sample.iter().map(|&v| OscType::Float(v as f32)).collect();

        let msg = OscPacket::Message(OscMessage {
            addr: "/openbci/eeg".to_string(),
            args,
        });

        let buf = rosc::encoder::encode(&msg).map_err(|e| e.to_string())?;
        self.socket
            .send_to(&buf, &self.target)
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn send_marker(&mut self, timestamp: f64, marker: &str) -> Result<(), String> {
        let msg = OscPacket::Message(OscMessage {
            addr: "/openbci/marker".to_string(),
            args: vec![
                OscType::Double(timestamp),
                OscType::String(marker.to_string()),
            ],
        });

        let buf = rosc::encoder::encode(&msg).map_err(|e| e.to_string())?;
        self.socket
            .send_to(&buf, &self.target)
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}
