use crate::{
    input::{DeviceInfo, DeviceType},
    instance::Instance,
};

use evdev::{uinput::VirtualDevice, Device, EventType, InputEvent, UinputAbsSetup};

use nix::{
    errno::Errno,
    poll::{poll, PollFd, PollFlags, PollTimeout},
};
use std::{
    io::ErrorKind,
    net::Shutdown,
    os::{fd::AsFd, unix::net::UnixStream},
    path::PathBuf,
    thread::{self, JoinHandle},
};

pub const VIRTUAL_GAMEPAD_NAME_PREFIX: &str = "PartyDeck Internal Virtual Gamepad";
const RECONNECT_RETRY_MS: u16 = 500;

pub fn is_partydeck_virtual_device(device: &Device) -> bool {
    device
        .name()
        .is_some_and(|name| name.starts_with(VIRTUAL_GAMEPAD_NAME_PREFIX))
}

struct VirtualGamepad {
    event_path: String,
    stop_signal: UnixStream,
    thread: Option<JoinHandle<()>>,
}

impl Drop for VirtualGamepad {
    fn drop(&mut self) {
        // Wake the relay thread if it is blocked inside poll().
        let _ = self.stop_signal.shutdown(Shutdown::Write);

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
                    virtual_gamepad.event_path
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
            .map(|gamepad| gamepad.event_path.as_str())
    }
}

fn create_virtual_gamepad(
    physical_event_path: &str,
    instance_index: usize,
    gamepad_index: usize,
) -> Result<VirtualGamepad, Box<dyn std::error::Error>> {
    let physical = open_nonblocking_device(physical_event_path)?;

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
    let virtual_event_path = find_event_path(&mut virtual_device)?;

    let physical_event_path = physical_event_path.to_owned();
    let (stop_signal, thread_stop_signal) = UnixStream::pair()?;

    let thread_name = format!(
        "partydeck-gamepad-relay-{}-{}",
        instance_index + 1,
        gamepad_index + 1
    );

    let thread = thread::Builder::new().name(thread_name).spawn(move || {
        relay_loop(
            physical_event_path,
            physical,
            virtual_device,
            thread_stop_signal,
        );
    })?;

    Ok(VirtualGamepad {
        event_path: virtual_event_path,
        stop_signal,
        thread: Some(thread),
    })
}
enum RelayPollResult {
    Stop,
    InputReady,
    Disconnected,
}

fn wait_for_input_or_stop(
    device: &Device,
    stop_signal: &UnixStream,
) -> Result<RelayPollResult, Errno> {
    loop {
        let mut fds = [
            PollFd::new(stop_signal.as_fd(), PollFlags::POLLIN),
            PollFd::new(device.as_fd(), PollFlags::POLLIN),
        ];

        match poll(&mut fds, PollTimeout::NONE) {
            Ok(_) => {}
            Err(Errno::EINTR) => continue,
            Err(error) => return Err(error),
        }

        let stop_events = fds[0].revents().unwrap_or(PollFlags::POLLERR);

        if !stop_events.is_empty() {
            return Ok(RelayPollResult::Stop);
        }

        let device_events = fds[1].revents().unwrap_or(PollFlags::POLLERR);

        if device_events.intersects(PollFlags::POLLERR | PollFlags::POLLHUP | PollFlags::POLLNVAL) {
            return Ok(RelayPollResult::Disconnected);
        }

        if device_events.contains(PollFlags::POLLIN) {
            return Ok(RelayPollResult::InputReady);
        }
    }
}

fn wait_for_reconnect_or_stop(stop_signal: &UnixStream) -> Result<bool, Errno> {
    loop {
        let mut fds = [PollFd::new(stop_signal.as_fd(), PollFlags::POLLIN)];

        match poll(&mut fds, RECONNECT_RETRY_MS) {
            Ok(0) => return Ok(false),
            Ok(_) => return Ok(true),
            Err(Errno::EINTR) => continue,
            Err(error) => return Err(error),
        }
    }
}

fn open_nonblocking_device(path: &str) -> std::io::Result<Device> {
    let device = Device::open(path)?;
    device.set_nonblocking(true)?;
    Ok(device)
}

fn relay_loop(
    physical_event_path: String,
    physical: Device,
    mut virtual_device: VirtualDevice,
    stop_signal: UnixStream,
) {
    println!(
        "[partydeck] Started virtual gamepad relay for {}",
        physical_event_path
    );

    let mut physical_device = Some(physical);
    let mut events = Vec::<InputEvent>::new();

    loop {
        if physical_device.is_none() {
            match wait_for_reconnect_or_stop(&stop_signal) {
                Ok(true) => break,
                Ok(false) => {}
                Err(error) => {
                    eprintln!(
                        "[partydeck] Failed while waiting to reconnect {}: {}",
                        physical_event_path, error
                    );
                    break;
                }
            }

            match open_nonblocking_device(&physical_event_path) {
                Ok(device) => {
                    println!(
                        "[partydeck] Physical gamepad reconnected: {}",
                        physical_event_path
                    );

                    physical_device = Some(device);
                }

                Err(_) => {
                    // Retry after another interruptible wait.
                }
            }

            continue;
        }

        // Block until input, disconnection, or shutdown.
        let poll_result = {
            let Some(device) = physical_device.as_ref() else {
                continue;
            };

            wait_for_input_or_stop(device, &stop_signal)
        };

        match poll_result {
            Ok(RelayPollResult::Stop) => break,

            Ok(RelayPollResult::Disconnected) => {
                eprintln!(
                    "[partydeck] Physical gamepad disconnected: {}",
                    physical_event_path
                );

                physical_device = None;
                continue;
            }

            Ok(RelayPollResult::InputReady) => {}

            Err(error) => {
                eprintln!(
                    "[partydeck] Failed to poll physical gamepad {}: {}",
                    physical_event_path, error
                );
                break;
            }
        }

        events.clear();

        let fetch_result = {
            let Some(device) = physical_device.as_mut() else {
                continue;
            };

            device.fetch_events().map(|incoming_events| {
                events.extend(
                    incoming_events
                        .filter(|event| event.event_type() != EventType::SYNCHRONIZATION),
                );
            })
        };

        match fetch_result {
            Ok(()) if events.is_empty() => {}

            Ok(()) => {
                if let Err(error) = virtual_device.emit(&events) {
                    eprintln!(
                        "[partydeck] Failed to emit virtual gamepad events: {}",
                        error
                    );
                    break;
                }
            }

            Err(error) if error.kind() == ErrorKind::WouldBlock => {}

            Err(error) => {
                eprintln!(
                    "[partydeck] Physical gamepad disconnected or failed \
                     ({}): {}",
                    physical_event_path, error
                );

                physical_device = None;
            }
        }
    }

    println!(
        "[partydeck] Stopped virtual gamepad relay for {}",
        physical_event_path
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
