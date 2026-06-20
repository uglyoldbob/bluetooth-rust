//! Windows specific bluetooth implementation using the windows crate.
//!
//! Implements the library traits for Windows using WinRT APIs exposed by
//! the `windows` crate.  Classic Bluetooth (RFCOMM) is supported via the
//! `Windows.Devices.Bluetooth.Rfcomm` namespace.
//!
//! # COM initialisation
//! The Windows Runtime must be initialised before any WinRT calls are made.
//! Applications should call `CoInitializeEx` with `COINIT_MULTITHREADED` (or
//! equivalent) before constructing a `BluetoothHandler`.

use windows::{
    Devices::Bluetooth::Rfcomm::{RfcommServiceId, RfcommServiceProvider},
    Devices::Bluetooth::{BluetoothAdapter as WinBtAdapter, BluetoothDevice as WinBtDevice},
    Devices::Enumeration::{DeviceInformation, DeviceWatcher},
    Foundation::TypedEventHandler,
    Networking::Sockets::{
        SocketProtectionLevel, StreamSocket, StreamSocketListener,
        StreamSocketListenerConnectionReceivedEventArgs,
    },
    Storage::Streams::{DataReader, DataWriter, InputStreamOptions},
    core::{GUID, HSTRING},
};

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Parse a hyphenated UUID string into a Windows `GUID`.
fn parse_uuid_to_guid(uuid_str: &str) -> Result<GUID, String> {
    let s = uuid_str.replace('-', "");
    if s.len() != 32 {
        return Err(format!(
            "Invalid UUID (expected 32 hex chars after removing hyphens): {uuid_str}"
        ));
    }
    let data1 = u32::from_str_radix(&s[0..8], 16).map_err(|e| e.to_string())?;
    let data2 = u16::from_str_radix(&s[8..12], 16).map_err(|e| e.to_string())?;
    let data3 = u16::from_str_radix(&s[12..16], 16).map_err(|e| e.to_string())?;
    let mut data4 = [0u8; 8];
    for i in 0..8 {
        data4[i] = u8::from_str_radix(&s[16 + i * 2..18 + i * 2], 16).map_err(|e| e.to_string())?;
    }
    Ok(GUID {
        data1,
        data2,
        data3,
        data4,
    })
}

/// Convert a Windows Bluetooth address (u64, lower 48 bits) to a six-byte MAC
/// address array in big-endian order.
fn bt_u64_to_bytes(addr: u64) -> [u8; 6] {
    let b = addr.to_be_bytes();
    [b[2], b[3], b[4], b[5], b[6], b[7]]
}

// ---------------------------------------------------------------------------
// Stream
// ---------------------------------------------------------------------------

/// A connected RFCOMM stream backed by a WinRT `StreamSocket`.
///
/// Implements `std::io::Read` and `std::io::Write` by blocking on the
/// underlying WinRT async operations via `futures::executor::block_on`.
/// Do **not** call these methods from inside a tokio async task without
/// wrapping them in `tokio::task::spawn_blocking`.
pub struct WindowsRfcommStream {
    _socket: windows::Networking::Sockets::StreamSocket,
    reader: windows::Storage::Streams::DataReader,
    writer: windows::Storage::Streams::DataWriter,
    read_fut: Option<std::pin::Pin<Box<dyn std::future::Future<Output = Result<u32, String>> + Send + 'static>>>,
    write_fut: Option<std::pin::Pin<Box<dyn std::future::Future<Output = Result<u32, String>> + Send + 'static>>>,
}

unsafe impl Send for WindowsRfcommStream {}
unsafe impl Sync for WindowsRfcommStream {}

impl tokio::io::AsyncRead for WindowsRfcommStream {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        if self.read_fut.is_none() {
            let want = buf.remaining() as u32;
            let reader = unsafe { &*(&self.reader as *const windows::Storage::Streams::DataReader) };
            let fut = Box::pin(async move {
                reader
                    .LoadAsync(want)
                    .map_err(|e| e.to_string())?
                    .await
                    .map_err(|e| e.to_string())
            });
            self.read_fut = Some(fut);
        }

        let fut = self.read_fut.as_mut().unwrap();
        match fut.as_mut().poll(cx) {
            std::task::Poll::Pending => std::task::Poll::Pending,
            std::task::Poll::Ready(Err(e)) => {
                self.read_fut = None;
                std::task::Poll::Ready(Err(std::io::Error::new(std::io::ErrorKind::Other, e)))
            }
            std::task::Poll::Ready(Ok(loaded)) => {
                self.read_fut = None;
                let loaded = loaded as usize;
                let dst = buf.initialize_unfilled();
                for i in 0..loaded {
                    dst[i] = self
                        .reader
                        .ReadByte()
                        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
                }
                buf.advance(loaded);
                std::task::Poll::Ready(Ok(()))
            }
        }
    }
}

