#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]
#![warn(unused_extern_crates)]

//! This library is intended to eventually be a cross-platform bluetooth handling platform
//! Android portions adapted from <https://github.com/wuwbobo2021/android-bluetooth-serial-rs>

#[cfg(target_os = "android")]
use std::sync::Arc;
#[cfg(target_os = "android")]
use std::sync::Mutex;
#[cfg(target_os = "android")]
mod android;
#[cfg(target_os = "android")]
pub use android::Bluetooth;
#[cfg(target_os = "android")]
pub use android::Java;
#[cfg(target_os = "android")]
use winit::platform::android::activity::AndroidApp;

#[cfg(target_os = "linux")]
mod linux;

mod bluetooth_uuid;
pub use bluetooth_uuid::BluetoothUuid;

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
    /// The passkey used for pairing devices
    DisplayPasskey(u32, tokio::sync::mpsc::Sender<ResponseToPasskey>),
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
#[derive(Clone)]
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

/// The trait that implements managing when bluetooth discovery is enabled
#[enum_dispatch::enum_dispatch]
pub trait BluetoothDiscoveryTrait {}

/// The trait for the object that manages bluetooth discovery
#[enum_dispatch::enum_dispatch(BluetoothDiscoveryTrait)]
pub enum BluetoothDiscovery {
    /// The android version
    #[cfg(target_os = "android")]
    Android(android::BluetoothDiscovery),
    /// Linux bluez library implementation
    #[cfg(target_os = "linux")]
    Bluez(linux::BluetoothDiscovery),
}

/// The address of a bluetooth adapter
pub enum BluetoothAdapterAddress {
    /// The address in string form
    String(String),
    /// The address in byte form
    Byte([u8; 6]),
}

/// Common async functionality for the bluetooth adapter
#[enum_dispatch::enum_dispatch]
#[async_trait::async_trait]
pub trait AsyncBluetoothAdapterTrait {
    /// Attempt to register a new rfcomm profile
    async fn register_rfcomm_profile(
        &self,
        settings: BluetoothRfcommProfileSettings,
    ) -> Result<BluetoothRfcommProfileAsync, String>;
    /// Attempt to register a new l2cap profile
    async fn register_l2cap_profile(
        &self,
        settings: BluetoothL2capProfileSettings,
    ) -> Result<BluetoothL2capProfileAsync, String>;
    ///Get a list of paired bluetooth devices
    fn get_paired_devices(&self) -> Option<Vec<BluetoothDevice>>;
    /// Start discovery of bluetooth devices. Run this and drop the result to cancel discovery
    fn start_discovery(&self) -> BluetoothDiscovery;
    /// Get the mac addresses of all bluetooth adapters for the system
    async fn addresses(&self) -> Vec<BluetoothAdapterAddress>;
    /// Set the discoverable property
    async fn set_discoverable(&self, d: bool) -> Result<(), ()>;
}

/// Common sync functionality for the bluetooth adapter
#[enum_dispatch::enum_dispatch]
pub trait SyncBluetoothAdapterTrait {
    /// Attempt to register a new rfcomm profile
    fn register_rfcomm_profile(
        &self,
        settings: BluetoothRfcommProfileSettings,
    ) -> Result<BluetoothRfcommProfileSync, String>;
    /// Attempt to register a new lc2ap profile
    fn register_l2cap_profile(
        &self,
        settings: BluetoothL2capProfileSettings,
    ) -> Result<BluetoothL2capProfileAsync, String>;
    ///Get a list of paired bluetooth devices
    fn get_paired_devices(&self) -> Option<Vec<BluetoothDevice>>;
    /// Start discovery of bluetooth devices. Run this and drop the result to cancel discovery
    fn start_discovery(&self) -> BluetoothDiscovery;
    /// Get the mac addresses of all bluetooth adapters for the system
    fn addresses(&self) -> Vec<BluetoothAdapterAddress>;
    /// Set the discoverable property
    fn set_discoverable(&self, d: bool) -> Result<(), ()>;
}

