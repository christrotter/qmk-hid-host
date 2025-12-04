use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::Arc;

use hidapi::{DeviceInfo, HidApi, HidDevice};
use tokio::sync::{broadcast, mpsc};

use crate::config::Device;
use reqwest::blocking::Client;
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
            name: device
                .name
                .as_ref()
                .map(|s| s.to_string())
                .unwrap_or_else(|| String::from("Unknown Device")),
            product_id: device.product_id,
            usage: device.usage.unwrap_or(0x61),
            usage_page: device.usage_page.unwrap_or(0xff60),
            reconnect_delay,
            is_connected: Arc::new(AtomicBool::new(false)),
        };
    }

    fn get_device_info(hid_api: &HidApi, product_id: &u16, usage: &u16, usage_page: &u16) -> Option<DeviceInfo> {
        let devices = hid_api.device_list();
        tracing::debug!("Searching for product id: {}", *product_id);
        for device_info in devices {
            // tracing::info!("{}: device", device_info.product_id().to_string());
            if device_info.product_id() == *product_id && device_info.usage() == *usage && device_info.usage_page() == *usage_page {
                tracing::debug!("{}: found device", device_info.product_id().to_string());
                return Some(device_info.clone());
            }
        }
        None
    }

    pub fn connect(
        &self,
        host_to_device_sender: broadcast::Sender<Vec<u8>>,
        device_to_host_sender: broadcast::Sender<Vec<u8>>,
        is_connected_sender: mpsc::Sender<bool>,
    ) -> std::thread::JoinHandle<()> {
        // Clone variables for use in the thread
        let name = self.name.clone();
        let pid = self.product_id;
        let usage = self.usage;
        let usage_page = self.usage_page;
        let reconnect_delay = self.reconnect_delay;
        let is_connected = self.is_connected.clone();

        std::thread::spawn(move || {
            // Thread handles and termination flags
            tracing::debug!("Starting connect thread.");
            let mut write_thread: Option<std::thread::JoinHandle<()>> = None;
            let mut read_thread: Option<std::thread::JoinHandle<()>> = None;
            let terminate_flag = Arc::new(AtomicBool::new(false));

            tracing::debug!("Waiting for {:?}...", name);
            loop {
                tracing::debug!("{:?}: trying to connect...", name);
                /*
                    The whole thread termination, read/write thread join, this is all copilot.
                    I'm uncertain if it's actually working...
                    yeah it's never doing anything with this...
                */
                // Set terminate flag to true to stop any existing threads
                terminate_flag.store(true, Relaxed);

                // Join previous threads if they exist
                if let Some(thread) = write_thread.take() {
                    let _ = thread.join();
                    tracing::debug!("{:?}: previous write thread joined", name);
                }

                if let Some(thread) = read_thread.take() {
                    let _ = thread.join();
                    tracing::debug!("{:?}: previous read thread joined", name);
                }

                // Reset terminate flag for new threads
                terminate_flag.store(false, Relaxed);

                // Continue with device connection logic...
                let hid_api = HidApi::new().unwrap();
                if let Some(device_info) = Self::get_device_info(&hid_api, &pid, &usage, &usage_page) {
                    let reconnect_timeout = 1000;
                    loop {
                        match device_info.open_device(&hid_api) {
                            Ok(device) => {
                                write_thread = Some(start_write(
                                    &name,
                                    device,
                                    &is_connected,
                                    &host_to_device_sender,
                                    terminate_flag.clone(),
                                ));
                                break;
                            }
                            Err(err) => tracing::error!("{}", err),
                        }
                        std::thread::sleep(std::time::Duration::from_millis(reconnect_timeout));
                    }
                    loop {
                        match device_info.open_device(&hid_api) {
                            Ok(device) => {
                                read_thread = Some(start_read(
                                    &name,
                                    device,
                                    &is_connected,
                                    &device_to_host_sender,
                                    terminate_flag.clone(),
                                ));
                                break;
                            }
                            Err(err) => tracing::error!("{}", err),
                        }

                        std::thread::sleep(std::time::Duration::from_millis(reconnect_timeout));
                    }
                    tracing::info!("{}: connected", name);
                    is_connected.store(true, Relaxed);
                    let _ = is_connected_sender.try_send(true);

                    loop {
                        if !is_connected.load(Relaxed) {
                            tracing::warn!("{}: disconnected", name);
                            let _ = is_connected_sender.try_send(false);
                            terminate_flag.store(true, Relaxed); // Set immediately
                            break;
                        }

                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(reconnect_delay));
                // i think the connect thread is closing ok
            }
        })
    }
}

