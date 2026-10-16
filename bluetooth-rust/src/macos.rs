//! macOS-specific Classic Bluetooth support through Apple's IOBluetooth framework.

use std::collections::VecDeque;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObjectProtocol};
use objc2::{AnyThread, DefinedClass, define_class, msg_send};
use objc2_foundation::{NSArray, NSObject};
use objc2_io_bluetooth::{
    IOBluetoothDevice as NativeDevice, IOBluetoothDeviceInquiry, IOBluetoothHostController,
    IOBluetoothRFCOMMChannel, IOBluetoothRFCOMMChannelDelegate,
};
use tokio::sync::mpsc;

/// The local macOS Bluetooth controller.
pub struct BluetoothHandler {
    /// IOBluetooth's default host controller.
    controller: Retained<IOBluetoothHostController>,
}

impl BluetoothHandler {
    /// Construct a handler for the system's default Bluetooth controller.
    pub async fn new() -> Result<Self, String> {
        let controller = unsafe { IOBluetoothHostController::defaultController() }
            .ok_or_else(|| "No Bluetooth controller is available".to_string())?;
        Ok(Self { controller })
    }

    /// Return the address of the local Bluetooth controller.
    pub async fn addresses(&self) -> Vec<crate::BluetoothAdapterAddress> {
        unsafe { self.controller.addressAsString() }
            .map(|address| vec![crate::BluetoothAdapterAddress::String(address.to_string())])
            .unwrap_or_default()
    }

    /// Return the devices currently paired with this Mac.
    pub async fn get_paired_devices(&self) -> Option<Vec<BluetoothDevice>> {
        let devices = unsafe { NativeDevice::pairedDevices() }?;
        Some(devices_from_array(devices))
    }

    /// Start a Classic Bluetooth inquiry.
    ///
    /// Read the devices found so far from [`BluetoothDiscovery::devices`].
    /// Dropping the discovery value stops the inquiry.
    pub fn start_discovery(&self) -> Result<BluetoothDiscovery, String> {
        let inquiry = unsafe { IOBluetoothDeviceInquiry::inquiryWithDelegate(None) }
            .ok_or_else(|| "Failed to create Bluetooth device inquiry".to_string())?;
        let result = unsafe { inquiry.start() };
        if result != 0 {
            return Err(format!("Failed to start Bluetooth inquiry: {result}"));
        }
        Ok(BluetoothDiscovery { inquiry })
    }

    /// IOBluetooth does not provide a supported API to set host discoverability.
    pub async fn set_discoverable(&self, _discoverable: bool) -> Result<(), ()> {
        log::warn!("Host Bluetooth discoverability cannot be controlled through IOBluetooth");
        Err(())
    }
}

/// An active Classic Bluetooth device inquiry.
pub struct BluetoothDiscovery {
    /// The IOBluetooth inquiry retained until discovery ends.
    inquiry: Retained<IOBluetoothDeviceInquiry>,
}

impl BluetoothDiscovery {
    /// Return the devices discovered by this inquiry so far.
    pub fn devices(&self) -> Vec<BluetoothDevice> {
        unsafe { self.inquiry.foundDevices() }
            .map(devices_from_array)
            .unwrap_or_default()
    }
}

impl Drop for BluetoothDiscovery {
    fn drop(&mut self) {
        let result = unsafe { self.inquiry.stop() };
        if result != 0 {
            log::debug!("Stopping Bluetooth inquiry returned {result}");
        }
    }
}

/// A Classic Bluetooth device.
pub struct BluetoothDevice {
    /// The IOBluetooth device object.
    inner: Retained<NativeDevice>,
}

impl BluetoothDevice {
    /// Return the device's friendly name, falling back to its address.
    pub async fn get_name(&self) -> Result<String, io::Error> {
        unsafe { self.inner.nameOrAddress() }
            .map(|name| name.to_string())
            .or_else(|| unsafe { self.inner.addressString() }.map(|address| address.to_string()))
            .ok_or_else(|| io::Error::other("IOBluetooth did not provide a device name or address"))
    }