/// Common functionality for the bluetooth adapter
#[enum_dispatch::enum_dispatch]
pub trait BluetoothAdapterTrait {
    /// Returns Some when the async interface is supported
    fn supports_async(&mut self) -> Option<&mut dyn AsyncBluetoothAdapterTrait>;
    /// Returns Some when the sync interface is supported
    fn supports_sync(&mut self) -> Option<&mut dyn SyncBluetoothAdapterTrait>;
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

/// The trait that all bluetooth devices must implement
#[enum_dispatch::enum_dispatch]
pub trait BluetoothDeviceTrait {
    /// Get all known uuids for this device
    fn get_uuids(&mut self) -> Result<Vec<BluetoothUuid>, std::io::Error>;

    /// Retrieve the device name
    fn get_name(&self) -> Result<String, std::io::Error>;

    /// Retrieve the device address
    fn get_address(&mut self) -> Result<String, std::io::Error>;

    /// Retrieve the device pairing status
    fn get_pair_state(&self) -> Result<PairingStatus, std::io::Error>;

    /// Attempt to get an rfcomm socket for the given uuid and security setting
    fn get_rfcomm_socket(
        &mut self,
        uuid: BluetoothUuid,
        is_secure: bool,
    ) -> Result<BluetoothSocket, String>;

    /// Attempt to get an l2cap socket for the given uuid and security setting
    fn get_l2cap_socket(
        &mut self,
        uuid: BluetoothUuid,
        is_secure: bool,
    ) -> Result<BluetoothSocket, String>;
}

/// A bluetooth device
#[enum_dispatch::enum_dispatch(BluetoothDeviceTrait)]
pub enum BluetoothDevice {
    /// Bluetooth device on android
    #[cfg(target_os = "android")]
    Android(android::BluetoothDevice),
    /// Bluetooth device on linux using the bluez library
    #[cfg(target_os = "linux")]
    Bluez(bluer::Device),
}

/// Represents a bluetooth adapter that communicates to bluetooth devices
#[enum_dispatch::enum_dispatch(BluetoothAdapterTrait)]
pub enum BluetoothAdapter {
    /// The bluetooth adapter for android systems
    #[cfg(target_os = "android")]
    Android(android::Bluetooth),
    /// On linux, bluetooth adapter using the bluez library
    #[cfg(target_os = "linux")]
    Bluez(linux::BluetoothHandler),
}

/// A builder for `BluetoothAdapter`
pub struct BluetoothAdapterBuilder {
    /// The androidapp object
    #[cfg(target_os = "android")]
    app: Option<AndroidApp>,
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
            s: None,
        }
    }

    /// Put the required `AndroidApp` object into the builder
    #[cfg(target_os = "android")]
    pub fn with_android_app(&mut self, app: AndroidApp) {
        self.app = Some(app);
    }

    /// Add the sender to the builder
    pub fn with_sender(&mut self, s: tokio::sync::mpsc::Sender<MessageToBluetoothHost>) {
        self.s = Some(s);
    }

    /// Do the build
    pub async fn build(self) -> Result<BluetoothAdapter, String> {
        #[cfg(target_os = "android")]
        {
            let java = android::Java::make(self.app.unwrap());
            return Ok(BluetoothAdapter::Android(android::Bluetooth::new(
                Arc::new(Mutex::new(java)),
            )));
        }
        #[cfg(target_os = "linux")]
        {
            return Ok(BluetoothAdapter::Bluez(
                linux::BluetoothHandler::new(self.s.unwrap()).await?,
            ));
        }
        Err("No builders available".to_string())
    }
}

/// An active stream for bluetooth communications
pub enum BluetoothStream {
    /// On linux, a stream using the bluez library
    #[cfg(target_os = "linux")]
    Bluez(std::pin::Pin<Box<bluer::rfcomm::Stream>>),
    /// Android code for a bluetooth stream
    #[cfg(target_os = "android")]
    Android(android::RfcommStream),
}