const API_ENDPOINT: &str = "http://192.168.86.43/json/state"; // this needs to be config

fn make_wled_api_call(name: &String, data: &[u8; 32]) {
    tracing::info!("{:?}: API call type 1 with data {:?}", name, data);
    // only the keyboard should be sending this data
    let layer = data[3] + 1; // this is the important part; also have to increment by 1 b/c wled layers do not start at 0
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
                tracing::debug!("{}: API call succeeded: {:?}", name, response.text().unwrap_or_default());
            } else {
                tracing::error!("{}: API call failed with status: {}", name, response.status());
            }
        }
        Err(e) => {
            tracing::error!("{}: API call error: {}", name, e);
        }
    }
}

fn start_write(
    name: &String,
    device: HidDevice,
    is_connected: &Arc<AtomicBool>,
    host_to_device_sender: &broadcast::Sender<Vec<u8>>,
    terminate_flag: Arc<AtomicBool>,
) -> std::thread::JoinHandle<()> {
    let name = name.clone();
    let is_connected = is_connected.clone();
    let mut host_to_device_receiver = host_to_device_sender.subscribe();

    std::thread::spawn(move || {
        // if the terminate_flag is true, thread exits
        while !terminate_flag.load(Relaxed) {
            tracing::debug!("{:?}: waiting for data to send...", name);
            std::thread::sleep(std::time::Duration::from_millis(10));

            // Use try_recv() to prevent blocking indefinitely
            if let Ok(mut received) = host_to_device_receiver.try_recv() {
                // Process data...
                match device.write(received.as_mut()) {
                    Ok(bytes_written) => {
                        // tracing::info!("{:?}: successfully wrote {} bytes", name, bytes_written);
                        // tracing::info!("{:?}: successfully wrote :{:?}", name, received);
                    }
                    Err(err) => {
                        tracing::error!("{:?}: failed to write to device: {}", name, err);
                        is_connected.store(false, Relaxed);
                        terminate_flag.store(true, Relaxed); // Set immediately
                        break;
                    }
                }
                // every 5 seconds we want to broadcast a heartbeat message of 0x00
            }
        }
        tracing::debug!("{:?}: write thread terminated", name);
    })
}

fn start_read(
    name: &String,
    device: HidDevice,
    is_connected: &Arc<AtomicBool>,
    device_to_host_sender: &broadcast::Sender<Vec<u8>>,
    terminate_flag: Arc<AtomicBool>,
) -> std::thread::JoinHandle<()> {
    let name = name.clone();
    let is_connected = is_connected.clone();
    let device_to_host_sender = device_to_host_sender.clone();
    let mut data = [0u8; 32];

    std::thread::spawn(move || {
        while !terminate_flag.load(Relaxed) {
            tracing::debug!("{:?}: waiting for data from keyboard...", name);
            std::thread::sleep(std::time::Duration::from_millis(10));

            // Set a timeout for device.read or use a non-blocking approach if available
            match device.read_timeout(data.as_mut(), 100) {
                Ok(result) if result > 0 => {
                    // Process data...
                    if result > 0 {
                        // todo this will have to change to match the new pid paradigm
                        if data[2] == 5 {
                            // tracing::debug!("{:?}: CE found {:?}", name, data);
                            make_wled_api_call(&name, &data);
                        }
                    }
                    let _ = device_to_host_sender.send(data.to_vec());
                }
                Err(err) => {
                    tracing::error!("{:?}: failed to read from device: {}", name, err);
                    is_connected.store(false, Relaxed);
                    terminate_flag.store(true, Relaxed); // Set immediately
                    break;
                }
                _ => {} // No data available
            }
        }
        tracing::debug!("{:?}: read thread terminated", name);
    })
}
