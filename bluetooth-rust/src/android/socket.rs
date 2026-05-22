//! bluetooth socket code on android

use super::jerr;
use jni::objects::Global;
use jni::objects::JObject;
use jni_min_helper::*;
use std::{
    collections::VecDeque,
    marker::PhantomData,
    sync::{Arc, Mutex},
    thread::JoinHandle,
    time::{Duration, SystemTime},
};

/// Manages the Bluetooth socket and IO streams. It uses a read buffer and a background thread,
/// because the timeout of the Java `InputStream` from the `BluetoothSocket` cannot be set.
/// The read timeout defaults to 0 (it does not block).
///
/// Reference:
/// <https://developer.android.com/develop/connectivity/bluetooth/transfer-data>
pub struct BluetoothSocket {
    internal: jni::objects::Global<JObject<'static>>,

    input_stream: jni::objects::Global<JObject<'static>>,
    buf_read: Arc<Mutex<VecDeque<u8>>>,
    thread_read: Option<JoinHandle<Result<(), std::io::Error>>>, // the returned value is unused
    read_callback: Arc<Mutex<Option<super::ReadCallback>>>,      // None by default
    read_timeout: Duration,                                      // set for the standard Read trait
    output_stream: jni::objects::Global<JObject<'static>>,
    array_write: jni::objects::Global<JObject<'static>>,
    uuid: String,
    _marker: PhantomData<*const ()>,
}

impl std::fmt::Debug for BluetoothSocket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BluetoothSocket")
    }
}

#[cfg_attr(feature = "async", async_trait::async_trait)]
impl BluetoothSocket {
    fn is_connected(&self) -> Result<bool, std::io::Error> {
        let java = jni_min_helper::jni_get_vm();
        java.attach_current_thread(|env| self.is_connected2(env))
            .map_err(|e| jerr(e))
    }

    /// connect to the socket
    #[cfg(feature = "async")]
    pub async fn async_connect(&mut self) -> Result<(), std::io::Error> {
        Err(std::io::Error::from(std::io::ErrorKind::NotFound))
    }

    /// connect to the socket
    pub fn sync_connect(&mut self) -> Result<(), std::io::Error> {
        if self.is_connected()? {
            return Ok(());
        }
        let java = jni_min_helper::jni_get_vm();
        let a = java.attach_current_thread(|env| {
            log::warn!("Connecting to {}", self.uuid);
            let connected = {
                env.call_method(
                    &self.internal,
                    jni::jni_str!("connect"),
                    jni::jni_sig!("()V"),
                    &[],
                )
                .inspect_err(|e| log::error!("Connect error is {:?}", e))?;
                self.is_connected2(env)
            }?;
            log::warn!("Connected status is {}", connected);
            if connected {
                let socket = env.new_global_ref(self.internal.as_obj()).unwrap();
                let input_stream = env.new_global_ref(self.input_stream.as_obj()).unwrap();
                let arc_buf_read = self.buf_read.clone();
                let arc_callback = self.read_callback.clone();
                self.thread_read.replace(std::thread::spawn(move || {
                    BluetoothSocket::read_loop(
                        socket,
                        input_stream,
                        arc_buf_read,
                        arc_callback,
                    )
                }));
                log::warn!("Done connecting");
                Ok(true)
            } else {
                Ok(false)
            }
        });
        match a {
            Ok(true) => Ok(()),
            Ok(false) => Err(std::io::Error::new(std::io::ErrorKind::NotConnected, "")),
            Err(e) => Err(jerr(e)),
        }
    }
}

impl BluetoothSocket {
    const ARRAY_SIZE: usize = 32 * 1024;

    /// build a new socket
    pub fn build(
        obj: jni::objects::Global<JObject<'static>>,
        uuid: &str,
    ) -> Result<Self, std::io::Error> {
        let java = jni_min_helper::jni_get_vm();
        java.attach_current_thread(|env| {
            let input_stream = env
                .call_method(
                    &obj,
                    jni::jni_str!("getInputStream"),
                    jni::jni_sig!("()Ljava/io/InputStream;"),
                    &[],
                )?
                .into_object()?;
            let input_stream = env.new_global_ref(input_stream)?;
            let output_stream = env
                .call_method(
                    &obj,
                    jni::jni_str!("getOutputStream"),
                    jni::jni_sig!("()Ljava/io/OutputStream;"),
                    &[],
                )?
                .into_object()?;
            let output_stream = env.new_global_ref(output_stream)?;

            let array_size = Self::ARRAY_SIZE as usize;
            let temp = env.new_byte_array(array_size)?.into();
            let array_write =
                env.new_global_ref::<JObject>(temp)?;
            Ok(Self {
                internal: obj,

                input_stream,
                buf_read: Arc::new(Mutex::new(VecDeque::new())),
                thread_read: None,
                read_callback: Arc::new(Mutex::new(None)),
                read_timeout: Duration::from_millis(1000),

                output_stream,
                array_write,
                uuid: uuid.to_string(),
                _marker: PhantomData,
            })
        })
        .map_err(jerr)
    }

