use std::error::Error;
use std::io::Write;
use std::os::fd::FromRawFd;
use std::path::PathBuf;

use std::os::fd::{AsFd, IntoRawFd};
use std::io::Read;
use nix::fcntl;
use nix::poll;
use nix::unistd;
use nix::unistd::Pid;

use wayland_client::Connection;
use wayland_client::Dispatch;
use wayland_client::Proxy;
use wayland_client::protocol::wl_output::WlOutput;
use wayland_client::protocol::wl_registry::Event as RegistryEvent;
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::protocol::wl_seat::WlSeat;

use crate::layout_manager::river_layout_v3::river_layout_v3;
use crate::layout_manager::wayland_client_code::river_layout_v3::river_layout_v3::RiverLayoutV3;
use crate::layout_manager::wayland_client_code::river_layout_v3::river_layout_manager_v3::RiverLayoutManagerV3;
use crate::layout_manager::wayland_client_code::river_control_unstable_v1::zriver_command_callback_v1::ZriverCommandCallbackV1;
use crate::layout_manager::wayland_client_code::river_control_unstable_v1::zriver_control_v1::ZriverControlV1;

use crate::layout_manager::wayland_client_code::wlr_output_mgmt_unstable_v1::zwlr_output_manager_v1;
use crate::layout_manager::wayland_client_code::wlr_output_mgmt_unstable_v1::zwlr_output_manager_v1::ZwlrOutputManagerV1;
use crate::layout_manager::wayland_client_code::wlr_output_mgmt_unstable_v1::zwlr_output_head_v1::ZwlrOutputHeadV1;
use crate::layout_manager::wayland_client_code::wlr_output_mgmt_unstable_v1::zwlr_output_configuration_head_v1::ZwlrOutputConfigurationHeadV1;
use crate::layout_manager::wayland_client_code::wlr_output_mgmt_unstable_v1::zwlr_output_mode_v1::ZwlrOutputModeV1;
use crate::layout_manager::wlr_output_mgmt_unstable_v1::zwlr_output_configuration_v1;
use crate::layout_manager::wlr_output_mgmt_unstable_v1::zwlr_output_configuration_v1::ZwlrOutputConfigurationV1;

use super::super::get_monitors_errorless;
use super::super::monitor::Monitor;
use std::env;

use std::sync::Arc;
use wayland_client::QueueHandle;
use wayland_client::backend::ObjectData;

struct DummyData {
    respond_to_opcode: bool,
}
impl ObjectData for DummyData {
    fn event(
        self: Arc<Self>,
        _backend: &wayland_backend::client::Backend,
        msg: wayland_backend::protocol::Message<
            wayland_backend::client::ObjectId,
            std::os::unix::prelude::OwnedFd,
        >,
    ) -> Option<Arc<dyn ObjectData>> {
        // I dont know, I was debugging, and 3 requies a new dummy object (if we do for the other is also crashes)...
        // Refers to evt: zwlr_output_head_v1#XXXXXXXXXX.mode(new id zwlr_output_mode_v1#XXXXXXXXXX)
        // Using the "respond_to_opcode" bool to make the child not respond to zwlr_output_mode_v1#XXXXXXXXXX.finished()
        if msg.opcode == 3 && self.respond_to_opcode {
            Some(Arc::new(DummyData {
                respond_to_opcode: false,
            }))
        } else {
            None
        }
    }
    fn destroyed(&self, _object_id: wayland_backend::client::ObjectId) {}
}

