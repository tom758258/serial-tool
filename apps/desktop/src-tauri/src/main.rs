mod runtime;
#[cfg(windows)]
mod webview2;

use std::{path::Path, sync::Arc, time::Duration};

use runtime::{Event, Mode, RunResult, SessionManager};
use serde::{Deserialize, Serialize};
use serial_tool_core::{
    DataBits, FlowControl, Parity, PortType, Sequence, SerialSettings, StopBits, list_ports,
};
use tauri::{State, ipc::Channel};

#[derive(Deserialize)]
struct SettingsDto {
    port: String,
    baud_rate: u32,
    data_bits: u8,
    parity: String,
    stop_bits: u8,
    flow_control: String,
    timeout_ms: u64,
}

impl TryFrom<SettingsDto> for SerialSettings {
    type Error = String;

    fn try_from(value: SettingsDto) -> Result<Self, Self::Error> {
        let settings = Self {
            port: value.port,
            baud_rate: value.baud_rate,
            data_bits: match value.data_bits {
                5 => DataBits::Five,
                6 => DataBits::Six,
                7 => DataBits::Seven,
                8 => DataBits::Eight,
                _ => return Err("Data bits must be 5, 6, 7, or 8".into()),
            },
            parity: match value.parity.as_str() {
                "none" => Parity::None,
                "odd" => Parity::Odd,
                "even" => Parity::Even,
                _ => return Err("Invalid parity".into()),
            },
            stop_bits: match value.stop_bits {
                1 => StopBits::One,
                2 => StopBits::Two,
                _ => return Err("Stop bits must be 1 or 2".into()),
            },
            flow_control: match value.flow_control.as_str() {
                "none" => FlowControl::None,
                "software" => FlowControl::Software,
                "hardware" => FlowControl::Hardware,
                _ => return Err("Invalid flow control".into()),
            },
            timeout: Duration::from_millis(value.timeout_ms),
        };
        settings.validate().map_err(|error| error.to_string())?;
        Ok(settings)
    }
}

#[derive(Serialize)]
struct PortDto {
    port_name: String,
    kind: &'static str,
    vid: Option<u16>,
    pid: Option<u16>,
    serial_number: Option<String>,
    manufacturer: Option<String>,
    product: Option<String>,
}

#[tauri::command]
fn list_serial_ports() -> Result<Vec<PortDto>, String> {
    list_ports()
        .map_err(|error| error.to_string())
        .map(|ports| {
            ports
                .into_iter()
                .map(|port| match port.port_type {
                    PortType::Usb(usb) => PortDto {
                        port_name: port.port_name,
                        kind: "usb",
                        vid: Some(usb.vid),
                        pid: Some(usb.pid),
                        serial_number: usb.serial_number,
                        manufacturer: usb.manufacturer,
                        product: usb.product,
                    },
                    other => PortDto {
                        port_name: port.port_name,
                        kind: match other {
                            PortType::Pci => "pci",
                            PortType::Bluetooth => "bluetooth",
                            _ => "unknown",
                        },
                        vid: None,
                        pid: None,
                        serial_number: None,
                        manufacturer: None,
                        product: None,
                    },
                })
                .collect()
        })
}

#[tauri::command]
async fn connect_serial(
    manager: State<'_, Arc<SessionManager>>,
    mode: String,
    settings: SettingsDto,
    events: Channel<Event>,
) -> Result<(), String> {
    let mode = match mode.as_str() {
        "live" => Mode::Live,
        "simulation" => Mode::Simulation,
        _ => return Err("Invalid connection mode".into()),
    };
    let mut settings = settings;
    if mode == Mode::Simulation {
        settings.port = "simulation".into();
    }
    let manager = Arc::clone(&manager);
    let settings = settings.try_into()?;
    tauri::async_runtime::spawn_blocking(move || {
        manager.connect(mode, settings, move |event| {
            let _ = events.send(event);
        })
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn disconnect_serial(manager: State<'_, Arc<SessionManager>>) -> Result<(), String> {
    let manager = Arc::clone(&manager);
    tauri::async_runtime::spawn_blocking(move || manager.disconnect())
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn send_serial(
    manager: State<'_, Arc<SessionManager>>,
    input: String,
    format: String,
) -> Result<(), String> {
    let bytes = match format.as_str() {
        "text" => input.into_bytes(),
        "hex" => parse_hex(&input)?,
        _ => return Err("Invalid input format".into()),
    };
    if bytes.is_empty() {
        return Err("Send data must not be empty".into());
    }
    let manager = Arc::clone(&manager);
    tauri::async_runtime::spawn_blocking(move || manager.send(bytes))
        .await
        .map_err(|error| error.to_string())?
}

fn parse_hex(input: &str) -> Result<Vec<u8>, String> {
    let digits: Vec<_> = input
        .bytes()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect();
    if digits.is_empty() || !digits.len().is_multiple_of(2) {
        return Err("Hex input must contain a nonempty, even number of digits".into());
    }
    digits
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            match (
                (pair[0] as char).to_digit(16),
                (pair[1] as char).to_digit(16),
            ) {
                (Some(high), Some(low)) => Ok(((high << 4) | low) as u8),
                _ => Err("Hex input contains invalid characters".into()),
            }
        })
        .collect()
}

#[tauri::command]
fn load_sequence(path: String) -> Result<String, String> {
    Sequence::load(Path::new(&path))
        .and_then(|sequence| sequence.to_json_pretty())
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn save_sequence(path: String, json: String) -> Result<(), String> {
    Sequence::from_json_str(&json)
        .and_then(|sequence| sequence.save(Path::new(&path)))
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn validate_sequence(json: String) -> Result<(), String> {
    Sequence::from_json_str(&json)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn run_sequence(
    manager: State<'_, Arc<SessionManager>>,
    json: String,
) -> Result<RunResult, String> {
    let sequence = Sequence::from_json_str(&json).map_err(|error| error.to_string())?;
    let manager = Arc::clone(&manager);
    tauri::async_runtime::spawn_blocking(move || manager.run(sequence))
        .await
        .map_err(|error| error.to_string())?
}

fn main() {
    #[cfg(windows)]
    if !webview2::preflight() {
        return;
    }

    if let Err(error) = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(Arc::new(SessionManager::default()))
        .invoke_handler(tauri::generate_handler![
            list_serial_ports,
            connect_serial,
            disconnect_serial,
            send_serial,
            load_sequence,
            save_sequence,
            validate_sequence,
            run_sequence
        ])
        .run(tauri::generate_context!())
    {
        #[cfg(windows)]
        webview2::show_startup_error(&error.to_string());
        #[cfg(not(windows))]
        panic!("failed to run Serial Tool desktop: {error}");
    }
}
