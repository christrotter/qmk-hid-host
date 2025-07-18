use objc2::runtime::{self, AnyObject, Sel};
use objc2::{class, msg_send, sel, ClassType};
use objc2_foundation::NSString;
use std::ptr;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tokio::sync::broadcast;

use crate::providers::_base::Provider;

/*
prompt:
Use the entire project as reference.  We need to create an AppSense provider with a distinct long-running thread that registers with the notification center, to listen for didActivateApplicationNotification events and store the event's NSRunningNotification as a string.
We need to create a long-running thread that registers with the notification center, to listen for didActivateApplicationNotification events and store the event's NSRunningNotification as a string.
tried w. gpt4.1 this time...
*/
pub struct AppSenseProvider {
    active_app: Arc<Mutex<String>>,
    running: Arc<Mutex<bool>>,
    thread_handle: Arc<Mutex<Option<thread::JoinHandle<()>>>>,
    host_to_device_sender: broadcast::Sender<Vec<u8>>,
}

// Global state for callback
static mut ACTIVE_APP_PTR: Option<*const Arc<Mutex<String>>> = None;
static mut ACTIVE_APP_PROVIDER_PTR: Option<*const AppSenseProvider> = None;

impl AppSenseProvider {
    pub fn new(host_to_device_sender: broadcast::Sender<Vec<u8>>) -> Box<dyn Provider> {
        let provider = AppSenseProvider {
            active_app: Arc::new(Mutex::new(String::new())),
            running: Arc::new(Mutex::new(false)),
            thread_handle: Arc::new(Mutex::new(None)),
            host_to_device_sender,
        };
        return Box::new(provider);
    }
    fn create_app_command(app_name: &str) -> Option<Vec<u8>> {
        // Format: [DataType, 0xCE (command type), app_code, 0x00, 0x00...]
        // 186,206 = 0xBACE, our pid for the app sense provider
        match app_name {
            "Code" => Some(vec![
                186, 206, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            ]),
            "Google Chrome" => Some(vec![
                186, 206, 1, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            ]),
            "Fusion" => Some(vec![
                186, 206, 1, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            ]),
            "KiCad" => Some(vec![
                186, 206, 1, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            ]),
            "Other" => Some(vec![
                186, 206, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            ]),
            _ => None,
        }
    }
}

