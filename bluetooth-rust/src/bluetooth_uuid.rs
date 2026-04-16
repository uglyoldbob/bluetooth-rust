//! UUID stuff for android bluetooth

#[cfg(target_os = "android")]
use super::android::Java;
#[cfg(target_os = "android")]
use super::android::jerr;
#[cfg(target_os = "android")]
use jni_min_helper::*;
#[cfg(target_os = "android")]
use std::sync::{Arc, Mutex};

/// Represents the uuid for a bluetooth service
#[derive(Debug, PartialEq)]
pub enum BluetoothUuid {
    /// Android auto
    AndroidAuto,
    /// Serial port protocol
    SPP,
    /// a2dp source
    A2dpSource,
    /// a2dp sink
    A2dpSink,
    /// base bluetooth profile
    Base,
    /// headset protocol, hs
    HspHs,
    /// headset protocol ag
    HspAg,
    /// handsfree protocol, ag
    HfpAg,
    /// Handsfree protocol, hs
    HfpHs,
    /// Obex opp protocol
    ObexOpp,
    /// Obex ftp protocol
    ObexFtp,
    /// Obex mas protocol
    ObexMas,
    /// Obex mns protocol
    ObexMns,
    /// Obex pse protocol
    ObexPse,
    /// Obex sync protocol
    ObexSync,
    /// Avrcp remote protocol
    AvrcpRemote,
    /// Network nap protocol for bluetooth networking
    NetworkingNap,
    /// An unknown bluetooth uuid
    Unknown(String),
}

impl BluetoothUuid {
    /// Get the 16-bit id
    pub fn get_16_bit_id(&self) -> u16 {
        match self {
            BluetoothUuid::SPP => 0x1101,
            BluetoothUuid::A2dpSource => 0x110a,
            BluetoothUuid::HfpHs => 0x111e,
            BluetoothUuid::ObexOpp => 0x1105,
            BluetoothUuid::ObexFtp => 0x1106,
            BluetoothUuid::ObexSync => 0x1104,
            BluetoothUuid::A2dpSink => 0x110b,
            BluetoothUuid::AvrcpRemote => 0x110e,
            BluetoothUuid::ObexPse => 0x112f,
            BluetoothUuid::HfpAg => 0x111f,
            BluetoothUuid::ObexMas => 0x1132,
            BluetoothUuid::ObexMns => 0x1133,
            BluetoothUuid::Base => 0,
            BluetoothUuid::NetworkingNap => 0x1116,
            BluetoothUuid::HspHs => 0x1108,
            BluetoothUuid::HspAg => 0x1112,
            BluetoothUuid::AndroidAuto => 0x7a00,
            BluetoothUuid::Unknown(s) => u16::from_str_radix(&s[4..8], 16).unwrap(),
        }
    }

    /// Get the uuid as a str reference
    pub fn as_str(&self) -> &str {
        match self {
            BluetoothUuid::SPP => "00001101-0000-1000-8000-00805F9B34FB",
            BluetoothUuid::A2dpSource => "0000110a-0000-1000-8000-00805f9b34fb",
            BluetoothUuid::HfpHs => "0000111e-0000-1000-8000-00805f9b34fb",
            BluetoothUuid::ObexOpp => "00001105-0000-1000-8000-00805f9b34fb",
            BluetoothUuid::ObexFtp => "00001106-0000-1000-8000-00805f9b34fb",
            BluetoothUuid::ObexSync => "00001104-0000-1000-8000-00805f9b34fb",
            BluetoothUuid::A2dpSink => "0000110b-0000-1000-8000-00805f9b34fb",
            BluetoothUuid::AvrcpRemote => "0000110e-0000-1000-8000-00805f9b34fb",
            BluetoothUuid::ObexPse => "0000112f-0000-1000-8000-00805f9b34fb",
            BluetoothUuid::HfpAg => "0000111f-0000-1000-8000-00805f9b34fb",
            BluetoothUuid::ObexMas => "00001132-0000-1000-8000-00805f9b34fb",
            BluetoothUuid::ObexMns => "00001133-0000-1000-8000-00805f9b34fb",
            BluetoothUuid::Base => "00000000-0000-1000-8000-00805f9b34fb",
            BluetoothUuid::NetworkingNap => "00001116-0000-1000-8000-00805f9b34fb",
            BluetoothUuid::HspHs => "00001108-0000-1000-8000-00805f9b34fb",
            BluetoothUuid::HspAg => "00001112-0000-1000-8000-00805f9b34fb",
            BluetoothUuid::AndroidAuto => "4de17a00-52cb-11e6-bdf4-0800200c9a66",
            BluetoothUuid::Unknown(s) => s,
        }
    }
}

