//! Linux specific bluetooth code

use std::collections::HashMap;

use bluer::AdapterEvent;
use futures::FutureExt;
use futures::StreamExt;

// ────────────────────────────────────────────────────────────────────────────
// BluetoothRfcommConnectableAsyncTrait for bluer::rfcomm::ConnectRequest
// ────────────────────────────────────────────────────────────────────────────

#[async_trait::async_trait]
impl super::BluetoothRfcommConnectableAsyncTrait for bluer::rfcomm::ConnectRequest {
    async fn accept(self) -> Result<(crate::BluetoothStream, [u8; 6], u8), String> {
        let s = bluer::rfcomm::ConnectRequest::accept(self);
        match s {
            Ok(s) => {
                let addr = s.peer_addr().map_err(|e| e.to_string())?;
                Ok((
                    crate::BluetoothStream::Bluez(Box::pin(s)),
                    *addr.addr,
                    addr.channel,
                ))
            }
            Err(e) => Err(e.to_string()),
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// BluetoothRfcommProfileAsyncTrait for bluer::rfcomm::ProfileHandle
// ────────────────────────────────────────────────────────────────────────────

impl super::BluetoothRfcommProfileAsyncTrait for bluer::rfcomm::ProfileHandle {
    async fn connectable(&mut self) -> Result<crate::BluetoothRfcommConnectableAsync, String> {
        self.next()
            .await
            .map(|a| crate::BluetoothRfcommConnectableAsync::Bluez(a))
            .ok_or_else(|| "Failed to get bluetooth connection".to_string())
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Internal active-connection holder (RFCOMM or L2CAP stream)
// ────────────────────────────────────────────────────────────────────────────

/// Holds the active stream for either an RFCOMM or L2CAP connection.
enum BluetoothConnection {
    /// An active RFCOMM stream
    Rfcomm(bluer::rfcomm::Stream),
    /// An active L2CAP stream
    L2cap(bluer::l2cap::Stream),
}

impl std::io::Read for BluetoothConnection {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                use tokio::io::AsyncReadExt;
                match self {
                    BluetoothConnection::Rfcomm(s) => s.read(buf).await,
                    BluetoothConnection::L2cap(s) => s.read(buf).await,
                }
            })
        })
    }
}

impl std::io::Write for BluetoothConnection {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                use tokio::io::AsyncWriteExt;
                match self {
                    BluetoothConnection::Rfcomm(s) => s.write(buf).await,
                    BluetoothConnection::L2cap(s) => s.write(buf).await,
                }
            })
        })
    }

    fn flush(&mut self) -> std::io::Result<()> {
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                use tokio::io::AsyncWriteExt;
                match self {
                    BluetoothConnection::Rfcomm(s) => s.flush().await,
                    BluetoothConnection::L2cap(s) => s.flush().await,
                }
            })
        })
    }
}

// ────────────────────────────────────────────────────────────────────────────
// BluetoothRfcommSocket – used for both RFCOMM and L2CAP outgoing connections
// ────────────────────────────────────────────────────────────────────────────

/// An outgoing bluetooth socket that may carry either an RFCOMM or L2CAP
/// connection.  The socket is created lazily; call `connect()` before doing
/// any I/O.
pub struct BluetoothRfcommSocket {
    /// Address of the remote device
    device_addr: bluer::Address,
    /// RFCOMM channel to connect on (mutually exclusive with `l2cap_psm`).
    ///
    /// BlueZ does not expose a direct SDP query API through the bluer crate, so
    /// callers are responsible for resolving the correct channel via
    /// `run_sdp` / `get_uuids` before creating the socket, or by relying on
    /// a well-known channel number for the service.  Channel 1 is used as a
    /// default placeholder when the channel is not otherwise known.
    rfcomm_channel: Option<u8>,
    /// L2CAP PSM to connect on (mutually exclusive with `rfcomm_channel`).
    l2cap_psm: Option<u16>,
    /// Whether to request an encrypted / authenticated link
    is_secure: bool,
    /// The live connection, present after a successful `connect()` call
    connection: Option<BluetoothConnection>,
}

impl BluetoothRfcommSocket {
    /// Create a new (unconnected) RFCOMM socket.
    fn new_rfcomm(device_addr: bluer::Address, channel: u8, is_secure: bool) -> Self {
        Self {
            device_addr,
            rfcomm_channel: Some(channel),
            l2cap_psm: None,
            is_secure,
            connection: None,
        }
    }

