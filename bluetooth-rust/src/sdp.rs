use std::collections::BTreeMap;

// ─────────────────────────────────────────────────────────────────────────────
// Shared types
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum SdpElement {
    Nil,
    UInt(u128),
    Int(i128),
    Uuid(u128),
    Str(String),
    Bool(bool),
    Sequence(Vec<SdpElement>),
    Alternative(Vec<SdpElement>),
    Url(String),
    Raw(Vec<u8>),
}

impl SdpElement {
    fn parse_element(data: &[u8], offset: &mut usize) -> Result<Self, String> {
        if *offset >= data.len() {
            return Err("unexpected end of data".into());
        }

        let header = data[*offset];
        *offset += 1;

        let dtype = header >> 3;
        let size_idx = header & 0x07;

        let size = match size_idx {
            0 => 1,
            1 => 2,
            2 => 4,
            3 => 8,
            4 => 16,
            5 => {
                if *offset >= data.len() {
                    return Err("unexpected end of data (size byte)".into());
                }
                let len = data[*offset] as usize;
                *offset += 1;
                len
            }
            6 => {
                if *offset + 1 >= data.len() {
                    return Err("unexpected end of data (size u16)".into());
                }
                let len = u16::from_be_bytes([data[*offset], data[*offset + 1]]) as usize;
                *offset += 2;
                len
            }
            7 => {
                if *offset + 3 >= data.len() {
                    return Err("unexpected end of data (size u32)".into());
                }
                let len = u32::from_be_bytes([
                    data[*offset],
                    data[*offset + 1],
                    data[*offset + 2],
                    data[*offset + 3],
                ]) as usize;
                *offset += 4;
                len
            }
            _ => return Err("invalid size descriptor".into()),
        };

        if *offset + size > data.len() {
            return Err(format!(
                "element extends past buffer (offset={}, size={}, buf={})",
                *offset,
                size,
                data.len()
            ));
        }

        match dtype {
            1 => {
                // unsigned int
                let bytes = &data[*offset..*offset + size];
                *offset += size;
                let mut v = 0u128;
                for b in bytes {
                    v = (v << 8) | (*b as u128);
                }
                Ok(SdpElement::UInt(v))
            }
            3 => {
                // UUID
                let bytes = &data[*offset..*offset + size];
                *offset += size;
                let mut v = 0u128;
                for b in bytes {
                    v = (v << 8) | (*b as u128);
                }
                Ok(SdpElement::Uuid(v))
            }
            4 => {
                // string
                let bytes = &data[*offset..*offset + size];
                *offset += size;
                Ok(SdpElement::Str(String::from_utf8_lossy(bytes).into_owned()))
            }
            6 => {
                // sequence
                let end = *offset + size;
                let mut items = Vec::new();
                while *offset < end {
                    items.push(SdpElement::parse_element(data, offset)?);
                }
                Ok(SdpElement::Sequence(items))
            }
            _ => {
                let bytes = data[*offset..*offset + size].to_vec();
                *offset += size;
                Ok(SdpElement::Raw(bytes))
            }
        }
    }
}

#[derive(Debug)]
pub struct SdpResponse {
    pub _pdu_id: u8,
    pub _transaction_id: u16,
    pub _parameter_length: u16,
    pub _attribute_lists_byte_count: u16,
    pub records: Vec<ServiceRecord>,
}

#[derive(Clone, Debug)]
pub struct ServiceRecord {
    pub attributes: BTreeMap<u16, SdpElement>,
}

impl ServiceRecord {
    fn parse_service_record(seq: Vec<SdpElement>) -> Result<Self, String> {
        let mut attrs = BTreeMap::new();
        let mut i = 0;
        while i + 1 < seq.len() {
            let attr_id = match &seq[i] {
                SdpElement::UInt(v) => *v as u16,
                _ => {
                    i += 1;
                    continue;
                }
            };
            let value = seq[i + 1].clone();
            attrs.insert(attr_id, value);
            i += 2;
        }
        Ok(ServiceRecord { attributes: attrs })
    }

    /// Get the rfcomm channel for the record
    pub fn rfcomm_channel(&self) -> Option<u8> {
        let proto = self.attributes.get(&0x0004)?;
        let SdpElement::Sequence(layers) = proto else {
            return None;
        };
        for layer in layers {
            let SdpElement::Sequence(desc) = layer else {
                continue;
            };
            for i in 0..desc.len() {
                if let SdpElement::Uuid(uuid) = desc[i] {
                    if uuid == 0x0003 {
                        if let Some(SdpElement::UInt(ch)) = desc.get(i + 1) {
                            return Some(*ch as u8);
                        }
                    }
                }
            }
        }
        None
    }
}

