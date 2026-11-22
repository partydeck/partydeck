use std::collections::HashMap;
use std::fmt::Display;
use std::io;
use std::os::fd::{AsRawFd, RawFd};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use evdev::uinput::VirtualDevice;
use evdev::*;

const POLL_TIMEOUT: Duration = Duration::from_millis(100);
const REPORT_INTERVAL: Duration = Duration::from_secs(30);
const INPUT_GAP_WARNING: Duration = Duration::from_secs(1);
const SLOW_RUMBLE: Duration = Duration::from_millis(100);
// Steam isn't keeping up if a single rumble call takes this long; rumble is turned off for the pad
const STUCK_RUMBLE: Duration = Duration::from_secs(1);

/// Re-exposes a Steam Input virtual gamepad as Steam's "pad 0". SDL and Proton turn the pad number
/// in Steam's device names into the player/XInput slot, and each instance only reads player 1.
///
/// Input and rumble run on separate threads: every rumble call has to round-trip through Steam,
/// and input must never wait on it. Rumble sent to Steam is coalesced and rate limited, since games
/// can send thousands of updates per second.
pub struct PadRelay {
    path: String,
    shared: Arc<Shared>,
    input_thread: Option<JoinHandle<()>>,
}

#[derive(Clone, Copy)]
enum RumbleMode {
    Off,
    Uncapped,
    Capped(Duration),
}

impl RumbleMode {
    // None turns rumble off; Some(updates per second sent to Steam), 0 = no cap
    fn from_setting(rumble: Option<u32>) -> RumbleMode {
        match rumble {
            None => RumbleMode::Off,
            Some(hz) if hz == 0 || hz > 1000 => RumbleMode::Uncapped,
            Some(hz) => RumbleMode::Capped(Duration::from_millis(1000 / hz as u64)),
        }
    }
}

impl Display for RumbleMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RumbleMode::Off => write!(f, "off in PartyDeck settings"),
            RumbleMode::Uncapped => write!(f, "on, uncapped"),
            RumbleMode::Capped(interval) => write!(
                f,
                "on, at most one update to Steam every {} ms",
                interval.as_millis()
            ),
        }
    }
}

// Per-interval value (reset by each report) plus a running total for the final summary
#[derive(Default)]
struct Count {
    interval: AtomicU64,
    total: AtomicU64,
}

impl Count {
    fn add(&self, n: u64) {
        self.interval.fetch_add(n, Ordering::Relaxed);
        self.total.fetch_add(n, Ordering::Relaxed);
    }
    fn max(&self, n: u64) {
        self.interval.fetch_max(n, Ordering::Relaxed);
        self.total.fetch_max(n, Ordering::Relaxed);
    }
    fn take(&self) -> u64 {
        self.interval.swap(0, Ordering::Relaxed)
    }
    fn total(&self) -> u64 {
        self.total.load(Ordering::Relaxed)
    }
}

struct Shared {
    tag: String,
    started: Instant,
    stop: AtomicBool,
    frames: Count,
    longest_gap_ms: Count,
    slowest_write_ms: Count,
    rumble_requests: Count,
    rumble_sent: Count,
    rumble_failures: Count,
    slowest_rumble_ms: Count,
}

impl Shared {
    fn new(tag: String) -> Shared {
        Shared {
            tag,
            started: Instant::now(),
            stop: AtomicBool::new(false),
            frames: Count::default(),
            longest_gap_ms: Count::default(),
            slowest_write_ms: Count::default(),
            rumble_requests: Count::default(),
            rumble_sent: Count::default(),
            rumble_failures: Count::default(),
            slowest_rumble_ms: Count::default(),
        }
    }

    fn log(&self, msg: impl Display) {
        println!(
            "[partydeck] padrelay +{}s {}: {}",
            self.started.elapsed().as_secs(),
            self.tag,
            msg
        );
    }

    fn report(&self) {
        self.log(format!(
            "last {} s: {} input frames (longest gap {} ms, slowest relay write {} ms), rumble: {} requests from the game, {} sent to Steam ({} failed, slowest {} ms)",
            REPORT_INTERVAL.as_secs(),
            self.frames.take(),
            self.longest_gap_ms.take(),
            self.slowest_write_ms.take(),
            self.rumble_requests.take(),
            self.rumble_sent.take(),
            self.rumble_failures.take(),
            self.slowest_rumble_ms.take()
        ));
    }

