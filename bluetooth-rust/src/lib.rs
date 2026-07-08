#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]
#![warn(unused_extern_crates)]

//! This library is intended to eventually be a cross-platform bluetooth handling platform

#[cfg(not(any(feature = "sync", feature = "async")))]
compile_error!("Must enable either the `sync` or `async` feature");

#[cfg(all(feature = "sync", feature = "async"))]
compile_error!("Cannot enable both `sync` and `async` features simultaneously");

#[cfg(target_os = "android")]
pub mod android;
#[cfg(target_os = "android")]
pub use android::Bluetooth;
#[cfg(target_os = "android")]
use winit::platform::android::activity::AndroidApp;

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "windows")]
mod windows;

mod bluetooth_uuid;
pub use bluetooth_uuid::BluetoothUuid;

#[cfg(not(target_os = "android"))]
mod sdp;

#[cfg(not(target_os = "android"))]
pub use sdp::run_sdp;

#[cfg(target_os = "android")]
type ServiceRecord = ();

/// Commands issued to the library
#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub enum BluetoothCommand {
    /// Detect all bluetooth adapters present on the system
    DetectAdapters,
    /// Find out how many bluetooth adapters are detected
    QueryNumAdapters,
}

/// Messages that can be sent specifically to the app user hosting the bluetooth controls
pub enum MessageToBluetoothHost {
    #[cfg(feature = "async")]
    /// The passkey used for pairing devices
    DisplayPasskey(u32, tokio::sync::mpsc::Sender<ResponseToPasskey>),
    #[cfg(feature = "async")]
    /// The passkey to confirm for pairing
    ConfirmPasskey(u32, tokio::sync::mpsc::Sender<ResponseToPasskey>),
    /// Cancal the passkey display
    CancelDisplayPasskey,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
/// Messages that are send directly from the bluetooth host
pub enum MessageFromBluetoothHost {
    /// A response about the active pairing passkey
    PasskeyMessage(ResponseToPasskey),
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
/// The user response to a bluetooth passkey
pub enum ResponseToPasskey {
    /// The passkey is accepted
    Yes,
    /// The passkey is not accepted
    No,
    /// The process is canceled by the user
    Cancel,
    /// Waiting on the user to decide
    Waiting,
}

/// Responses issued by the library
pub enum BluetoothResponse {
    /// The number of bluetooth adapters detected
    Adapters(usize),
}

/// Settings for an rfcomm profile
#[derive(Clone, Debug)]
pub struct BluetoothRfcommProfileSettings {
    /// The uuid for the profile
    pub uuid: String,
    /// User readable name for the profile
    pub name: Option<String>,
    /// The service uuid for the profile (can be the same as service)
    pub service_uuid: Option<String>,
    /// The channel to use
    pub channel: Option<u16>,
    /// PSM number used for UUIDS and SDP (if applicable)
    pub psm: Option<u16>,
    /// Is authentication required for a connection
    pub authenticate: Option<bool>,
    /// Is authorization required for a connection
    pub authorize: Option<bool>,
    /// For client profiles, This will force connection of the channel when a remote device is connected
    pub auto_connect: Option<bool>,
    /// manual SDP record
    pub sdp_record: Option<String>,
    /// SDP version
    pub sdp_version: Option<u16>,
    /// SDP profile features
    pub sdp_features: Option<u16>,
}

/// Settings for an rfcomm profile
#[derive(Clone)]
pub struct BluetoothL2capProfileSettings {
    /// The uuid for the profile
    pub uuid: String,
    /// User readable name for the profile
    pub name: Option<String>,
    /// The service uuid for the profile (can be the same as service)
    pub service_uuid: Option<String>,
    /// The channel to use
    pub channel: Option<u16>,
    /// PSM number used for UUIDS and SDP (if applicable)
    pub psm: Option<u16>,
    /// Is authentication required for a connection
    pub authenticate: Option<bool>,
    /// Is authorization required for a connection
    pub authorize: Option<bool>,
    /// For client profiles, This will force connection of the channel when a remote device is connected
    pub auto_connect: Option<bool>,
    /// manual SDP record
    pub sdp_record: Option<String>,
    /// SDP version
    pub sdp_version: Option<u16>,
    /// SDP profile features
    pub sdp_features: Option<u16>,
}

/// Used for discovering bluetooth devices
#[cfg(target_os = "android")]
pub type BluetoothDiscovery = android::BluetoothDiscovery;

/// Used for discovering bluetooth devices
#[cfg(target_os = "linux")]
pub type BluetoothDiscovery = linux::BluetoothDiscovery;

/// Used for discovering bluetooth devices
#[cfg(target_os = "windows")]
pub type BluetoothDiscovery = windows::BluetoothDiscovery;

/// The address of a bluetooth adapter
pub enum BluetoothAdapterAddress {
    /// The address in string form
    String(String),
    /// The address in byte form
    Byte([u8; 6]),
}

/// The pairing status of a bluetooth device
pub enum PairingStatus {
    /// The device is not paired
    NotPaired,
    /// The device is in the pairing process
    Pairing,
    /// The device is paired
    Paired,
    /// The status is unknown or invalid
    Unknown,
}

/// A bluetooth device
#[cfg(target_os = "android")]
pub type BluetoothDevice = android::BluetoothDevice;

/// A bluetooth device
#[cfg(target_os = "linux")]
pub type BluetoothDevice = linux::LinuxBluetoothDevice;

/// A bluetooth device
#[cfg(target_os = "windows")]
pub type BluetoothDevice = windows::BluetoothDevice;

/// Represents a bluetooth adapter that communicates to bluetooth devices
#[cfg(target_os = "android")]
pub type BluetoothAdapter = android::Bluetooth;

/// Represents a bluetooth adapter that communicates to bluetooth devices
#[cfg(target_os = "linux")]
pub type BluetoothAdapter = linux::BluetoothHandler;

/// Represents a bluetooth adapter that communicates to bluetooth devices
#[cfg(target_os = "windows")]
pub type BluetoothAdapter = windows::BluetoothHandler;

/// A builder for `BluetoothAdapter`
pub struct BluetoothAdapterBuilder {
    /// The androidapp object
    #[cfg(target_os = "android")]
    app: Option<AndroidApp>,
    #[cfg(feature = "async")]
    /// The sender to send messages to the bluetooth host
    s: Option<tokio::sync::mpsc::Sender<MessageToBluetoothHost>>,
}

impl Default for BluetoothAdapterBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl BluetoothAdapterBuilder {
    /// Construct a new self
    pub fn new() -> Self {
        Self {
            #[cfg(target_os = "android")]
            app: None,
            #[cfg(feature = "async")]
            s: None,
        }
    }

