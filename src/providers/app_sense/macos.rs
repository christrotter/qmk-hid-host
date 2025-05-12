use objc2::runtime::{self, AnyObject, Sel};
use objc2::{class, msg_send, sel, ClassType};
use objc2_foundation::NSString;
use std::ptr;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

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
}

// Global state for callback
static mut ACTIVE_APP_PTR: Option<*const Arc<Mutex<String>>> = None;

impl AppSenseProvider {
    pub fn new() -> Box<dyn Provider> {
        let provider = AppSenseProvider {
            active_app: Arc::new(Mutex::new(String::new())),
            running: Arc::new(Mutex::new(false)),
            thread_handle: Arc::new(Mutex::new(None)),
        };
        return Box::new(provider);
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
        }

        *self.thread_handle.lock().unwrap() = Some(thread::spawn(move || {
            unsafe {
                let pool: *mut AnyObject = msg_send![class!(NSAutoreleasePool), new];

                let workspace: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
                let nc: *mut AnyObject = msg_send![workspace, notificationCenter];

                // Create observer class
                let superclass = class!(NSObject);
                let mut class_decl = runtime::ClassBuilder::new("NotificationObserver", superclass).unwrap();

                extern "C" fn handle_notification(_this: *mut AnyObject, _sel: Sel, notification: *mut AnyObject) {
                    unsafe {
                        let user_info: *mut AnyObject = msg_send![notification, userInfo];
                        let key = NSString::from_str("NSWorkspaceApplicationKey");
                        let app: *mut AnyObject = msg_send![user_info, objectForKey:&*key];

                        if !app.is_null() {
                            let name: *mut AnyObject = msg_send![app, localizedName];
                            let bundle_id: *mut AnyObject = msg_send![app, bundleIdentifier];

                            if !name.is_null() && !bundle_id.is_null() {
                                let name_str: &NSString = unsafe { &*(name as *const NSString) };
                                let app_name = name_str.to_string();

                                let bundle_str: &NSString = unsafe { &*(bundle_id as *const NSString) };
                                let bundle = bundle_str.to_string();

                                // if let Some(ptr) = ACTIVE_APP_PTR {
                                //     let active_app = &**ptr;
                                //     let mut app_info = active_app.lock().unwrap();
                                //     *app_info = format!("{} ({})", app_name, bundle);
                                // }
                                tracing::info!("Application changed to: {:?}", app_name);
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
            }
        }
    }
}

impl Drop for AppSenseProvider {
    fn drop(&mut self) {
        self.stop();
    }
}