    fn summary(&self) {
        self.log(format!(
            "stopped after {} s: {} input frames (longest gap {} ms, slowest relay write {} ms), rumble: {} requests from the game, {} sent to Steam ({} failed, slowest {} ms)",
            self.started.elapsed().as_secs(),
            self.frames.total(),
            self.longest_gap_ms.total(),
            self.slowest_write_ms.total(),
            self.rumble_requests.total(),
            self.rumble_sent.total(),
            self.rumble_failures.total(),
            self.slowest_rumble_ms.total()
        ));
    }
}

enum Rumble {
    Upload(i16, FFEffectData),
    Erase(i16),
    Play(i16, i32),
    Gain(u16),
}

// Latest state the game asked for, and the matching effect on Steam's pad once it's been sent
struct Effect {
    data: FFEffectData,
    data_dirty: bool,
    play: Option<i32>,
    steam: Option<FFEffect>,
}

impl PadRelay {
    /// `rumble` is None to drop rumble, or the most updates per second to send to Steam (0 = no cap).
    pub fn new(source_path: &str, rumble: Option<u32>) -> io::Result<PadRelay> {
        let src = Device::open(source_path)?;
        let src_name = src.name().unwrap_or("").to_string();
        let name = relay_name(&src_name);
        let ff_slots = src.supported_ff().map(|_| src.max_ff_effects());

        let mut builder = VirtualDevice::builder()?
            .name(&name)
            .input_id(src.input_id());
        if let Some(keys) = src.supported_keys() {
            builder = builder.with_keys(keys)?;
        }
        for (code, info) in src.get_absinfo()? {
            builder = builder.with_absolute_axis(&UinputAbsSetup::new(code, info))?;
        }
        if let Some(ff) = src.supported_ff() {
            builder = builder
                .with_ff(ff)?
                .with_ff_effects_max(src.max_ff_effects() as u32);
        }
        let mut relay = builder.build()?;

        let path = relay
            .enumerate_dev_nodes_blocking()?
            .next()
            .ok_or_else(|| io::Error::other("relay has no event node"))??
            .to_string_lossy()
            .to_string();
        wait_for_node(&path)?;

        let shared = Arc::new(Shared::new(format!("{} -> {}", source_path, path)));
        let mode = RumbleMode::from_setting(rumble);
        let id = src.input_id();
        shared.log(format!(
            "relaying '{}' ({:04x}:{:04x}) as '{}', rumble {}",
            src_name,
            id.vendor(),
            id.product(),
            name,
            match ff_slots {
                Some(slots) => format!("{} ({} effect slots)", mode, slots),
                None => "not supported by this pad".to_string(),
            }
        ));

        let relay_fd = relay.as_raw_fd();
        let relay = Arc::new(Mutex::new(relay));

        let input_thread = {
            let (relay, shared) = (relay.clone(), shared.clone());
            std::thread::Builder::new()
                .name("padrelay".to_string())
                .spawn(move || {
                    if let Err(e) = relay_input(src, &relay, &shared) {
                        shared.log(format!("input stopped: {}", e));
                    }
                    shared.summary();
                })?
        };

        if ff_slots.is_some() {
            // Not joined on drop: a call stuck in Steam can take up to 30 s to time out
            let rumble = Device::open(source_path).and_then(|ff_src| {
                let shared = shared.clone();
                std::thread::Builder::new()
                    .name("padrelay-ff".to_string())
                    .spawn(move || relay_rumble(ff_src, &relay, relay_fd, &shared, mode))
            });
            if let Err(e) = rumble {
                shared.log(format!("rumble disabled: {}", e));
            }
        }

        Ok(PadRelay {
            path,
            shared,
            input_thread: Some(input_thread),
        })
    }

    pub fn path(&self) -> &str {
        &self.path
    }
}

impl Drop for PadRelay {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.input_thread.take() {
            let _ = thread.join();
        }
    }
}

// "Microsoft X-Box 360 pad 1" -> "Microsoft X-Box 360 pad 0"
fn relay_name(name: &str) -> String {
    if let Some(i) = name.rfind("pad ") {
        let digits = &name[i + 4..];
        if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
            return format!("{}0", &name[..i + 4]);
        }
    }
    "Microsoft X-Box 360 pad 0".to_string()
}