    /// Return the device's Bluetooth hardware address.
    pub fn get_address(&mut self) -> Result<String, io::Error> {
        unsafe { self.inner.addressString() }
            .map(|address| address.to_string())
            .ok_or_else(|| io::Error::other("IOBluetooth did not provide a device address"))
    }

    /// Return whether the device is paired with this Mac.
    pub async fn get_pair_state(&self) -> Result<crate::PairingStatus, io::Error> {
        let paired = unsafe { self.inner.isPaired() };
        Ok(if paired {
            crate::PairingStatus::Paired
        } else {
            crate::PairingStatus::NotPaired
        })
    }

    /// Start an SDP query for services on the device.
    ///
    /// IOBluetooth completes SDP queries asynchronously; service records become
    /// available after the system has finished the query.
    pub fn run_sdp(&self) -> Result<(), String> {
        let result = unsafe { self.inner.performSDPQuery(None) };
        if result == 0 {
            Ok(())
        } else {
            Err(format!("Failed to start SDP query: {result}"))
        }
    }

    /// Return the well-known service UUIDs found in cached SDP records.
    ///
    /// Call [`BluetoothDevice::run_sdp`] first when records have not yet been
    /// queried. IOBluetooth does not expose an iterator over a record's full
    /// service-class UUID list, so this reports recognized standard UUIDs.
    pub async fn get_uuids(&mut self) -> Result<Vec<crate::BluetoothUuid>, io::Error> {
        let Some(services) = (unsafe { self.inner.services() }) else {
            return Ok(Vec::new());
        };
        let services: Retained<NSArray<objc2_io_bluetooth::IOBluetoothSDPServiceRecord>> =
            unsafe { Retained::cast_unchecked(services) };
        let known = [
            (0x1101, crate::BluetoothUuid::SPP),
            (0x110a, crate::BluetoothUuid::A2dpSource),
            (0x110b, crate::BluetoothUuid::A2dpSink),
            (0x1108, crate::BluetoothUuid::HspHs),
            (0x1112, crate::BluetoothUuid::HspAg),
            (0x111f, crate::BluetoothUuid::HfpAg),
            (0x111e, crate::BluetoothUuid::HfpHs),
            (0x1105, crate::BluetoothUuid::ObexOpp),
            (0x1106, crate::BluetoothUuid::ObexFtp),
            (0x1104, crate::BluetoothUuid::ObexSync),
            (0x110e, crate::BluetoothUuid::AvrcpRemote),
            (0x1116, crate::BluetoothUuid::NetworkingNap),
            (0x112f, crate::BluetoothUuid::ObexPse),
            (0x1132, crate::BluetoothUuid::ObexMas),
            (0x1133, crate::BluetoothUuid::ObexMns),
        ];
        let mut uuids = Vec::new();
        for (uuid16, uuid) in known {
            let found = (0..services.count()).any(|index| {
                let record = unsafe { services.objectAtIndex_unchecked(index) };
                unsafe { record.matchesUUID16(uuid16) }
            });
            if found {
                uuids.push(uuid);
            }
        }
        Ok(uuids)
    }

    /// Create an RFCOMM socket for the specified channel.
    ///
    /// The UUID is accepted for compatibility with other backends because the
    /// caller supplies the RFCOMM channel directly. When `is_secure` is true,
    /// macOS authentication is requested before opening the channel.
    pub fn get_rfcomm_socket(
        &mut self,
        channel: u8,
        _uuid: crate::BluetoothUuid,
        is_secure: bool,
    ) -> Result<BluetoothSocket, String> {
        Ok(BluetoothSocket {
            device: self.inner.clone(),
            channel,
            is_secure,
            stream: None,
        })
    }
}

