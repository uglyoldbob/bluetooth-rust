//! A linux example for the bluetooth-rust crate

use std::io::{Read, Write};

use bluetooth_rust::{
    BluetoothAdapterBuilder, BluetoothAdapterTrait, BluetoothDevice, BluetoothDeviceTrait,
    BluetoothSocket, BluetoothSocketTrait,
};

pub enum MessageType {
    Email,
    SmsGsm,
    SmsCdma,
    Mms,
}

impl std::fmt::Display for MessageType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            MessageType::Email => "EMAIL",
            MessageType::SmsGsm => "SMS_GSM",
            MessageType::SmsCdma => "SMS_CDMA",
            MessageType::Mms => "MMS",
        };
        f.write_str(s)?;
        Ok(())
    }
}

impl MessageType {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "EMAIL" => Ok(Self::Email),
            "SMS_GSM" => Ok(Self::SmsGsm),
            "SMS_CDMA" => Ok(Self::SmsCdma),
            "MMS" => Ok(Self::Mms),
            _ => Err(format!("Unknown type {s}")),
        }
    }
}

#[derive(Default)]
pub struct VCard {
    version: String,
    formatted_name: Option<String>,
    name: Option<String>,
    numbers: Vec<String>,
    emails: Vec<String>,
    bt_uid: Vec<String>,
    bt_uci: Vec<String>,
}

impl std::fmt::Display for VCard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BEGIN:VCARD\r\n")?;
        f.write_str(&format!("VERSION:{}\r\n", self.version))?;
        if self.version.as_str() == "3.0" {
            if let Some(formatted_name) = &self.formatted_name {
                f.write_str(&format!("FN:{}\r\n", formatted_name))?;
            }
        }
        if let Some(name) = &self.name {
            f.write_str(&format!("N:{}\r\n", name))?;
        }
        for n in &self.numbers {
            f.write_str(&format!("TEL:{}\r\n", n))?;
        }
        for n in &self.emails {
            f.write_str(&format!("EMAIL:{}\r\n", n))?;
        }
        for n in &self.bt_uid {
            f.write_str(&format!("X-BT-UID:{}\r\n", n))?;
        }
        for n in &self.bt_uci {
            f.write_str(&format!("X-BT-UCI:{}\r\n", n))?;
        }
        f.write_str("END:VCARD\r\n")?;
        Ok(())
    }
}

impl VCard {
    pub fn parse(c: &mut std::io::Lines<std::io::Cursor<&str>>) -> Result<Self, String> {
        let mut out = Self::default();
        if let Some(Ok(line)) = c.next() {
            if line.as_str() != "BEGIN:VCARD" {
                return Err("No begin line found".to_string());
            }
        } else {
            return Err("No begin line found".to_string());
        }
        loop {
            if let Some(Ok(line)) = c.next() {
                if line.as_str() == "END:VCARD" {
                    break;
                }
                if line.starts_with("VERSION:") {
                    if let Some(v) = line.split_once(":") {
                        out.version = v.1.to_string();
                    }
                }
                if line.starts_with("FN:") {
                    if let Some(v) = line.split_once(":") {
                        out.formatted_name = Some(v.1.to_string());
                    }
                }
                if line.starts_with("N:") {
                    if let Some(v) = line.split_once(":") {
                        out.name = Some(v.1.to_string());
                    }
                }
                if line.starts_with("TEL:") {
                    if let Some(v) = line.split_once(":") {
                        out.numbers.push(v.1.to_string());
                    }
                }
                if line.starts_with("EMAIL:") {
                    if let Some(v) = line.split_once(":") {
                        out.emails.push(v.1.to_string());
                    }
                }
                if line.starts_with("X-BT-UID:") {
                    if let Some(v) = line.split_once(":") {
                        out.bt_uid.push(v.1.to_string());
                    }
                }
                if line.starts_with("X-BT-UCI:") {
                    if let Some(v) = line.split_once(":") {
                        out.bt_uci.push(v.1.to_string());
                    }
                }
            } else {
                return Err("Not enough lines found".to_string());
            }
        }
        Ok(out)
    }
}

