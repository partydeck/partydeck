use crate::app::PadFilterType;

use evdev::*;
use evdev::uinput::VirtualDevice;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Clone, PartialEq, Copy)]
pub enum DeviceType {
    Gamepad,
    Keyboard,
    Mouse,
    Other,
}

pub enum PadButton {
    Left,
    Right,
    Up,
    Down,
    ABtn,
    BBtn,
    XBtn,
    YBtn,
    StartBtn,
    SelectBtn,

    AKey,
    RKey,
    XKey,
    ZKey,

    RightClick,
}

#[derive(Clone)]
pub struct DeviceInfo {
    pub path: String,
    pub _enabled: bool,
    pub device_type: DeviceType,
}

pub struct InputDevice {
    path: String,
    dev: Device,
    enabled: bool,
    device_type: DeviceType,
    has_button_held: bool,
}
impl InputDevice {
    pub fn name(&self) -> &str {
        self.dev.name().unwrap_or_else(|| "")
    }
    pub fn emoji(&self) -> &str {
        match self.device_type() {
            DeviceType::Gamepad => "🎮",
            DeviceType::Keyboard => "🖮",
            DeviceType::Mouse => "🖱",
            DeviceType::Other => "",
        }
    }
    pub fn fancyname(&self) -> &str {
        match self.dev.input_id().vendor() {
            0x045e => "Xbox Controller",
            0x054c => "PS Controller",
            0x057e => "NT Pro Controller",
            0x28de => "Steam Input",
            _ => self.name(),
        }
    }
    pub fn path(&self) -> &str {
        &self.path
    }
    pub fn enabled(&self) -> bool {
        self.enabled
    }
    pub fn device_type(&self) -> DeviceType {
        self.device_type
    }
    pub fn has_button_held(&self) -> bool {
        self.has_button_held
    }
    pub fn info(&self) -> DeviceInfo {
        DeviceInfo {
            path: self.path().to_string(),
            _enabled: self.enabled(),
            device_type: self.device_type(),
        }
    }
    pub fn poll(&mut self) -> Option<PadButton> {
        let mut btn: Option<PadButton> = None;
        if let Ok(events) = self.dev.fetch_events() {
            for event in events {
                let summary = event.destructure();

                match summary {
                    EventSummary::Key(_, _, 1) => {
                        self.has_button_held = true;
                    }
                    EventSummary::Key(_, _, 0) => {
                        self.has_button_held = false;
                    }
                    _ => {}
                }

                btn = match summary {
                    EventSummary::Key(_, KeyCode::BTN_SOUTH, 1) => Some(PadButton::ABtn),
                    EventSummary::Key(_, KeyCode::BTN_EAST, 1) => Some(PadButton::BBtn),
                    EventSummary::Key(_, KeyCode::BTN_NORTH, 1) => Some(PadButton::XBtn),
                    EventSummary::Key(_, KeyCode::BTN_WEST, 1) => Some(PadButton::YBtn),
                    EventSummary::Key(_, KeyCode::BTN_START, 1) => Some(PadButton::StartBtn),
                    EventSummary::Key(_, KeyCode::BTN_SELECT, 1) => Some(PadButton::SelectBtn),
                    EventSummary::AbsoluteAxis(_, AbsoluteAxisCode::ABS_HAT0X, -1) => {
                        Some(PadButton::Left)
                    }
                    EventSummary::AbsoluteAxis(_, AbsoluteAxisCode::ABS_HAT0X, 1) => {
                        Some(PadButton::Right)
                    }
                    EventSummary::AbsoluteAxis(_, AbsoluteAxisCode::ABS_HAT0Y, -1) => {
                        Some(PadButton::Up)
                    }
                    EventSummary::AbsoluteAxis(_, AbsoluteAxisCode::ABS_HAT0Y, 1) => {
                        Some(PadButton::Down)
                    }
                    EventSummary::Key(_, KeyCode::KEY_A, 1) => Some(PadButton::AKey),
                    EventSummary::Key(_, KeyCode::KEY_R, 1) => Some(PadButton::RKey),
                    EventSummary::Key(_, KeyCode::KEY_X, 1) => Some(PadButton::XKey),
                    EventSummary::Key(_, KeyCode::KEY_Z, 1) => Some(PadButton::ZKey),
                    EventSummary::Key(_, KeyCode::BTN_RIGHT, 1) => Some(PadButton::RightClick),
                    _ => btn,
                };
            }
        }
        btn
    }
}