    /// Create a new (unconnected) L2CAP socket.
    fn new_l2cap(device_addr: bluer::Address, psm: u16, is_secure: bool) -> Self {
        Self {
            device_addr,
            rfcomm_channel: None,
            l2cap_psm: Some(psm),
            is_secure,
            connection: None,
        }
    }
}

impl crate::BluetoothSocketTrait for BluetoothRfcommSocket {
    fn is_connected(&self) -> Result<bool, std::io::Error> {
        Ok(self.connection.is_some())
    }

    fn connect(&mut self) -> Result<(), std::io::Error> {
        if self.connection.is_some() {
            return Ok(());
        }
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                if let Some(channel) = self.rfcomm_channel {
                    let addr = bluer::rfcomm::SocketAddr::new(self.device_addr, channel);
                    let socket = bluer::rfcomm::Socket::new()
                        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
                    if self.is_secure {
                        socket
                            .set_security(bluer::rfcomm::Security {
                                level: bluer::rfcomm::SecurityLevel::Medium,
                                key_size: 0,
                            })
                            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
                    }
                    let stream = socket
                        .connect(addr)
                        .await
                        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
                    log::info!("STREAM {:?} to {:?}", stream.as_ref().local_addr(), stream.peer_addr());
                    self.connection = Some(BluetoothConnection::Rfcomm(stream));
                    log::info!("Got an rfcomm stream");
                } else if let Some(psm) = self.l2cap_psm {
                    let addr = bluer::l2cap::SocketAddr::new(
                        self.device_addr,
                        bluer::AddressType::BrEdr,
                        psm,
                    );
                    let socket = bluer::l2cap::Socket::<bluer::l2cap::Stream>::new_stream()
                        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
                    if self.is_secure {
                        socket
                            .set_security(bluer::l2cap::Security {
                                level: bluer::l2cap::SecurityLevel::Medium,
                                key_size: 0,
                            })
                            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
                    }
                    let stream = socket
                        .connect(addr)
                        .await
                        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
                    self.connection = Some(BluetoothConnection::L2cap(stream));
                } else {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "BluetoothRfcommSocket has neither an RFCOMM channel nor an L2CAP PSM configured",
                    ));
                }
                Ok(())
            })
        })
    }
}

impl std::io::Read for BluetoothRfcommSocket {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match &mut self.connection {
            Some(conn) => conn.read(buf),
            None => Err(std::io::Error::from(std::io::ErrorKind::NotConnected)),
        }
    }
}