struct LayoutState {
    river_layout: Option<RiverLayoutV3>,
    river_layout_man: Option<RiverLayoutManagerV3>,
    river_control: Option<ZriverControlV1>,
    wlr_output: Option<ZwlrOutputManagerV1>,
    output_head: Option<ZwlrOutputHeadV1>,
    pending_config: Option<ZwlrOutputConfigurationV1>,
    wlr_output_done: bool,
    seat: Option<WlSeat>,
    outputs_name: u32,
    outputs: usize,
    layout_width: i32,
    layout_height: i32,
}
impl Dispatch<WlRegistry, ()> for LayoutState {
    fn event(
        state: &mut Self,
        registry: &WlRegistry,
        event: <WlRegistry as wayland_client::Proxy>::Event,
        _: &(),
        _conn: &Connection,
        qh: &wayland_client::QueueHandle<Self>,
    ) {
        match event {
            RegistryEvent::Global {
                name,
                interface,
                version,
            } => {
                if interface == "zwlr_output_manager_v1" {
                    let wlr_output_mgr =
                        registry.bind::<ZwlrOutputManagerV1, _, _>(name, version, qh, ());
                    state.wlr_output = Some(wlr_output_mgr);
                }
                if interface == "zriver_control_v1" {
                    let control = registry.bind::<ZriverControlV1, _, _>(name, version, qh, ());
                    state.river_control = Some(control);
                }
                if interface == "river_layout_manager_v3" {
                    let manager =
                        registry.bind::<RiverLayoutManagerV3, _, _>(name, version, qh, ());
                    state.river_layout_man = Some(manager);
                }
                if interface == "wl_seat" {
                    let seat = registry.bind::<WlSeat, _, _>(name, version, qh, ());
                    state.seat = Some(seat);
                }
                if interface == "wl_output" {
                    let output = registry.bind::<WlOutput, _, _>(name, version, qh, ());
                    if let Some(manager) = &state.river_layout_man {
                        state.river_layout = Some(manager.get_layout(
                            &output,
                            "river-game-layout".to_string(),
                            qh,
                            (),
                        ));

                        if let Some(ctrl) = &state.river_control {
                            if let Some(seat) = &state.seat {
                                println!("Enabling layout!");

                                // Load the layout we created
                                ctrl.add_argument("output-layout".to_string());
                                ctrl.add_argument("river-game-layout".to_string());
                                ctrl.run_command(&seat, qh, ());
                            }
                        }
                    }
                    state.outputs_name = name;
                    state.outputs += 1;
                }
            }
            RegistryEvent::GlobalRemove { name } => {
                if name == state.outputs_name {
                    state.outputs = state.outputs.saturating_sub(1); // Prevent errors from underflow
                    if state.outputs == 0 {
                        if let Some(ctrl) = &state.river_control {
                            if let Some(seat) = &state.seat {
                                println!("Exiting!!!");

                                ctrl.add_argument("exit".to_string());
                                ctrl.run_command(seat, qh, ());
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<ZwlrOutputManagerV1, ()> for LayoutState {
    fn event(
        state: &mut Self,
        _proxy: &ZwlrOutputManagerV1,
        event: <ZwlrOutputManagerV1 as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        qh: &wayland_client::QueueHandle<Self>,
    ) {
        match event {
            zwlr_output_manager_v1::Event::Done { serial } => {
                if !state.wlr_output_done {
                    if let Some(output_manager) = &state.wlr_output {
                        if let Some(head) = &state.output_head {
                            let output_config = output_manager.create_configuration(serial, qh, ());
                            let output_config_head = output_config.enable_head(head, qh, ());
                            output_config_head.set_custom_mode(
                                state.layout_width,
                                state.layout_height,
                                0,
                            ); // Refresh must be 0 or will be denied by some compositors
                            output_config.apply();
                            state.pending_config = Some(output_config);
                        }
                    }
                }
            }
            zwlr_output_manager_v1::Event::Head { head } => {
                state.output_head = Some(head);
            }
            _ => {}
        }
    }

    fn event_created_child(_opcode: u16, _qh: &QueueHandle<Self>) -> Arc<dyn ObjectData> {
        println!("Output manager created child");
        Arc::new(DummyData {
            respond_to_opcode: true,
        })
    }
}

impl Dispatch<ZwlrOutputConfigurationV1, ()> for LayoutState {
    fn event(
        state: &mut Self,
        proxy: &ZwlrOutputConfigurationV1,
        event: <ZwlrOutputConfigurationV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_output_configuration_v1::Event::Succeeded => {
                println!("configuration succeeded");
                state.wlr_output_done = true;
                proxy.destroy();
                state.pending_config = None;
            }
            zwlr_output_configuration_v1::Event::Failed => {
                println!("configuration failed");
                state.wlr_output_done = true;
                proxy.destroy();
                state.pending_config = None;
            }
            _ => {}
        }
    }
}

impl Dispatch<RiverLayoutV3, ()> for LayoutState {
    fn event(
        state: &mut Self,
        _proxy: &RiverLayoutV3,
        event: <RiverLayoutV3 as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &wayland_client::QueueHandle<Self>,
    ) {
        match event {
            river_layout_v3::Event::LayoutDemand {
                view_count,
                usable_width,
                usable_height,
                tags: _,
                serial,
            } => {
                let Some(layout) = &state.river_layout else {
                    return;
                };

                let (mut w, mut h) = (0, 0);

                while w * h < view_count {
                    if usable_width * h * 9 > usable_height * w * 16 {
                        w += 1;
                    } else {
                        h += 1;
                    }
                }

                while view_count <= w * (h - 1) {
                    h -= 1
                }
                while view_count <= (w - 1) * h {
                    w -= 1
                }

                for i in 0..view_count {
                    let cur_row_width = (w - ((i % h >= (view_count - 1) % h + 1) as u32)).max(1);

                    layout.push_view_dimensions(
                        ((i / h) * usable_width / cur_row_width) as i32,
                        ((i % h) * usable_height / h) as i32,
                        (usable_width / cur_row_width) as u32,
                        (usable_height / h) as u32,
                        serial,
                    );
                }
                layout.commit("[]=".to_string(), serial); // []= is the layout name, using this due to its default in other managers.
            }
            _ => {}
        }
    }
}

macro_rules! impl_empty_dispatch {
    ($state:ty, $iface:ty) => {
        impl Dispatch<$iface, ()> for $state {
            fn event(
                _state: &mut $state,
                _proxy: &$iface,
                _event: <$iface as wayland_client::Proxy>::Event,
                _data: &(),
                _conn: &Connection,
                _qhandle: &wayland_client::QueueHandle<$state>,
            ) {
                // ignore
            }
        }
    };
}

impl_empty_dispatch!(LayoutState, ZriverCommandCallbackV1);
impl_empty_dispatch!(LayoutState, ZriverControlV1);
impl_empty_dispatch!(LayoutState, RiverLayoutManagerV3);
impl_empty_dispatch!(LayoutState, WlSeat);
impl_empty_dispatch!(LayoutState, WlOutput);

impl_empty_dispatch!(LayoutState, ZwlrOutputConfigurationHeadV1);
impl_empty_dispatch!(LayoutState, ZwlrOutputModeV1);
impl_empty_dispatch!(LayoutState, ZwlrOutputHeadV1);

fn flush_file_data_layout_manager(fd: i32, main_monitor: &Monitor) {
    let mut file = unsafe { std::fs::File::from_raw_fd(fd) };

    let way_disp_str = env::var_os("WAYLAND_DISPLAY").expect("Failed to decode wayland display");
    let x11_disp_str = env::var_os("DISPLAY").expect("Failed to decode x11 display");
    let way_disp = way_disp_str.as_encoded_bytes();
    let x11_disp = x11_disp_str.as_encoded_bytes();
    let mut buf = Vec::with_capacity(4 + 4 + 4 + 4 + way_disp.len() + x11_disp.len());
    buf.extend_from_slice(&(way_disp.len() as u32).to_be_bytes());
    buf.extend_from_slice(&(x11_disp.len() as u32).to_be_bytes());
    buf.extend_from_slice(&(main_monitor.width() as u32).to_be_bytes());
    buf.extend_from_slice(&(main_monitor.height() as u32).to_be_bytes());
    buf.extend_from_slice(way_disp);
    buf.extend_from_slice(x11_disp);
    file.write_all(&buf).expect("Failed to write display data");
    println!(
        "Wrote monitor info over fd: X11: {:?}, Wayland: {:?}, Width: {}, Heigh: {}",
        x11_disp_str,
        way_disp_str,
        main_monitor.width(),
        main_monitor.height()
    );

    let _ = file.flush();
    drop(file);
}

pub fn start_layout_manager(fd: i32, layout_width: i32, layout_height: i32) {
    let conn = Connection::connect_to_env().expect("Failed to connect to wayland session");
    let mut event_queue = conn.new_event_queue();
    let qh = event_queue.handle();

    let _registry = conn.display().get_registry(&qh, ());
    let mut state = LayoutState {
        river_layout: None,
        river_layout_man: None,
        river_control: None,
        wlr_output: None,
        pending_config: None,
        wlr_output_done: false,
        output_head: None,
        seat: None,
        outputs_name: 0,
        outputs: 0,
        layout_width: layout_width,
        layout_height: layout_height,
    };

    let mut has_sent_data = false;
    let mut startup_commands_stage = 0;
    loop {
        if let Err(e) = event_queue.blocking_dispatch(&mut state) {
            eprintln!("Wayland connection closed {e}");
            break;
        }

        // Used so we can maybe wait for wlr to attempt connection resize
        if (state.wlr_output == None || state.wlr_output_done) && !has_sent_data {
            let monitors = get_monitors_errorless();
            println!("[Layout] Monitors detected:");
            for monitor in &monitors {
                println!(
                    "[Layout] {} ({}x{})",
                    monitor.name(),
                    monitor.width(),
                    monitor.height()
                );
            }
            flush_file_data_layout_manager(fd, &monitors[0]);
            has_sent_data = true;
        }


        match startup_commands_stage {
            0 => {
                if let Some(ctrl) = &state.river_control {
                    if let Some(seat) = &state.seat {
                        // Set border-width so we dont have an area arround it.
                        ctrl.add_argument("border-width".to_string());
                        ctrl.add_argument("0".to_string());
                        ctrl.run_command(seat, &qh, ());

                        startup_commands_stage = 1;
                    }
                }
            }
            1 => {
                if let Some(ctrl) = &state.river_control {
                    if let Some(seat) = &state.seat {
                        // Set background color to more "natural" one than default light blue for games.
                        ctrl.add_argument("background-color".to_string());
                        ctrl.add_argument("0x000000".to_string());
                        ctrl.run_command(seat, &qh, ());

                        startup_commands_stage = 2;
                    }
                }
            }
            _ => {}
        }
    }
}

// Sends the splitscreen script to the active KWin session through DBus
pub fn kwin_dbus_start_script(file: PathBuf) -> Result<(), Box<dyn Error>> {
    println!(
        "[partydeck] util::kwin_dbus_start_script - Loading script {}...",
        file.display()
    );
    if !file.exists() {
        return Err("[partydeck] util::kwin_dbus_start_script - Script file doesn't exist!".into());
    }

    let conn = zbus::blocking::Connection::session()?;
    let proxy = zbus::blocking::Proxy::new(
        &conn,
        "org.kde.KWin",
        "/Scripting",
        "org.kde.kwin.Scripting",
    )?;

    let _: i32 = proxy.call("loadScript", &(file.to_string_lossy(), "splitscreen"))?;
    println!("[partydeck] util::kwin_dbus_start_script - Script loaded. Starting...");
    let _: () = proxy.call("start", &())?;

    println!("[partydeck] util::kwin_dbus_start_script - KWin script started.");
    Ok(())
}

pub fn kwin_dbus_unload_script() -> Result<(), Box<dyn Error>> {
    println!("[partydeck] util::kwin_dbus_unload_script - Unloading splitscreen script...");
    let conn = zbus::blocking::Connection::session()?;
    let proxy = zbus::blocking::Proxy::new(
        &conn,
        "org.kde.KWin",
        "/Scripting",
        "org.kde.kwin.Scripting",
    )?;

    let _: bool = proxy.call("unloadScript", &("splitscreen"))?;

    println!("[partydeck] util::kwin_dbus_unload_script - Script unloaded.");
    Ok(())
}


pub fn spawn_comp_and_get_display(
    comp_executable: &str,
    primary_monitor: Monitor,
) -> Option<(String, String, Monitor, Pid)> {
    let base_program_buf = env::current_exe().expect("Failed to get partydeck executable");
    let base_program = base_program_buf.to_str()?;

    let (read_fd, write_fd) = unistd::pipe().unwrap();

    let flags = fcntl::FdFlag::from_bits_truncate(
        fcntl::fcntl(&write_fd, fcntl::FcntlArg::F_GETFD).unwrap(),
    );
    fcntl::fcntl(
        &write_fd,
        fcntl::FcntlArg::F_SETFD(flags & !fcntl::FdFlag::FD_CLOEXEC),
    )
    .expect("Failed to open pipe to river");

    let mut cmd = std::process::Command::new(comp_executable);

    match comp_executable {
        "river" => {
            cmd.args([
                "-c",
                format!(
                    "{} --internal-layout {}:{}:{}",
                    base_program,
                    &write_fd.into_raw_fd(),
                    primary_monitor.width(),
                    primary_monitor.height()
                )
                .as_str(),
            ]);
        }
        "kwin_wayland" => {
            cmd.args([
                "--xwayland",
                "--exit-with-session",
                "--height",
                &primary_monitor.height().to_string(),
                "--width",
                &primary_monitor.width().to_string(),
                "--",
                format!(
                    "{} --internal-layout {}:{}:{}",
                    base_program,
                    &write_fd.into_raw_fd(),
                    primary_monitor.width(),
                    primary_monitor.height()
                )
                .as_str(),
            ]);
        }
        _ => {
            println!(
                "Unknown comp ({}); trusting that it works, MAY FAIL",
                comp_executable
            );
            cmd.args([
                "--",
                format!(
                    "{} --internal-layout {}:{}:{}",
                    base_program,
                    &write_fd.into_raw_fd(),
                    primary_monitor.width(),
                    primary_monitor.height()
                )
                .as_str(),
            ]);
        }
    }

    let child_pid;
    match cmd.spawn() {
        Ok(child) => child_pid = Pid::from_raw((child.id()) as i32),
        Err(e) => {
            eprintln!(
                "[partydeck] Failed to start COMP ({}): {}",
                comp_executable, e
            );
            return None;
        }
    }

    let mut fds = [poll::PollFd::new(read_fd.as_fd(), poll::PollFlags::POLLIN)];
    let res = poll::poll(&mut fds, 2000 as u16).unwrap_or(0);
    if res == 0 {
        eprintln!("[partydeck] NO DATA FROM COMP HANDLER");
        return None;
    }

    let mut reader = unsafe { std::fs::File::from_raw_fd(read_fd.into_raw_fd()) };

    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf).expect("Failed to read FD");
    let way_disp_len = u32::from_be_bytes(len_buf) as usize;
    reader.read_exact(&mut len_buf).expect("Failed to read FD");
    let x11_disp_len = u32::from_be_bytes(len_buf) as usize;

    reader.read_exact(&mut len_buf).expect("Failed to read FD");
    let monitor_width = u32::from_be_bytes(len_buf);
    reader.read_exact(&mut len_buf).expect("Failed to read FD");
    let monitor_height = u32::from_be_bytes(len_buf);
    let remote_monitor = Monitor::new("REMOTE_MONITOR".to_owned(), monitor_width, monitor_height);

    let mut way_disp_buf = vec![0u8; way_disp_len];
    reader
        .read_exact(&mut way_disp_buf)
        .expect("Failed to read FD");
    let mut x11_disp_buf = vec![0u8; x11_disp_len];
    reader
        .read_exact(&mut x11_disp_buf)
        .expect("Failed to read FD");

    let way_disp = String::from_utf8(way_disp_buf).expect("Failed to decode FD");
    let x11_disp = String::from_utf8(x11_disp_buf).expect("Failed to decode FD");

    println!(
        "Got DISPLAY_WAYLAND from COMP: {} and DISPLAY: {}, with resolution: {}x{}",
        way_disp, x11_disp, monitor_width, monitor_height
    );

    return Some((
        way_disp.to_string(),
        x11_disp.to_string(),
        remote_monitor,
        child_pid,
    ));
}