pub struct BMessage {
    status_read: bool,
    mtype: MessageType,
    folder: String,
    originator: Vec<VCard>,
}

fn last_512(s: &str) -> String {
    let len = s.chars().count();

    s.chars().skip(len.saturating_sub(512)).collect()
}

impl std::fmt::Display for BMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BEGIN:BMSG\r\n")?;
        f.write_str("VERSION:1.0\r\n")?;
        f.write_str(&format!(
            "STATUS:{}\r\n",
            if self.status_read { "READ" } else { "UNREAD" }
        ))?;
        f.write_str(&format!("TYPE:{}\r\n", self.mtype))?;
        f.write_str(&format!("FOLDER:{}\r\n", &last_512(&self.folder)))?;
        for o in &self.originator {
            f.write_str(&format!("{}", o))?;
        }
        f.write_str("END:BMSG\r\n")?;
        Ok(())
    }
}

#[derive(Default)]
struct ObexConnect {
    version: u8,
    flags: u8,
    max_packet: u16,
    headers: Vec<Vec<u8>>,
}

impl ObexConnect {
    fn new(max_packet: u16) -> Self {
        Self {
            version: 0x10,
            flags: 0x00,
            max_packet,
            headers: vec![],
        }
    }

    /// 0x46 Target header (byte sequence)
    fn target(mut self, data: &[u8]) -> Self {
        let mut h = Vec::with_capacity(3 + data.len());

        h.push(0x46); // Target header
        h.extend_from_slice(&(data.len() as u16 + 3).to_be_bytes());
        h.extend_from_slice(data);

        self.headers.push(h);
        self
    }

    /// Add raw byte-sequence header (0x42 style)
    fn byte_seq(mut self, header_id: u8, data: &[u8]) -> Self {
        let mut h = Vec::with_capacity(3 + data.len());

        h.push(header_id);
        h.extend_from_slice(&(data.len() as u16 + 3).to_be_bytes());
        h.extend_from_slice(data);

        self.headers.push(h);
        self
    }

    fn build(self) -> Vec<u8> {
        let mut packet = Vec::new();

        // placeholder for opcode + length
        packet.push(0x80); // CONNECT
        packet.extend_from_slice(&[0x00, 0x00]); // filled later

        // fixed fields
        packet.push(self.version);
        packet.push(self.flags);
        packet.extend_from_slice(&self.max_packet.to_be_bytes());

        // headers
        for h in &self.headers {
            packet.extend_from_slice(h);
        }

        // fix length
        let len = packet.len() as u16;
        packet[1..3].copy_from_slice(&len.to_be_bytes());

        packet
    }
}

#[derive(Debug)]
pub struct ObexConnectResponse {
    pub response_code: u8,
    pub packet_length: u16,
    pub obex_version: u8,
    pub flags: u8,
    pub max_packet_length: u16,
    pub headers: Vec<ObexHeader>,
}

#[derive(Debug)]
pub enum ObexHeader {
    ConnectionId(u32),
    Who(Vec<u8>),
    Target(Vec<u8>),
    Unknown { id: u8, data: Vec<u8> },
}

#[derive(Debug)]
pub enum ObexParseError {
    UnexpectedEof,
    InvalidResponseCode(u8),
    InvalidPacketLength,
}

impl std::fmt::Display for ObexParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnexpectedEof => write!(f, "Unexpected end of data"),
            Self::InvalidResponseCode(c) => write!(f, "Invalid response code: {:#X}", c),
            Self::InvalidPacketLength => write!(f, "Packet length mismatch"),
        }
    }
}

impl std::error::Error for ObexParseError {}

