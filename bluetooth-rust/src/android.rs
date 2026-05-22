//! Android specific bluetooth code

use jni::objects::JObject;
use jni::refs::Global;
use jni::sys::_jobject;
use jni_min_helper::*;
use winit::platform::android::activity::AndroidApp;

#[cfg(feature = "async")]
compile_error!("Android does not support async");

static JAVA_CONTEXT: OnceLock<Arc<::jni::refs::Global<::jni::objects::JObject>>> = OnceLock::new();

/// Maps unexpected JNI errors to `std::io::Error`.
#[inline(always)]
pub(crate) fn jerr(err: jni::errors::Error) -> std::io::Error {
    std::io::Error::other(err)
}

/// the deprecated java struct for storing the android app and javavm
pub struct Java {
    app: AndroidApp,
    java: jni::JavaVM,
}

impl Java {
    /// Use the java environment with a closure that returns a type. Generally used to make calls to java code.
    pub fn use_env<T, F: FnOnce(&mut jni::Env, jni::objects::JObject) -> T>(&mut self, f: F) -> T {
        self.java
            .attach_current_thread(|e| {
                let context = unsafe {
                    jni::objects::JObject::from_raw(
                        e,
                        self.app.activity_as_ptr() as *mut jni::sys::_jobject,
                    )
                };
                Ok::<T, jni::errors::Error>(f(e, context))
            })
            .unwrap()
    }

    /// Retrieve a clone of the androidapp object
    pub fn get_app(&self) -> AndroidApp {
        self.app.clone()
    }

    /// Make a new java object using the androidapp object
    pub fn make(app: AndroidApp) -> Self {
        let vm = unsafe {
            jni::JavaVM::from_raw(app.vm_as_ptr() as *mut *const jni::sys::JNIInvokeInterface_)
        };
        Java { app, java: vm }
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

/// the object for discovering bluetooth devices
pub struct BluetoothDiscovery {
    adapter: jni::refs::Global<JObject<'static>>,
}

impl BluetoothDiscovery {
    fn new(adapter: jni::refs::Global<JObject<'static>>) -> Self {
        Self { adapter }
    }
}

impl Drop for BluetoothDiscovery {
    fn drop(&mut self) {
        let java = jni_min_helper::jni_get_vm();
        java.attach_current_thread(|env| {
            let _ = env.call_method(
                &self.adapter,
                jni::jni_str!("cancelDiscovery"),
                jni::jni_sig!("()Z"),
                &[],
            );
            Ok::<(), jni::errors::Error>(())
        })
        .unwrap();
    }
}

/// And object used for communication with a remote bluetooth device
pub struct RfcommStream {
    /// The BluetoothSocket object
    socket: OnceLock<::jni::refs::Global<JObject<'static>>>,
    /// The input stream
    input: OnceLock<::jni::refs::Global<JObject<'static>>>,
    /// The output stream
    output: OnceLock<::jni::refs::Global<JObject<'static>>>,
}

impl RfcommStream {
    /// Build a new Self, getting the input and output streams needed for communication
    pub fn new(socket: OnceLock<::jni::refs::Global<JObject<'static>>>) -> Result<Self, String> {
        let (input, output) = {
            let java = jni_min_helper::jni_get_vm();
            java.attach_current_thread(|env| {
                let socket = socket.get().unwrap().as_obj();
                let e = env
                    .call_method(
                        socket,
                        jni::jni_str!("getInputStream"),
                        jni::jni_sig!("()Ljava/io/InputStream;"),
                        &[],
                    )?
                    .l()?;
                let input = env.new_global_ref(&e)?;
                let e = env
                    .call_method(
                        socket,
                        jni::jni_str!("getOutputStream"),
                        jni::jni_sig!("()Ljava/io/OutputStream;"),
                        &[],
                    )?
                    .l()?;
                let output = env.new_global_ref(&e)?;
                Ok::<(Global<JObject<'static>>, Global<JObject<'static>>), jni::errors::Error>((
                    input, output,
                ))
            })
            .map_err(|e| jerr(e).to_string())?
        };
        Ok(Self {
            socket,
            input: input.into(),
            output: output.into(),
        })
    }
}

impl std::io::Read for RfcommStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let mut java = jni_min_helper::jni_get_vm();
        java.attach_current_thread(|env| {
            let ba = env.new_byte_array(buf.len() as usize)?;
            let socket = self.socket.get().unwrap().as_obj();
            let l = env
                .call_method(
                    socket,
                    jni::jni_str!("readNBytes"),
                    jni::jni_sig!("([BII)I"),
                    &[(&ba).into()],
                )?
                .into_int()?;
            let a = env.convert_byte_array(ba)?;
            buf[0..l as usize].copy_from_slice(&a);
            Ok(l as usize)
        })
        .map_err(jerr)
    }
}