impl BluetoothStream {
    /// Used to check to see if the object supports async read, and then use the functionality
    pub fn supports_async_read(
        self: std::pin::Pin<&mut Self>,
    ) -> Option<&mut dyn tokio::io::AsyncRead> {
        match self.get_mut() {
            #[cfg(target_os = "linux")]
            BluetoothStream::Bluez(pin) => Some(pin),
            #[cfg(target_os = "android")]
            BluetoothStream::Android(_pin) => None,
        }
    }

    /// Used to check to see if the object supports async write, and then use the functionality
    pub fn supports_async_write(
        self: std::pin::Pin<&mut Self>,
    ) -> Option<&mut dyn tokio::io::AsyncWrite> {
        match self.get_mut() {
            #[cfg(target_os = "linux")]
            BluetoothStream::Bluez(pin) => Some(pin),
            #[cfg(target_os = "android")]
            BluetoothStream::Android(_pin) => None,
        }
    }

    /// Used to try to use synchronous read functionality
    pub fn supports_sync_read(self: std::pin::Pin<&mut Self>) -> Option<&mut dyn std::io::Read> {
        match self.get_mut() {
            #[cfg(target_os = "linux")]
            BluetoothStream::Bluez(pin) => None,
            #[cfg(target_os = "android")]
            BluetoothStream::Android(pin) => Some(pin),
        }
    }

    /// Used to try to use synchronous write functionality
    pub fn supports_sync_write(self: std::pin::Pin<&mut Self>) -> Option<&mut dyn std::io::Write> {
        match self.get_mut() {
            #[cfg(target_os = "linux")]
            BluetoothStream::Bluez(pin) => None,
            #[cfg(target_os = "android")]
            BluetoothStream::Android(pin) => Some(pin),
        }
    }
}

/// The trait for bluetooth rfcomm objects that can be connected or accepted
#[enum_dispatch::enum_dispatch]
pub trait BluetoothRfcommConnectableAsyncTrait {
    /// Accept a connection from a bluetooth peer
    async fn accept(self) -> Result<BluetoothStream, String>;
}

/// A bluetooth profile for rfcomm channels
#[enum_dispatch::enum_dispatch(BluetoothRfcommConnectableTrait)]
pub enum BluetoothRfcommConnectableAsync {
    /// The android object for the profile
    #[cfg(target_os = "android")]
    Android(android::BluetoothRfcommConnectable),
    /// The bluez library in linux is responsible for the profile
    #[cfg(target_os = "linux")]
    Bluez(bluer::rfcomm::ConnectRequest),
}

/// The trait for bluetooth rfcomm objects that can be connected or accepted
#[enum_dispatch::enum_dispatch]
pub trait BluetoothRfcommConnectableSyncTrait {
    /// Accept a connection from a bluetooth peer
    fn accept(self, timeout: std::time::Duration) -> Result<BluetoothStream, String>;
}

/// A bluetooth profile for rfcomm channels
#[enum_dispatch::enum_dispatch(BluetoothRfcommConnectableSyncTrait)]
pub enum BluetoothRfcommConnectableSync {
    /// The android object for the profile
    #[cfg(target_os = "android")]
    Android(android::BluetoothRfcommConnectable),
}


/// The trait for bluetooth rfcomm objects that can be connected or accepted
#[enum_dispatch::enum_dispatch]
pub trait BluetoothL2capConnectableAsyncTrait {
    /// Accept a connection from a bluetooth peer
    async fn accept(self) -> Result<BluetoothStream, String>;
}

/// A bluetooth profile for rfcomm channels
#[enum_dispatch::enum_dispatch(BluetoothL2capConnectableTrait)]
pub enum BluetoothL2capConnectableAsync {
    /// The android object for the profile
    #[cfg(target_os = "android")]
    Android(android::BluetoothRfcommConnectable),
    /// The bluez library in linux is responsible for the profile
    #[cfg(target_os = "linux")]
    Bluez(bluer::rfcomm::ConnectRequest),
}

