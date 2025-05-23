//! Linux specific bluetooth code

use std::collections::{HashMap, HashSet};
use std::marker::PhantomData;

use bluer::{AdapterEvent, DeviceProperty};
use futures::FutureExt;
use futures::StreamExt;

impl super::BluetoothRfcommConnectableTrait for bluer::rfcomm::ConnectRequest {
    async fn accept(self) -> Result<crate::BluetoothStream, String> {
        bluer::rfcomm::ConnectRequest::accept(self)
            .map(|a| crate::BluetoothStream::Bluez(Box::pin(a)))
            .map_err(|e| e.to_string())
    }
}

impl super::BluetoothRfcommProfileTrait for bluer::rfcomm::ProfileHandle {
    async fn connectable(&mut self) -> Result<crate::BluetoothRfcommConnectable,String> {
        self.next().await.map(|a|a.into()).ok_or("Failed to get bluetooth connection".to_string())
    }
}

impl super::BluetoothDeviceTrait for bluer::Device {
    #[doc = " Get all known uuids for this device"]
    fn get_uuids(&mut self) -> Result<Vec<crate::BluetoothUuid>, std::io::Error> {
        todo!()
    }

    #[doc = " Retrieve the device name"]
    fn get_name(&self) -> Result<String, std::io::Error> {
        todo!()
    }

    #[doc = " Retrieve the device address"]
    fn get_address(&mut self) -> Result<String, std::io::Error> {
        todo!()
    }

    #[doc = " Retrieve the device pairing (bonding) status"]
    fn get_pair_state(&self) -> Result<crate::PairingStatus, std::io::Error> {
        todo!()
    }

    #[doc = " Attempt to get an rfcomm socket for the given uuid and seciruty setting"]
    fn get_rfcomm_socket(
        &mut self,
        uuid: crate::BluetoothUuid,
        is_secure: bool,
    ) -> Result<crate::BluetoothRfcommSocket, String> {
        todo!()
    }
}

/// An rfcomm socket with a bluetooth peer
pub struct BluetoothRfcommSocket {
    /// The rfcomm socket data, TODO
    inner: u32,
}

/// A struct for managing discovery of bluetooth devices
pub struct BluetoothDiscovery<'a> {
    /// phantom so the struct acts like it has a lifetime
    _dummy: PhantomData<&'a ()>,
}

impl<'a> BluetoothDiscovery<'a> {
    /// construct a new self
    fn new() -> Self {
        Self {
            _dummy: PhantomData,
        }
    }
}

impl<'a> super::BluetoothDiscoveryTrait for BluetoothDiscovery<'a> {}

impl<'a> Drop for BluetoothDiscovery<'a> {
    fn drop(&mut self) {}
}

/// The general bluetooth handler for the library. There should be only one per application on linux.
pub struct BluetoothHandler {
    /// The current bluetooth session
    session: bluer::Session,
    /// The list of bluetooth adapters for the system
    adapters: Vec<bluer::Adapter>,
    /// The agent for the handler
    _blue_agent_handle: bluer::agent::AgentHandle,
}

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
            role: None,
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

impl super::BluetoothAdapterTrait for BluetoothHandler {
    async fn register_rfcomm_profile(
        &self,
        settings: super::BluetoothRfcommProfileSettings,
    ) -> Result<crate::BluetoothRfcommProfile, String> {
        self.session
            .register_profile(settings.try_into()?)
            .await
            .map(|a| super::BluetoothRfcommProfile::Bluez(a.into()))
            .map_err(|e| e.to_string())
    }

    fn start_discovery(&self) -> crate::BluetoothDiscovery {
        BluetoothDiscovery::new().into()
    }

    fn get_paired_devices(&self) -> Option<Vec<crate::BluetoothDevice>> {
        let list = Vec::new();
        for adapter in &self.adapters {
            todo!();
        }
        Some(list)
    }

    async fn addresses(&self) -> Vec<[u8;6]> {
        let mut a = Vec::new();
        for adapter in &self.adapters {
            if let Ok(adr) = adapter.address().await {
                a.push(adr.0);
            }
        }
        a
    }

    async fn set_discoverable(&self, d: bool) -> Result<(), ()> {
        for adapter in &self.adapters {
            adapter.set_discoverable(d).await.map_err(|_|())?;
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

    /// Enable or disable the discoverable of all bluetooth adapters
    pub async fn set_discoverable(&mut self, d: bool) {
        for adapter in &self.adapters {
            adapter.set_discoverable(d).await.unwrap();
        }
    }

    /// Register a profile with the bluetooth session
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
                let _ = s3.send(super::MessageToBluetoothHost::DisplayPasskey(a.passkey, chan.0)).await;
                loop {
                    let f = tokio::time::timeout(std::time::Duration::from_secs(5), chan.1.recv());
                    tokio::select! {
                        asdf = f => {
                            match asdf {
                                Ok(Some(m)) => {
                                    match m {
                                        super::ResponseToPasskey::Yes => {
                                            let _ = s3.send(super::MessageToBluetoothHost::CancelDisplayPasskey).await;
                                            return Ok(());
                                        }
                                        super::ResponseToPasskey::No => {
                                            let _ = s3.send(super::MessageToBluetoothHost::CancelDisplayPasskey).await;
                                            return Err(bluer::agent::ReqError::Rejected);
                                        }
                                        super::ResponseToPasskey::Cancel => {
                                            let _ = s3.send(super::MessageToBluetoothHost::CancelDisplayPasskey).await;
                                            return Err(bluer::agent::ReqError::Canceled);
                                        }
                                        super::ResponseToPasskey::Waiting => {}
                                    }
                                }
                                Ok(None) => {}
                                _ => {
                                    let _ = s3.send(super::MessageToBluetoothHost::CancelDisplayPasskey).await;
                                    return Err(bluer::agent::ReqError::Canceled);
                                }
                            }
                        }
                        _ = &mut a.cancel => {
                            let _ = s3.send(super::MessageToBluetoothHost::CancelDisplayPasskey).await;
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
                Some(super::BluetoothResponse::Adapters(0))
            }
            _ => None,
        }
    }

    /// run a scan on all the bluetooth adapters
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
                        println!("Device added {:?}", addr);
                        bluetooth_devices.insert(addr, (adapt, None));
                    }
                    AdapterEvent::DeviceRemoved(addr) => {
                        println!("Device removed {:?}", addr);
                        bluetooth_devices.remove_entry(&addr);
                    }
                    AdapterEvent::PropertyChanged(prop) => {
                        println!("Property changed {:?}", prop);
                    }
                }
            }
        }
        for (addr, (adapter, dev)) in bluetooth_devices {
            if dev.is_none() {
                if let Ok(d) = adapter.device(*addr) {
                    if let Ok(ps) = d.all_properties().await {
                        for p in ps {}
                    }
                    *dev = Some(d);
                }
            }
        }
    }
}