impl std::io::Write for RfcommStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut java = jni_min_helper::jni_get_vm();
        java.attach_current_thread(|env| {
            let ba = env.byte_array_from_slice(buf)?;
            let socket = self.socket.get().unwrap().as_obj();
            let _ = env
                .call_method(
                    socket,
                    jni::jni_str!("write"),
                    jni::jni_sig!("([B)V"),
                    &[(&ba).into()],
                )?;
            Ok(buf.len())
        })
        .map_err(jerr)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let mut java = jni_min_helper::jni_get_vm();
        java.attach_current_thread(|env| {
            let socket = self.socket.get().unwrap().as_obj();
            let _ = env
                .call_method(socket, jni::jni_str!("flush"), jni::jni_sig!("()V"), &[])?;
            Ok(())
        })
        .map_err(jerr)
    }
}

/// Very similar to the BluetoothRfcommProfile
pub struct BluetoothRfcommConnectable {
    /// A socket that can be used to accept bluetooth connections
    socket: OnceLock<::jni::refs::Global<JObject<'static>>>,
}

impl BluetoothRfcommConnectable {
    fn accept(
        self,
        timeout: std::time::Duration,
    ) -> Result<(crate::BluetoothStream, [u8; 6], u8), String> {
        let mut java = jni_min_helper::jni_get_vm();
        let a = java
            .attach_current_thread(|env| {
                let millis = (timeout.as_millis() as i32).into();
                let socket = self.socket.get().unwrap().as_obj();
                let e = env
                    .call_method(
                        socket,
                        jni::jni_str!("accept"),
                        jni::jni_sig!("(I)Landroid/bluetooth/BluetoothSocket;"),
                        &[millis],
                    )?
                    .l()?;
                let socket = env.new_global_ref(&e)?;
                Ok(socket)
            })
            .map_err(|e| jerr(e).to_string())?;
        let mut aa = OnceLock::new();
        aa.set(a);
        let s = RfcommStream::new(aa)?;
        Ok((s, [0; 6], 42))
    }
}

/// A bluetooth rfcomm profile
pub struct BluetoothRfcommProfile {
    /// A socket that can be used to accept bluetooth connections
    socket: OnceLock<::jni::refs::Global<JObject<'static>>>,
}

impl BluetoothRfcommProfile {
    fn connectable(&mut self) -> Result<crate::BluetoothRfcommConnectable, String> {
        let mut java = jni_min_helper::jni_get_vm();
        let ss = java
            .attach_current_thread(|env| env.new_global_ref(self.socket.get().unwrap()))
            .map_err(|e| jerr(e).to_string())?;
        let s = OnceLock::new();
        s.set(ss);
        Ok(BluetoothRfcommConnectable { socket: s })
    }
}

/// The bluetooth adapter struct for android code
pub struct Bluetooth {
    adapter: jni::refs::Global<JObject<'static>>,
    /// An instance of Intent, created with registerReceiver
    receiver: Option<::jni::refs::Global<JObject<'static>>>,
    /// The broadcast_receiver for the bluetooth uuid
    blue_uuid_receiver: Option<jni_min_helper::BroadcastReceiver>,
}