impl SdpResponse {
    fn parse_response(data: &[u8]) -> Result<Self, String> {
        if data.len() < 7 {
            return Err("response too short".into());
        }
        let pdu_id = data[0];
        let transaction_id = u16::from_be_bytes([data[1], data[2]]);
        let parameter_length = u16::from_be_bytes([data[3], data[4]]);
        let attr_len = u16::from_be_bytes([data[5], data[6]]);

        let mut offset = 7;
        let record_elem = SdpElement::parse_element(data, &mut offset)?;

        let records = match record_elem {
            SdpElement::Sequence(list) => {
                let mut out = Vec::new();
                for item in list {
                    if let SdpElement::Sequence(attr_list) = item {
                        out.push(ServiceRecord::parse_service_record(attr_list)?);
                    }
                }
                out
            }
            _ => return Err("expected outer sequence".into()),
        };

        Ok(SdpResponse {
            _pdu_id: pdu_id,
            _transaction_id: transaction_id,
            _parameter_length: parameter_length,
            _attribute_lists_byte_count: attr_len,
            records,
        })
    }
}

fn parse_mac(mac: &str) -> Result<[u8; 6], std::io::Error> {
    let mut out = [0u8; 6];
    let parts: Vec<&str> = mac.split(':').collect();
    if parts.len() != 6 {
        return Err(std::io::Error::other("invalid MAC address format"));
    }
    for (i, part) in parts.iter().rev().enumerate() {
        out[i] = u8::from_str_radix(part, 16)
            .map_err(|_| std::io::Error::other("invalid MAC address hex byte"))?;
    }
    Ok(out)
}

fn build_sdp_request(txid: u16, uuid: u16) -> Vec<u8> {
    let mut out = Vec::new();

    // PDU ID = ServiceSearchAttributeRequest (0x06)
    out.push(0x06);

    // Transaction ID
    out.extend_from_slice(&txid.to_be_bytes());

    let mut params = Vec::new();

    let uuid_bytes = uuid.to_be_bytes();
    // Service search pattern: UUID16
    params.extend_from_slice(&[0x35, 0x03, 0x19, uuid_bytes[0], uuid_bytes[1]]);

    // Max attribute byte count
    params.extend_from_slice(&0xFFFFu16.to_be_bytes());

    // Attribute ID list: 0x0000–0xFFFF
    params.extend_from_slice(&[0x35, 0x05, 0x0A, 0x00, 0x00, 0xFF, 0xFF]);

    // Continuation state: none
    params.push(0x00);

    out.extend_from_slice(&(params.len() as u16).to_be_bytes());
    out.extend_from_slice(&params);
    out
}

