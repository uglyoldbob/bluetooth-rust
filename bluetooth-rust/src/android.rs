//! Android specific bluetooth code

use jni::objects::GlobalRef;
use jni::sys::_jobject;
use jni_min_helper::*;
use winit::platform::android::activity::AndroidApp;

/// Maps unexpected JNI errors to `std::io::Error`.
/// (`From<jni::errors::Error>` cannot be implemented for `std::io::Error`
/// here because of the orphan rule). Side effect: `jni_last_cleared_ex()`.
#[inline(always)]
pub(crate) fn jerr(env: &mut jni::JNIEnv, err: jni::errors::Error) -> std::io::Error {
    use jni::errors::Error::*;
    if let JavaException = err {
        let err = jni_min_helper::jni_clear_ex(err);
        jni_min_helper::jni_last_cleared_ex()
            .ok_or(JavaException)
            .and_then(|ex| Ok((ex.get_class_name(env)?, ex.get_throwable_msg(env)?)))
            .map(|(cls, msg)| {
                if cls.contains("SecurityException") {
                    std::io::Error::new(std::io::ErrorKind::PermissionDenied, msg)
                } else if cls.contains("IllegalArgumentException") {
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, msg)
                } else {
                    std::io::Error::other(format!("{cls}: {msg}"))
                }
            })
            .unwrap_or(std::io::Error::other(err))
    } else {
        std::io::Error::other(err)
    }
}

#[ouroboros::self_referencing]
pub struct Java {
    app: AndroidApp,
    java: jni::JavaVM,
    #[borrows(java)]
    #[not_covariant]
    env: jni::AttachGuard<'this>,
}

impl Java {
    /// Use the java environment with a closure that returns a type. Generally used to make calls to java code.
    pub fn use_env<T, F: FnOnce(&mut jni::JNIEnv, jni::objects::JObject) -> T>(
        &mut self,
        f: F,
    ) -> T {
        let context = unsafe {
            jni::objects::JObject::from_raw(
                self.borrow_app().activity_as_ptr() as *mut jni::sys::_jobject
            )
        };
        self.with_env_mut(|a| f(a, context))
    }

    /// Retrieve a clone of the androidapp object
    pub fn get_app(&self) -> AndroidApp {
        self.borrow_app().clone()
    }

    /// Make a new java object using the androidapp object
    pub fn make(app: AndroidApp) -> Self {
        let vm = unsafe {
            jni::JavaVM::from_raw(app.vm_as_ptr() as *mut *const jni::sys::JNIInvokeInterface_)
        }
        .unwrap();
        JavaBuilder {
            app,
            java: vm,
            env_builder: |java: &jni::JavaVM| java.attach_current_thread().unwrap(),
        }
        .build()
    }
}

use std::convert::TryInto;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;

mod socket;
pub use socket::BluetoothSocket;

use crate::bluetooth_uuid::ParcelUuid;

mod device;
pub use device::BluetoothDevice;

pub struct BluetoothDiscovery {
    adapter: jni::objects::GlobalRef,
    java: Arc<Mutex<super::Java>>,
}

impl<'a> BluetoothDiscovery {
    fn new(adapter: jni::objects::GlobalRef, java: Arc<Mutex<super::Java>>) -> Self {
        Self { adapter, java }
    }
}

impl Drop for BluetoothDiscovery {
    fn drop(&mut self) {
        let mut java = self.java.lock().unwrap();
        java.use_env(|env, _context| {
            let _ = env
                .call_method(&self.adapter, "cancelDiscovery", "()Z", &[])
                .clear_ex();
        });
    }
}

/// And object used for communication with a remote bluetooth device
pub struct RfcommStream {
    /// The BluetoothSocket object
    socket: OnceLock<jni::objects::GlobalRef>,
    /// The input stream
    input: OnceLock<jni::objects::GlobalRef>,
    /// The output stream
    output: OnceLock<jni::objects::GlobalRef>,
    /// The java instance
    java: Arc<Mutex<super::Java>>,
}