    /// Gets the connection status of this socket.
    #[inline(always)]
    fn is_connected2(&self, env: &mut jni::Env) -> Result<bool, jni::errors::Error> {
        env.call_method(
            &self.internal,
            jni::jni_str!("isConnected"),
            jni::jni_sig!("()Z"),
            &[],
        )?
        .into_bool()
    }

    fn read_loop(
        socket: jni::objects::Global<JObject<'static>>,
        input_stream: jni::objects::Global<JObject<'static>>,
        buf_read: Arc<Mutex<VecDeque<u8>>>,
        read_callback: Arc<Mutex<Option<super::ReadCallback>>>,
    ) -> Result<(), std::io::Error> {
        let java = jni_min_helper::jni_get_vm();
        java.attach_current_thread(|env| {
            let jmethod_read = env.get_method_id(
                jni::jni_str!("java/io/InputStream"),
                jni::jni_str!("read"),
                jni::jni_sig!("([BII)I"),
            )?;
            let read_size = env
                .call_method(
                    &socket,
                    jni::jni_str!("getMaxReceivePacketSize"),
                    jni::jni_sig!("()I"),
                    &[],
                )?
                .into_int()
                .map(|i| {
                    if i > 0 {
                        let sz = i as usize;
                        (Self::ARRAY_SIZE / sz) * sz
                    } else {
                        Self::ARRAY_SIZE
                    }
                })
                .unwrap_or(Self::ARRAY_SIZE);

            let mut vec_read = vec![0u8; read_size];
            let array_read = env.new_byte_array(read_size)?;

            loop {
                let array_read2 = env.new_local_ref(&array_read)?;
                // Safety: arguments passed to `call_method_unchecked` are correct.
                let read_len = unsafe {
                    env.call_method_unchecked(
                        &input_stream,
                        jmethod_read,
                        jni::signature::ReturnType::Primitive(jni::signature::Primitive::Int),
                        &[
                            jni::sys::jvalue {
                                l: array_read2.into_raw(),
                            },
                            jni::sys::jvalue {
                                i: 0 as jni::sys::jint,
                            },
                            jni::sys::jvalue {
                                i: read_size as jni::sys::jint,
                            },
                        ],
                    )
                }?
                .into_int();
                let array_read2 = env.new_local_ref(&array_read)?;
                if let Ok(len) = read_len {
                    use std::io::Write;
                    let len = if len > 0 {
                        len as usize
                    } else {
                        continue;
                    };
                    // Safety: casts `&mut [u8]` to `&mut [i8]` for `get_byte_array_region`,
                    // `input_stream.read(..)` = `len` <= `read_size` = `vec_read.len()`.
                    let tmp_read = unsafe {
                        std::slice::from_raw_parts_mut(vec_read.as_mut_ptr() as *mut i8, len)
                    };
                    env.get_byte_array_region(array_read2, 0, tmp_read)?;
                    buf_read
                        .lock()
                        .unwrap()
                        .write_all(&vec_read[..len])
                        .unwrap();
                    Self::read_callback(&read_callback, Some(len));
                } else {
                    /*TODO NOT SURE HOW TO DO THIS IN jni 0.22.4
                    if let Some(ex) = jni_last_cleared_ex() {
                        let ex_msg = ex.get_throwable_msg(env).unwrap().to_lowercase();
                        if ex_msg.contains("closed") {
                            // Note: will it change in future Android versions?
                            let _ = env
                                .call_method(&socket, "close", "()V", &[]);
                            Self::read_callback(&read_callback, None);
                            return Ok(());
                        }
                    }
                    */
                    let is_connected = env
                        .call_method(
                            &socket,
                            jni::jni_str!("isConnected"),
                            jni::jni_sig!("()Z"),
                            &[],
                        )?
                        .into_bool()?;
                    if !is_connected {
                        Self::read_callback(&read_callback, None);
                        return Ok(());
                    }
                }
            }
        })
        .map_err(jerr)
    }

    fn read_callback(cb: impl AsRef<Mutex<Option<super::ReadCallback>>>, val: Option<usize>) {
        let mut lck = cb.as_ref().lock().unwrap();
        if let Some(callback) = lck.take() {
            drop(lck);
            callback(val);
            let mut lck = cb.as_ref().lock().unwrap();
            if lck.is_none() {
                lck.replace(callback);
            }
        }
    }