// ─────────────────────────────────────────────────────────────────────────────
// Linux implementation  (BlueZ / L2CAP raw socket)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    use libc::{c_int, sockaddr, socklen_t};
    use std::io::{Read, Write};
    use std::mem;
    use std::os::fd::{FromRawFd, RawFd};

    const AF_BLUETOOTH: c_int = 31;
    const BTPROTO_L2CAP: c_int = 0;
    const SOCK_SEQPACKET: c_int = 5;
    const BDADDR_BREDR: u8 = 0;

    #[repr(C)]
    #[derive(Copy, Clone)]
    struct SockAddrL2 {
        l2_family: libc::sa_family_t,
        l2_psm: u16,
        l2_bdaddr: [u8; 6],
        l2_cid: u16,
        l2_bdaddr_type: u8,
    }

    /// Run the sdp algorithm
    pub fn run_sdp(mac: &str, uuid: u16) -> std::io::Result<ServiceRecord> {
        let bdaddr = parse_mac(mac)?;

        let fd = unsafe { libc::socket(AF_BLUETOOTH, SOCK_SEQPACKET, BTPROTO_L2CAP) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }

        let addr = SockAddrL2 {
            l2_family: AF_BLUETOOTH as _,
            l2_psm: 0x0001u16.to_le(), // SDP PSM; must be little-endian in BlueZ sockaddr
            l2_bdaddr: bdaddr,
            l2_cid: 0,
            l2_bdaddr_type: BDADDR_BREDR,
        };

        let ret = unsafe {
            libc::connect(
                fd,
                &addr as *const _ as *const sockaddr,
                mem::size_of::<SockAddrL2>() as socklen_t,
            )
        };

        if ret < 0 {
            let err = std::io::Error::last_os_error();
            unsafe { libc::close(fd) };
            return Err(err);
        }

        // SAFETY: we own the fd and it is valid and connected
        let mut stream = unsafe { std::fs::File::from_raw_fd(fd as RawFd) };

        let req = build_sdp_request(1, uuid);
        stream.write_all(&req)?;

        let mut buf = [0u8; 4096];
        let n = stream.read(&mut buf)?;

        match SdpResponse::parse_response(&buf[..n]) {
            Ok(resp) => resp
                .records
                .into_iter()
                .next()
                .ok_or_else(|| std::io::Error::other("no SDP records found")),
            Err(e) => Err(std::io::Error::other(e)),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Windows implementation  (WinSock2 Bluetooth / ws2bth)
//
// Requires:  windows = { version = "0.58", features = [
//   "Win32_Networking_WinSock",
//   "Win32_NetworkManagement_Bluetooth",
// ] }
// in Cargo.toml.
//
// The Windows Bluetooth stack exposes a raw L2CAP SDP socket through
// AF_BTH / BTHPROTO_L2CAP with PSM 1 exactly like BlueZ, but uses
// SOCKADDR_BTH and WinSock initialisation.
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
mod platform {
    use super::*;
    use std::io::{Read, Write};

    use windows::Win32::Devices::Bluetooth::{
        AF_BTH, BTHPROTO_L2CAP, SOCKADDR_BTH,
    };
    use windows::Win32::Networking::WinSock::{
        closesocket, connect, recv, send, socket, WSACleanup, WSAStartup, INVALID_SOCKET, SOCK_SEQPACKET,
        SOCKET, WSADATA,
    };

    fn parse_mac_as_u64(mac: &str) -> Result<u64, std::io::Error> {
        // u64 is a u64; bytes go in MSB-first (same order as MAC string)
        let parts: Vec<&str> = mac.split(':').collect();
        if parts.len() != 6 {
            return Err(std::io::Error::other("invalid MAC address format"));
        }
        let mut addr: u64 = 0;
        for part in &parts {
            let byte = u8::from_str_radix(part, 16)
                .map_err(|_| std::io::Error::other("invalid MAC address hex byte"))?;
            addr = (addr << 8) | (byte as u64);
        }
        Ok(addr)
    }

    /// A minimal RAII wrapper so the socket is closed on drop.
    struct OwnedSocket(SOCKET);
    impl Drop for OwnedSocket {
        fn drop(&mut self) {
            unsafe { closesocket(self.0) };
        }
    }

    pub fn run_sdp(mac: &str, uuid: u16) -> std::io::Result<ServiceRecord> {
        // ── 1. Initialise WinSock ──────────────────────────────────────────
        let mut wsa_data = WSADATA::default();
        let rc = unsafe { WSAStartup(0x0202 /* v2.2 */, &mut wsa_data) };
        if rc != 0 {
            return Err(std::io::Error::from_raw_os_error(rc));
        }
        // WSACleanup on exit (best-effort; ignore error)
        struct WsaGuard;
        impl Drop for WsaGuard {
            fn drop(&mut self) {
                unsafe { WSACleanup() };
            }
        }
        let _wsa = WsaGuard;

        // ── 2. Create socket ──────────────────────────────────────────────
        let sock = unsafe {
            socket(
                AF_BTH as i32,
                SOCK_SEQPACKET,
                BTHPROTO_L2CAP as i32,
            )
        };
        if Ok(INVALID_SOCKET) == sock {
            return Err(std::io::Error::last_os_error());
        }
        let sock = OwnedSocket(sock.unwrap());

        // ── 3. Connect ────────────────────────────────────────────────────
        let u64 = parse_mac_as_u64(mac)?;

        let addr = SOCKADDR_BTH {
            addressFamily: AF_BTH,
            btAddr: u64,
            serviceClassId: windows::core::GUID::zeroed(),
            port: 1, // SDP PSM
        };

        let ret = unsafe {
            connect(
                sock.0,
                &addr as *const _ as *const windows::Win32::Networking::WinSock::SOCKADDR,
                std::mem::size_of::<SOCKADDR_BTH>() as i32,
            )
        };
        if ret != 0 {
            return Err(std::io::Error::last_os_error());
        }

        // ── 4. Send SDP request ───────────────────────────────────────────
        let req = build_sdp_request(1, uuid);
        let mut sent = 0usize;
        while sent < req.len() {
            let n = unsafe {
                send(
                    sock.0,
                    &req[sent..],
                    windows::Win32::Networking::WinSock::SEND_RECV_FLAGS(0),
                )
            };
            if n < 0 {
                return Err(std::io::Error::last_os_error());
            }
            sent += n as usize;
        }

        // ── 5. Receive SDP response ───────────────────────────────────────
        let mut buf = [0u8; 4096];
        let n = unsafe { recv(sock.0, &mut buf, windows::Win32::Networking::WinSock::SEND_RECV_FLAGS(0)) };
        if n < 0 {
            return Err(std::io::Error::last_os_error());
        }

        // ── 6. Parse ──────────────────────────────────────────────────────
        match SdpResponse::parse_response(&buf[..n as usize]) {
            Ok(resp) => resp
                .records
                .into_iter()
                .next()
                .ok_or_else(|| std::io::Error::other("no SDP records found")),
            Err(e) => Err(std::io::Error::other(e)),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Unsupported platform stub
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
mod platform {
    use super::*;

    pub fn run_sdp(_mac: &str, _uuid: u16) -> std::io::Result<ServiceRecord> {
        Err(std::io::Error::other(
            "Bluetooth SDP is only supported on Linux and Windows",
        ))
    }
}

/// run the service discovery protocol for the given bluetooth hardware address and uuid short form
pub fn run_sdp(mac: &str, uuid: u16) -> std::io::Result<ServiceRecord> {
    platform::run_sdp(mac, uuid)
}