impl Bluetooth {
    fn register_l2cap_profile(
        &self,
        _settings: super::BluetoothL2capProfileSettings,
    ) -> Result<crate::BluetoothL2capProfile, String> {
        todo!()
    }

    fn register_rfcomm_profile(
        &self,
        settings: crate::BluetoothRfcommProfileSettings,
    ) -> Result<crate::BluetoothRfcommProfile, String> {
        let mut java = jni_min_helper::jni_get_vm();
        java.attach_current_thread(|env| {
            let jsettings = {
                log::error!("Register rfcomm 1");
                log::error!("Finding builder class");
                let ss = env
                    .find_class(jni::jni_str!("android/bluetooth/BluetoothSocketSettings$Builder"))?;
                log::error!("Found builder class");
                let builder_constructor = env
                    .get_method_id(&ss, jni::jni_str!("<init>"), jni::jni_sig!("()V"))?;
                log::error!("Got constructor");
                let obj = env
                    .new_object(&ss, jni::jni_sig!("()V"), &[])?;
                log::error!("Success in making socket settings builder?");
                let mut jsettings = obj;
                log::error!("Register rfcomm 2");
                if let Some(auth) = settings.authenticate {
                    let e = env
                        .call_method(
                            jsettings,
                            jni::jni_str!("setAuthenticationRequired"),
                            jni::jni_sig!("(Z)Landroid/bluetooth/BluetoothSocketSettings$Builder;"),
                            &[auth.into()],
                        )?
                        .l()?;
                    jsettings = env
                        .new_local_ref(&e)?;
                }
                log::error!("Register rfcomm 3");
                if let Some(val) = settings.psm {
                    let e = env
                        .call_method(
                            jsettings,
                            jni::jni_str!("setL2capPsm"),
                            jni::jni_sig!("(I)Landroid/bluetooth/BluetoothSocketSettings$Builder;"),
                            &[val.into()],
                        )?
                        .l()?;
                    jsettings = env
                        .new_local_ref(&e)?;
                }
                log::error!("Register rfcomm 4");
                if let Some(name) = &settings.name {
                    let arg = env.new_string(name)?;
                    let e = env
                        .call_method(jsettings, jni::jni_str!("setRfcommServiceName"), jni::jni_sig!("(Ljava/lang/String;)Landroid/bluetooth/BluetoothSocketSettings$Builder;"), &[(&arg).into()])?
                        .l()?;
                    jsettings = env
                        .new_local_ref(&e)?;
                }
                log::error!("Register rfcomm 5");
                {
                    let arg = env.new_string(settings
                        .uuid)?;
                    let uuid_class = env
                        .find_class(jni::jni_str!("java/util/UUID"))?;
                    let uuid = env
                        .call_static_method(
                            uuid_class,
                            jni::jni_str!("fromString"),
                            jni::jni_sig!("(Ljava/lang/String;)Ljava/util/UUID;"),
                            &[(&arg).into()],
                        )?;
                    let e = env
                        .call_method(
                            jsettings,
                            jni::jni_str!("setRfcommUuid"),
                            jni::jni_sig!("(Ljava/util/UUID;)Landroid/bluetooth/BluetoothSocketSettings$Builder;"),
                            &[uuid.borrow()],
                        )?
                        .l()?;
                    jsettings = env
                        .new_local_ref(&e)?;
                }
                log::error!("Register rfcomm 6");
                let e = env
                    .call_method(
                        jsettings,
                        jni::jni_str!("build"),
                        jni::jni_sig!("()Landroid/bluetooth/BluetoothSocketSettings;"),
                        &[],
                    )?
                    .l()?;
                jsettings = env
                    .new_local_ref(&e)?;
                log::error!("Register rfcomm 7");
                Ok::<jni::objects::JObject<'_>, jni::errors::Error>(jsettings)
            }?;
            log::error!("Register rfcomm 8");
            let jsettings = jni::objects::JValue::Object(&jsettings);
            log::error!("Register rfcomm 9");
            let e = env
                .call_method(
                    &self.adapter,
                    jni::jni_str!("listenUsingSocketSettings"),
                    jni::jni_sig!("(Landroid/bluetooth/BluetoothSocketSettings;)Landroid/bluetooth/BluetoothServerSocket;"),
                    &[jsettings],
                )?
                .l()?;
            log::error!("Register rfcomm 10");
            let socket = env
                .new_global_ref(&e)?;
            log::error!("Register rfcomm 11");
            Ok(BluetoothRfcommProfile {
                socket: socket.into(),
            })
        }).map_err(|e| jerr(e).to_string())
    }