impl tokio::io::AsyncWrite for WindowsRfcommStream {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        data: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        if self.write_fut.is_none() {
            self.writer
                .WriteBytes(data)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

            let writer = unsafe { &*(&self.writer as *const windows::Storage::Streams::DataWriter) };
            let fut = Box::pin(async move {
                writer
                    .StoreAsync()
                    .map_err(|e| e.to_string())?
                    .await
                    .map_err(|e| e.to_string())
            });
            self.write_fut = Some(fut);

            return self.poll_write(cx, &[]);
        }

        let fut = self.write_fut.as_mut().unwrap();
        match fut.as_mut().poll(cx) {
            std::task::Poll::Pending => std::task::Poll::Pending,
            std::task::Poll::Ready(Err(e)) => {
                self.write_fut = None;
                std::task::Poll::Ready(Err(std::io::Error::new(std::io::ErrorKind::Other, e)))
            }
            std::task::Poll::Ready(Ok(stored)) => {
                self.write_fut = None;
                std::task::Poll::Ready(Ok(stored as usize))
            }
        }
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        if self.write_fut.is_none() {
            let writer = unsafe { &*(&self.writer as *const windows::Storage::Streams::DataWriter) };
            let fut = Box::pin(async move {
                writer
                    .FlushAsync()
                    .map_err(|e| e.to_string())?
                    .await
                    .map_err(|e| e.to_string())
                    .map(|_| 42)
            });
            self.write_fut = Some(fut);
        }

        let fut = self.write_fut.as_mut().unwrap();
        match fut.as_mut().poll(cx) {
            std::task::Poll::Pending => std::task::Poll::Pending,
            std::task::Poll::Ready(Err(e)) => {
                self.write_fut = None;
                std::task::Poll::Ready(Err(std::io::Error::new(std::io::ErrorKind::Other, e)))
            }
            std::task::Poll::Ready(Ok(_)) => {
                self.write_fut = None;
                std::task::Poll::Ready(Ok(()))
            }
        }
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        self.poll_flush(cx)
    }
}

impl WindowsRfcommStream {
    /// Wrap an accepted or connected `StreamSocket`, creating the reader and
    /// writer for its streams.
    fn new(socket: StreamSocket) -> windows::core::Result<Self> {
        let input = socket.InputStream()?;
        let reader = DataReader::CreateDataReader(&input)?;
        // Partial mode: LoadAsync returns as soon as *any* data is available,
        // rather than waiting for the full requested count.
        reader.SetInputStreamOptions(InputStreamOptions::Partial)?;
        let output = socket.OutputStream()?;
        let writer = DataWriter::CreateDataWriter(&output)?;
        Ok(Self {
            _socket: socket,
            reader,
            writer,
            read_fut: None,
            write_fut: None,
        })
    }
}

impl std::io::Read for WindowsRfcommStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let count = futures::executor::block_on(async {
            self.reader
                .LoadAsync(buf.len() as u32)
                .map_err(|e| std::io::Error::other(e.to_string()))?
                .await
                .map_err(|e| std::io::Error::other(e.to_string()))
        })? as usize;
        self.reader
            .ReadBytes(&mut buf[..count])
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        Ok(count)
    }
}

impl std::io::Write for WindowsRfcommStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        // WriteBytes copies data into the DataWriter's internal buffer.
        self.writer
            .WriteBytes(buf)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        // StoreAsync sends the buffered data; FlushAsync ensures the OS has
        // forwarded it to the remote device.
        futures::executor::block_on(async {
            self.writer
                .StoreAsync()
                .map_err(|e| std::io::Error::other(e.to_string()))?
                .await
                .map_err(|e| std::io::Error::other(e.to_string()))?;
            self.writer
                .FlushAsync()
                .map_err(|e| std::io::Error::other(e.to_string()))?
                .await
                .map_err(|e| std::io::Error::other(e.to_string()))?;
            Ok::<(), std::io::Error>(())
        })
    }
}

// ---------------------------------------------------------------------------
// RFCOMM connectable  (server-side pending connection)
// ---------------------------------------------------------------------------

