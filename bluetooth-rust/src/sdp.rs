use libc::{c_int, sockaddr, socklen_t};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::mem;
use std::os::fd::{FromRawFd, RawFd};

const AF_BLUETOOTH: c_int = 31;
const BTPROTO_L2CAP: c_int = 0;
const SOCK_SEQPACKET: c_int = 5;

// BlueZ sockaddr_l2
#[repr(C)]
#[derive(Copy, Clone)]
struct SockAddrL2 {
    l2_family: libc::sa_family_t,
    l2_psm: u16,
    l2_bdaddr: [u8; 6],
    l2_cid: u16,
    l2_bdaddr_type: u8,
}

const BDADDR_BREDR: u8 = 0;

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
                let len = data[*offset] as usize;
                *offset += 1;
                len
            }
            6 => {
                let len = u16::from_be_bytes([data[*offset], data[*offset + 1]]) as usize;
                *offset += 2;
                len
            }
            7 => {
                let len = u32::from_be_bytes([
                    data[*offset],
                    data[*offset + 1],
                    data[*offset + 2],
                    data[*offset + 3],
                ]) as usize;
                *offset += 4;
                len
            }
            _ => return Err("invalid size".into()),
        };

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
    pub pdu_id: u8,
    pub transaction_id: u16,
    pub parameter_length: u16,
    pub attribute_lists_byte_count: u16,
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

    pub fn rfcomm_channel(&self) -> Option<u8> {
        let proto = self.attributes.get(&0x0004)?;

        let SdpElement::Sequence(layers) = proto else {
            return None;
        };

        for layer in layers {
            let SdpElement::Sequence(desc) = layer else {
                continue;
            };

            // Look for RFCOMM UUID (0x0003)
            for i in 0..desc.len() {
                if let SdpElement::Uuid(uuid) = desc[i] {
                    if uuid == 0x0003 {
                        // next element should be channel
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

        // 🔥 FIX: unwrap TWO nested SEQs
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
            pdu_id,
            transaction_id,
            parameter_length,
            attribute_lists_byte_count: attr_len,
            records,
        })
    }
}

fn parse_mac(mac: &str) -> [u8; 6] {
    let mut out = [0u8; 6];
    for (i, part) in mac.split(':').rev().enumerate() {
        out[i] = u8::from_str_radix(part, 16).unwrap();
    }
    out
}

fn build_sdp_request(txid: u16, uuid: u16) -> Vec<u8> {
    let mut out = Vec::new();

    // PDU ID = ServiceSearchAttributeRequest
    out.push(0x06);

    // Transaction ID
    out.extend_from_slice(&txid.to_be_bytes());

    let mut params = Vec::new();

    let uuid = uuid.to_be_bytes();
    // Service search pattern: UUID 0x1132 (MAP MAS)
    params.extend_from_slice(&[
        0x35, 0x03, // sequence len 3
        0x19, uuid[0], uuid[1], // UUID16
    ]);

    // Max attribute byte count
    params.extend_from_slice(&0xFFFFu16.to_be_bytes());

    // Attribute ID list: 0x0000 - 0xFFFF
    params.extend_from_slice(&[0x35, 0x05, 0x0A, 0x00, 0x00, 0xFF, 0xFF]);

    // Continuation state
    params.push(0x00);

    out.extend_from_slice(&(params.len() as u16).to_be_bytes());
    out.extend_from_slice(&params);

    out
}

pub fn run_sdp(mac: &str, uuid: u16) -> std::io::Result<ServiceRecord> {
    let fd = unsafe { libc::socket(AF_BLUETOOTH, SOCK_SEQPACKET, BTPROTO_L2CAP) };

    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }

    let addr = SockAddrL2 {
        l2_family: AF_BLUETOOTH as _,
        l2_psm: 0x0001u16.to_le(), // IMPORTANT: little-endian for kernel sockaddr
        l2_bdaddr: parse_mac(mac),
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
        return Err(std::io::Error::last_os_error());
    }

    let mut stream = unsafe { std::fs::File::from_raw_fd(fd as RawFd) };

    let req = build_sdp_request(1, uuid);
    stream.write_all(&req)?;

    let mut buf = [0u8; 4096];
    let n = stream.read(&mut buf)?;

    if let Ok(resp) = SdpResponse::parse_response(&buf[..n]) {
        if let Some(rec) = resp.records.first() {
            return Ok(rec.to_owned());
        }
    }

    Err(std::io::Error::other("Failed to find record".to_string()))
}