impl ObexConnectResponse {
    pub fn parse(data: &[u8]) -> Result<Self, ObexParseError> {
        if data.len() < 7 {
            return Err(ObexParseError::UnexpectedEof);
        }

        let response_code = data[0];

        // Final bit (0x80) must be set; success codes are 0xA0 (200 OK)
        if response_code & 0x80 == 0 {
            return Err(ObexParseError::InvalidResponseCode(response_code));
        }

        let packet_length = u16::from_be_bytes([data[1], data[2]]);

        if data.len() < packet_length as usize {
            return Err(ObexParseError::InvalidPacketLength);
        }

        let obex_version = data[3];
        let flags = data[4];
        let max_packet_length = u16::from_be_bytes([data[5], data[6]]);

        // Parse optional headers starting at offset 7
        let headers = parse_headers(&data[7..packet_length as usize])?;

        Ok(Self {
            response_code,
            packet_length,
            obex_version,
            flags,
            max_packet_length,
            headers,
        })
    }

    pub fn is_success(&self) -> bool {
        (self.response_code & 0x7F) == 0x20 // 0xA0 with final bit masked
    }

    pub fn connection_id(&self) -> Option<u32> {
        self.headers.iter().find_map(|h| {
            if let ObexHeader::ConnectionId(id) = h {
                Some(*id)
            } else {
                None
            }
        })
    }

    pub fn who_uuid(&self) -> Option<&[u8]> {
        self.headers.iter().find_map(|h| {
            if let ObexHeader::Who(data) = h {
                Some(data.as_slice())
            } else {
                None
            }
        })
    }
}

fn parse_headers(mut data: &[u8]) -> Result<Vec<ObexHeader>, ObexParseError> {
    let mut headers = Vec::new();

    while !data.is_empty() {
        let id = data[0];
        let encoding = id & 0xC0; // top 2 bits define the header encoding

        let header = match encoding {
            // 0x00 = Unicode string (null-terminated, 2-byte length)
            // 0x40 = Byte sequence (2-byte length)
            0x00 | 0x40 => {
                if data.len() < 3 {
                    return Err(ObexParseError::UnexpectedEof);
                }
                let length = u16::from_be_bytes([data[1], data[2]]) as usize;
                if data.len() < length {
                    return Err(ObexParseError::UnexpectedEof);
                }
                let body = data[3..length].to_vec();
                data = &data[length..];

                match id {
                    0x4A => ObexHeader::Who(body),
                    0x46 => ObexHeader::Target(body),
                    _ => ObexHeader::Unknown { id, data: body },
                }
            }

            // 0x80 = 1-byte value (no length field)
            0x80 => {
                if data.len() < 2 {
                    return Err(ObexParseError::UnexpectedEof);
                }
                let byte = data[1];
                data = &data[2..];
                ObexHeader::Unknown {
                    id,
                    data: vec![byte],
                }
            }

            // 0xC0 = 4-byte value (no length field)
            0xC0 => {
                if data.len() < 5 {
                    return Err(ObexParseError::UnexpectedEof);
                }
                let value = u32::from_be_bytes([data[1], data[2], data[3], data[4]]);
                data = &data[5..];

                match id {
                    0xCB => ObexHeader::ConnectionId(value),
                    _ => ObexHeader::Unknown {
                        id,
                        data: value.to_be_bytes().to_vec(),
                    },
                }
            }

            _ => return Err(ObexParseError::UnexpectedEof),
        };

        headers.push(header);
    }

    Ok(headers)
}

#[derive(Debug)]
pub enum SetpathDirection {
    Root,          // flags = 0x03, no Name header
    Parent,        // flags = 0x02, no Name header
    Child(String), // flags = 0x00, Name header with folder name
}

