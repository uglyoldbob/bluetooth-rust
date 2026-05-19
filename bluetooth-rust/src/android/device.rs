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

impl crate::BluetoothDeviceTrait for BluetoothDevice {
    fn run_sdp(&mut self, uuid: crate::BluetoothUuid) -> Result<crate::ServiceRecord, String> {
        let mut java = self.java.lock().unwrap();
        let _result = java.use_env(|env, _context| {
            let dev_name = env
                .call_method(&self.internal, "fetchUuidsWithSdp", "()Z", &[])
                .get_boolean();
            dev_name.map_err(|e| jerr(env, e))
        });
        Ok(())
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

    fn get_l2cap_socket(
        &mut self,
        psm: u16,
        is_secure: bool,
    ) -> Result<crate::BluetoothSocket, String> {
        Err("Not supported".to_string())
    }

    fn get_rfcomm_socket(
        &mut self,
        channel: u8,
        is_secure: bool,
    ) -> Result<crate::BluetoothSocket, String> {
        Err("Not supported".to_string())
    }

    #[cfg(feature = "sync")]
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

    #[cfg(feature = "sync")]
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

    #[cfg(feature = "sync")]
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