impl std::io::Write for BluetoothRfcommSocket {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match &mut self.connection {
            Some(conn) => conn.write(buf),
            None => Err(std::io::Error::from(std::io::ErrorKind::NotConnected)),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match &mut self.connection {
            Some(conn) => conn.flush(),
            None => Err(std::io::Error::from(std::io::ErrorKind::NotConnected)),
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// LinuxBluetoothDevice – wraps bluer::Device and owns its open sockets
// ────────────────────────────────────────────────────────────────────────────

/// A Linux bluetooth device backed by bluer.  Wraps a `bluer::Device` and
/// stores open RFCOMM / L2CAP sockets so that returned `BluetoothSocket`
/// references remain valid for the lifetime of this struct.
pub struct LinuxBluetoothDevice {
    /// The underlying bluer device handle
    device: bluer::Device,
}

impl LinuxBluetoothDevice {
    /// Wrap a `bluer::Device`.
    pub fn new(device: bluer::Device) -> Self {
        Self { device }
    }
}

impl super::BluetoothDeviceTrait for LinuxBluetoothDevice {
    fn get_uuids(&mut self) -> Result<Vec<crate::BluetoothUuid>, std::io::Error> {
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let uuids =
                    self.device.uuids().await.map_err(|e| {
                        std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
                    })?;
                Ok(uuids
                    .unwrap_or_default()
                    .into_iter()
                    .map(|u| {
                        use std::str::FromStr;
                        crate::BluetoothUuid::from_str(&u.to_string())
                            .unwrap_or_else(|_| crate::BluetoothUuid::Unknown(u.to_string()))
                    })
                    .collect())
            })
        })
    }

    /// Returns the alias (display name) of the device, falling back to the
    /// hardware name when no alias is set.
    fn get_name(&self) -> Result<String, std::io::Error> {
        let device = self.device.clone();
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async move {
                device
                    .alias()
                    .await
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
            })
        })
    }

    fn get_address(&mut self) -> Result<String, std::io::Error> {
        Ok(self.device.address().to_string())
    }

    fn get_pair_state(&self) -> Result<crate::PairingStatus, std::io::Error> {
        let device = self.device.clone();
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async move {
                let paired = device
                    .is_paired()
                    .await
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
                Ok(if paired {
                    crate::PairingStatus::Paired
                } else {
                    crate::PairingStatus::NotPaired
                })
            })
        })
    }

    /// Return a socket suitable for an outgoing L2CAP connection to the given
    /// service UUID.
    ///
    /// The PSM for the service must be known in advance.  Because bluer does
    /// not expose a raw SDP query API, a proper PSM lookup is left as a
    /// `// TODO` below; the default PSM of 1 (SDP channel itself) is a
    /// placeholder and will need to be replaced with the actual dynamic PSM
    /// discovered via SDP for real services.
    fn get_l2cap_socket(
        &mut self,
        psm: u16,
        is_secure: bool,
    ) -> Result<crate::BluetoothSocket, String> {
        let addr = self.device.address();
        let socket = BluetoothRfcommSocket::new_l2cap(addr, psm, is_secure);
        Ok(crate::BluetoothSocket::Bluez(socket))
    }

    /// Return a socket suitable for an outgoing RFCOMM connection to the given
    /// service UUID.
    ///
    /// The RFCOMM channel for the service must be known in advance.  Because
    /// bluer does not expose a raw SDP query API, a proper channel lookup is
    /// left as a `// TODO` below; channel 1 is used as a placeholder and will
    /// need to be replaced with the channel discovered via SDP for real
    /// services (e.g. by using `sdptool` or an external SDP library).
    fn get_rfcomm_socket(
        &mut self,
        channel: u8,
        is_secure: bool,
    ) -> Result<crate::BluetoothSocket, String> {
        let addr = self.device.address();
        let socket = BluetoothRfcommSocket::new_rfcomm(addr, channel, is_secure);
        Ok(crate::BluetoothSocket::Bluez(socket))
    }
}

// ────────────────────────────────────────────────────────────────────────────
// BluetoothDiscovery
// ────────────────────────────────────────────────────────────────────────────

/// A struct for managing discovery of bluetooth devices
pub struct BluetoothDiscovery {}

impl BluetoothDiscovery {
    /// Construct a new self
    fn new() -> Self {
        Self {}
    }
}

impl super::BluetoothDiscoveryTrait for BluetoothDiscovery {}

impl Drop for BluetoothDiscovery {
    fn drop(&mut self) {}
}

// ────────────────────────────────────────────────────────────────────────────
// TryFrom conversions for profile settings → bluer::rfcomm::Profile
// ────────────────────────────────────────────────────────────────────────────

impl TryFrom<super::BluetoothRfcommProfileSettings> for bluer::rfcomm::Profile {
    type Error = String;
    fn try_from(value: super::BluetoothRfcommProfileSettings) -> Result<Self, Self::Error> {
        let service = if let Some(v) = value.service_uuid {
            Some(bluer::Uuid::parse_str(&v).map_err(|e| e.to_string())?)
        } else {
            None
        };
        Ok(Self {
            uuid: bluer::Uuid::parse_str(&value.uuid).map_err(|e| e.to_string())?,
            name: value.name,
            service,
            role: if value.channel.is_some() {
                Some(bluer::rfcomm::Role::Server)
            } else {
                None
            },
            channel: value.channel,
            psm: value.psm,
            require_authentication: value.authenticate,
            require_authorization: value.authorize,
            auto_connect: value.auto_connect,
            service_record: value.sdp_record,
            version: value.sdp_version,
            features: value.sdp_features,
            ..Default::default()
        })
    }
}