impl SetpathDirection {
    pub fn build(&self, connection_id: Option<u32>) -> Vec<u8> {
        let flags: u8 = match self {
            SetpathDirection::Root => 0x03,
            SetpathDirection::Parent => 0x02,
            SetpathDirection::Child(_) => 0x00,
        };

        let mut pkt: Vec<u8> = Vec::new();

        // Opcode + placeholder length + flags + constants
        pkt.push(0x85);
        pkt.extend_from_slice(&[0x00, 0x00]); // length placeholder
        pkt.push(flags);
        pkt.push(0x00); // constants

        // Optional: Connection-ID header
        if let Some(id) = connection_id {
            pkt.push(0xCB);
            pkt.extend_from_slice(&id.to_be_bytes());
        }

        // Optional: Name header (only for Child)
        if let SetpathDirection::Child(name) = self {
            // Encode as UTF-16BE + null terminator
            let mut utf16: Vec<u8> = name.encode_utf16().flat_map(|c| c.to_be_bytes()).collect();
            utf16.extend_from_slice(&[0x00, 0x00]); // null terminator

            let header_len = (3 + utf16.len()) as u16;
            pkt.push(0x01); // Name header ID
            pkt.extend_from_slice(&header_len.to_be_bytes());
            pkt.extend_from_slice(&utf16);
        }

        // Patch in total length
        let total = pkt.len() as u16;
        pkt[1] = (total >> 8) as u8;
        pkt[2] = (total & 0xFF) as u8;

        pkt
    }
}

#[derive(Debug)]
pub struct MapGetMessagesListing {
    pub connection_id: u32,
    pub max_list_count: u16,
    pub list_start_offset: u16,
    pub filter_message_type: u8,
    pub filter_read_status: u8,
    pub subject_length: Option<u8>,
    pub filter_period_begin: Option<String>,
    pub filter_period_end: Option<String>,
}

impl MapGetMessagesListing {
    pub fn serialize(&self) -> Vec<u8> {
        let type_str = b"x-bt/MAP-msg-listing\0";

        // Build app parameters TLV blob
        let mut app_params: Vec<u8> = vec![
            0x01,
            0x02,
            (self.max_list_count >> 8) as u8,
            (self.max_list_count & 0xFF) as u8,
            0x02,
            0x02,
            (self.list_start_offset >> 8) as u8,
            (self.list_start_offset & 0xFF) as u8,
            0x07,
            0x01,
            self.filter_message_type,
            0x0A,
            0x01,
            self.filter_read_status,
        ];

        if let Some(subj_len) = self.subject_length {
            app_params.extend_from_slice(&[0x03, 0x01, subj_len]);
        }

        if let Some(ref begin) = self.filter_period_begin {
            let b = begin.as_bytes();
            app_params.push(0x08);
            app_params.push(b.len() as u8);
            app_params.extend_from_slice(b);
        }

        if let Some(ref end) = self.filter_period_end {
            let b = end.as_bytes();
            app_params.push(0x09);
            app_params.push(b.len() as u8);
            app_params.extend_from_slice(b);
        }

        let mut pkt: Vec<u8> = Vec::new();

        // Opcode
        pkt.push(0x83);

        // Placeholder for length (fill in later)
        pkt.extend_from_slice(&[0x00, 0x00]);

        // Connection-ID header (0xCB, 4-byte value, no length field)
        pkt.push(0xCB);
        pkt.extend_from_slice(&self.connection_id.to_be_bytes());

        // Type header (0x42, byte sequence with 2-byte length)
        let type_len = (3 + type_str.len()) as u16;
        pkt.push(0x42);
        pkt.extend_from_slice(&type_len.to_be_bytes());
        pkt.extend_from_slice(type_str);

        // App Parameters header (0x4C)
        let app_len = (3 + app_params.len()) as u16;
        pkt.push(0x4C);
        pkt.extend_from_slice(&app_len.to_be_bytes());
        pkt.extend_from_slice(&app_params);

        // Patch in total length
        let total_len = pkt.len() as u16;
        pkt[1] = (total_len >> 8) as u8;
        pkt[2] = (total_len & 0xFF) as u8;

        pkt
    }
}