// The node appears right away, but udev applies its permissions asynchronously
fn wait_for_node(path: &str) -> io::Result<()> {
    let start = Instant::now();
    loop {
        match Device::open(path) {
            Ok(_) => return Ok(()),
            Err(e) if start.elapsed() > Duration::from_secs(2) => return Err(e),
            Err(_) => std::thread::sleep(Duration::from_millis(20)),
        }
    }
}

fn poll_readable(fd: RawFd, timeout: Duration) -> libc::c_short {
    let mut pfd = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    let timeout_ms = timeout.as_millis().min(i32::MAX as u128) as i32;
    // SAFETY: pfd is a valid pollfd for the duration of the call
    if unsafe { libc::poll(&mut pfd, 1, timeout_ms) } <= 0 {
        return 0;
    }
    pfd.revents
}

fn relay_input(mut src: Device, relay: &Mutex<VirtualDevice>, shared: &Shared) -> io::Result<()> {
    let mut frame: Vec<InputEvent> = Vec::new();
    let mut last_report = Instant::now();
    let mut last_frame: Option<Instant> = None;

    while !shared.stop.load(Ordering::Relaxed) {
        if last_report.elapsed() >= REPORT_INTERVAL {
            shared.report();
            last_report = Instant::now();
        }

        let revents = poll_readable(src.as_raw_fd(), POLL_TIMEOUT);
        if revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
            return Err(io::Error::other("source pad disconnected"));
        }
        if revents & libc::POLLIN == 0 {
            continue;
        }

        // Forward whole frames; emit() terminates each one with its own SYN_REPORT
        for ev in src.fetch_events()? {
            if ev.event_type() != EventType::SYNCHRONIZATION {
                frame.push(ev);
            } else if ev.code() == SynchronizationCode::SYN_REPORT.0 {
                let now = Instant::now();
                if let Some(prev) = last_frame {
                    let gap = now - prev;
                    shared.longest_gap_ms.max(gap.as_millis() as u64);
                    if gap >= INPUT_GAP_WARNING {
                        shared.log(format!("no input from Steam for {} ms", gap.as_millis()));
                    }
                }
                last_frame = Some(now);

                relay
                    .lock()
                    .map_err(|_| io::Error::other("relay lock poisoned"))?
                    .emit(&frame)?;
                shared
                    .slowest_write_ms
                    .max(now.elapsed().as_millis() as u64);
                shared.frames.add(1);
                frame.clear();
            }
        }
    }
    Ok(())
}

fn relay_rumble(
    mut src: Device,
    relay: &Mutex<VirtualDevice>,
    relay_fd: RawFd,
    shared: &Shared,
    mut mode: RumbleMode,
) {
    let mut effects: HashMap<i16, Effect> = HashMap::new();
    let mut gain: Option<u16> = None;
    let mut urgent = false;
    let mut next_flush = Instant::now();

    while !shared.stop.load(Ordering::Relaxed) {
        let pending = gain.is_some() || effects.values().any(|e| e.data_dirty || e.play.is_some());
        let timeout = match mode {
            RumbleMode::Capped(_) if pending => next_flush
                .saturating_duration_since(Instant::now())
                .max(Duration::from_millis(1)),
            _ => POLL_TIMEOUT,
        };

        if poll_readable(relay_fd, timeout) & libc::POLLIN != 0 {
            // Answer the game right away; Steam is only called after the relay lock is released
            let requests = match relay.lock() {
                Ok(mut relay) => take_rumble_requests(&mut relay),
                Err(_) => break,
            };
            let requests = match requests {
                Ok(requests) => requests,
                Err(e) => {
                    shared.log(format!("rumble stopped: {}", e));
                    break;
                }
            };
            shared.rumble_requests.add(requests.len() as u64);

            for request in requests {
                match request {
                    Rumble::Upload(id, data) => {
                        let effect = effects.entry(id).or_insert(Effect {
                            data,
                            data_dirty: true,
                            play: None,
                            steam: None,
                        });
                        effect.data = data;
                        effect.data_dirty = true;
                    }
                    Rumble::Play(id, count) => {
                        if let Some(effect) = effects.get_mut(&id) {
                            effect.play = Some(count);
                            urgent |= count == 0;
                        }
                    }
                    Rumble::Erase(id) => {
                        if let Some(effect) = effects.remove(&id)
                            && let Some(mut steam) = effect.steam
                            && !call_steam(shared, || {
                                let _ = steam.stop();
                                drop(steam);
                                Ok(())
                            })
                        {
                            mode = give_up_rumble(&mut effects);
                        }
                    }
                    Rumble::Gain(value) => gain = Some(value),
                }
            }
        }

        if let RumbleMode::Off = mode {
            for effect in effects.values_mut() {
                effect.data_dirty = false;
                effect.play = None;
            }
            gain = None;
            urgent = false;
            continue;
        }

        let pending = gain.is_some() || effects.values().any(|e| e.data_dirty || e.play.is_some());
        let now = Instant::now();
        let due = match mode {
            RumbleMode::Capped(_) => urgent || now >= next_flush,
            _ => true,
        };
        if pending && due {
            if !flush_rumble(&mut src, &mut effects, &mut gain, shared) {
                mode = give_up_rumble(&mut effects);
            }
            urgent = false;
            if let RumbleMode::Capped(interval) = mode {
                next_flush = now + interval;
            }
        }
    }

    // Nothing should keep vibrating once the game is gone; dropping the effects erases them
    for effect in effects.values_mut() {
        if let Some(steam) = effect.steam.as_mut() {
            let _ = steam.stop();
        }
    }
}