impl RfcommStream {
    /// Build a new Self, getting the input and output streams needed for communication
    pub fn new(
        socket: OnceLock<jni::objects::GlobalRef>,
        java: Arc<Mutex<super::Java>>,
    ) -> Result<Self, String> {
        let (input, output) = {
            let mut java2 = java.lock().unwrap();
            java2.use_env(|env, _context| {
                let socket = socket.get().unwrap().as_obj();
                let e = env
                    .call_method(socket, "getInputStream", "()Ljava/io/InputStream;", &[])
                    .get_object(env)
                    .map_err(|e| jerr(env, e).to_string())?;
                let input = env
                    .new_global_ref(&e)
                    .map_err(|e| jerr(env, e).to_string())?;
                let e = env
                    .call_method(socket, "getOutputStream", "()Ljava/io/OutputStream;", &[])
                    .get_object(env)
                    .map_err(|e| jerr(env, e).to_string())?;
                let output = env
                    .new_global_ref(&e)
                    .map_err(|e| jerr(env, e).to_string())?;
                Ok::<(GlobalRef, GlobalRef), String>((input, output))
            })
        }?;
        Ok(Self {
            socket,
            input: input.into(),
            output: output.into(),
            java,
        })
    }
}

impl std::io::Read for RfcommStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let mut java2 = self.java.lock().unwrap();
        java2.use_env(|env, _context| {
            let ba = env
                .new_byte_array(buf.len() as i32)
                .map_err(|e| std::io::Error::other(e))?;
            let socket = self.socket.get().unwrap().as_obj();
            let e = env
                .call_method(socket, "readNBytes", "([BII)I", &[(&ba).into()])
                .get_object(env)
                .map_err(|e| std::io::Error::other(jerr(env, e).to_string()))?;
            let l = e.get_int(env).map_err(|e| std::io::Error::other(e))?;
            let a = env
                .convert_byte_array(ba)
                .map_err(|e| std::io::Error::other(e))?;
            buf[0..l as usize].copy_from_slice(&a);
            Ok(l as usize)
        })
    }
}

impl std::io::Write for RfcommStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut java2 = self.java.lock().unwrap();
        java2.use_env(|env, _context| {
            let ba = env
                .byte_array_from_slice(buf)
                .map_err(|e| std::io::Error::other(e))?;
            let socket = self.socket.get().unwrap().as_obj();
            let _ = env
                .call_method(socket, "write", "([B)", &[(&ba).into()])
                .get_object(env)
                .map_err(|e| jerr(env, e))?;
            Ok(buf.len())
        })
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let mut java2 = self.java.lock().unwrap();
        java2.use_env(|env, _context| {
            let socket = self.socket.get().unwrap().as_obj();
            let _ = env
                .call_method(socket, "flush", "()", &[])
                .get_object(env)
                .map_err(|e| jerr(env, e))?;
            Ok(())
        })
    }
}

/// Very similar to the BluetoothRfcommProfile
pub struct BluetoothRfcommConnectable {
    /// A socket that can be used to accept bluetooth connections
    socket: OnceLock<jni::objects::GlobalRef>,
    /// The java instance
    java: Arc<Mutex<super::Java>>,
}

impl super::BluetoothRfcommConnectableSyncTrait for BluetoothRfcommConnectable {
    fn accept(self, timeout: std::time::Duration) -> Result<crate::BluetoothStream, String> {
        let mut java2 = self.java.lock().unwrap();
        let millis = (timeout.as_millis() as i64).into();
        java2.use_env(|env, _context| {
            let socket = self.socket.get().unwrap().as_obj();
            let e = env
                .call_method(
                    socket,
                    "accept",
                    "(I)Landroid/bluetooth/BluetoothSocket;",
                    &[millis],
                )
                .get_object(env)
                .map_err(|e| jerr(env, e).to_string())?;
            let socket = env
                .new_global_ref(&e)
                .map_err(|e| jerr(env, e).to_string())?;
            let s = RfcommStream::new(socket.into(), self.java.clone())?;
            let comm = crate::BluetoothStream::Android(s);
            Ok(comm)
        })
    }
}