#[derive(Debug, Clone)]
pub struct MapMessage {
    pub handle: String,
    pub subject: Option<String>,
    pub datetime: Option<String>,
    pub sender_name: Option<String>,
    pub sender_addressing: Option<String>,
    pub recipient_addressing: Option<String>,
    pub msg_type: Option<MapMessageType>,
    pub size: Option<u32>,
    pub read: bool,
    pub sent: bool,
    pub priority: bool,
    pub protected: bool,
    pub reception_status: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MapMessageType {
    SmsGsm,
    SmsCdma,
    Mms,
    Email,
    Unknown(String),
}

impl MapMessageType {
    fn from_str(s: &str) -> Self {
        match s {
            "SMS_GSM" => Self::SmsGsm,
            "SMS_CDMA" => Self::SmsCdma,
            "MMS" => Self::Mms,
            "EMAIL" => Self::Email,
            other => Self::Unknown(other.to_string()),
        }
    }
}

#[derive(Debug)]
pub struct MapListingResponse {
    pub response_code: u8,
    pub connection_id: Option<u32>,
    pub app_params: MapListingAppParams,
    pub xml_body: String,
    pub messages: Vec<MapMessage>,
}

#[derive(Debug, Default)]
pub struct MapListingAppParams {
    pub new_message: Option<bool>, // tag 0x0D
    pub mse_time: Option<String>,  // tag 0x02 — device timestamp
    pub listing_size: Option<u16>, // tag 0x11
}

impl MapListingResponse {
    pub fn parse(data: &[u8]) -> Result<Self, String> {
        if data.len() < 3 {
            return Err("Too short".into());
        }

        let response_code = data[0];
        let packet_length = u16::from_be_bytes([data[1], data[2]]) as usize;
        let data = &data[..packet_length.min(data.len())];

        let mut pos = 3;
        let mut connection_id = None;
        let mut app_params = MapListingAppParams::default();
        let mut xml_body = Vec::new();

        while pos < data.len() {
            let header_id = data[pos];
            pos += 1;

            match header_id {
                // Connection-ID — 4-byte value
                0xCB => {
                    if pos + 4 > data.len() {
                        break;
                    }
                    connection_id = Some(u32::from_be_bytes([
                        data[pos],
                        data[pos + 1],
                        data[pos + 2],
                        data[pos + 3],
                    ]));
                    pos += 4;
                }

                // Application Parameters — byte sequence
                0x4C => {
                    if pos + 2 > data.len() {
                        break;
                    }
                    let len = u16::from_be_bytes([data[pos], data[pos + 1]]) as usize;
                    pos += 2;
                    let end = pos + len - 3;
                    app_params = parse_map_listing_app_params(&data[pos..end.min(data.len())]);
                    pos = end;
                }

                // Body (0x48) or End-of-Body (0x49)
                0x48 | 0x49 => {
                    if pos + 2 > data.len() {
                        break;
                    }
                    let len = u16::from_be_bytes([data[pos], data[pos + 1]]) as usize;
                    pos += 2;
                    let data_len = len - 3;
                    if pos + data_len > data.len() {
                        break;
                    }
                    xml_body.extend_from_slice(&data[pos..pos + data_len]);
                    pos += data_len;
                }

                // Skip unknown byte-sequence headers (top 2 bits = 01)
                id if (id & 0xC0) == 0x40 => {
                    if pos + 2 > data.len() {
                        break;
                    }
                    let len = u16::from_be_bytes([data[pos], data[pos + 1]]) as usize;
                    pos += (len - 1).max(2);
                }

                // Skip unknown 4-byte headers
                id if (id & 0xC0) == 0xC0 => {
                    pos += 4;
                }

                // Skip unknown 1-byte headers
                id if (id & 0xC0) == 0x80 => {
                    pos += 1;
                }

                _ => break,
            }
        }

        let xml_str =
            String::from_utf8(xml_body).map_err(|e| format!("Invalid UTF-8 in XML: {}", e))?;

        let messages = parse_msg_listing_xml(&xml_str);

        Ok(Self {
            response_code,
            connection_id,
            app_params,
            xml_body: xml_str,
            messages,
        })
    }

