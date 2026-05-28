//! Code for bluetooth devices

use super::BluetoothSocket;
use super::{ParcelUuid, jerr};
use crate::BluetoothUuid;
use jni::objects::JObject;
use jni_min_helper::*;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

/// an android bluetooth device
pub struct BluetoothDevice {
    internal: jni::objects::Global<JObject<'static>>,
    rfcomm_sockets: BTreeMap<String, BluetoothSocket>,
}

impl BluetoothDevice {
    /// Run the sdp protocol
    pub fn run_sdp(&self) -> Result<crate::ServiceRecord, String> {
        let java = jni_min_helper::jni_get_vm();
        let mut env = java
            .attach_current_thread(|env| {
                let dev_name = env
                    .call_method(
                        &self.internal,
                        jni::jni_str!("fetchUuidsWithSdp"),
                        jni::jni_sig!("()Z"),
                        &[],
                    )?
                    .into_bool()?;
                Ok(dev_name)
            })
            .map_err(|e| jerr(e).to_string())?;
        Ok(())
    }

    /// get the bluetooth address
    pub fn get_address(&mut self) -> Result<String, std::io::Error> {
        let java = jni_min_helper::jni_get_vm();
        java.attach_current_thread(|env| {
            let dev_name = env
                .call_method(
                    &self.internal,
                    jni::jni_str!("getAddress"),
                    jni::jni_sig!("()Ljava/lang/String;"),
                    &[],
                )?
                .l()?;
            let temp = env.new_local_ref(&dev_name)?;
            let jstr = env.cast_local::<jni::objects::JString>(temp)?;
            Ok(env.get_string(&jstr)?.to_str().to_string())
        })
        .map_err(jerr)
    }

    fn get_l2cap_socket(
        &mut self,
        psm: u16,
        uuid: BluetoothUuid,
        is_secure: bool,
    ) -> Result<crate::BluetoothSocket, String> {
        let uuid = uuid.as_str();
        log::warn!("Checking rfcomm for {}", uuid);
        let java = jni_min_helper::jni_get_vm();
        java.attach_current_thread(|env| {
            if !self.rfcomm_sockets.contains_key(uuid) {
                log::warn!("Building rfcomm for {}", uuid);
                let uuid2 = env.new_string(uuid)?;
                let uuid3 = env
                    .call_static_method(
                        jni::jni_str!("java/util/UUID"),
                        jni::jni_str!("fromString"),
                        jni::jni_sig!("(Ljava/lang/String;)Ljava/util/UUID;"),
                        &[(&uuid2).into()],
                    )?
                    .l()?;

                let method_name = if is_secure {
                    jni::jni_str!("createRfcommSocketToServiceRecord")
                } else {
                    jni::jni_str!("createInsecureRfcommSocketToServiceRecord")
                };
                let object = env.call_method(
                    &self.internal,
                    method_name,
                    jni::jni_sig!("(Ljava/util/UUID;)Landroid/bluetooth/BluetoothSocket;"),
                    &[(&uuid3).into()],
                )?;
                let socket = env.new_global_ref(
                    object
                    .l()?,
                )?;
                log::warn!("Building2 rfcomm for {}", uuid);
                let socket = BluetoothSocket::build(socket, uuid);
                if let Ok(a) = socket {
                    self.rfcomm_sockets.insert(uuid.to_string(), a);
                }
                log::warn!("Done building rfcomm for {}", uuid);
            }
            Ok(())
        })
        .map_err(|e| jerr(e).to_string())?;
        self.rfcomm_sockets
            .remove(uuid.into())
            .map(|a| a.into())
            .ok_or("Socket does not exist".to_string())
    }