/// An outgoing RFCOMM connection.
pub struct BluetoothSocket {
    /// Device to connect to when `async_connect` is called.
    device: Retained<NativeDevice>,
    /// RFCOMM channel selected by the caller.
    channel: u8,
    /// Whether to request link authentication before opening the channel.
    is_secure: bool,
    /// Active RFCOMM stream after a successful connection.
    stream: Option<BluetoothStream>,
}

impl BluetoothSocket {
    /// Return whether the socket currently has an open RFCOMM channel.
    pub fn is_connected(&self) -> Result<bool, io::Error> {
        Ok(self
            .stream
            .as_ref()
            .is_some_and(|stream| unsafe { stream.channel.isOpen() }))
    }

    /// Open the RFCOMM channel.
    ///
    /// IOBluetooth only exposes a synchronous channel-open operation, so this
    /// call can block while macOS establishes the Classic Bluetooth link.
    pub async fn async_connect(&mut self) -> Result<(), io::Error> {
        if self.is_connected()? {
            return Ok(());
        }
        if self.is_secure {
            let result = unsafe { self.device.requestAuthentication() };
            if result != 0 {
                return Err(io::Error::other(format!(
                    "Failed to authenticate before opening RFCOMM channel {}: {result}",
                    self.channel
                )));
            }
        }
        let (tx, rx) = mpsc::unbounded_channel();
        let delegate = RfcommDelegate::new(tx);
        let delegate_object: &AnyObject = &delegate;
        let mut channel_out = None;
        let result = unsafe {
            self.device.openRFCOMMChannelSync_withChannelID_delegate(
                Some(&mut channel_out),
                self.channel,
                Some(delegate_object),
            )
        };
        if result != 0 {
            return Err(io::Error::other(format!(
                "Failed to open RFCOMM channel {}: {result}",
                self.channel
            )));
        }
        let channel = channel_out.ok_or_else(|| {
            io::Error::other("IOBluetooth reported success without returning an RFCOMM channel")
        })?;
        self.stream = Some(BluetoothStream {
            channel,
            _delegate: delegate,
            incoming: rx,
            buffered: VecDeque::new(),
            closed: false,
        });
        Ok(())
    }

    fn not_connected_error() -> io::Error {
        io::Error::new(
            io::ErrorKind::NotConnected,
            "RFCOMM socket is not connected",
        )
    }
}

/// Wrap the device objects returned by one of IOBluetooth's device-list APIs.
fn devices_from_array(devices: Retained<NSArray>) -> Vec<BluetoothDevice> {
    // IOBluetooth documents these arrays as containing IOBluetoothDevice objects.
    let devices: Retained<NSArray<NativeDevice>> = unsafe { Retained::cast_unchecked(devices) };
    let mut result = Vec::with_capacity(devices.count());
    for index in 0..devices.count() {
        let address = unsafe { devices.objectAtIndex_unchecked(index).addressString() };
        let Some(address) = address else {
            continue;
        };
        if let Some(inner) = unsafe { NativeDevice::deviceWithAddressString(Some(&address)) } {
            result.push(BluetoothDevice { inner });
        }
    }
    result
}

/// Callback state for incoming RFCOMM data.
struct RfcommDelegateIvars {
    /// Receiver notified when IOBluetooth delivers channel data.
    sender: mpsc::UnboundedSender<Option<Vec<u8>>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[ivars = RfcommDelegateIvars]
    struct RfcommDelegate;

    unsafe impl NSObjectProtocol for RfcommDelegate {}

    unsafe impl IOBluetoothRFCOMMChannelDelegate for RfcommDelegate {
        #[unsafe(method(rfcommChannelData:data:length:))]
        unsafe fn receive_data(
            &self,
            _channel: Option<&IOBluetoothRFCOMMChannel>,
            data: *mut std::ffi::c_void,
            length: usize,
        ) {
            if data.is_null() || length == 0 {
                return;
            }
            let bytes = unsafe { std::slice::from_raw_parts(data.cast::<u8>(), length) }.to_vec();
            let _ = self.ivars().sender.send(Some(bytes));
        }