/// L2CAP profiles are registered through the same BlueZ D-Bus
/// ProfileManager1 mechanism as RFCOMM profiles, but with the `psm` field
/// set instead of `channel`.
impl TryFrom<super::BluetoothL2capProfileSettings> for bluer::rfcomm::Profile {
    type Error = String;
    fn try_from(value: super::BluetoothL2capProfileSettings) -> Result<Self, Self::Error> {
        let service = if let Some(v) = value.service_uuid {
            Some(bluer::Uuid::parse_str(&v).map_err(|e| e.to_string())?)
        } else {
            None
        };
        Ok(Self {
            uuid: bluer::Uuid::parse_str(&value.uuid).map_err(|e| e.to_string())?,
            name: value.name,
            service,
            role: None,
            channel: None,
            psm: value.psm,
            require_authentication: value.authenticate,
            require_authorization: value.authorize,
            auto_connect: value.auto_connect,
            service_record: value.sdp_record,
            version: value.sdp_version,
            features: value.sdp_features,
            ..Default::default()
        })
    }
}

// ────────────────────────────────────────────────────────────────────────────
// BluetoothHandler – main adapter / session manager
// ────────────────────────────────────────────────────────────────────────────

/// The general bluetooth handler for the library. There should be only one per application on linux.
pub struct BluetoothHandler {
    /// The current bluetooth session
    session: bluer::Session,
    /// The list of bluetooth adapters for the system
    adapters: Vec<bluer::Adapter>,
    /// The agent for the handler
    _blue_agent_handle: bluer::agent::AgentHandle,
}

impl super::BluetoothAdapterTrait for BluetoothHandler {
    fn supports_async(&self) -> Option<&dyn super::AsyncBluetoothAdapterTrait> {
        Some(self)
    }

    fn supports_sync(&self) -> Option<&dyn super::SyncBluetoothAdapterTrait> {
        None
    }
}

#[async_trait::async_trait]
impl super::AsyncBluetoothAdapterTrait for BluetoothHandler {
    async fn register_rfcomm_profile(
        &self,
        settings: super::BluetoothRfcommProfileSettings,
    ) -> Result<crate::BluetoothRfcommProfileAsync, String> {
        self.session
            .register_profile(settings.try_into()?)
            .await
            .map(|a| super::BluetoothRfcommProfileAsync::Bluez(a.into()))
            .map_err(|e| e.to_string())
    }

    /// Register an L2CAP profile with BlueZ.
    ///
    /// BlueZ exposes a single `RegisterProfile` D-Bus method (ProfileManager1)
    /// that handles both RFCOMM and L2CAP profiles.  The `psm` field in the
    /// settings selects L2CAP; the RFCOMM `channel` field is left as `None`.
    async fn register_l2cap_profile(
        &self,
        settings: super::BluetoothL2capProfileSettings,
    ) -> Result<crate::BluetoothL2capProfileAsync, String> {
        self.session
            .register_profile(settings.try_into()?)
            .await
            .map(|a| super::BluetoothL2capProfileAsync::Bluez(a.into()))
            .map_err(|e| e.to_string())
    }

    fn start_discovery(&self) -> crate::BluetoothDiscovery {
        BluetoothDiscovery::new().into()
    }

    /// Return all paired devices across every adapter.
    fn get_paired_devices(&self) -> Option<Vec<crate::BluetoothDevice>> {
        let mut list = Vec::new();
        for adapter in &self.adapters {
            let result = tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(async {
                    let addrs = adapter.device_addresses().await?;
                    let mut paired = Vec::new();
                    for addr in addrs {
                        if let Ok(dev) = adapter.device(addr) {
                            if dev.is_paired().await.unwrap_or(false) {
                                paired.push(dev);
                            }
                        }
                    }
                    Ok::<Vec<bluer::Device>, bluer::Error>(paired)
                })
            });
            if let Ok(devices) = result {
                for dev in devices {
                    list.push(crate::BluetoothDevice::Bluez(LinuxBluetoothDevice::new(
                        dev,
                    )));
                }
            }
        }
        Some(list)
    }

    async fn addresses(&self) -> Vec<super::BluetoothAdapterAddress> {
        let mut a = Vec::new();
        for adapter in &self.adapters {
            if let Ok(adr) = adapter.address().await {
                a.push(super::BluetoothAdapterAddress::Byte(adr.0));
            }
        }
        a
    }

    async fn set_discoverable(&self, d: bool) -> Result<(), ()> {
        for adapter in &self.adapters {
            adapter.set_discoverable(d).await.map_err(|_| ())?;
        }
        Ok(())
    }
}