    /// get an rfcomm socket
    pub fn get_rfcomm_socket(
        &mut self,
        channel: u8,
        uuid: BluetoothUuid,
        is_secure: bool,
    ) -> Result<crate::BluetoothSocket, String> {
        let uuid = uuid.as_str();
        log::warn!("Checking rfcomm for {}", uuid);
        let java = jni_min_helper::jni_get_vm();
        java.attach_current_thread(|env| {
            if !self.rfcomm_sockets.contains_key(uuid) {
                log::warn!("Building rfcomm for {}", uuid);
                let uuid2 = env.new_string(uuid)?;
                let uuid3 = env
                    .call_static_method(
                        jni::jni_str!("java/util/UUID"),
                        jni::jni_str!("fromString"),
                        jni::jni_sig!("(Ljava/lang/String;)Ljava/util/UUID;"),
                        &[(&uuid2).into()],
                    )?
                    .l()?;

                let method_name = if is_secure {
                    jni::jni_str!("createRfcommSocketToServiceRecord")
                } else {
                    jni::jni_str!("createInsecureRfcommSocketToServiceRecord")
                };
                let object = env.call_method(
                    &self.internal,
                    method_name,
                    jni::jni_sig!("(Ljava/util/UUID;)Landroid/bluetooth/BluetoothSocket;"),
                    &[(&uuid3).into()],
                )?;
                let socket = env.new_global_ref(
                    object
                    .l()?,
                )?;
                log::warn!("Building2 rfcomm for {}", uuid);
                let socket = BluetoothSocket::build(socket, uuid);
                if let Ok(a) = socket {
                    self.rfcomm_sockets.insert(uuid.to_string(), a);
                }
                log::warn!("Done building rfcomm for {}", uuid);
            }
            Ok(())
        })
        .map_err(|e| jerr(e).to_string())?;
        self.rfcomm_sockets
            .remove(uuid.into())
            .map(|a| a.into())
            .ok_or("Socket does not exist".to_string())
    }

    /// Get all known uuids for the device
    #[cfg(feature = "sync")]
    pub fn get_uuids(&mut self) -> Result<Vec<BluetoothUuid>, std::io::Error> {
        let p = self.get_parcel_uuids();
        match p {
            Ok(p) => {
                use std::convert::TryInto;
                Ok(p.into_iter().map(|a| a.try_into().unwrap()).collect())
            }
            Err(e) => Err(e),
        }
    }

    #[cfg(feature = "sync")]
    fn get_name(&self) -> Result<String, std::io::Error> {
        let java = jni_min_helper::jni_get_vm();
        java.attach_current_thread(|env| {
            let dev_name = env
                .call_method(
                    &self.internal,
                    jni::jni_str!("getName"),
                    jni::jni_sig!("()Ljava/lang/String;"),
                    &[],
                )?
                .l()?;
            let temp = env.new_local_ref(&dev_name)?;
            let jstr = env.cast_local::<jni::objects::JString>(temp)?;
            Ok(env.get_string(&jstr)?.to_str().to_string())
        })
        .map_err(jerr)
    }

    /// Get the current pair state of the device
    #[cfg(feature = "sync")]
    pub fn get_pair_state(&self) -> Result<crate::PairingStatus, std::io::Error> {
        let java = jni_min_helper::jni_get_vm();
        java.attach_current_thread(|env| {
            let s: i32 = env
                .call_method(
                    &self.internal,
                    jni::jni_str!("getBondState"),
                    jni::jni_sig!("()I"),
                    &[],
                )?
                .into_int()?;
            let s = match s {
                10 => crate::PairingStatus::NotPaired,
                11 => crate::PairingStatus::Pairing,
                12 => crate::PairingStatus::Paired,
                _ => crate::PairingStatus::Unknown,
            };
            Ok(s)
        })
        .map_err(jerr)
    }
}

impl BluetoothDevice {
    /// construct a new device
    pub fn new(internal: jni::objects::Global<JObject<'static>>) -> Self {
        Self {
            internal,
            rfcomm_sockets: BTreeMap::new(),
        }
    }

    /// Get the parcel uuids for the device
    pub fn get_parcel_uuids(&mut self) -> Result<Vec<ParcelUuid>, std::io::Error> {
        let java = jni_min_helper::jni_get_vm();
        java.attach_current_thread(|env| {
            let objs = env
                .call_method(
                    &self.internal,
                    jni::jni_str!("getUuids"),
                    jni::jni_sig!("()[Landroid/os/ParcelUuid;"),
                    &[],
                )?
                .l()?;
            if objs.is_null() {
                return Ok(Vec::new());
            }
            let temp = env.new_local_ref(&objs)?;
            let jarr = env.cast_local::<jni::objects::JObjectArray>(temp)?;
            let len = env.get_array_length(&jarr)?;
            let mut vec = Vec::with_capacity(len as usize);
            for i in 0..len as usize {
                let temp = env.get_object_array_element(&jarr, i)?;
                let uuid = env.new_global_ref(temp)?;
                vec.push(ParcelUuid::new(uuid));
            }
            Ok(vec)
        })
        .map_err(jerr)
    }
}