/// A bluetooth rfcomm profile
pub struct BluetoothRfcommProfile {
    /// A socket that can be used to accept bluetooth connections
    socket: OnceLock<jni::objects::GlobalRef>,
    /// The java instance
    java: Arc<Mutex<super::Java>>,
}

impl crate::BluetoothRfcommProfileSyncTrait for BluetoothRfcommProfile {
    fn connectable(&mut self) -> Result<crate::BluetoothRfcommConnectableSync, String> {
        Ok(crate::BluetoothRfcommConnectableSync::Android(
            BluetoothRfcommConnectable {
                socket: self.socket.clone(),
                java: self.java.clone(),
            },
        ))
    }
}

/// The bluetooth adapter struct for android code
pub struct Bluetooth {
    adapter: jni::objects::GlobalRef,
    java: Arc<Mutex<super::Java>>,
    /// An instance of Intent, created with registerReceiver
    receiver: Option<jni::objects::GlobalRef>,
    /// The broadcast_receiver for the bluetooth uuid
    blue_uuid_receiver: Option<jni_min_helper::BroadcastReceiver>,
}

impl super::BluetoothAdapterTrait for Bluetooth {
    fn supports_async(&mut self) -> Option<&mut dyn super::AsyncBluetoothAdapterTrait> {
        None
    }

    fn supports_sync(&mut self) -> Option<&mut dyn super::SyncBluetoothAdapterTrait> {
        Some(self)
    }
}