impl std::str::FromStr for BluetoothUuid {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "00001101-0000-1000-8000-00805F9B34FB" => BluetoothUuid::SPP,
            "0000110a-0000-1000-8000-00805f9b34fb" => BluetoothUuid::A2dpSource,
            "0000111e-0000-1000-8000-00805f9b34fb" => BluetoothUuid::HfpHs,
            "00001105-0000-1000-8000-00805f9b34fb" => BluetoothUuid::ObexOpp,
            "00001106-0000-1000-8000-00805f9b34fb" => BluetoothUuid::ObexFtp,
            "00001104-0000-1000-8000-00805f9b34fb" => BluetoothUuid::ObexSync,
            "0000110b-0000-1000-8000-00805f9b34fb" => BluetoothUuid::A2dpSink,
            "0000110e-0000-1000-8000-00805f9b34fb" => BluetoothUuid::AvrcpRemote,
            "0000112f-0000-1000-8000-00805f9b34fb" => BluetoothUuid::ObexPse,
            "0000111f-0000-1000-8000-00805f9b34fb" => BluetoothUuid::HfpAg,
            "00001132-0000-1000-8000-00805f9b34fb" => BluetoothUuid::ObexMas,
            "00001133-0000-1000-8000-00805f9b34fb" => BluetoothUuid::ObexMns,
            "00000000-0000-1000-8000-00805f9b34fb" => BluetoothUuid::Base,
            "00001116-0000-1000-8000-00805f9b34fb" => BluetoothUuid::NetworkingNap,
            "00001108-0000-1000-8000-00805f9b34fb" => BluetoothUuid::HspHs,
            "00001112-0000-1000-8000-00805f9b34fb" => BluetoothUuid::HspAg,
            "4de17a00-52cb-11e6-bdf4-0800200c9a66" => BluetoothUuid::AndroidAuto,
            _ => BluetoothUuid::Unknown(s.to_string()),
        })
    }
}

#[cfg(target_os = "android")]
impl From<ParcelUuid> for BluetoothUuid {
    fn from(value: ParcelUuid) -> Self {
        use std::str::FromStr;
        BluetoothUuid::from_str(&value.to_string().unwrap()).unwrap()
    }
}

#[cfg(target_os = "android")]
pub struct ParcelUuid {
    internal: jni::objects::GlobalRef,
    java: Arc<Mutex<Java>>,
}

#[cfg(target_os = "android")]
impl std::fmt::Display for ParcelUuid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.to_string() {
            Ok(s) => f.write_str(&s),
            Err(e) => f.write_str(&format!("ERR: {}", e)),
        }
    }
}

#[cfg(target_os = "android")]
impl ParcelUuid {
    pub fn new(uuid: jni::objects::GlobalRef, java: Arc<Mutex<Java>>) -> Self {
        Self {
            internal: uuid,
            java,
        }
    }

    pub fn to_string(&self) -> Result<String, std::io::Error> {
        let mut java = self.java.lock().unwrap();
        java.use_env(|env, _context| {
            let dev_name = env
                .call_method(&self.internal, "toString", "()Ljava/lang/String;", &[])
                .get_object(env)
                .map_err(|e| jerr(env, e))?;
            if dev_name.is_null() {
                return Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
            }
            dev_name.get_string(env).map_err(|e| jerr(env, e))
        })
    }
}