    fn set_discoverable(&self, d: bool) -> Result<(), ()> {
        let mut java = jni_min_helper::jni_get_vm();
        java.attach_current_thread(|env| {
            let context = JAVA_CONTEXT.get().unwrap();
            let context = env.new_local_ref(context.as_obj())?;
            let arg = env.new_string("android.bluetooth.adapter.action.REQUEST_DISCOVERABLE")?;
            let intent = env
                .new_object(
                    jni::jni_str!("android/content/Intent"),
                    jni::jni_sig!("(Ljava/lang/String;)V"),
                    &[(&arg).into()],
                )
                .unwrap();
            let mut args = Vec::new();
            args.push(&intent);
            let mut args2: Vec<jni::objects::JValue> =
                args.iter().map(|a| a.try_into().unwrap()).collect();
            args2.push(1.into());
            let a = env.call_method(
                context,
                jni::jni_str!("startActivityForResult"),
                jni::jni_sig!("(Landroid/content/Intent;I)V"),
                args2.as_slice(),
            );
            log::error!("Results of bluetooth enable discoverable is {:?}", a);
            Ok(())
        }).map_err(|_e: jni::errors::Error| ())
    }

    /// get paired bluetooth devices
    pub fn get_paired_devices(&self) -> Option<Vec<crate::BluetoothDevice>> {
        let bd = self.get_bonded_devices();
        if let Ok(bd) = bd {
            let mut devs = Vec::new();
            for b in bd {
                devs.push(b);
            }
            Some(devs)
        } else {
            None
        }
    }

    fn start_discovery(&self) -> crate::BluetoothDiscovery {
        let mut java = jni_min_helper::jni_get_vm();
        let ss = java
            .attach_current_thread(|env| env.new_global_ref(self.adapter.as_obj()))
            .map_err(|e| jerr(e).to_string()).unwrap();
        BluetoothDiscovery::new(ss).into()
    }

    fn addresses(&self) -> Vec<super::BluetoothAdapterAddress> {
        let mut a = Vec::new();
        let mut java = jni_min_helper::jni_get_vm();
        java.attach_current_thread(|env| {
            let dev_name = env
                .call_method(
                    &self.adapter,
                    jni::jni_str!("getAddress"),
                    jni::jni_sig!("()Ljava/lang/String;"),
                    &[],
                )?
                .l()?;
            let temp = env.new_local_ref(&dev_name)?;
            let n = env.cast_local::<jni::objects::JString>(temp);
            if let Ok(n) = n {
                a.push(super::BluetoothAdapterAddress::String(env.get_string(&n)?.to_str().to_string()));
            }
            Ok::<_, jni::errors::Error>(a)
        }).unwrap()
    }
}

type ReadCallback = Box<dyn Fn(Option<usize>) + 'static + Send>;

const BLUETOOTH_SERVICE: &str = "bluetooth";