    /// Put the required `AndroidApp` object into the builder
    #[cfg(target_os = "android")]
    pub fn with_android_app(&mut self, app: AndroidApp) {
        self.app = Some(app);
    }

    #[cfg(feature = "async")]
    /// Add the sender to the builder
    pub fn with_sender(&mut self, s: tokio::sync::mpsc::Sender<MessageToBluetoothHost>) {
        self.s = Some(s);
    }

    /// Do the build
    #[cfg(all(target_os = "android", feature = "sync"))]
    pub fn build<'a>(self) -> Result<BluetoothAdapter, String> {
        return Ok(android::Bluetooth::new(self.app.unwrap()));
    }

    /// Do the build
    #[cfg(all(target_os = "android", feature = "async"))]
    pub async fn build(self) -> Result<BluetoothAdapter, String> {
        return self.build();
    }

    #[cfg(all(not(target_os = "android"), feature = "async"))]
    /// Do the build
    pub async fn build(self) -> Result<BluetoothAdapter, String> {
        #[cfg(target_os = "linux")]
        {
            return Ok(linux::BluetoothHandler::new(self.s.unwrap()).await?);
        }
        #[cfg(target_os = "windows")]
        {
            return Ok(windows::BluetoothHandler::new(self.s.unwrap()).await?);
        }
    }
}

/// An active stream for bluetooth communications
#[cfg(target_os = "linux")]
pub type BluetoothStream = linux::BluetoothStream;

/// An active stream for bluetooth communications
#[cfg(target_os = "android")]
pub type BluetoothStream = android::RfcommStream;

/// An active stream for bluetooth communications
#[cfg(target_os = "windows")]
pub type BluetoothStream = windows::WindowsRfcommStream;

/// A type that allows rfcomm connections
#[cfg(target_os = "android")]
pub type BluetoothRfcommConnectable = android::BluetoothRfcommConnectable;

/// A type that allows rfcomm connections
#[cfg(target_os = "linux")]
pub type BluetoothRfcommConnectable = linux::BluetoothRfCommConnectable;

/// A type that allows rfcomm connections
#[cfg(target_os = "windows")]
pub type BluetoothRfcommConnectable = windows::BluetoothRfcommConnectable;

#[cfg(all(target_os = "android", feature = "async"))]
/// A bluetooth profile for l2cap channels
pub type BluetoothL2capConnectable = android::BluetoothRfcommConnectable;

#[cfg(all(target_os = "linux", feature = "async"))]
/// A bluetooth profile for l2cap channels
pub type BluetoothL2capConnectable = linux::BluetoothL2capConnectable;

#[cfg(all(target_os = "linux", feature = "async"))]
/// A bluetooth profile for rfcomm channels
pub type BluetoothRfcommProfile = linux::BluetoothRfcommProfile;

#[cfg(all(target_os = "windows", feature = "async"))]
/// A bluetooth profile for rfcomm channels
pub type BluetoothRfcommProfile = windows::BluetoothRfcommProfile;

#[cfg(all(target_os = "android", feature = "sync"))]
/// A bluetooth profile for rfcomm channels
pub type BluetoothRfcommProfile = android::BluetoothRfcommProfile;

#[cfg(all(target_os = "linux", feature = "async"))]
/// A bluetooth profile for l2cap channels
pub type BluetoothL2capProfile = linux::BluetoothL2capProfile;

#[cfg(all(target_os = "android", feature = "sync"))]
/// A bluetooth profile for l2cap channels
pub type BluetoothL2capProfile = android::BluetoothRfcommProfile;

#[cfg(target_os = "windows")]
/// A bluetooth profile for l2cap channels
pub type BluetoothL2capProfile = ();

/// A bluetooth rfcomm socket
#[cfg(target_os = "android")]
pub type BluetoothSocket = android::BluetoothSocket;

/// A bluetooth rfcomm socket
#[cfg(target_os = "linux")]
pub type BluetoothSocket = linux::BluetoothRfcommSocket;

/// A bluetooth rfcomm socket
#[cfg(target_os = "windows")]
pub type BluetoothSocket = windows::BluetoothRfcommSocket;