// Sends the latest requested state of every changed effect to Steam's pad
fn flush_rumble(
    src: &mut Device,
    effects: &mut HashMap<i16, Effect>,
    gain: &mut Option<u16>,
    shared: &Shared,
) -> bool {
    if let Some(value) = gain.take()
        && !call_steam(shared, || src.set_ff_gain(value))
    {
        return false;
    }
    for effect in effects.values_mut() {
        if effect.data_dirty {
            effect.data_dirty = false;
            let data = effect.data;
            let ok = call_steam(shared, || {
                if let Some(steam) = effect.steam.as_mut() {
                    steam.update(data)
                } else {
                    effect.steam = Some(src.upload_ff_effect(data)?);
                    Ok(())
                }
            });
            if !ok {
                return false;
            }
        }
        if let Some(count) = effect.play.take()
            && let Some(steam) = effect.steam.as_mut()
            && !call_steam(shared, || {
                if count > 0 {
                    steam.play(count)
                } else {
                    steam.stop()
                }
            })
        {
            return false;
        }
    }
    true
}

// Times one call into Steam; false means Steam took so long that rumble should be given up on
fn call_steam(shared: &Shared, call: impl FnOnce() -> io::Result<()>) -> bool {
    let start = Instant::now();
    let result = call();
    let took = start.elapsed();

    shared.rumble_sent.add(1);
    shared.slowest_rumble_ms.max(took.as_millis() as u64);
    if let Err(e) = result {
        if shared.rumble_failures.total() == 0 {
            shared.log(format!("rumble call to Steam failed: {}", e));
        }
        shared.rumble_failures.add(1);
    }

    if took >= STUCK_RUMBLE {
        shared.log(format!(
            "rumble call to Steam took {} ms, turning rumble off for this controller",
            took.as_millis()
        ));
        return false;
    }
    if took >= SLOW_RUMBLE {
        shared.log(format!("rumble call to Steam took {} ms", took.as_millis()));
    }
    true
}

fn give_up_rumble(effects: &mut HashMap<i16, Effect>) -> RumbleMode {
    for effect in effects.values_mut() {
        effect.data_dirty = false;
        effect.play = None;
        if let Some(steam) = effect.steam.as_mut() {
            let _ = steam.stop();
        }
    }
    RumbleMode::Off
}

fn take_rumble_requests(relay: &mut VirtualDevice) -> io::Result<Vec<Rumble>> {
    let events: Vec<InputEvent> = relay.fetch_events()?.collect();
    let mut requests = Vec::new();
    for ev in events {
        match ev.destructure() {
            EventSummary::UInput(ev, UInputCode::UI_FF_UPLOAD, _) => {
                let mut upload = relay.process_ff_upload(ev)?;
                requests.push(Rumble::Upload(upload.effect_id(), upload.effect()));
                upload.set_retval(0);
            }
            EventSummary::UInput(ev, UInputCode::UI_FF_ERASE, _) => {
                let mut erase = relay.process_ff_erase(ev)?;
                requests.push(Rumble::Erase(erase.effect_id() as i16));
                erase.set_retval(0);
            }
            EventSummary::ForceFeedback(_, FFEffectCode::FF_GAIN, value) => {
                requests.push(Rumble::Gain(value as u16));
            }
            EventSummary::ForceFeedback(_, code, value) => {
                requests.push(Rumble::Play(code.0 as i16, value));
            }
            _ => {}
        }
    }
    Ok(requests)
}