impl Bluetooth {
    /// constructs a new Self with the protected java instance
    pub fn new(app: AndroidApp) -> Self {
        let java = Arc::new(Mutex::new(Java::make(app)));
        let adapter = {
            let mut java2 = java.lock().unwrap();
            java2.use_env(|env, context| Self::get_adapter(env, &context).unwrap())
        };
        Self {
            adapter,
            receiver: None,
            blue_uuid_receiver: None,
        }
    }

    fn check_adapter(&mut self) {
        if self.receiver.is_none() {
            let arg1 = jni_min_helper::BroadcastReceiver::build(|env, _context, intent| {
                let action = env
                    .call_method(intent, jni::jni_str!("getAction"), jni::jni_sig!("()Ljava/lang/String;"), &[])?
                    .l()?;
                if action.is_null() {
                    return Err(jni::errors::Error::NullPtr("No action"));
                }
                let temp = env.new_local_ref(&action)?;
                let jstr = env.cast_local::<jni::objects::JString>(temp)?;
                let _ = env.get_string(&jstr)?.to_str().to_string();
                Ok(())
            })
            .unwrap();
            let r = register_receiver(&arg1, "android.bluetooth.device.action.UUID");
            self.blue_uuid_receiver.replace(arg1);
            if let Some(r) = r {
                log::error!("Receiver is {:?}", r);
                self.receiver.replace(r);
            }
        }
    }

    /// Request permission if it is not present because we need it
    pub fn try_get_permissions(
        &self,
        app: AndroidApp,
        permission: &str,
    ) -> Result<bool, std::io::Error> {
        let mut java = jni_min_helper::jni_get_vm();
        java.attach_current_thread(|env| {
            let context = JAVA_CONTEXT.get().unwrap();
            if !self.check_permission2(env, &context, permission)? {
                let mut stat = false;
                self.get_permission2(env, &context, permission, app)
            } else {
                Ok(true)
            }
        }).map_err(jerr)
    }

    /// Check to see if we have the specified permission
    pub fn check_permission(&self, permission: &str) -> Result<bool, std::io::Error> {
        let mut java = jni_min_helper::jni_get_vm();
        java.attach_current_thread(|env| {
            let context = JAVA_CONTEXT.get().unwrap();
            self.check_permission2(env, &context, permission)
        }).map_err(jerr)
    }

    /// Attempt to get the permisssion needed
    pub fn get_permission2(
        &self,
        env: &mut jni::Env,
        context: &jni::objects::JObject,
        permission: &str,
        app: AndroidApp,
    ) -> Result<bool, jni::errors::Error> {
        // Get ClassLoader instance from activity
        let class_loader_obj = env
            .call_method(&context, jni::jni_str!("getClassLoader"), jni::jni_sig!("()Ljava/lang/ClassLoader;"), &[])
            .expect("Failed to get ClassLoader")
            .l()?;

        // Name of the class you want to load
        let class_name = env.new_string("com.example.android.JniBridge")?;

        // Call loadClass method (no need to get_method_id explicitly, call_method resolves it)
        let jni_bridge_class_obj : jni::objects::JObject = env
            .call_method(
                class_loader_obj,
                jni::jni_str!("loadClass"),
                jni::jni_sig!("(Ljava/lang/String;)Ljava/lang/Class;"),
                &[(&class_name).into()],
            )
            .expect("Failed to call loadClass")
            .l()?;

        let jni_bridge_class_obj: jni::objects::JClass = env.cast_local::<jni::objects::JClass>(jni_bridge_class_obj)?;

        // Convert JObject to JClass for new_object
        let jni_bridge_class = jni::objects::JClass::from(jni_bridge_class_obj);

        // Instantiate JniBridge using its no-arg constructor
        let jni_bridge_obj = env
            .new_object(jni_bridge_class, jni::jni_sig!("()V"), &[])?;

        let activity = env.new_local_ref(JAVA_CONTEXT.get().unwrap().as_obj())?;

        let arg2 = env.new_string(permission)?;
        let asdf = env
            .call_method(
                jni_bridge_obj,
                jni::jni_str!("requestPermission"),
                jni::jni_sig!("(Landroid/app/Activity;Ljava/lang/String;)I"),
                &[jni::objects::JValue::Object(&activity), (&arg2).try_into().unwrap()],
            )?;
        asdf.i().map(|v| v == 0)
    }

