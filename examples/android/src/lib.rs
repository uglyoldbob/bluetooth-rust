//! This is the code for the android app that pairs with the custom electronics and software in an automotive radio.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use eframe::egui;
use eframe::{NativeOptions, Renderer};

use bluetooth_rust::{BluetoothAdapterTrait, Java};

#[cfg(target_os = "android")]
use winit::platform::android::activity::AndroidApp;

#[derive(Default, Debug, serde::Serialize, serde::Deserialize)]
struct AppConfig {
    asdf: bool,
}

#[derive(Debug)]
enum AppConfigError {
    NotLoaded,
    Corrupt,
    UnableToCreate,
}

#[derive(Debug)]
struct BluetoothConfig {
    connect_nap: bool,
}

impl BluetoothConfig {
    fn new() -> Self {
        Self { connect_nap: false }
    }
}

/// The main struct for holding data for the gui of the application
pub struct MainWindow {
    local_storage: Option<std::path::PathBuf>,
    settings: Result<AppConfig, AppConfigError>,
    _java: Arc<Mutex<Java>>,
    bluetooth: bluetooth_rust::BluetoothAdapter,
    known_uuids: BTreeMap<String, Vec<bluetooth_rust::BluetoothUuid>>,
    bluetooth_devs: BTreeMap<String, BluetoothConfig>,
    bluetooth_discovery: Option<bluetooth_rust::BluetoothDiscovery>,
    profile: Option<Result<bluetooth_rust::BluetoothRfcommProfile, String>>,
}

impl MainWindow {
    /// Get the minimum size for ui elements
    pub fn min_size(ui: &egui::Ui) -> egui::Vec2 {
        let m = ui.pixels_per_point();
        egui::vec2(10.0 * m, 10.0 * m)
    }

    /// Get the font size
    pub fn font_size() -> f32 {
        24.0
    }
}

impl eframe::App for MainWindow {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        //self.bluetooth.enable();
        ctx.request_repaint_after(std::time::Duration::from_millis(10));
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.label(
                egui::RichText::new(format!("Size 1: {}", ui.pixels_per_point()))
                    .size(Self::font_size()),
            );
            let min_size = Self::min_size(ui);
            if ui.button("Start discovery").clicked() {
                if let Some(s) = self.bluetooth.supports_sync() {
                    self.bluetooth_discovery = Some(s.start_discovery());
                }
            }
            if self.bluetooth_discovery.is_some() {
                if ui.button("Cancel discovery").clicked() {
                    self.bluetooth_discovery.take();
                }
            }
            if let Some(profile) = &self.profile {
                match profile {
                    Ok(_p) => {
                        ui.label("Got a valid bluetooth profile");
                    }
                    Err(e) => {
                        ui.label(format!("Failed to get a valid profile: {}", e));
                    }
                }
            }
        });
    }
}

impl MainWindow {
    fn load_config(&mut self) {
        if let Some(p) = &self.local_storage {
            let mut config = p.clone();
            config.push("config.bin");
            let settings = if let Ok(false) = std::fs::exists(&config) {
                let settings = AppConfig::default();
                let encoded: Vec<u8> =
                    bincode::serde::encode_to_vec(&settings, bincode::config::standard()).unwrap();
                let f = std::fs::File::create(&config);
                if let Ok(mut f) = f {
                    use std::io::Write;
                    match f.write(&encoded) {
                        Ok(_l) => Ok(settings),
                        Err(e) => {
                            log::error!("Unable to create config file: {:?}", e);
                            Err(AppConfigError::UnableToCreate)
                        }
                    }
                } else {
                    log::error!("Unable to create config file2: {:?}", f);
                    Err(AppConfigError::UnableToCreate)
                }
            } else {
                let f = std::fs::read(&config);
                if let Ok(a) = f {
                    let s = bincode::serde::decode_from_slice(&a, bincode::config::standard());
                    if let Ok((s, _len)) = s {
                        Ok(s)
                    } else {
                        Err(AppConfigError::Corrupt)
                    }
                } else {
                    Err(AppConfigError::Corrupt)
                }
            };
            self.settings = settings;
        }
    }

    fn new(_cc: &eframe::CreationContext<'_>, options: NativeOptions, app: AndroidApp) -> Self {
        let java = Java::make(app.clone());
        let java2 = Arc::new(Mutex::new(java));
        let b = bluetooth_rust::Bluetooth::new(java2.clone());
        let mut s = Self {
            local_storage: options.android_app.unwrap().internal_data_path(),
            settings: Err(AppConfigError::NotLoaded),
            _java: java2,
            bluetooth: bluetooth_rust::BluetoothAdapter::Android(b),
            known_uuids: BTreeMap::new(),
            bluetooth_devs: BTreeMap::new(),
            bluetooth_discovery: None,
            profile: None,
        };
        s.load_config();
        if let Some(st) = s.bluetooth.supports_sync() {
            let profile = st.register_rfcomm_profile(bluetooth_rust::BluetoothRfcommProfileSettings { 
                uuid: "00001812-0000-1000-8000-00805f9b34fb".to_string(), 
                name: Some("NES joystick".to_string()), 
                service_uuid: None, 
                channel: None, 
                psm: None, 
                authenticate: Some(true), 
                authorize: Some(true), 
                auto_connect: Some(true), 
                sdp_record: None, 
                sdp_version: None, 
                sdp_features: None, 
            });
            s.profile = Some(profile);
        }
        s
    }
}

fn _main(mut options: NativeOptions, app: AndroidApp) {
    options.renderer = Renderer::Wgpu;
    let o = options.clone();
    let _run = eframe::run_native(
        "Android Example",
        options,
        Box::new(move |cc| Ok(Box::new(MainWindow::new(cc, o, app)))),
    )
    .unwrap();
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Debug)
            .with_tag("android_example"),
    );
    log::info!("Android example startup");
    let mut options = NativeOptions::default();
    options.viewport.fullscreen = Some(true);
    let app2 = app.clone();
    options.android_app = Some(app);
    _main(options, app2);
}
