# bluetooth-rust

[![Crates.io](https://img.shields.io/crates/v/bluetooth-rust)](https://crates.io/crates/bluetooth-rust)
[![License](https://img.shields.io/crates/l/bluetooth-rust)](https://github.com/uglyoldbob/bluetooth-rust)

A cross-platform Bluetooth communication library for Rust. `bluetooth-rust` provides a unified API for discovering adapters, scanning for devices, managing pairing, and establishing RFCOMM and L2CAP connections across Linux, Windows, and Android.

## Platform Support

| Platform | Backend | Async API | Sync API |
|----------|---------|-----------|----------|
| Linux    | BlueZ (`bluer`) | ✅ | ❌ |
| Windows  | Windows API | ✅ | ✅ |
| Android  | JNI (Android SDK) | ❌ | ✅ |

## Features

- **Adapter discovery** — enumerate Bluetooth adapters on the host system
- **Device discovery** — scan for nearby Bluetooth devices
- **Paired device listing** — retrieve bonded/paired devices
- **RFCOMM profiles** — register and accept RFCOMM connections
- **L2CAP profiles** — register and accept L2CAP connections
- **Passkey / pairing** — display and confirm passkeys during the pairing process
- **Discoverability** — make the local adapter discoverable
- **Well-known UUIDs** — built-in enum of standard Bluetooth service UUIDs

## Installation

Add the crate to your `Cargo.toml`:

```toml
[dependencies]
bluetooth-rust = "0.3"
tokio = { version = "1", features = ["full"] }
```

### Linux Prerequisites

On Linux the library relies on [BlueZ](http://www.bluez.org/) via the [`bluer`](https://crates.io/crates/bluer) crate. Make sure BlueZ is installed and the `bluetoothd` daemon is running:

```sh
sudo apt install bluez          # Debian / Ubuntu
sudo systemctl start bluetooth  # start the daemon
```

### Windows Prerequisites

No additional runtime setup is required. The library uses the built-in Windows Bluetooth APIs via the [`windows`](https://crates.io/crates/windows) crate.

### Android Prerequisites

The library uses JNI to call into the Android Bluetooth SDK. You must integrate it with an Android activity that provides an `AndroidApp` handle (via [`winit`](https://crates.io/crates/winit) with the `android-native-activity` feature) and grant the appropriate Bluetooth permissions in your `AndroidManifest.xml`.

## Quick Start

### Building an Adapter (Linux / Windows)

```rust
use bluetooth_rust::{BluetoothAdapterBuilder, MessageToBluetoothHost};
use tokio::sync::mpsc;

#[tokio::main]
async fn main() {
    // Channel for receiving passkey / pairing messages from the stack
    let (tx, mut rx) = mpsc::channel::<MessageToBluetoothHost>(8);

    let adapter = BluetoothAdapterBuilder::new()
        .with_sender(tx)  // required so the library can forward pairing events
        .async_build()
        .await
        .expect("Failed to build Bluetooth adapter");

    println!("Adapter built successfully");
}
```

### Building an Adapter (Android)

```rust
use bluetooth_rust::BluetoothAdapterBuilder;
use winit::platform::android::activity::AndroidApp;

fn start_bluetooth(app: AndroidApp) {
    let mut builder = BluetoothAdapterBuilder::new();
    builder.with_android_app(app);
    let adapter = builder.build().expect("Failed to build Bluetooth adapter");
}
```

### Listing Paired Devices

```rust
use bluetooth_rust::AsyncBluetoothAdapterTrait;

// `adapter` is a BluetoothAdapter obtained from BluetoothAdapterBuilder
let devices = adapter.get_paired_devices();
for device in devices {
    println!("Device: {:?}", device.get_name());
    println!("Address: {:?}", device.get_address());
    println!("UUIDs: {:?}", device.get_uuids());
}
```

### Registering an RFCOMM Profile

```rust
use bluetooth_rust::{BluetoothRfcommProfileSettings, AsyncBluetoothAdapterTrait};

let settings = BluetoothRfcommProfileSettings {
    uuid: "00001101-0000-1000-8000-00805F9B34FB".to_string(), // SPP
    name: Some("My Serial Port".to_string()),
    service_uuid: None,
    channel: Some(1),
    psm: None,
    authenticate: Some(false),
    authorize: Some(false),
    auto_connect: Some(false),
    sdp_record: None,
    sdp_version: None,
    sdp_features: None,
};

let profile = adapter
    .register_rfcomm_profile(settings)
    .await
    .expect("Failed to register RFCOMM profile");
```

### Handling Passkey / Pairing Events

```rust
use bluetooth_rust::{MessageToBluetoothHost, ResponseToPasskey};
use tokio::sync::mpsc;

let (tx, mut rx) = mpsc::channel::<MessageToBluetoothHost>(8);

tokio::spawn(async move {
    while let Some(msg) = rx.recv().await {
        match msg {
            MessageToBluetoothHost::DisplayPasskey(passkey, reply_tx) => {
                println!("Pairing passkey: {:06}", passkey);
                // Automatically confirm — replace with real UI logic
                let _ = reply_tx.send(ResponseToPasskey::Yes).await;
            }
            MessageToBluetoothHost::ConfirmPasskey(passkey, reply_tx) => {
                println!("Confirm passkey: {:06}?", passkey);
                let _ = reply_tx.send(ResponseToPasskey::Yes).await;
            }
            MessageToBluetoothHost::CancelDisplayPasskey => {
                println!("Pairing canceled");
            }
        }
    }
});
```

## Well-Known UUIDs

The `BluetoothUuid` enum covers a wide range of standard Bluetooth profiles:

| Variant | UUID | Profile |
|---------|------|---------|
| `SPP` | `00001101-...` | Serial Port Profile |
| `A2dpSource` | `0000110a-...` | A2DP Source |
| `A2dpSink` | `0000110b-...` | A2DP Sink |
| `HspHs` | `00001108-...` | Headset (HS) |
| `HspAg` | `00001112-...` | Headset Audio Gateway |
| `HfpAg` | `0000111f-...` | Hands-Free Audio Gateway |
| `HfpHs` | `0000111e-...` | Hands-Free Headset |
| `ObexOpp` | `00001105-...` | OBEX Object Push |
| `ObexFtp` | `00001106-...` | OBEX File Transfer |
| `ObexMas` | `00001132-...` | OBEX Message Access |
| `ObexMns` | `00001133-...` | OBEX Message Notification |
| `ObexPse` | `0000112f-...` | OBEX Phone Book Access |
| `ObexSync` | `00001104-...` | OBEX Sync |
| `AvrcpRemote` | `0000110e-...` | AVRCP Remote Control |
| `NetworkingNap` | `00001116-...` | Bluetooth NAP |
| `AndroidAuto` | `4de17a00-...` | Android Auto |
| `Base` | `00000000-...` | Bluetooth Base |
| `Unknown(String)` | _any_ | Unrecognized UUID |

UUIDs can be parsed from strings with `str::parse::<BluetoothUuid>()` and converted back with `.as_str()`.

## Key Types and Traits

| Type / Trait | Description |
|---|---|
| `BluetoothAdapterBuilder` | Builder for constructing a `BluetoothAdapter` |
| `BluetoothAdapter` | Platform-dispatched adapter (Linux/Windows/Android) |
| `AsyncBluetoothAdapterTrait` | Async adapter operations (Linux, Windows) |
| `SyncBluetoothAdapterTrait` | Sync adapter operations (Android) |
| `BluetoothDevice` | A discovered or paired remote device |
| `BluetoothDeviceTrait` | Query device name, address, UUIDs, sockets, and pair state |
| `BluetoothStream` | Active async or sync communication stream |
| `BluetoothRfcommProfileSettings` | Configuration for an RFCOMM profile |
| `BluetoothL2capProfileSettings` | Configuration for an L2CAP profile |
| `BluetoothUuid` | Well-known Bluetooth service UUIDs |
| `PairingStatus` | `NotPaired` / `Pairing` / `Paired` / `Unknown` |
| `MessageToBluetoothHost` | Pairing events forwarded to the application |
| `ResponseToPasskey` | Application's response to a pairing challenge |

## License

Licensed under either of:

- [MIT License](LICENSE-MIT)
- [Apache License, Version 2.0](LICENSE-APACHE)

at your option.

## Acknowledgements

Android portions adapted from [android-bluetooth-serial-rs](https://github.com/wuwbobo2021/android-bluetooth-serial-rs).