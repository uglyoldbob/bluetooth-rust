//! Code for bluetooth devices

use super::super::Java;
use super::BluetoothSocket;
use super::{ParcelUuid, jerr};
use crate::BluetoothUuid;
use jni_min_helper::*;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

pub struct BluetoothDevice {
    internal: jni::objects::GlobalRef,
    rfcomm_sockets: BTreeMap<String, BluetoothSocket>,
    java: Arc<Mutex<Java>>,
}

impl BluetoothDevice {
    fn run_sdp(&mut self) {
        let mut java = self.java.lock().unwrap();
        let _result = java.use_env(|env, _context| {
            let dev_name = env
                .call_method(&self.internal, "fetchUuidsWithSdp", "()Z", &[])
                .get_boolean();
            dev_name.map_err(|e| jerr(env, e))
        });
    }
}

impl crate::BluetoothDeviceTrait for BluetoothDevice {
    fn get_uuids(&mut self) -> Result<Vec<BluetoothUuid>, std::io::Error> {
        let p = self.get_parcel_uuids();
        match p {
            Ok(p) => {
                use std::convert::TryInto;
                Ok(p.into_iter().map(|a| a.try_into().unwrap()).collect())
            }
            Err(e) => Err(e),
        }
    }

    fn get_name(&self) -> Result<String, std::io::Error> {
        let mut java = self.java.lock().unwrap();
        java.use_env(|env, _context| {
            let dev_name = env
                .call_method(&self.internal, "getName", "()Ljava/lang/String;", &[])
                .get_object(env)
                .map_err(|e| jerr(env, e))?;
            if dev_name.is_null() {
                return Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
            }
            dev_name.get_string(env).map_err(|e| jerr(env, e))
        })
    }

    fn get_address(&mut self) -> Result<String, std::io::Error> {
        let mut java = self.java.lock().unwrap();
        java.use_env(|env, _context| {
            let dev_name = env
                .call_method(&self.internal, "getAddress", "()Ljava/lang/String;", &[])
                .get_object(env)
                .map_err(|e| jerr(env, e))?;
            if dev_name.is_null() {
                return Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
            }
            dev_name.get_string(env).map_err(|e| jerr(env, e))
        })
    }

    fn get_pair_state(&self) -> Result<crate::PairingStatus, std::io::Error> {
        let mut java = self.java.lock().unwrap();
        let s: i32 = java.use_env(|env, _context| {
            let dev_name = env
                .call_method(&self.internal, "getBondState", "()I", &[])
                .get_int();
            dev_name.map_err(|e| jerr(env, e))
        })?;
        let s = match s {
            10 => crate::PairingStatus::NotPaired,
            11 => crate::PairingStatus::Pairing,
            12 => crate::PairingStatus::Paired,
            _ => crate::PairingStatus::Unknown,
        };
        Ok(s)
    }

    fn get_l2cap_socket(
        &mut self,
        uuid: BluetoothUuid,
        is_secure: bool,
    ) -> Result<crate::BluetoothSocket, String> {
        let uuid = uuid.as_str();
        log::warn!("Checking rfcomm for {}", uuid);
        let mut java = self.java.lock().unwrap();
        if !self.rfcomm_sockets.contains_key(uuid) {
            log::warn!("Building rfcomm for {}", uuid);
            let socket = java
                .use_env(|env, _context| {
                    let uuid = uuid.new_jobject(env).map_err(|e| jerr(env, e))?;
                    let uuid = env
                        .call_static_method(
                            "java/util/UUID",
                            "fromString",
                            "(Ljava/lang/String;)Ljava/util/UUID;",
                            &[(&uuid).into()],
                        )
                        .get_object(env)
                        .map_err(|e| jerr(env, e))?;

                    let method_name = if is_secure {
                        "createRfcommSocketToServiceRecord"
                    } else {
                        "createInsecureRfcommSocketToServiceRecord"
                    };
                    env.call_method(
                        &self.internal,
                        method_name,
                        "(Ljava/util/UUID;)Landroid/bluetooth/BluetoothSocket;",
                        &[(&uuid).into()],
                    )
                    .get_object(env)
                    .globalize(env)
                    // TODO: distinguish IOException and other unexpected exceptions
                    .map_err(|e| jerr(env, e))
                })
                .map_err(|e| e.to_string())?;
            drop(java);
            log::warn!("Building2 rfcomm for {}", uuid);
            let socket = BluetoothSocket::build(socket, self.java.clone(), uuid);
            if let Ok(a) = socket {
                self.rfcomm_sockets.insert(uuid.to_string(), a);
            }
            log::warn!("Done building rfcomm for {}", uuid);
        }
        self.rfcomm_sockets
            .get_mut(uuid.into())
            .map(|a| a.into())
            .ok_or("Socket does not exist".to_string())
    }