/// The trait for bluetooth rfcomm objects that can be connected or accepted
#[enum_dispatch::enum_dispatch]
pub trait BluetoothL2capConnectableSyncTrait {
    /// Accept a connection from a bluetooth peer
    fn accept(self, timeout: std::time::Duration) -> Result<BluetoothStream, String>;
}

/// A bluetooth profile for rfcomm channels
#[enum_dispatch::enum_dispatch(BluetoothL2capConnectableSyncTrait)]
pub enum BluetoothL2capConnectableSync {
    /// The android object for the profile
    #[cfg(target_os = "android")]
    Android(android::BluetoothRfcommConnectable),
}

/// Allows building an object to connect to bluetooth devices
#[enum_dispatch::enum_dispatch]
pub trait BluetoothRfcommProfileAsyncTrait {
    /// Get an object in order to accept a connection from or connect to a bluetooth peer
    async fn connectable(&mut self) -> Result<BluetoothRfcommConnectableAsync, String>;
}

/// Allows building an object to connect to bluetooth devices
#[enum_dispatch::enum_dispatch]
pub trait BluetoothRfcommProfileSyncTrait {
    /// Get an object in order to accept a connection from or connect to a bluetooth peer
    fn connectable(&mut self) -> Result<BluetoothRfcommConnectableSync, String>;
}

/// A bluetooth profile for rfcomm channels
#[enum_dispatch::enum_dispatch(BluetoothRfcommProfileAsyncTrait)]
pub enum BluetoothRfcommProfileAsync {
    /// The bluez library in linux is responsible for the profile
    #[cfg(target_os = "linux")]
    Bluez(bluer::rfcomm::ProfileHandle),
    /// A dummy handler
    Dummy(Dummy),
}

/// A bluetooth profile for rfcomm channels
#[enum_dispatch::enum_dispatch(BluetoothRfcommProfileSyncTrait)]
pub enum BluetoothRfcommProfileSync {
    /// Android rfcomm profile
    #[cfg(target_os = "android")]
    Android(android::BluetoothRfcommProfile),
    /// A dummy handler
    Dummy(Dummy),
}

/// A bluetooth profile for rfcomm channels
#[enum_dispatch::enum_dispatch(BluetoothL2capProfileAsyncTrait)]
pub enum BluetoothL2capProfileAsync {
    /// The bluez library in linux is responsible for the profile
    #[cfg(target_os = "linux")]
    Bluez(bluer::rfcomm::ProfileHandle),
    /// A dummy handler
    Dummy(Dummy),
}

/// A bluetooth profile for rfcomm channels
#[enum_dispatch::enum_dispatch(BluetoothL2capProfileSyncTrait)]
pub enum BluetoothL2capProfileSync {
    /// Android rfcomm profile
    #[cfg(target_os = "android")]
    Android(android::BluetoothRfcommProfile),
    /// A dummy handler
    Dummy(Dummy),
}

/// A dummy struct for ensuring enums are not empty
pub struct Dummy {}

impl BluetoothRfcommProfileSyncTrait for Dummy {
    fn connectable(&mut self) -> Result<BluetoothRfcommConnectableSync,String> {
        unimplemented!()
    }
}

impl BluetoothRfcommProfileAsyncTrait for Dummy {
    async fn connectable(&mut self) -> Result<BluetoothRfcommConnectableAsync,String> {
        unimplemented!()
    }
}

/// The common functions for all bluetooth rfcomm sockets
#[enum_dispatch::enum_dispatch]
pub trait BluetoothSocketTrait {}

/// A bluetooth rfcomm socket
#[enum_dispatch::enum_dispatch(BluetoothSocketTrait)]
pub enum BluetoothSocket<'a> {
    /// The android based rfcomm socket
    #[cfg(target_os = "android")]
    Android(&'a mut android::BluetoothSocket),
    /// Linux using bluez library
    #[cfg(target_os = "linux")]
    Bluez(&'a mut linux::BluetoothRfcommSocket),
}