impl crate::SyncBluetoothAdapterTrait for Bluetooth {
    fn register_rfcomm_profile(
        &self,
        settings: crate::BluetoothRfcommProfileSettings,
    ) -> Result<crate::BluetoothRfcommProfileSync, String> {
        let mut java2 = self.java.lock().unwrap();
        {
            java2.use_env(|env, context| {
                let jsettings = {
                    log::error!("Register rfcomm 1");
                    log::error!("Finding builder class");
                    let ss = env.find_class("android/bluetooth/BluetoothSocketSettings$Builder").map_err(|e| jerr(env, e).to_string())?;
                    log::error!("Found builder class");
                    let builder_constructor = env.get_method_id(&ss, "<init>", "()V").map_err(|e| jerr(env, e).to_string())?;
                    log::error!("Got constructor");
                    let obj = env.new_object(&ss, "()V", &[]).map_err(|e| jerr(env, e).to_string())?;
                    log::error!("Success in making socket settings builder?");
                    let mut jsettings = obj;
                    log::error!("Register rfcomm 2");
                    if let Some(auth) = settings.authenticate {
                        let e = env
                            .call_method(jsettings, "setAuthenticationRequired", "(Z)Landroid/bluetooth/BluetoothSocketSettings$Builder;", &[auth.into()])
                            .get_object(env)
                            .map_err(|e| jerr(env, e).to_string())?;
                        jsettings = env.new_local_ref(&e).map_err(|e| jerr(env, e).to_string())?;
                    }
                    log::error!("Register rfcomm 3");
                    if let Some(val) = settings.psm {
                        let e = env
                            .call_method(jsettings, "setL2capPsm", "(I)Landroid/bluetooth/BluetoothSocketSettings$Builder;", &[val.into()])
                            .get_object(env)
                            .map_err(|e| jerr(env, e).to_string())?;
                        jsettings = env.new_local_ref(&e).map_err(|e| jerr(env, e).to_string())?;
                    }
                    log::error!("Register rfcomm 4");
                    if let Some(name) = &settings.name {
                        let arg = name
                            .new_jobject(env)
                            .map_err(|e| jerr(env, e))
                            .unwrap();
                        let e = env
                            .call_method(jsettings, "setRfcommServiceName", "(Ljava/lang/String;)Landroid/bluetooth/BluetoothSocketSettings$Builder;", &[(&arg).into()])
                            .get_object(env)
                            .map_err(|e| jerr(env, e).to_string())?;
                        jsettings = env.new_local_ref(&e).map_err(|e| jerr(env, e).to_string())?;
                    }
                    log::error!("Register rfcomm 5");
                    {
                        let arg = settings.uuid.as_str()
                            .new_jobject(env)
                            .map_err(|e| jerr(env, e))
                            .unwrap();
                        let uuid_class = env.find_class("java/util/UUID").map_err(|e| jerr(env, e).to_string())?;
                        let uuid = env.call_static_method(uuid_class, "fromString", "(Ljava/lang/String;)Ljava/util/UUID;", &[(&arg).into()]).map_err(|e| jerr(env, e).to_string())?;
                        let e = env
                            .call_method(jsettings, "setRfcommUuid", "(Ljava/util/UUID;)Landroid/bluetooth/BluetoothSocketSettings$Builder;", &[uuid.borrow()])
                            .get_object(env)
                            .map_err(|e| jerr(env, e).to_string())?;
                        jsettings = env.new_local_ref(&e).map_err(|e| jerr(env, e).to_string())?;
                    }
                    log::error!("Register rfcomm 6");
                    let e = env
                            .call_method(jsettings, "build", "()Landroid/bluetooth/BluetoothSocketSettings;", &[])
                            .get_object(env)
                            .map_err(|e| jerr(env, e).to_string())?;
                    jsettings = env.new_local_ref(&e).map_err(|e| jerr(env, e).to_string())?;
                    log::error!("Register rfcomm 7");
                    Ok::<jni::objects::JObject<'_>, String>(jsettings)
                }?;
                log::error!("Register rfcomm 8");
                let jsettings = jni::objects::JValueGen::try_from(jsettings).map_err(|e| e.to_string())?;
                log::error!("Register rfcomm 9");
                let mut sig = String::new();
                log::error!("Register rfcomm 9.1");
                sig.push_str("(Landroid/bluetooth/BluetoothSocketSettings;)");
                sig.push_str("Landroid/bluetooth/BluetoothServerSocket;");
                let e = env
                    .call_method(
                        &self.adapter,
                        "listenUsingSocketSettings",
                        &sig,
                        &[jsettings.borrow()],
                    )
                    .get_object(env)
                    .map_err(|e| jerr(env, e).to_string())?;
                log::error!("Register rfcomm 10");
                let socket = env
                    .new_global_ref(&e)
                    .map_err(|e| jerr(env, e).to_string())?;
                log::error!("Register rfcomm 11");
                Ok(crate::BluetoothRfcommProfileSync::Android(
                    BluetoothRfcommProfile {
                        socket: socket.into(),
                        java: self.java.clone(),
                    },
                ))
            })
        }
    }

    fn set_discoverable(&self, d: bool) -> Result<(), ()> {
        let mut java = self.java.lock().unwrap();
        java.use_env(|env, context| {
            let arg = "android.bluetooth.adapter.action.REQUEST_DISCOVERABLE"
                .new_jobject(env)
                .map_err(|e| jerr(env, e))
                .unwrap();
            let intent = env
                .new_object(
                    "android/content/Intent",
                    "(Ljava/lang/String;)V",
                    &[(&arg).into()],
                )
                .unwrap();
            let mut args = Vec::new();
            args.push(&intent);
            let mut args2: Vec<jni::objects::JValueGen<&jni::objects::JObject>> =
                args.iter().map(|a| a.try_into().unwrap()).collect();
            args2.push(1.into());
            let a = env.call_method(
                context,
                "startActivityForResult",
                "(Landroid/content/Intent;I)V",
                args2.as_slice(),
            );
            log::error!("Results of bluetooth enable discoverable is {:?}", a);
        });
        Ok(())
    }

    fn get_paired_devices(&self) -> Option<Vec<crate::BluetoothDevice>> {
        let bd = self.get_bonded_devices();
        if let Some(bd) = bd {
            let mut devs = Vec::new();
            for b in bd {
                devs.push(crate::BluetoothDevice::Android(b));
            }
            Some(devs)
        } else {
            None
        }
    }