    /// Closes this socket and releases any system resources associated with it.
    /// If the stream is already closed then invoking this method has no effect.
    pub fn close(&mut self) -> Result<(), std::io::Error> {
        use std::io::Write;
        if !self.is_connected()? {
            return Ok(());
        }
        let _ = self.flush();
        let java = jni_min_helper::jni_get_vm();
        java.attach_current_thread(|env| {
            env.call_method(
                &self.internal,
                jni::jni_str!("close"),
                jni::jni_sig!("()V"),
                &[],
            )?;
            if let Some(th) = self.thread_read.take() {
                let _ = th.join();
            }
            Ok(())
        })
        .map_err(jerr)
    }
}

impl std::io::Read for BluetoothSocket {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }

        let t_timeout = SystemTime::now() + self.read_timeout;

        let mut cnt_read = 0;
        let mut disconnected = false;
        while cnt_read < buf.len() {
            let mut lck_buf_read = self.buf_read.lock().unwrap();
            if let Ok(cnt) = lck_buf_read.read(&mut buf[cnt_read..]) {
                cnt_read += cnt;
            }
            drop(lck_buf_read);
            if cnt_read >= buf.len() {
                break;
            } else if !self.is_connected()? {
                disconnected = true;
                break;
            } else if let Ok(dur_rem) = t_timeout.duration_since(SystemTime::now()) {
                std::thread::sleep(Duration::from_millis(100).min(dur_rem));
            } else {
                break;
            }
        }

        if cnt_read > 0 {
            Ok(cnt_read)
        } else if !disconnected {
            Err(std::io::Error::from(std::io::ErrorKind::TimedOut))
        } else {
            Err(std::io::Error::from(std::io::ErrorKind::NotConnected))
        }
    }
}

impl std::io::Write for BluetoothSocket {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }

        let java = jni_min_helper::jni_get_vm();
        java.attach_current_thread(|env| {
            let ab = env.new_global_ref(self.array_write.as_obj())?;
            let temp = env.new_local_ref(ab.as_obj())?;
            let array_write: jni::objects::JByteArray<'_> =
                env.cast_local::<jni::objects::JByteArray>(temp)?;
            let al = env.get_array_length(&array_write)? as usize;
            if al < buf.len() {
                // replace the prepared reusable Java array with a larger array
                let temp = env.byte_array_from_slice(buf)?.into();
                self.array_write =
                    env.new_global_ref::<JObject>(temp)?;
            } else {
                // Safety: casts `&[u8]` to `&[i8]` for `set_byte_array_region`.
                let buf =
                    unsafe { std::slice::from_raw_parts(buf.as_ptr() as *const i8, buf.len()) };
                env.set_byte_array_region(array_write, 0, buf)?;
            }
            // Safety: arguments passed to `call_method_unchecked` are correct.
            let jmethod_write = env.get_method_id(
                jni::jni_str!("java/io/OutputStream"),
                jni::jni_str!("write"),
                jni::jni_sig!("([BII)V"),
            )?;
            unsafe {
                env.call_method_unchecked(
                    &self.output_stream,
                    jmethod_write,
                    jni::signature::ReturnType::Primitive(jni::signature::Primitive::Void),
                    &[
                        jni::sys::jvalue {
                            l: self.array_write.as_raw(),
                        },
                        jni::sys::jvalue {
                            i: 0 as jni::sys::jint,
                        },
                        jni::sys::jvalue {
                            i: buf.len() as jni::sys::jint,
                        },
                    ],
                )
            }
            .map(|_| buf.len())
        })
        .map_err(|e| {
            if !self.is_connected().unwrap_or(false) {
                std::io::Error::from(std::io::ErrorKind::NotConnected)
            } else {
                jerr(e)
            }
        })
    }

    #[inline]
    fn flush(&mut self) -> std::io::Result<()> {
        let java = jni_min_helper::jni_get_vm();
        java.attach_current_thread(|env| {
            let jmethod_flush = env.get_method_id(
                jni::jni_str!("java/io/OutputStream"),
                jni::jni_str!("flush"),
                jni::jni_sig!("()V"),
            )?;
            unsafe {
                let a = env.call_method_unchecked(
                    &self.output_stream,
                    jmethod_flush,
                    jni::signature::ReturnType::Primitive(jni::signature::Primitive::Void),
                    &[],
                )?;
                Ok(())
            }
        })
        .map(|_| ())
        .map_err(jerr)
    }
}

impl Drop for BluetoothSocket {
    fn drop(&mut self) {
        let _ = self.close();
    }
}