impl BluetoothHandler {
    /// Retrieve the bluetooth addresses for all bluetooth adapters present
    pub async fn addresses(&self) -> Vec<bluer::Address> {
        let mut addrs = Vec::new();
        for a in &self.adapters {
            if let Ok(addr) = a.address().await {
                addrs.push(addr);
            }
        }
        addrs
    }

    /// Construct a new self
    pub async fn new(
        s: tokio::sync::mpsc::Sender<super::MessageToBluetoothHost>,
    ) -> Result<Self, String> {
        let session = bluer::Session::new().await.map_err(|e| e.to_string())?;

        let adapter_names = session.adapter_names().await.map_err(|e| e.to_string())?;
        let adapters: Vec<bluer::Adapter> = adapter_names
            .iter()
            .filter_map(|n| session.adapter(n).ok())
            .collect();

        let blue_agent = Self::build_agent(s);
        let blue_agent_handle = session.register_agent(blue_agent).await;
        println!("Registered a bluetooth agent {}", blue_agent_handle.is_ok());
        Ok(Self {
            session,
            adapters,
            _blue_agent_handle: blue_agent_handle.map_err(|e| e.to_string())?,
        })
    }

    /// Enable all bluetooth adapters
    async fn enable(&mut self) {
        for adapter in &self.adapters {
            adapter.set_powered(true).await.unwrap();
            adapter.set_pairable(true).await.unwrap();
        }
    }

    /// Disable all bluetooth adapters
    async fn disable(&mut self) {
        self.set_discoverable(false).await;
        for adapter in &self.adapters {
            adapter.set_powered(false).await.unwrap();
            adapter.set_pairable(false).await.unwrap();
        }
    }

    /// Enable or disable discoverable mode on all bluetooth adapters
    pub async fn set_discoverable(&mut self, d: bool) {
        for adapter in &self.adapters {
            adapter.set_discoverable(d).await.unwrap();
        }
    }

    /// Register an RFCOMM profile with the bluetooth session
    pub async fn register_rfcomm_profile(
        &mut self,
        profile: bluer::rfcomm::Profile,
    ) -> Result<bluer::rfcomm::ProfileHandle, bluer::Error> {
        self.session.register_profile(profile).await
    }