    fn start_discovery(&self) -> crate::BluetoothDiscovery {
        BluetoothDiscovery::new(self.adapter.clone(), self.java.clone()).into()
    }

    fn addresses(&self) -> Vec<super::BluetoothAdapterAddress> {
        let mut a = Vec::new();
        let mut java = self.java.lock().unwrap();
        let n = java.use_env(|env, context| {
            let action = env
                .call_method(&self.adapter, "getAddress", "()Ljava/lang/String;", &[])
                .get_object(env)?;
            if action.is_null() {
                return Err(jni::errors::Error::NullPtr("No action"));
            }
            action.get_string(env)
        });
        if let Ok(n) = n {
            a.push(super::BluetoothAdapterAddress::String(n));
        }
        a
    }
}

type ReadCallback = Box<dyn Fn(Option<usize>) + 'static + Send>;

const BLUETOOTH_SERVICE: &str = "bluetooth";

impl Bluetooth {
    /// constructs a new Self with the protected java instance
    pub fn new(java: Arc<Mutex<super::Java>>) -> Self {
        let adapter = {
            let mut java2 = java.lock().unwrap();
            java2.use_env(|env, context| {
                Self::get_adapter(env, &context).unwrap()
            })
        };
        Self {
            adapter,
            java,
            receiver: None,
            blue_uuid_receiver: None,
        }
    }

    fn check_adapter(&mut self) {
        if self.receiver.is_none() {
            let arg1 = jni_min_helper::BroadcastReceiver::build(|env, _context, intent| {
                let action = env
                    .call_method(intent, "getAction", "()Ljava/lang/String;", &[])
                    .get_object(env)?;
                if action.is_null() {
                    return Err(jni::errors::Error::NullPtr("No action"));
                }
                let _ = action.get_string(env).map_err(|e| jerr(env, e));
                Ok(())
            })
            .unwrap();
            let r = register_receiver(&self.java, &arg1, "android.bluetooth.device.action.UUID");
            self.blue_uuid_receiver.replace(arg1);
            if let Some(r) = r {
                log::error!("Receiver is {:?}", r);
                self.receiver.replace(r);
            }
        }
    }

    /// Request permission if it is not present because we need it
    pub fn try_get_permissions(&self, app: AndroidApp, permission: &str,) -> Result<bool, std::io::Error> {
        let mut java = self.java.lock().unwrap();
        java.use_env(|env, context| {
            if !self.check_permission2(env, &context, permission)? {
                let mut stat = false;
                self.get_permission2(env, &context, permission, app)
            }
            else {
                Ok(true)
            }
        })
    }

    /// Check to see if we have the specified permission
    pub fn check_permission(&self, permission: &str,) -> Result<bool, std::io::Error> {
        let mut java = self.java.lock().unwrap();
        java.use_env(|env, context| {
            self.check_permission2(env, &context, permission)
        })
    }

    /// Attempt to get the permisssion needed
    pub fn get_permission2(&self, env: &mut jni::JNIEnv, context: &jni::objects::JObject, permission: &str, app: AndroidApp, ) -> Result<bool, std::io::Error> {
        // Get ClassLoader instance from activity
        let class_loader_obj = env
            .call_method(&context, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])
            .expect("Failed to get ClassLoader")
            .l()
            .map_err(|e| jerr(env, e))?;

        // Name of the class you want to load
        let class_name = "com.example.android.JniBridge"
            .new_jobject(env)
            .map_err(|e| jerr(env, e))?;

        // Call loadClass method (no need to get_method_id explicitly, call_method resolves it)
        let jni_bridge_class_obj = env
            .call_method(
                class_loader_obj,
                "loadClass",
                "(Ljava/lang/String;)Ljava/lang/Class;",
                &[(&class_name).into()],
            )
            .expect("Failed to call loadClass")
            .l()
            .map_err(|e| jerr(env, e))?;