    pub fn is_success(&self) -> bool {
        (self.response_code & 0x7F) == 0x20
    }
}

fn parse_map_listing_app_params(data: &[u8]) -> MapListingAppParams {
    let mut params = MapListingAppParams::default();
    let mut pos = 0;

    while pos + 2 <= data.len() {
        let tag = data[pos];
        let len = data[pos + 1] as usize;
        pos += 2;

        if pos + len > data.len() {
            break;
        }
        let value = &data[pos..pos + len];
        pos += len;

        match tag {
            0x0D => {
                // NewMessage
                params.new_message = Some(value.first().copied().unwrap_or(0) != 0);
            }
            0x12 => {
                // ListingSize — it's 0x12, not 0x11!
                if value.len() >= 2 {
                    params.listing_size = Some(u16::from_be_bytes([value[0], value[1]]));
                }
            }
            0x19 => {
                // MseTime — it's 0x19, not 0x02!
                params.mse_time = String::from_utf8(value.to_vec()).ok();
            }
            _ => {}
        }
    }

    params
}

fn parse_msg_listing_xml(xml: &str) -> Vec<MapMessage> {
    let mut messages = Vec::new();
    let mut search = xml;

    while let Some(start) = search.find("<msg ") {
        search = &search[start..];
        match search.find("/>") {
            Some(end) => {
                let tag = &search[..end + 2];
                if let Ok(msg) = parse_msg_tag(tag) {
                    messages.push(msg);
                }
                search = &search[end + 2..];
            }
            None => break,
        }
    }

    messages
}

fn parse_msg_tag(tag: &str) -> Result<MapMessage, String> {
    let get = |attr: &str| -> Option<String> {
        let needle = format!("{}=\"", attr);
        let start = tag.find(&needle)? + needle.len();
        let end = tag[start..].find('"')? + start;
        Some(tag[start..end].to_string())
    };

    Ok(MapMessage {
        handle: get("handle").ok_or("missing handle")?,
        subject: get("subject"),
        datetime: get("datetime"),
        sender_name: get("sender_name"),
        sender_addressing: get("sender_addressing"),
        recipient_addressing: get("recipient_addressing"),
        msg_type: get("type").map(|t| MapMessageType::from_str(&t)),
        size: get("size").and_then(|s| s.parse().ok()),
        read: get("read")
            .map(|v| v.eq_ignore_ascii_case("yes"))
            .unwrap_or(false),
        sent: get("sent")
            .map(|v| v.eq_ignore_ascii_case("yes"))
            .unwrap_or(false),
        priority: get("priority")
            .map(|v| v.eq_ignore_ascii_case("yes"))
            .unwrap_or(false),
        protected: get("protected")
            .map(|v| v.eq_ignore_ascii_case("yes"))
            .unwrap_or(false),
        reception_status: get("reception_status"),
    })
}

struct MessageClient {
    socket: BluetoothSocket,
    message_handle: u32,
}

impl MessageClient {
    pub fn new(mut socket: BluetoothSocket) -> Self {
        log::info!("Socket is connected? {:?}", socket.is_connected());
        std::thread::sleep(std::time::Duration::from_millis(100));
        // this is the value for mas obex target
        let mas_obex_uuid = [
            0xBB, 0x58, 0x2B, 0x40, 0x42, 0x0C, 0x11, 0xDB, 0xB0, 0xDE, 0x08, 0x00, 0x20, 0x0C,
            0x9A, 0x66,
        ];

        let packet = ObexConnect::new(0x1000).target(&mas_obex_uuid).build();
        let a = socket.write_all(&packet);
        let b = socket.flush();
        log::info!("Sent data: {:?} {:?} {:x?}", a, b, packet);
        let mut buf = [0u8; 1024];
        let mut message_handle = 0;
        if let Ok(a) = socket.read(&mut buf) {
            if a > 0 {
                log::info!("READ DATA {:?} BYTES {:x?}", a, &buf[0..a]);
                let resp = ObexConnectResponse::parse(&buf[0..a]);
                log::info!("Response is {:x?}", resp);
                if let Ok(r) = resp {
                    if let Some(i) = r.connection_id() {
                        message_handle = i;
                    }
                }
            }
        }
        Self {
            socket,
            message_handle,
        }
    }