/// A socket that the OS has handed us after a remote device connected to our
/// advertised RFCOMM service.  Call `accept` to promote it into a
/// `BluetoothStream`.
pub struct BluetoothRfcommConnectable {
    /// The already-connected socket that is ready for I/O.
    socket: StreamSocket,
}

impl BluetoothRfcommConnectable {
    pub async fn accept(self) -> Result<(crate::BluetoothStream, [u8; 6], u8), String> {
        let mac = {
            let info = self.socket.Information().map_err(|e| e.to_string())?;
            let addr_str = info.RemoteAddress()
                .map_err(|e| e.to_string())?
                .DisplayName()
                .map_err(|e| e.to_string())?
                .to_string();
            
            // Bluetooth MAC addresses come back as "XX:XX:XX:XX:XX:XX"
            Self::parse_mac(&addr_str).ok_or_else(|| format!("Invalid MAC address: {addr_str}"))?
        };
        let stream = WindowsRfcommStream::new(self.socket).map_err(|e| e.to_string())?;
        
        Ok((stream, mac, 42))
    }

    fn parse_mac(s: &str) -> Option<[u8; 6]> {
        let mut bytes = [0u8; 6];
        let mut parts = s.split(':');
        for b in &mut bytes {
            *b = u8::from_str_radix(parts.next()?, 16).ok()?;
        }
        // Ensure there are no extra parts
        if parts.next().is_some() {
            return None;
        }
        Some(bytes)
    }
}

// ---------------------------------------------------------------------------
// RFCOMM profile  (server-side listener)
// ---------------------------------------------------------------------------

/// An active RFCOMM server profile that accepts incoming connections.
///
/// Created by `BluetoothHandler::register_rfcomm_profile`.  Dropping this
/// value stops the SDP advertisement and closes the socket listener.
pub struct BluetoothRfcommProfile {
    /// The WinRT RFCOMM service provider that owns the SDP advertisement.
    provider: RfcommServiceProvider,
    /// The socket listener that accepts raw connections from the OS.
    listener: StreamSocketListener,
    /// Channel through which accepted sockets are forwarded from the WinRT
    /// event handler.
    rx: tokio::sync::mpsc::Receiver<StreamSocket>,
    /// Token used to unregister the `ConnectionReceived` handler on drop.
    token: i64,
}

impl Drop for BluetoothRfcommProfile {
    fn drop(&mut self) {
        // Best-effort cleanup; errors during teardown are silently ignored.
        let _ = self.listener.RemoveConnectionReceived(self.token);
        let _ = self.provider.StopAdvertising();
    }
}

impl BluetoothRfcommProfile {
    pub async fn connectable(&mut self) -> Result<crate::BluetoothRfcommConnectable, String> {
        self.rx
            .recv()
            .await
            .map(|socket| {
                BluetoothRfcommConnectable {
                    socket,
                }
            })
            .ok_or_else(|| "Connection channel closed".to_string())
    }
}

// ---------------------------------------------------------------------------
// Discovery
// ---------------------------------------------------------------------------

/// Manages Bluetooth Classic device discovery using a WinRT `DeviceWatcher`.
///
/// Discovery runs for as long as this value is alive; dropping it stops the
/// watcher.
pub struct BluetoothDiscovery {
    /// The underlying OS device watcher.
    watcher: DeviceWatcher,
}

impl BluetoothDiscovery {
    /// Wrap an already-started `DeviceWatcher`.
    fn new(watcher: DeviceWatcher) -> Self {
        Self { watcher }
    }
}

impl Drop for BluetoothDiscovery {
    fn drop(&mut self) {
        let _ = self.watcher.Stop();
    }
}

// ---------------------------------------------------------------------------
// Device
// ---------------------------------------------------------------------------

/// A Bluetooth Classic device that is visible to or paired with this machine.
pub struct BluetoothDevice {
    /// The underlying WinRT device object.
    inner: WinBtDevice,
}

impl BluetoothDevice {
    fn get_uuids(&mut self) -> Result<Vec<crate::BluetoothUuid>, std::io::Error> {
        // UUIDs are obtained by calling GetRfcommServicesAsync() and collecting
        // the ServiceId GUIDs from each returned RfcommDeviceService — an async
        // operation not yet implemented here.
        todo!("Windows RFCOMM service UUID enumeration not yet implemented")
    }

    fn get_name(&self) -> Result<String, std::io::Error> {
        self.inner
            .Name()
            .map(|n| n.to_string())
            .map_err(|e| std::io::Error::other(e.to_string()))
    }