    /// Build a bluetooth agent for the handler
    fn build_agent(
        s: tokio::sync::mpsc::Sender<super::MessageToBluetoothHost>,
    ) -> bluer::agent::Agent {
        let mut blue_agent = bluer::agent::Agent::default();
        blue_agent.request_default = true;
        blue_agent.request_pin_code = None;
        blue_agent.request_passkey = None;
        let s2 = s.clone();
        blue_agent.display_passkey = Some(Box::new(move |mut a| {
            println!("Running process for display_passkey: {:?}", a);
            let s3 = s2.clone();
            async move {
                let mut chan = tokio::sync::mpsc::channel(5);
                let _ = s3
                    .send(super::MessageToBluetoothHost::DisplayPasskey(a.passkey, chan.0))
                    .await;
                loop {
                    let f = tokio::time::timeout(std::time::Duration::from_secs(5), chan.1.recv());
                    tokio::select! {
                        asdf = f => {
                            match asdf {
                                Ok(Some(m)) => match m {
                                    super::ResponseToPasskey::Yes => {
                                        let _ = s3
                                            .send(super::MessageToBluetoothHost::CancelDisplayPasskey)
                                            .await;
                                        return Ok(());
                                    }
                                    super::ResponseToPasskey::No => {
                                        let _ = s3
                                            .send(super::MessageToBluetoothHost::CancelDisplayPasskey)
                                            .await;
                                        return Err(bluer::agent::ReqError::Rejected);
                                    }
                                    super::ResponseToPasskey::Cancel => {
                                        let _ = s3
                                            .send(super::MessageToBluetoothHost::CancelDisplayPasskey)
                                            .await;
                                        return Err(bluer::agent::ReqError::Canceled);
                                    }
                                    super::ResponseToPasskey::Waiting => {}
                                },
                                Ok(None) => {}
                                _ => {
                                    let _ = s3
                                        .send(super::MessageToBluetoothHost::CancelDisplayPasskey)
                                        .await;
                                    return Err(bluer::agent::ReqError::Canceled);
                                }
                            }
                        }
                        _ = &mut a.cancel => {
                            let _ = s3
                                .send(super::MessageToBluetoothHost::CancelDisplayPasskey)
                                .await;
                            break Err(bluer::agent::ReqError::Canceled);
                        }
                    }
                }
            }
            .boxed()
        }));
        blue_agent.display_pin_code = Some(Box::new(|a| {
            async move {
                println!("Need to display pin code {:?}", a);
                a.cancel.await.unwrap();
                Ok(())
            }
            .boxed()
        }));
        let s2 = s.clone();
        blue_agent.request_confirmation = Some(Box::new(move |a| {
            println!("Need to confirm {:?}", a);
            let s3 = s2.clone();
            async move {
                let mut chan = tokio::sync::mpsc::channel(5);
                let _ = s3
                    .send(super::MessageToBluetoothHost::ConfirmPasskey(
                        a.passkey, chan.0,
                    ))
                    .await;
                loop {
                    let f = tokio::time::timeout(std::time::Duration::from_secs(5), chan.1.recv());
                    let asdf = f.await;
                    match asdf {
                        Ok(Some(m)) => match m {
                            super::ResponseToPasskey::Yes => {
                                let _ = s3
                                    .send(super::MessageToBluetoothHost::CancelDisplayPasskey)
                                    .await;
                                return Ok(());
                            }
                            super::ResponseToPasskey::No => {
                                let _ = s3
                                    .send(super::MessageToBluetoothHost::CancelDisplayPasskey)
                                    .await;
                                return Err(bluer::agent::ReqError::Rejected);
                            }
                            super::ResponseToPasskey::Cancel => {
                                let _ = s3
                                    .send(super::MessageToBluetoothHost::CancelDisplayPasskey)
                                    .await;
                                return Err(bluer::agent::ReqError::Canceled);
                            }
                            super::ResponseToPasskey::Waiting => {}
                        },
                        Ok(None) => {}
                        _ => {
                            let _ = s3
                                .send(super::MessageToBluetoothHost::CancelDisplayPasskey)
                                .await;
                            return Err(bluer::agent::ReqError::Canceled);
                        }
                    }
                }
            }
            .boxed()
        }));
        blue_agent.request_authorization = Some(Box::new(|a| {
            async move {
                println!("Need to authorize {:?}", a);
                Ok(())
            }
            .boxed()
        }));
        blue_agent.authorize_service = Some(Box::new(|a| {
            async move {
                println!("Need to authorize service {:?}", a);
                Ok(())
            }
            .boxed()
        }));
        blue_agent
    }

    /// Issues the specified bluetooth command, with an optional response for the command
    pub async fn issue_command(
        &mut self,
        cmd: super::BluetoothCommand,
    ) -> Option<super::BluetoothResponse> {
        match cmd {
            super::BluetoothCommand::QueryNumAdapters => {
                Some(super::BluetoothResponse::Adapters(self.adapters.len()))
            }
            _ => None,
        }
    }

    /// Run a scan on all the bluetooth adapters, updating `bluetooth_devices`
    /// with newly discovered or removed devices.
    pub async fn scan<'a>(
        &'a mut self,
        bluetooth_devices: &mut HashMap<
            bluer::Address,
            (&'a bluer::Adapter, Option<bluer::Device>),
        >,
    ) {
        let mut adapter_scanner = Vec::new();
        for a in &self.adapters {
            let da = a.discover_devices_with_changes().await.unwrap();
            adapter_scanner.push((a, da));
        }

        for (adapt, da) in &mut adapter_scanner {
            if let Some(e) = da.next().await {
                match e {
                    AdapterEvent::DeviceAdded(addr) => {
                        log::debug!("Device added {:?}", addr);
                        bluetooth_devices.insert(addr, (adapt, None));
                    }
                    AdapterEvent::DeviceRemoved(addr) => {
                        log::debug!("Device removed {:?}", addr);
                        bluetooth_devices.remove_entry(&addr);
                    }
                    AdapterEvent::PropertyChanged(prop) => {
                        log::debug!("Adapter property changed {:?}", prop);
                    }
                }
            }
        }

        for (addr, (adapter, dev)) in bluetooth_devices.iter_mut() {
            if dev.is_none() {
                if let Ok(d) = adapter.device(*addr) {
                    if let Ok(ps) = d.all_properties().await {
                        for p in ps {
                            log::debug!("Device {:?} property: {:?}", addr, p);
                        }
                    }
                    *dev = Some(d);
                }
            }
        }
    }
}