    pub fn setpath(&mut self, p: &str) {
        let p = SetpathDirection::Child(p.to_string()).build(Some(self.message_handle));
        self.socket.write_all(&p);
        self.socket.flush();
        let mut buf = [0u8; 1024];
        if let Ok(a) = self.socket.read(&mut buf) {
            log::info!("READ DATA {:?} BYTES {:x?}", a, &buf[0..a]);
        }
    }

    pub fn get_messages(&mut self) {
        let req = MapGetMessagesListing {
            connection_id: self.message_handle,
            max_list_count: 127,
            list_start_offset: 0,
            filter_message_type: 0x00, // all types
            filter_read_status: 0x00,  // all messages
            subject_length: Some(50),
            filter_period_begin: None,
            filter_period_end: None,
        };
        let p = req.serialize();
        log::info!("Sending get message listing");
        self.socket.write_all(&p);
        self.socket.flush();
        let mut buf = [0u8; 1024];
        if let Ok(a) = self.socket.read(&mut buf) {
            log::info!("READ DATA {:?} BYTES {:x?}", a, &buf[0..a]);
            match MapListingResponse::parse(&buf[0..a]) {
                Ok(resp) => {
                    log::info!("LISTING: {:?}", resp);
                    println!("Success:       {}", resp.is_success());
                    println!("Connection ID: {:?}", resp.connection_id);
                    println!("New Message:   {:?}", resp.app_params.new_message);
                    println!("Device Time:   {:?}", resp.app_params.mse_time);
                    println!("Message Count: {}", resp.messages.len());
                    println!("XML:\n{}", resp.xml_body);
                }
                Err(e) => eprintln!("Error: {}", e),
            }
        }
    }
}

fn try_map_connect(dev: &mut BluetoothDevice, channel: u8) -> Result<BluetoothSocket, String> {
    if let Ok(mut socket) = dev.get_rfcomm_socket(channel, true) {
        match socket.connect() {
            Ok(_) => {
                log::info!("Got a socket");
                Ok(socket)
            }
            Err(e) => Err(format!("Socket connect not good {}", e)),
        }
    } else {
        Err("Failed to get rfcomm socket".to_string())
    }
}

#[tokio::main]
async fn main() -> Result<(), String> {
    simple_logger::SimpleLogger::new()
        .with_level(log::LevelFilter::Info)
        .init()
        .expect("Failed to init log");
    let mut ba = BluetoothAdapterBuilder::new();
    let (s, r) = tokio::sync::mpsc::channel(10);
    ba.with_sender(s);
    let adapter = ba.async_build().await.map_err(|e| e.to_string())?;
    let mut macs = Vec::new();
    if let Some(a) = adapter.supports_async() {
        if let Some(devs) = a.get_paired_devices() {
            for mut dev in devs {
                match dev.get_uuids() {
                    Ok(uuids) => {
                        if uuids.contains(&bluetooth_rust::BluetoothUuid::ObexMas) {
                            let channel = if let Ok(a) =
                                dev.run_sdp(bluetooth_rust::BluetoothUuid::ObexMas)
                            {
                                if let Some(channel) = a.rfcomm_channel() {
                                    channel
                                } else {
                                    1
                                }
                            } else {
                                1
                            };
                            for _ in 0..1 {
                                match try_map_connect(&mut dev, channel) {
                                    Ok(s) => {
                                        log::info!("Got a bluetoothsocket");
                                        macs.push(s);
                                        break;
                                    }
                                    Err(e) => {
                                        log::error!("Error trying to connect map: {}", e);
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => log::error!("Error getting uuids: {}", e),
                }
            }
        }
    }
    for s in macs {
        log::info!("Building a map message client");
        let mut client = MessageClient::new(s);
        client.setpath("telecom");
        client.setpath("msg");
        client.setpath("INBOX");
        client.get_messages();
    }
    Ok(())
}
