use crate::{
    input::{DeviceInfo, DeviceType},
    instance::Instance,
};

use evdev::{Device, EventType, InputEvent, UinputAbsSetup, uinput::VirtualDevice};

use std::{
    io::ErrorKind,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

pub const VIRTUAL_GAMEPAD_NAME_PREFIX: &str = "PartyDeck Internal Virtual Gamepad";

pub fn is_partydeck_virtual_device(device: &Device) -> bool {
    device
        .name()
        .is_some_and(|name| name.starts_with(VIRTUAL_GAMEPAD_NAME_PREFIX))
}

struct VirtualGamepad {
    path: String,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for VirtualGamepad {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);

        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

pub struct VirtualGamepadSession {
    instance_gamepads: Vec<Vec<VirtualGamepad>>,
}

impl VirtualGamepadSession {
    pub fn start_if_enabled(
        enabled: bool,
        input_devices: &[DeviceInfo],
        instances: &[Instance],
    ) -> Result<Option<Self>, Box<dyn std::error::Error>> {
        if !enabled {
            return Ok(None);
        }

        let mut instance_gamepads = Vec::with_capacity(instances.len());
        let mut has_gamepads = false;

        for (instance_index, instance) in instances.iter().enumerate() {
            let mut gamepads = Vec::new();

            for &device_index in &instance.devices {
                let device_info = input_devices.get(device_index).ok_or_else(|| {
                    format!(
                        "Instance {} references invalid input device index {}",
                        instance_index + 1,
                        device_index
                    )
                })?;

                if device_info.device_type != DeviceType::Gamepad {
                    continue;
                }

                has_gamepads = true;

                let virtual_gamepad =
                    create_virtual_gamepad(&device_info.path, instance_index, gamepads.len())?;

                println!(
                    "[partydeck] Created virtual gamepad for instance {}: {}",
                    instance_index + 1,
                    virtual_gamepad.path
                );

                gamepads.push(virtual_gamepad);
            }

            instance_gamepads.push(gamepads);
        }

        if !has_gamepads {
            println!(
                "[partydeck] Virtual gamepad mode is enabled, \
                 but no gamepads are assigned"
            );

            return Ok(None);
        }

        Ok(Some(Self { instance_gamepads }))
    }

    pub fn gamepads_for_instance(&self, instance_index: usize) -> impl Iterator<Item = &str> {
        self.instance_gamepads
            .get(instance_index)
            .into_iter()
            .flatten()
            .map(|gamepad| gamepad.path.as_str())
    }
}

fn create_virtual_gamepad(
    physical_path: &str,
    instance_index: usize,
    gamepad_index: usize,
) -> Result<VirtualGamepad, Box<dyn std::error::Error>> {
    let physical = Device::open(physical_path)?;

    let name = format!(
        "{} {}-{}",
        VIRTUAL_GAMEPAD_NAME_PREFIX,
        instance_index + 1,
        gamepad_index + 1
    );

    let mut builder = VirtualDevice::builder()?
        .name(&name)
        .input_id(physical.input_id());

    if let Some(keys) = physical.supported_keys() {
        builder = builder.with_keys(keys)?;
    }

    for (axis, abs_info) in physical.get_absinfo()? {
        let setup = UinputAbsSetup::new(axis, abs_info);
        builder = builder.with_absolute_axis(&setup)?;
    }

    let mut virtual_device = builder.build()?;
    let virtual_path = find_event_path(&mut virtual_device)?;

    let physical_path = physical_path.to_owned();
    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = Arc::clone(&stop);

    let thread = thread::spawn(move || {
        relay_loop(physical_path, physical, virtual_device, thread_stop);
    });

    Ok(VirtualGamepad {
        path: virtual_path,
        stop,
        thread: Some(thread),
    })
}

fn relay_loop(
    physical_path: String,
    physical: Device,
    mut virtual_device: VirtualDevice,
    stop: Arc<AtomicBool>,
) {
    if let Err(error) = physical.set_nonblocking(true) {
        eprintln!(
            "[partydeck] Failed to set gamepad {} to non-blocking mode: {}",
            physical_path, error
        );
        return;
    }

    println!(
        "[partydeck] Started virtual gamepad relay for {}",
        physical_path
    );

    let mut physical_device = Some(physical);

    while !stop.load(Ordering::Relaxed) {
        if physical_device.is_none() {
            match Device::open(&physical_path) {
                Ok(device) => {
                    if let Err(error) = device.set_nonblocking(true) {
                        eprintln!(
                            "[partydeck] Failed to reopen gamepad {}: {}",
                            physical_path, error
                        );

                        thread::sleep(Duration::from_millis(500));
                        continue;
                    }

                    println!(
                        "[partydeck] Physical gamepad reconnected: {}",
                        physical_path
                    );

                    physical_device = Some(device);
                }

                Err(_) => {
                    thread::sleep(Duration::from_millis(500));
                    continue;
                }
            }
        }

        let fetch_result = {
            let Some(device) = physical_device.as_mut() else {
                continue;
            };

            device.fetch_events().map(|events| {
                events
                    .filter(|event| event.event_type() != EventType::SYNCHRONIZATION)
                    .collect::<Vec<InputEvent>>()
            })
        };

        match fetch_result {
            Ok(events) => {
                if events.is_empty() {
                    thread::sleep(Duration::from_millis(2));
                    continue;
                }

                if let Err(error) = virtual_device.emit(&events) {
                    eprintln!(
                        "[partydeck] Failed to emit virtual gamepad events: {}",
                        error
                    );
                }
            }

            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(2));
            }

            Err(error) => {
                eprintln!(
                    "[partydeck] Physical gamepad disconnected or failed ({}): {}",
                    physical_path, error
                );

                physical_device = None;
                thread::sleep(Duration::from_millis(500));
            }
        }
    }

    println!(
        "[partydeck] Stopped virtual gamepad relay for {}",
        physical_path
    );
}

fn find_event_path(device: &mut VirtualDevice) -> Result<String, Box<dyn std::error::Error>> {
    let paths: Vec<PathBuf> = device
        .enumerate_dev_nodes_blocking()?
        .collect::<Result<Vec<_>, _>>()?;

    let path = paths
        .into_iter()
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("event"))
        })
        .ok_or("Virtual gamepad event node was not created")?;

    Ok(path.to_string_lossy().into_owned())
}