        #[unsafe(method(rfcommChannelClosed:))]
        unsafe fn channel_closed(&self, _channel: Option<&IOBluetoothRFCOMMChannel>) {
            let _ = self.ivars().sender.send(None);
        }
    }
);

impl RfcommDelegate {
    /// Create a delegate that forwards incoming data to the socket's receiver.
    fn new(sender: mpsc::UnboundedSender<Option<Vec<u8>>>) -> Retained<Self> {
        let allocated = Self::alloc().set_ivars(RfcommDelegateIvars { sender });
        unsafe { msg_send![super(allocated), init] }
    }
}

/// An open RFCOMM channel.
pub struct BluetoothStream {
    /// Native channel used for RFCOMM reads, writes, and shutdown.
    channel: Retained<IOBluetoothRFCOMMChannel>,
    /// IOBluetooth holds its delegate weakly; keep it alive for the channel.
    _delegate: Retained<RfcommDelegate>,
    /// Receives byte chunks delivered by the native channel delegate.
    incoming: mpsc::UnboundedReceiver<Option<Vec<u8>>>,
    /// Bytes left over after satisfying an earlier read.
    buffered: VecDeque<u8>,
    /// Whether the remote side closed the RFCOMM channel.
    closed: bool,
}

impl tokio::io::AsyncRead for BluetoothSocket {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.get_mut().stream.as_mut() {
            Some(stream) => Pin::new(stream).poll_read(cx, buf),
            None => Poll::Ready(Err(Self::not_connected_error())),
        }
    }
}

impl tokio::io::AsyncWrite for BluetoothSocket {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.get_mut().stream.as_mut() {
            Some(stream) => Pin::new(stream).poll_write(cx, bytes),
            None => Poll::Ready(Err(Self::not_connected_error())),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut().stream.as_mut() {
            Some(stream) => Pin::new(stream).poll_flush(cx),
            None => Poll::Ready(Err(Self::not_connected_error())),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut().stream.as_mut() {
            Some(stream) => Pin::new(stream).poll_shutdown(cx),
            None => Poll::Ready(Err(Self::not_connected_error())),
        }
    }
}

impl tokio::io::AsyncRead for BluetoothStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if buf.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        if self.closed {
            return Poll::Ready(Ok(()));
        }
        while self.buffered.is_empty() {
            match Pin::new(&mut self.incoming).poll_recv(cx) {
                Poll::Ready(Some(Some(bytes))) => self.buffered.extend(bytes),
                Poll::Ready(Some(None)) | Poll::Ready(None) => {
                    self.closed = true;
                    return Poll::Ready(Ok(()));
                }
                Poll::Pending => return Poll::Pending,
            }
        }
        let count = buf.remaining().min(self.buffered.len());
        let bytes: Vec<u8> = self.buffered.drain(..count).collect();
        buf.put_slice(&bytes);
        Poll::Ready(Ok(()))
    }
}

impl tokio::io::AsyncWrite for BluetoothStream {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        if bytes.is_empty() {
            return Poll::Ready(Ok(0));
        }
        let this = self.get_mut();
        let mtu = unsafe { this.channel.getMTU() as usize };
        let count = bytes.len().min(mtu).min(u16::MAX as usize);
        if count == 0 {
            return Poll::Ready(Err(io::Error::other(
                "RFCOMM channel reported a zero-byte MTU",
            )));
        }
        let mut chunk = bytes[..count].to_vec();
        let result = unsafe {
            this.channel
                .writeSync_length(chunk.as_mut_ptr().cast(), chunk.len() as u16)
        };
        if result == 0 {
            Poll::Ready(Ok(count))
        } else {
            Poll::Ready(Err(io::Error::other(format!(
                "Failed to write to RFCOMM channel: {result}"
            ))))
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let result = unsafe { self.channel.closeChannel() };
        if result == 0 {
            Poll::Ready(Ok(()))
        } else {
            Poll::Ready(Err(io::Error::other(format!(
                "Failed to close RFCOMM channel: {result}"
            ))))
        }
    }
}