    fn get_address(&mut self) -> Result<String, std::io::Error> {
        let addr = self
            .inner
            .BluetoothAddress()
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        let b = bt_u64_to_bytes(addr);
        Ok(format!(
            "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
            b[0], b[1], b[2], b[3], b[4], b[5]
        ))
    }

    fn get_pair_state(&self) -> Result<crate::PairingStatus, std::io::Error> {
        let info = self
            .inner
            .DeviceInformation()
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        let pairing = info
            .Pairing()
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        let is_paired = pairing
            .IsPaired()
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        Ok(if is_paired {
            crate::PairingStatus::Paired
        } else {
            crate::PairingStatus::NotPaired
        })
    }

    fn get_rfcomm_socket(
        &mut self,
        _uuid: crate::BluetoothUuid,
        _is_secure: bool,
    ) -> Result<crate::BluetoothSocket, String> {
        // Requires GetRfcommServicesForIdAsync() then StreamSocket::ConnectAsync().
        todo!("Windows client-side RFCOMM socket not yet implemented")
    }

    fn get_l2cap_socket(
        &mut self,
        _uuid: crate::BluetoothUuid,
        _is_secure: bool,
    ) -> Result<crate::BluetoothSocket, String> {
        // Classic BT L2CAP is not exposed via WinRT; only BLE L2CAP CoC is.
        todo!("Windows client-side L2CAP socket not yet implemented")
    }

    fn run_sdp(&mut self) {
        // On Windows, SDP records are fetched on demand via
        // GetRfcommServicesAsync; there is no explicit "run SDP" step.
    }
}

// ---------------------------------------------------------------------------
// Client-side RFCOMM socket
// ---------------------------------------------------------------------------

/// A client-side Bluetooth RFCOMM socket.
pub struct BluetoothRfcommSocket {
    /// The underlying WinRT socket.
    socket: StreamSocket,
    /// Whether `ConnectAsync` has completed successfully.
    connected: bool,
}

impl BluetoothRfcommSocket {
    fn is_connected(&self) -> Result<bool, std::io::Error> {
        Ok(self.connected)
    }

    fn connect(&mut self) -> Result<(), std::io::Error> {
        // Full implementation: call socket.ConnectAsync() with the device's
        // ConnectionHostName and ConnectionServiceName from RfcommDeviceService.
        todo!("Windows BluetoothRfcommSocket::connect not yet implemented")
    }
}

impl std::io::Read for BluetoothRfcommSocket {
    fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
        todo!("Windows BluetoothRfcommSocket::read not yet implemented")
    }
}

impl std::io::Write for BluetoothRfcommSocket {
    fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
        todo!("Windows BluetoothRfcommSocket::write not yet implemented")
    }

    fn flush(&mut self) -> std::io::Result<()> {
        todo!("Windows BluetoothRfcommSocket::flush not yet implemented")
    }
}

// ---------------------------------------------------------------------------
// Adapter handler
// ---------------------------------------------------------------------------

/// The top-level Bluetooth handler for Windows.
///
/// Wraps the system default `BluetoothAdapter` and implements
/// `AsyncBluetoothAdapterTrait` using WinRT async APIs.
pub struct BluetoothHandler {
    /// The system's default Bluetooth radio.
    adapter: WinBtAdapter,
    /// Channel back to the application for pairing UI messages.
    _sender: tokio::sync::mpsc::Sender<super::MessageToBluetoothHost>,
}

