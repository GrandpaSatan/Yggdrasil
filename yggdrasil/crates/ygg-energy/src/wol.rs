use std::net::UdpSocket;

/// Send a Wake-on-LAN magic packet to the specified MAC address.
pub fn send_wol(mac: &str, broadcast: &str) -> Result<(), WolError> {
    let mac_bytes = parse_mac(mac)?;

    // WoL magic packet: 6 bytes of 0xFF followed by 16 repetitions of the MAC
    let mut packet = vec![0xFF_u8; 6];
    for _ in 0..16 {
        packet.extend_from_slice(&mac_bytes);
    }

    let socket = UdpSocket::bind("0.0.0.0:0").map_err(|e| WolError::Socket(e.to_string()))?;
    socket
        .set_broadcast(true)
        .map_err(|e| WolError::Socket(e.to_string()))?;
    socket
        .send_to(&packet, format!("{}:9", broadcast))
        .map_err(|e| WolError::Send(e.to_string()))?;

    Ok(())
}

fn parse_mac(mac: &str) -> Result<[u8; 6], WolError> {
    let parts: Vec<&str> = mac.split([':', '-']).collect();
    if parts.len() != 6 {
        return Err(WolError::InvalidMac(mac.to_string()));
    }

    let mut bytes = [0u8; 6];
    for (i, part) in parts.iter().enumerate() {
        bytes[i] =
            u8::from_str_radix(part, 16).map_err(|_| WolError::InvalidMac(mac.to_string()))?;
    }
    Ok(bytes)
}

#[derive(Debug, thiserror::Error)]
pub enum WolError {
    #[error("invalid MAC address: {0}")]
    InvalidMac(String),

    #[error("socket error: {0}")]
    Socket(String),

    #[error("send error: {0}")]
    Send(String),
}
