use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::Arc;

use hidapi::{DeviceInfo, HidApi, HidDevice};
use tokio::sync::{broadcast, mpsc};
use tracing_subscriber::layer;

use crate::config::Device;
use crate::data_type::DataType;
use reqwest::blocking::Client;
use reqwest::Url;
use serde_json::json;
use std::time::Duration;

pub struct Keyboard {
    name: String,
    product_id: u16,
    usage: u16,
    usage_page: u16,
    reconnect_delay: u64,
    is_connected: Arc<AtomicBool>,
}

impl Keyboard {
    pub fn new(device: &Device, reconnect_delay: u64) -> Self {
        return Self {
            name: device.name.clone().unwrap_or("keyboard".to_string()),
            product_id: device.product_id,
            usage: device.usage.unwrap_or(0x61),
            usage_page: device.usage_page.unwrap_or(0xff60),
            reconnect_delay,
            is_connected: Arc::new(AtomicBool::new(false)),
        };
    }

    fn get_device_info(hid_api: &HidApi, product_id: &u16, usage: &u16, usage_page: &u16) -> Option<DeviceInfo> {
        let devices = hid_api.device_list();
        tracing::info!("product id to search for: {}", *product_id);
        for device_info in devices {
            // tracing::info!("{}: device", device_info.product_id().to_string());
            if device_info.product_id() == *product_id && device_info.usage() == *usage && device_info.usage_page() == *usage_page {
                tracing::info!("{}: found device", device_info.product_id().to_string());
                return Some(device_info.clone());
            } else {
                tracing::info!("{}: not found device", device_info.product_id().to_string());
            }
        }

        None
    }