pub fn scan_input_devices(filter: &PadFilterType) -> Vec<InputDevice> {
    let mut pads: Vec<InputDevice> = Vec::new();
    for dev in evdev::enumerate() {
        if let Some(phys) = dev.1.physical_path() {
            if phys.is_empty() { continue; }
        } else { continue; }

        let enabled = match filter {
            PadFilterType::All => true,
            PadFilterType::NoSteamInput => dev.1.input_id().vendor() != 0x28de,
            PadFilterType::OnlySteamInput => dev.1.input_id().vendor() == 0x28de,
        };

        let device_type = if dev.1.supported_keys().map_or(false, |keys| keys.contains(KeyCode::BTN_SOUTH)) {
            DeviceType::Gamepad
        } else if dev.1.supported_keys().map_or(false, |keys| keys.contains(KeyCode::BTN_LEFT)) {
            DeviceType::Mouse
        } else if dev.1.supported_keys().map_or(false, |keys| keys.contains(KeyCode::KEY_SPACE)) {
            DeviceType::Keyboard
        } else {
            DeviceType::Other
        };

        if device_type != DeviceType::Other {
            let _ = dev.1.set_nonblocking(true);
            pads.push(InputDevice {
                path: dev.0.to_str().unwrap().to_string(),
                dev: dev.1,
                enabled,
                device_type,
                has_button_held: false,
            });
        }
    }
    pads.sort_by_key(|pad| pad.path().to_string());
    pads
}

pub struct RouterSlot {
    pub virtual_device: uinput::VirtualDevice,
    pub physical_path: Option<String>,
    pub last_log_time: Instant,
}

pub struct InputRouter {
    pub slots: Arc<Mutex<Vec<RouterSlot>>>,
    pub stop_signal: Arc<AtomicBool>,
}

impl Clone for InputRouter {
    fn clone(&self) -> Self {
        Self {
            slots: self.slots.clone(),
            stop_signal: self.stop_signal.clone(),
        }
    }
}