impl BluetoothHandler {
    pub async fn register_rfcomm_profile(
        &self,
        settings: super::BluetoothRfcommProfileSettings,
    ) -> Result<crate::BluetoothRfcommProfile, String> {
        // 1. Build the RFCOMM service ID from the profile UUID.
        let guid = parse_uuid_to_guid(&settings.uuid)?;
        let service_id = RfcommServiceId::FromUuid(guid).map_err(|e| e.to_string())?;

        // 2. Create the service provider; this registers an SDP record with the
        //    Bluetooth stack.
        let provider = RfcommServiceProvider::CreateAsync(&service_id)
            .map_err(|e| e.to_string())?
            .await
            .map_err(|e| e.to_string())?;

        // 3. Create a socket listener and route accepted sockets through a
        //    bounded channel so callers can await them with `connectable()`.
        let listener = StreamSocketListener::new().map_err(|e| e.to_string())?;

        let (tx, rx) = tokio::sync::mpsc::channel::<StreamSocket>(16);

        let token = listener
            .ConnectionReceived(&TypedEventHandler::<
                StreamSocketListener,
                StreamSocketListenerConnectionReceivedEventArgs,
            >::new(move |_sender, args| {
                if let Some(args) = args.as_ref() {
                    if let Ok(socket) = args.Socket() {
                        let _ = tx.try_send(socket);
                    }
                }
                Ok(())
            }))
            .map_err(|e| e.to_string())?;

        // 4. Bind the listener to the RFCOMM service name (= the UUID string
        //    as produced by RfcommServiceId::AsString) with the requested
        //    protection level.
        let protection_level = if settings.authenticate.unwrap_or(false) {
            SocketProtectionLevel::BluetoothEncryptionWithAuthentication
        } else {
            SocketProtectionLevel::BluetoothEncryptionAllowNullAuthentication
        };

        let service_name: HSTRING = provider
            .ServiceId()
            .map_err(|e| e.to_string())?
            .AsString()
            .map_err(|e| e.to_string())?;

        listener
            .BindServiceNameWithProtectionLevelAsync(&service_name, protection_level)
            .map_err(|e| e.to_string())?
            .await
            .map_err(|e| e.to_string())?;

        // 5. Advertise the service so that remote devices can discover it.
        provider
            .StartAdvertising(&listener)
            .map_err(|e| e.to_string())?;

        Ok(
            BluetoothRfcommProfile {
                provider,
                listener,
                rx,
                token,
            },
        )
    }

    async fn register_l2cap_profile(
        &self,
        _settings: super::BluetoothL2capProfileSettings,
    ) -> Result<crate::BluetoothL2capProfile, String> {
        // Classic BT L2CAP profile registration is not exposed via WinRT.
        Err(
            "Classic Bluetooth L2CAP profile registration is not supported              on Windows via WinRT"
                .to_string(),
        )
    }

    fn get_paired_devices(&self) -> Option<Vec<crate::BluetoothDevice>> {
        let selector = WinBtDevice::GetDeviceSelectorFromPairingState(true).ok()?;

        let collection = futures::executor::block_on(async {
            DeviceInformation::FindAllAsyncAqsFilter(&selector)
                .map_err(|e| e.to_string())?
                .await
                .map_err(|e| e.to_string())
        })
        .ok()?;

        let count = collection.Size().ok()?;
        let mut devices = Vec::with_capacity(count as usize);

        for i in 0..count {
            let info = match collection.GetAt(i) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let id = match info.Id() {
                Ok(v) => v,
                Err(_) => continue,
            };
            if let Ok(device) = futures::executor::block_on(async {
                WinBtDevice::FromIdAsync(&id)
                    .map_err(|e| e.to_string())?
                    .await
                    .map_err(|e| e.to_string())
            }) {
                devices.push(BluetoothDevice {
                    inner: device,
                });
            }
        }
        Some(devices)
    }

    fn start_discovery(&self) -> crate::BluetoothDiscovery {
        let selector = WinBtDevice::GetDeviceSelector()
            .expect("Failed to build Bluetooth device AQS selector");
        let watcher = DeviceInformation::CreateWatcherAqsFilter(&selector)
            .expect("Failed to create DeviceWatcher");
        watcher.Start().expect("Failed to start DeviceWatcher");
        BluetoothDiscovery::new(watcher).into()
    }

    async fn addresses(&self) -> Vec<super::BluetoothAdapterAddress> {
        match self.adapter.BluetoothAddress() {
            Ok(addr) => vec![super::BluetoothAdapterAddress::Byte(bt_u64_to_bytes(addr))],
            Err(_) => vec![],
        }
    }

    async fn set_discoverable(&self, _d: bool) -> Result<(), ()> {
        // WinRT does not expose an API for controlling adapter discoverability
        // from third-party apps; this is handled by the OS Settings app.
        log::warn!(
            "Bluetooth discoverability cannot be set programmatically on              Windows via WinRT"
        );
        Ok(())
    }
}

impl BluetoothHandler {
    /// Construct a new `BluetoothHandler` using the system default Bluetooth
    /// adapter.
    ///
    /// Returns `Err` when no Bluetooth radio is present or when the Windows
    /// Runtime has not been initialised in the calling process.
    pub async fn new(
        s: tokio::sync::mpsc::Sender<super::MessageToBluetoothHost>,
    ) -> Result<Self, String> {
        let adapter = WinBtAdapter::GetDefaultAsync()
            .map_err(|e| e.to_string())?
            .await
            .map_err(|e| e.to_string())?;
        Ok(Self {
            adapter,
            _sender: s,
        })
    }
}