    pub fn connect(
        &self,
        host_to_device_sender: broadcast::Sender<Vec<u8>>,
        device_to_host_sender: broadcast::Sender<Vec<u8>>,
        is_connected_sender: mpsc::Sender<bool>,
    ) {
        let name = self.name.clone();
        let pid = self.product_id;
        let usage = self.usage;
        let usage_page = self.usage_page;
        let reconnect_delay = self.reconnect_delay;
        let is_connected = self.is_connected.clone();

        std::thread::spawn(move || {
            tracing::debug!("Waiting for {}...", name);
            loop {
                tracing::debug!("{}: trying to connect...", name);

                let hid_api = HidApi::new().unwrap();
                if let Some(device_info) = Self::get_device_info(&hid_api, &pid, &usage, &usage_page) {
                    let reconnect_timeout = 1000;
                    loop {
                        match device_info.open_device(&hid_api) {
                            Ok(device) => {
                                start_write(&name, device, &is_connected, &host_to_device_sender);
                                break;
                            }
                            Err(err) => tracing::error!("{}", err),
                        }
                        std::thread::sleep(std::time::Duration::from_millis(reconnect_timeout));
                    }
                    loop {
                        match device_info.open_device(&hid_api) {
                            Ok(device) => {
                                start_read(&name, device, &is_connected, &device_to_host_sender);
                                break;
                            }
                            Err(err) => tracing::error!("{}", err),
                        }

                        std::thread::sleep(std::time::Duration::from_millis(reconnect_timeout));
                    }
                    tracing::info!("{}: read", name);
                    tracing::info!("{}: connected", name);
                    is_connected.store(true, Relaxed);
                    let _ = is_connected_sender.try_send(true);

                    loop {
                        if !is_connected.load(Relaxed) {
                            tracing::warn!("{}: disconnected", name);
                            let _ = is_connected_sender.try_send(false);
                            break;
                        }

                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(reconnect_delay));
            }
        });
    }
}

fn handle_received_data(name: &String, received_data: &[u8]) {
    tracing::info!("{}: handling received data {:?}", name, received_data);

    // Parse the received data to determine the type of API call
    if received_data.is_empty() {
        tracing::warn!("{}: received empty data, skipping API call", name);
        return;
    }
    // so this is going to be where we set our type of API call - but, it's currently returning a padded 0...
    match received_data[2] {
        6 => {
            tracing::info!("{}: making API call type 1", name);
            // Example API call type 1
            // make_api_call_type_1(name, received_data);
        }
        0x02 => {
            tracing::info!("{}: making API call type 2", name);
            // thing
        }
        _ => {
            tracing::warn!("{}: unknown data type, skipping API call", name);
            tracing::info!("{}: received data content: {:?}", name, received_data);
        }
    }
}

const API_ENDPOINT: &str = "http://192.168.86.43/json/state";

fn make_api_call_type_1(name: &String, data: &[u8; 32]) {
    tracing::info!("{}: API call type 1 with data {:?}", name, data);
    // Add logic for API call type 1
    let layer = data[2];
    tracing::info!("Changing to layer: {}", layer);
    // Create a blocking HTTP client
    let client = Client::new();

    // Format the data for the API call
    let payload = json!({
        "ps": layer,
    });

    // Make the API call
    match client.post(API_ENDPOINT).timeout(Duration::from_secs(5)).json(&payload).send() {
        Ok(response) => {
            if response.status().is_success() {
                tracing::info!("{}: API call succeeded: {:?}", name, response.text().unwrap_or_default());
            } else {
                tracing::error!("{}: API call failed with status: {}", name, response.status());
            }
        }
        Err(e) => {
            tracing::error!("{}: API call error: {}", name, e);
        }
    }
}

fn start_write(name: &String, device: HidDevice, is_connected: &Arc<AtomicBool>, host_to_device_sender: &broadcast::Sender<Vec<u8>>) {
    let name = name.clone();
    let is_connected = is_connected.clone();
    // tracing::info!("is_connected: {:?}", is_connected.load(Relaxed));
    let mut host_to_device_receiver = host_to_device_sender.subscribe();
    // tracing::info!("{}: starting write thread", name);
    std::thread::spawn(move || loop {
        tracing::debug!("{}: waiting for data to send...", name);
        std::thread::sleep(std::time::Duration::from_millis(10));
        if let Ok(mut received) = host_to_device_receiver.try_recv() {
            // tracing::debug!("{}: sending {:?}", name, received);
            received.truncate(33);
            received.resize_with(33, Default::default);
            // received.insert(0, 0);
            received.pop();
            // tracing::info!("write: device: {:?}", device.get_product_string());
            // tracing::info!("write: data:   {:?}", <Vec<u8> as AsMut<[u8]>>::as_mut(&mut received));
            match device.write(received.as_mut()) {
                Ok(bytes_written) => {
                    tracing::debug!("{}: successfully wrote {} bytes", name, bytes_written);
                }
                Err(err) => {
                    tracing::error!("{}: failed to write to device: {}", name, err);
                    is_connected.store(false, Relaxed);
                    break;
                }
            }
        }
    });
}

fn start_read(name: &String, device: HidDevice, is_connected: &Arc<AtomicBool>, device_to_host_sender: &broadcast::Sender<Vec<u8>>) {
    let name = name.clone();
    let is_connected = is_connected.clone();
    let device_to_host_sender = device_to_host_sender.clone();
    let mut data = [0u8; 32];
    std::thread::spawn(move || loop {
        tracing::debug!("{}: waiting for data from keyboard...", name);
        std::thread::sleep(std::time::Duration::from_millis(10));
        if let Ok(result) = device.read(data.as_mut()) {
            tracing::debug!("{}: received {:?}", name, data);
            if result > 0 {
                if data[1] == 206 {
                    tracing::info!("{}: CE found {:?}", name, data);
                    make_api_call_type_1(&name, &data);
                    // make api call here
                    // break;
                }
                // tracing::info!("reading from: {:?}", device.get_product_string());
                // tracing::info!("read data: {} {}", data[0], data[1]);
                // handle_received_data(&name, &received);
                let _ = device_to_host_sender.send(data.to_vec());
            }
        } else {
            is_connected.store(false, Relaxed);
            break;
        }
    });
}