        // Convert JObject to JClass for new_object
        let jni_bridge_class = jni::objects::JClass::from(jni_bridge_class_obj);

        // Instantiate JniBridge using its no-arg constructor
        let jni_bridge_obj = env
            .new_object(jni_bridge_class, "()V", &[])
            .map_err(|e| jerr(env, e))?;

        let activity = unsafe { jni::objects::JObject::from_raw(app.activity_as_ptr() as *mut _jobject) };

        let arg2 = permission
            .new_jobject(env)
            .map_err(|e| jerr(env, e))
            .unwrap();
        let asdf = env.call_method(jni_bridge_obj, "requestPermission", "(Landroid/app/Activity;Ljava/lang/String;)I", &[(&activity).try_into().unwrap(), (&arg2).try_into().unwrap()]).map_err(|e| jerr(env, e))?;
        asdf.i().map(|v| v == 0).map_err(|e| jerr(env, e))
    }

    /// Check to see if we have the specified permission
    pub fn check_permission2(&self, env: &mut jni::JNIEnv, context: &jni::objects::JObject, permission: &str,) -> Result<bool, std::io::Error> {
        // Get ClassLoader instance from activity
        let class_loader_obj = env
            .call_method(&context, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])
            .expect("Failed to get ClassLoader")
            .l()
            .map_err(|e| jerr(env, e))?;

        // Name of the class you want to load
        let class_name = "com.example.android.JniBridge"
            .new_jobject(env)
            .map_err(|e| jerr(env, e))?;

        // Call loadClass method (no need to get_method_id explicitly, call_method resolves it)
        let jni_bridge_class_obj = env
            .call_method(
                class_loader_obj,
                "loadClass",
                "(Ljava/lang/String;)Ljava/lang/Class;",
                &[(&class_name).into()],
            )
            .expect("Failed to call loadClass")
            .l()
            .map_err(|e| jerr(env, e))?;

        // Convert JObject to JClass for new_object
        let jni_bridge_class = jni::objects::JClass::from(jni_bridge_class_obj);

        // Instantiate JniBridge using its no-arg constructor
        let jni_bridge_obj = env
            .new_object(jni_bridge_class, "()V", &[])
            .map_err(|e| jerr(env, e))?;

        let arg2 = permission
            .new_jobject(env)
            .map_err(|e| jerr(env, e))
            .unwrap();
        let asdf = env.call_method(jni_bridge_obj, "checkSelfPermission", "(Landroid/content/Context;Ljava/lang/String;)I", &[(&context).try_into().unwrap(), (&arg2).try_into().unwrap()]).map_err(|e| jerr(env, e))?;
        asdf.i().map(|v| v == 0).map_err(|e| jerr(env, e))
    }

    /// Enables the bluetooth adapter
    pub fn enable(&mut self) {
        if !self.is_enabled() {
            log::error!("Bluetooth not enabled. Requesting it to be enabled");
            let mut java = self.java.lock().unwrap();
            java.use_env(|env, context| {
                let arg = "android.bluetooth.adapter.action.REQUEST_ENABLE"
                    .new_jobject(env)
                    .map_err(|e| jerr(env, e))
                    .unwrap();
                let intent = env
                    .new_object(
                        "android/content/Intent",
                        "(Ljava/lang/String;)V",
                        &[(&arg).into()],
                    )
                    .unwrap();
                let mut args = Vec::new();
                args.push(&intent);
                let mut args2: Vec<jni::objects::JValueGen<&jni::objects::JObject>> =
                    args.iter().map(|a| a.try_into().unwrap()).collect();
                args2.push(1.into());
                let a = env.call_method(
                    context,
                    "startActivityForResult",
                    "(Landroid/content/Intent;I)V",
                    args2.as_slice(),
                );
                log::error!("Results of bluetooth enable is {:?}", a);
            })
        }
    }

    /// Returns the enabled state of the bluetooth adapter
    pub fn is_enabled(&mut self) -> bool {
        self.check_adapter();
        let mut java = self.java.lock().unwrap();
        java.use_env::<bool, _>(|env, _context| -> bool {
            let a = env
                .call_method(&self.adapter, "isEnabled", "()Z", &[])
                .get_boolean()
                .map_err(|e| jerr(env, e));
            a.unwrap()
        })
    }

    /// Get the list of bonded devices for the bluetooth adapter
    pub fn get_bonded_devices(&self) -> Option<Vec<BluetoothDevice>> {
        let mut java = self.java.lock().unwrap();
        java.use_env(
            |env, _context| -> Result<Vec<BluetoothDevice>, std::io::Error> {
                let dev_set = env
                    .call_method(&self.adapter, "getBondedDevices", "()Ljava/util/Set;", &[])
                    .get_object(env)
                    .map_err(|e| jerr(env, e))?;
                if dev_set.is_null() {
                    return Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
                }
                let jarr = env
                    .call_method(&dev_set, "toArray", "()[Ljava/lang/Object;", &[])
                    .get_object(env)
                    .map_err(|e| jerr(env, e))?;
                let jarr: &jni::objects::JObjectArray = jarr.as_ref().into();
                let len = env.get_array_length(jarr).map_err(|e| jerr(env, e))?;
                let mut vec = Vec::with_capacity(len as usize);
                for i in 0..len {
                    vec.push(BluetoothDevice::new(
                        env.get_object_array_element(jarr, i)
                            .global_ref(env)
                            .map_err(|e| jerr(env, e))?,
                        self.java.clone(),
                    ));
                }
                Ok(vec)
            },
        )
        .ok()
    }

    fn get_adapter<'a>(
        env: &mut jni::JNIEnv<'a>,
        context: &jni::objects::JObject,
    ) -> Result<jni::objects::GlobalRef, std::io::Error> {
        let bluetooth_service = BLUETOOTH_SERVICE
            .new_jobject(env)
            .map_err(|e| jerr(env, e))?;
        let manager = env
            .call_method(
                context,
                "getSystemService",
                "(Ljava/lang/String;)Ljava/lang/Object;",
                &[(&bluetooth_service).into()],
            )
            .get_object(env)
            .map_err(|e| jerr(env, e))?;
        if manager.is_null() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "Cannot get BLUETOOTH_SERVICE",
            ));
        }
        let adapter = env
            .call_method(
                manager,
                "getAdapter",
                "()Landroid/bluetooth/BluetoothAdapter;",
                &[],
            )
            .get_object(env)
            .map_err(|e| jerr(env, e))?;
        if !adapter.is_null() {
            Ok(env.new_global_ref(&adapter).map_err(|e| jerr(env, e))?)
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "`getAdapter` returned null",
            ))
        }
    }
}