    /// Check to see if we have the specified permission
    pub fn check_permission2(
        &self,
        env: &mut jni::Env,
        context: &jni::objects::JObject,
        permission: &str,
    ) -> Result<bool, jni::errors::Error> {
        // Get ClassLoader instance from activity
        let class_loader_obj = env
            .call_method(&context, jni::jni_str!("getClassLoader"), jni::jni_sig!("()Ljava/lang/ClassLoader;"), &[])
            .expect("Failed to get ClassLoader")
            .l()?;

        // Name of the class you want to load
        let class_name = env.new_string("com.example.android.JniBridge")?;

        // Call loadClass method (no need to get_method_id explicitly, call_method resolves it)
        let jni_bridge_class_obj : jni::objects::JObject = env
            .call_method(
                class_loader_obj,
                jni::jni_str!("loadClass"),
                jni::jni_sig!("(Ljava/lang/String;)Ljava/lang/Class;"),
                &[(&class_name).into()],
            )
            .expect("Failed to call loadClass")
            .l()?;

        let jni_bridge_class_obj: jni::objects::JClass = env.cast_local::<jni::objects::JClass>(jni_bridge_class_obj)?;

        // Convert JObject to JClass for new_object
        let jni_bridge_class = jni::objects::JClass::from(jni_bridge_class_obj);

        // Instantiate JniBridge using its no-arg constructor
        let jni_bridge_obj = env
            .new_object(jni_bridge_class, jni::jni_sig!("()V"), &[])?;

        let activity = env.new_local_ref(JAVA_CONTEXT.get().unwrap().as_obj())?;

        let arg2 = env.new_string(permission)?;
        let asdf = env
            .call_method(
                jni_bridge_obj,
                jni::jni_str!("checkSelfPermission"),
                jni::jni_sig!("(Landroid/content/Context;Ljava/lang/String;)I"),
                &[jni::objects::JValue::Object(&activity), (&arg2).try_into().unwrap()],
            )?;
        asdf.i().map(|v| v == 0)
    }

    /// Enables the bluetooth adapter
    pub fn enable(&mut self) {
        if !self.is_enabled() {
            log::error!("Bluetooth not enabled. Requesting it to be enabled");
            let mut java = jni_min_helper::jni_get_vm();
            java.attach_current_thread(|env| {
                let context = env.new_local_ref(JAVA_CONTEXT.get().unwrap().as_obj())?;
                let arg = env.new_string("android.bluetooth.adapter.action.REQUEST_ENABLE")?;
                let intent = env
                    .new_object(
                        jni::jni_str!("android/content/Intent"),
                        jni::jni_sig!("(Ljava/lang/String;)V"),
                        &[(&arg).into()],
                    )
                    .unwrap();
                let mut args = Vec::new();
                args.push(&intent);
                let mut args2: Vec<jni::objects::JValue> = args
                    .iter()
                    .map(|a: &&jni::objects::JObject| jni::objects::JValue::Object(*a))
                    .collect();
                args2.push(1.into());
                let a = env.call_method(
                    context,
                    jni::jni_str!("startActivityForResult"),
                    jni::jni_sig!("(Landroid/content/Intent;I)V"),
                    args2.as_slice(),
                );
                log::error!("Results of bluetooth enable is {:?}", a);
                Ok::<(), jni::errors::Error>(())
            }).unwrap();
        }
    }

    /// Returns the enabled state of the bluetooth adapter
    pub fn is_enabled(&mut self) -> bool {
        self.check_adapter();
        let mut java = jni_min_helper::jni_get_vm();
        java.attach_current_thread(|env| {
            env.call_method(&self.adapter, jni::jni_str!("isEnabled"), jni::jni_sig!("()Z"), &[])?.into_bool()
        }).unwrap()
    }