    fn get_rfcomm_socket(
        &mut self,
        uuid: BluetoothUuid,
        is_secure: bool,
    ) -> Result<crate::BluetoothSocket, String> {
        let uuid = uuid.as_str();
        log::warn!("Checking rfcomm for {}", uuid);
        let mut java = self.java.lock().unwrap();
        if !self.rfcomm_sockets.contains_key(uuid) {
            log::warn!("Building rfcomm for {}", uuid);
            let socket = java
                .use_env(|env, _context| {
                    let uuid = uuid.new_jobject(env).map_err(|e| jerr(env, e))?;
                    let uuid = env
                        .call_static_method(
                            "java/util/UUID",
                            "fromString",
                            "(Ljava/lang/String;)Ljava/util/UUID;",
                            &[(&uuid).into()],
                        )
                        .get_object(env)
                        .map_err(|e| jerr(env, e))?;

                    let method_name = if is_secure {
                        "createRfcommSocketToServiceRecord"
                    } else {
                        "createInsecureRfcommSocketToServiceRecord"
                    };
                    env.call_method(
                        &self.internal,
                        method_name,
                        "(Ljava/util/UUID;)Landroid/bluetooth/BluetoothSocket;",
                        &[(&uuid).into()],
                    )
                    .get_object(env)
                    .globalize(env)
                    // TODO: distinguish IOException and other unexpected exceptions
                    .map_err(|e| jerr(env, e))
                })
                .map_err(|e| e.to_string())?;
            drop(java);
            log::warn!("Building2 rfcomm for {}", uuid);
            let socket = BluetoothSocket::build(socket, self.java.clone(), uuid);
            if let Ok(a) = socket {
                self.rfcomm_sockets.insert(uuid.to_string(), a);
            }
            log::warn!("Done building rfcomm for {}", uuid);
        }
        self.rfcomm_sockets
            .get_mut(uuid.into())
            .map(|a| a.into())
            .ok_or("Socket does not exist".to_string())
    }
}

impl BluetoothDevice {
    pub fn new(internal: jni::objects::GlobalRef, java: Arc<Mutex<Java>>) -> Self {
        Self {
            internal,
            rfcomm_sockets: BTreeMap::new(),
            java,
        }
    }

    pub fn get_parcel_uuids(&mut self) -> Result<Vec<ParcelUuid>, std::io::Error> {
        let java2 = self.java.clone();
        let mut java = self.java.lock().unwrap();
        java.use_env(|env, _context| {
            let objs = env
                .call_method(
                    &self.internal,
                    "getUuids",
                    "()[Landroid/os/ParcelUuid;",
                    &[],
                )
                .get_object(env)
                .map_err(|e| jerr(env, e))?;
            let jarr: &jni::objects::JObjectArray = objs.as_ref().into();
            let len = env.get_array_length(jarr).map_err(|e| jerr(env, e))?;
            let mut vec = Vec::with_capacity(len as usize);
            for i in 0..len {
                let uuid = env
                    .get_object_array_element(jarr, i)
                    .global_ref(env)
                    .map_err(|e| jerr(env, e))?;
                vec.push(ParcelUuid::new(uuid, java2.clone()));
            }
            Ok(vec)
        })
    }
}