impl Provider for AppSenseProvider {
    fn start(&self) {
        let active_app = self.active_app.clone();
        let running = self.running.clone();

        {
            let mut is_running = running.lock().unwrap();
            *is_running = true;
        }

        // Set global pointer
        unsafe {
            ACTIVE_APP_PTR = Some(&active_app as *const _);
            ACTIVE_APP_PROVIDER_PTR = Some(self as *const _);
        }

        *self.thread_handle.lock().unwrap() = Some(thread::spawn(move || {
            unsafe {
                // Create autorelease pool
                let pool: *mut AnyObject = msg_send![class!(NSAutoreleasePool), new];
                if pool.is_null() {
                    tracing::error!("Failed to create NSAutoreleasePool");
                    return;
                }

                let workspace: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
                if workspace.is_null() {
                    tracing::error!("Failed to get sharedWorkspace");
                    return;
                }

                let nc: *mut AnyObject = msg_send![workspace, notificationCenter];
                if nc.is_null() {
                    tracing::error!("Failed to get notificationCenter");
                    return;
                }

                // Create observer class - use a unique name with timestamp
                let superclass = class!(NSObject);
                let mut attempts = 0;
                let mut class_decl = None;

                while attempts < 5 && class_decl.is_none() {
                    let timestamp = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_nanos();
                    let class_name = format!("NotificationObserver_{}_{}", timestamp, attempts);

                    if runtime::Class::get(class_name.as_str()).is_none() {
                        class_decl = runtime::ClassBuilder::new(&class_name, superclass);
                    }

                    attempts += 1;
                    if class_decl.is_none() && attempts < 5 {
                        // Small delay before retry
                        thread::sleep(Duration::from_millis(1));
                    }
                }

                if let Some(mut class_decl) = class_decl {
                    extern "C" fn handle_notification(_this: *mut AnyObject, _sel: Sel, notification: *mut AnyObject) {
                        unsafe {
                            let user_info: *mut AnyObject = msg_send![notification, userInfo];
                            let value: *mut AnyObject =
                                msg_send![user_info, objectForKey:&*NSString::from_str("NSWorkspaceApplicationKey")];
                            if value.is_null() {
                                tracing::error!("Method call returned null");
                                return;
                            }

                            let app: *mut AnyObject = value;

                            if !app.is_null() {
                                let name: *mut AnyObject = msg_send![app, localizedName];
                                // let bundle_id: *mut AnyObject = msg_send![app, bundleIdentifier];

                                if !name.is_null() {
                                    let name_str: &NSString = unsafe { &*(name as *const NSString) };
                                    let app_name = name_str.to_string();
                                    tracing::info!("Application changed to: {:?}", app_name);

                                    // Handle different applications with match
                                    match app_name.as_str() {
                                        "Code" => {
                                            tracing::info!("VS Code detected.");
                                            // Send command using the stored sender
                                            if let Some(command) = AppSenseProvider::create_app_command("Code") {
                                                let _ = unsafe {
                                                    if let Some(ptr) = ACTIVE_APP_PROVIDER_PTR {
                                                        let provider = &*ptr;
                                                        provider.host_to_device_sender.send(command)
                                                    } else {
                                                        Err(broadcast::error::SendError(vec![]))
                                                    }
                                                };
                                            }
                                        }
                                        "Fusion" => {
                                            tracing::info!("Fusion detected.");
                                            // Send command using the stored sender
                                            if let Some(command) = AppSenseProvider::create_app_command("Fusion") {
                                                let _ = unsafe {
                                                    if let Some(ptr) = ACTIVE_APP_PROVIDER_PTR {
                                                        let provider = &*ptr;
                                                        provider.host_to_device_sender.send(command)
                                                    } else {
                                                        Err(broadcast::error::SendError(vec![]))
                                                    }
                                                };
                                            }
                                        }
                                        "Google Chrome" => {
                                            tracing::info!("Chrome detected.");
                                            if let Some(command) = AppSenseProvider::create_app_command("Google Chrome") {
                                                let _ = unsafe {
                                                    if let Some(ptr) = ACTIVE_APP_PROVIDER_PTR {
                                                        let provider = &*ptr;
                                                        provider.host_to_device_sender.send(command)
                                                    } else {
                                                        Err(broadcast::error::SendError(vec![]))
                                                    }
                                                };
                                            }
                                        }
                                        "KiCad" => {
                                            tracing::info!("KiCad detected.");
                                            if let Some(command) = AppSenseProvider::create_app_command("KiCad") {
                                                let _ = unsafe {
                                                    if let Some(ptr) = ACTIVE_APP_PROVIDER_PTR {
                                                        let provider = &*ptr;
                                                        provider.host_to_device_sender.send(command)
                                                    } else {
                                                        Err(broadcast::error::SendError(vec![]))
                                                    }
                                                };
                                            }
                                        }
                                        // Similar patterns for other apps
                                        _ => {
                                            tracing::info!("Other app: {}", app_name);
                                            if let Some(command) = AppSenseProvider::create_app_command("Other") {
                                                let _ = unsafe {
                                                    if let Some(ptr) = ACTIVE_APP_PROVIDER_PTR {
                                                        let provider = &*ptr;
                                                        provider.host_to_device_sender.send(command)
                                                    } else {
                                                        Err(broadcast::error::SendError(vec![]))
                                                    }
                                                };
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    class_decl.add_method(
                        sel!(handleNotification:),
                        handle_notification as extern "C" fn(*mut AnyObject, Sel, *mut AnyObject),
                    );

                    let observer_class = class_decl.register();

                    let observer: *mut AnyObject = msg_send![observer_class, new];

                    // Define the notification name
                    let notification_name = NSString::from_str("NSWorkspaceDidActivateApplicationNotification");

                    // Register for notifications
                    let _: () = msg_send![nc,
                        addObserver:observer
                        selector:sel!(handleNotification:)
                        name:&*notification_name
                        object:ptr::null_mut::<AnyObject>()
                    ];

                    // Keep thread running
                    while *running.lock().unwrap() {
                        // Sleep to prevent high CPU usage
                        thread::sleep(Duration::from_millis(100));
                    }

                    // Clean up before thread exits
                    let notification_name = NSString::from_str("NSWorkspaceDidActivateApplicationNotification");
                    let _: () = msg_send![nc,
                        removeObserver:observer
                        name:&*notification_name
                        object:ptr::null_mut::<AnyObject>()
                    ];

                    let _: () = msg_send![observer, release];
                    let _: () = msg_send![pool, drain];
                } else {
                    tracing::error!("Failed to create a unique observer class after {} attempts", attempts);
                    return;
                }
            }
        }));
    }

    fn stop(&self) {
        if let Some(handle) = self.thread_handle.lock().unwrap().take() {
            {
                let mut is_running = self.running.lock().unwrap();
                *is_running = false;
            }
            let _ = handle.join();

            unsafe {
                ACTIVE_APP_PTR = None;
                ACTIVE_APP_PROVIDER_PTR = None;
            }
        }
    }
}

impl Drop for AppSenseProvider {
    fn drop(&mut self) {
        self.stop();
    }
}