    /// Get the list of bonded devices for the bluetooth adapter
    pub fn get_bonded_devices(&self) -> Result<Vec<BluetoothDevice>, std::io::Error> {
        let mut java = jni_min_helper::jni_get_vm();
        java.attach_current_thread(|env| {
            let dev_set = env
                .call_method(&self.adapter, jni::jni_str!("getBondedDevices"), jni::jni_sig!("()Ljava/util/Set;"), &[])?
                .l()?;
            let jarr = env
                .call_method(&dev_set, jni::jni_str!("toArray"), jni::jni_sig!("()[Ljava/lang/Object;"), &[])?
                .l()?;
            let temp = env.new_local_ref(&jarr)?;
            let jarr = env.cast_local::<jni::objects::JObjectArray>(temp)?;
            let len = env.get_array_length(&jarr)? as usize;
            let mut vec = Vec::with_capacity(len);
            for i in 0..len {
                let temp = env.get_object_array_element(&jarr, i)?;
                vec.push(BluetoothDevice::new(
                    env.new_global_ref(temp)?,
                ));
            }
            Ok(vec)
        }).map_err(jerr)
    }

    fn get_adapter<'a>(
        env: &mut jni::Env,
        context: &jni::objects::JObject,
    ) -> Result<::jni::refs::Global<JObject<'static>>, jni::errors::Error> {
        let t = env.new_string(BLUETOOTH_SERVICE)?;
        let bluetooth_service = env.new_local_ref(t)?;
        let manager = env
            .call_method(
                context,
                jni::jni_str!("getSystemService"),
                jni::jni_sig!("(Ljava/lang/String;)Ljava/lang/Object;"),
                &[(&bluetooth_service).into()],
            )?
            .l()?;
        let adapter = env
            .call_method(
                manager,
                jni::jni_str!("getAdapter"),
                jni::jni_sig!("()Landroid/bluetooth/BluetoothAdapter;"),
                &[],
            )?
            .l()?;
        Ok(env.new_global_ref(&adapter)?)
    }
}

fn register_receiver(
    arg1: &jni_min_helper::BroadcastReceiver,
    intent_str: &str,
) -> Option<::jni::refs::Global<JObject<'static>>> {
    let mut java = jni_min_helper::jni_get_vm();
    java.attach_current_thread(|env| {
        let context = env.new_local_ref(JAVA_CONTEXT.get().unwrap().as_obj())?;

        let mut args = Vec::new();
        let intent_str = env.new_string(intent_str)?;
        let arg2 = env.new_object(
            jni::jni_str!("android/content/IntentFilter"),
            jni::jni_sig!("(Ljava/lang/String;)V"),
            &[(&intent_str).into()],
        );
        let arg2 = arg2.unwrap();
        args.push(arg1.as_ref());
        args.push(&arg2);
        let args2: Vec<jni::objects::JValue> =
            args.iter().map(|a| a.try_into().unwrap()).collect();
        let e = env
            .call_method(context, jni::jni_str!("registerReceiver"), 
            jni::jni_sig!("(Landroid/content/BroadcastReceiver;Landroid/content/IntentFilter;)Landroid/content/Intent;"), 
            args2.as_slice())?
            .l()?;
        env.new_global_ref(&e)
    }).ok()
}

/// initialize java for android
pub fn init_java(app: &AndroidApp) {
    let mut java = jni_min_helper::jni_get_vm();
    java.attach_current_thread(|env| {
        let context = unsafe {
            jni::objects::JObject::from_raw(env, app.activity_as_ptr() as *mut jni::sys::_jobject)
        };
        JAVA_CONTEXT
            .set(Arc::new(env.new_global_ref(context)?))
            .unwrap();
        Ok::<_, jni::errors::Error>(())
    });
}