fn register_receiver(
    java: &Arc<Mutex<super::Java>>,
    arg1: &jni_min_helper::BroadcastReceiver,
    intent_str: &str,
) -> Option<jni::objects::GlobalRef> {
    let mut java2 = java.lock().unwrap();
    let mut sig = String::new();
    sig.push_str("(");
    sig.push_str("Landroid/content/BroadcastReceiver;");
    sig.push_str("Landroid/content/IntentFilter;");
    sig.push_str(")Landroid/content/Intent;");
    java2.use_env(|env, context| {
        let mut args = Vec::new();
        let intent_str = intent_str.new_jobject(env).unwrap();
        let arg2 = env.new_object(
            "android/content/IntentFilter",
            "(Ljava/lang/String;)V",
            &[(&intent_str).into()],
        );
        let arg2 = arg2.unwrap();
        args.push(arg1.as_ref());
        args.push(&arg2);
        let args2: Vec<jni::objects::JValueGen<&jni::objects::JObject>> =
            args.iter().map(|a| a.try_into().unwrap()).collect();
        let e = env
            .call_method(context, "registerReceiver", &sig, args2.as_slice())
            .get_object(env)
            .map_err(|e| jerr(env, e))
            .ok()?;
        env.new_global_ref(&e).map_err(|e| jerr(env, e)).ok()
    })
}