impl InputRouter {
    pub fn new() -> Self {
        Self {
            slots: Arc::new(Mutex::new(Vec::new())),
            stop_signal: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn start_routing(&self) {
        let slots = self.slots.clone();
        let stop_signal = self.stop_signal.clone();

        thread::spawn(move || {
            let mut physical_devices: std::collections::HashMap<String, Device> = std::collections::HashMap::new();
            let mut last_open_attempt: std::collections::HashMap<String, Instant> = std::collections::HashMap::new();

            println!("[partydeck-debug] ROUTING THREAD START");

            while !stop_signal.load(Ordering::Relaxed) {
                let mut slots_lock = slots.lock().unwrap();
                
                // 1. Ensure all physical devices are opened and grabbed
                for slot in slots_lock.iter() {
                    if let Some(path) = &slot.physical_path {
                        if !physical_devices.contains_key(path) {
                            let now = Instant::now();
                            let last = last_open_attempt.get(path).cloned();
                            if last.is_none() || now.duration_since(last.unwrap()) >= Duration::from_secs(1) {
                                last_open_attempt.insert(path.clone(), now);
                                if let Ok(mut dev) = Device::open(path) {
                                    let _ = dev.set_nonblocking(true);
                                    let _ = dev.grab();
                                    println!("[partydeck-debug] Successfully OPENED and GRABBED physical: {}", path);
                                    physical_devices.insert(path.clone(), dev);
                                }
                            }
                        }
                    }
                }

                // 2. Fetch events once per unique physical device
                let mut events_by_path: std::collections::HashMap<String, Vec<InputEvent>> = std::collections::HashMap::new();
                let mut dead_paths = Vec::new();
                for (path, dev) in physical_devices.iter_mut() {
                    match dev.fetch_events() {
                        Ok(events) => {
                            let ev_vec: Vec<_> = events.collect();
                            if !ev_vec.is_empty() {
                                events_by_path.insert(path.clone(), ev_vec);
                            }
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                        Err(e) => {
                            eprintln!("[partydeck-debug] Physical device error (likely disconnected): {} - {}", path, e);
                            dead_paths.push(path.clone());
                        }
                    }
                }

                for path in dead_paths {
                    physical_devices.remove(&path);
                }

                // 3. Distribute events to all slots that use that device
                for (idx, slot) in slots_lock.iter_mut().enumerate() {
                    if let Some(path) = &slot.physical_path {
                        if let Some(events) = events_by_path.get(path) {
                            let now = Instant::now();
                            let should_log = now.duration_since(slot.last_log_time) >= Duration::from_secs(2);
                            if should_log {
                                slot.last_log_time = now;
                                println!("[partydeck-input-log] Slot {}: Received Event Batch", idx);
                            }

                            let mut batch = Vec::new();
                            for event in events {
                                batch.push(*event);
                                if event.event_type() == EventType::SYNCHRONIZATION && event.code() == 0 {
                                    if let Err(e) = slot.virtual_device.emit(&batch) {
                                        eprintln!("[partydeck-debug] EMIT ERROR on slot {}: {}", idx, e);
                                    }
                                    batch.clear();
                                }
                            }
                            if !batch.is_empty() {
                                let _ = slot.virtual_device.emit(&batch);
                            }
                        }
                    }
                }
                drop(slots_lock);
                thread::sleep(Duration::from_millis(4));
            }
            println!("[partydeck-debug] ROUTING THREAD STOPPED");
        });
    }

    pub fn add_slot(&self, phys_path: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
        let phys_dev = Device::open(phys_path)?;
        let mut builder = VirtualDevice::builder()?;

        builder = builder.name(phys_dev.name().unwrap_or("Virtual Gamepad"));
        builder = builder.input_id(phys_dev.input_id());

        if let Some(keys) = phys_dev.supported_keys() {
            builder = builder.with_keys(keys)?;
        }
        if let Some(rel) = phys_dev.supported_relative_axes() {
            builder = builder.with_relative_axes(rel)?;
        }
        if let Ok(abs_infos) = phys_dev.get_absinfo() {
            for (code, info) in abs_infos {
                builder = builder.with_absolute_axis(&UinputAbsSetup::new(code, info))?;
            }
        }
        if let Some(sw) = phys_dev.supported_switches() {
            builder = builder.with_switches(sw)?;
        }

        let mut vdev = builder.build()?;

        // Wait for udev and joydev to stabilize permissions and legacy nodes
        std::thread::sleep(std::time::Duration::from_millis(1000));

        let mut nodes = Vec::new();
        for node_res in vdev.enumerate_dev_nodes_blocking()? {
            if let Ok(node) = node_res {
                let path_str = node.to_string_lossy().to_string();
                nodes.push(path_str.clone());

                // INJECT LEGACY JS NODES
                if let Some(event_name) = std::path::Path::new(&path_str).file_name().and_then(|n| n.to_str()) {
                    let sysfs_dir = format!("/sys/class/input/{}/device", event_name);
                    if let Ok(entries) = std::fs::read_dir(sysfs_dir) {
                        for entry in entries.flatten() {
                            if let Ok(name) = entry.file_name().into_string() {
                                if name.starts_with("js") {
                                    let js_path = format!("/dev/input/{}", name);
                                    nodes.push(js_path.clone());
                                    println!("[partydeck-debug] Bound legacy node: {}", js_path);
                                }
                            }
                        }
                    }
                }
            }
        }

        self.slots.lock().unwrap().push(RouterSlot {
            virtual_device: vdev,
            physical_path: Some(phys_path.to_string()),
            last_log_time: Instant::now(),
        });

        Ok(nodes)
    }
}
