//! Simple UDP output for raw sample data.

use std::net::UdpSocket;

pub struct UdpSender {
    socket: UdpSocket,
    target: String,
}

impl UdpSender {
    pub fn new(target: &str) -> Result<Self, String> {
        let target = target.trim();
        if target.parse::<std::net::SocketAddr>().is_err() {
            return Err(format!(
                "invalid host:port {:?}. Example: 127.0.0.1:12345",
                target
            ));
        }
        let socket = UdpSocket::bind("0.0.0.0:0").map_err(|e| e.to_string())?;
        socket.set_nonblocking(true).ok();
        Ok(Self {
            socket,
            target: target.to_string(),
        })
    }

    pub fn send(&mut self, data: &str) -> Result<(), String> {
        self.socket
            .send_to(data.as_bytes(), &self.target)
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}
