// SPDX-License-Identifier: GPL-3.0-only
//! Four-finger horizontal swipe to switch windows (like Ctrl+Tab / Windows+Tab).
//!
//! Tracks four-finger horizontal gestures on the touchpad via evdev and triggers
//! the COSMIC window switcher with smooth modifier holding (Ctrl+Tab or Alt+Tab),
//! maintaining a stable, flicker-free switcher overlay during gesture movement
//! and cleanly activating the chosen window when fingers are lifted.

use anyhow::{Context, Result, bail};
use std::{
    fs::{self, File, OpenOptions},
    io::Read,
    os::fd::AsRawFd,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

use crate::{user_service, window_controls::find_setup_binary};

pub const UNIT: &str = "cosmic-macos-four-finger-swipe.service";

const EV_SYN: u16 = 0x00;
const SYN_REPORT: u16 = 0x00;
const EV_KEY: u16 = 0x01;
const EV_ABS: u16 = 0x03;

const BTN_TOOL_QUADTAP: u16 = 0x14f;

const ABS_MT_SLOT: u16 = 0x2f;
const ABS_MT_POSITION_X: u16 = 0x35;
const ABS_MT_POSITION_Y: u16 = 0x36;
const ABS_MT_TRACKING_ID: u16 = 0x39;

pub const KEY_TAB: u16 = 15;
pub const KEY_ENTER: u16 = 28;
pub const KEY_LEFTCTRL: u16 = 29;
pub const KEY_LEFTSHIFT: u16 = 42;
pub const KEY_LEFTALT: u16 = 56;
pub const KEY_LEFTMETA: u16 = 125;

const MAX_SLOTS: usize = 16;
const DEFAULT_THRESHOLD: i32 = 350;
const CONTACT_GRACE: Duration = Duration::from_millis(150);
const RELEASE_GRACE: Duration = Duration::from_millis(400);

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct InputEvent {
    pub time: libc::timeval,
    pub type_: u16,
    pub code: u16,
    pub value: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct UInputSetup {
    id: libc::input_id,
    name: [libc::c_char; 80],
    ff_effects_max: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct InputAbsInfo {
    value: i32,
    minimum: i32,
    maximum: i32,
    fuzz: i32,
    flat: i32,
    resolution: i32,
}

/// What stops four-finger swipe from working.
#[must_use]
pub fn missing_prerequisites(input_readable: bool, uinput_writable: bool) -> Vec<&'static str> {
    let mut missing = Vec::new();
    if !input_readable {
        missing.push(
            "this session cannot read touchpads in /dev/input \
             (join the `input` group, then log out and back in)",
        );
    }
    if !uinput_writable {
        missing.push(
            "this session cannot write /dev/uinput \
             (udev rule 60-uinput.rules and the uinput module)",
        );
    }
    missing
}

fn input_readable() -> bool {
    fs::read_dir("/dev/input").is_ok_and(|entries| {
        entries.filter_map(Result::ok).any(|e| {
            e.file_name().to_string_lossy().starts_with("event")
                && OpenOptions::new().read(true).open(e.path()).is_ok()
        })
    })
}

fn uinput_writable() -> bool {
    OpenOptions::new().write(true).open("/dev/uinput").is_ok()
}

#[must_use]
pub fn missing_now() -> Vec<&'static str> {
    missing_prerequisites(input_readable(), uinput_writable())
}

pub fn check_can_enable() -> Result<()> {
    let missing = missing_now();
    if missing.is_empty() {
        return Ok(());
    }
    let list: String = missing.iter().map(|m| format!("\n  - {m}")).collect();
    bail!("four-finger swipe is not ready:{list}")
}

fn unit_path() -> Result<PathBuf> {
    Ok(user_service::unit_dir()?.join(UNIT))
}

#[must_use]
pub fn unit_contents(setup_binary: &Path) -> String {
    format!(
        "[Unit]\n\
         Description=Four-finger swipe window switch (cosmic-macos-applet)\n\
         PartOf=graphical-session.target\n\
         After=graphical-session.target\n\
         \n\
         [Service]\n\
         Type=exec\n\
         ExecStart={} four-finger-swipe-watch\n\
         Restart=on-failure\n\
         RestartSec=2\n\
         \n\
         [Install]\n\
         WantedBy=graphical-session.target\n",
        setup_binary.display()
    )
}

#[must_use]
pub fn enabled() -> bool {
    unit_path().is_ok_and(|p| p.is_file()) && user_service::is_enabled(UNIT)
}

pub fn set_enabled(enable: bool) -> Result<()> {
    let path = unit_path()?;
    if enable {
        let current = std::env::current_exe().context("locate the running executable")?;
        let binary = find_setup_binary(&current, std::env::var_os("PATH").as_deref())
            .context("cosmic-macos-setup is not installed; run `just install`")?;
        fs::create_dir_all(path.parent().context("unit dir")?)?;
        fs::write(&path, unit_contents(&binary))
            .with_context(|| format!("write {}", path.display()))?;
        user_service::daemon_reload()?;
        user_service::enable_now(UNIT)?;
    } else if path.is_file() {
        user_service::disable_now(UNIT)?;
        fs::remove_file(&path).with_context(|| format!("remove {}", path.display()))?;
        user_service::daemon_reload()?;
    }
    Ok(())
}

/// Discovers the active touchpad evdev device.
/// Prioritizes linux-3-finger-drag/proxy if present, otherwise returns regular touchpad.
#[must_use]
pub fn find_touchpad_device() -> Option<PathBuf> {
    if let Ok(content) = fs::read_to_string("/proc/bus/input/devices") {
        let mut proxy_device = None;
        let mut regular_touchpad = None;

        let mut current_name = String::new();
        let mut current_phys = String::new();
        let mut current_handlers = Vec::new();

        for line in content.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("N: Name=") {
                current_name = rest.trim_matches('"').to_lowercase();
            } else if let Some(rest) = line.strip_prefix("P: Phys=") {
                current_phys = rest.to_lowercase();
            } else if let Some(rest) = line.strip_prefix("H: Handlers=") {
                current_handlers = rest.split_whitespace().map(ToOwned::to_owned).collect();
            } else if line.is_empty() {
                if current_name.contains("touchpad") {
                    for h in &current_handlers {
                        if h.starts_with("event") {
                            let path = PathBuf::from(format!("/dev/input/{h}"));
                            if current_phys.contains("proxy") {
                                proxy_device = Some(path.clone());
                            } else if regular_touchpad.is_none() {
                                regular_touchpad = Some(path.clone());
                            }
                        }
                    }
                }
                current_name.clear();
                current_phys.clear();
                current_handlers.clear();
            }
        }

        if let Some(dev) = proxy_device {
            return Some(dev);
        }
        if let Some(dev) = regular_touchpad {
            return Some(dev);
        }
    }

    // Fallback: search /dev/input for event devices that look like a touchpad
    if let Ok(entries) = fs::read_dir("/dev/input") {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with("event") {
                let path = entry.path();
                if let Ok(f) = OpenOptions::new().read(true).open(&path) {
                    let fd = f.as_raw_fd();
                    let mut buf = [0u8; 256];
                    unsafe {
                        if libc::ioctl(fd, 0x82004506 /* EVIOCGNAME */, buf.as_mut_ptr()) > 0 {
                            let dev_name = std::ffi::CStr::from_ptr(buf.as_ptr() as *const _)
                                .to_string_lossy()
                                .to_lowercase();
                            if dev_name.contains("touchpad") {
                                return Some(path);
                            }
                        }
                    }
                }
            }
        }
    }

    None
}

/// Queries the MT width to scale gesture thresholds appropriately.
fn query_abs_width(fd: libc::c_int) -> Option<i32> {
    let mut info = std::mem::MaybeUninit::<InputAbsInfo>::uninit();
    let res = unsafe {
        libc::ioctl(fd, 0x80184575 /* EVIOCGABS(0x35) */, info.as_mut_ptr())
    };
    if res >= 0 {
        let info = unsafe { info.assume_init() };
        let span = info.maximum - info.minimum;
        if span > 0 {
            return Some(span);
        }
    }
    None
}

/// Detects whether the user mapped WindowSwitcher to Ctrl, Super, or Alt.
#[must_use]
pub fn detect_switcher_modifier() -> u16 {
    if let Ok(home) = std::env::var("HOME") {
        let custom = PathBuf::from(home)
            .join(".config/cosmic/com.system76.CosmicSettings.Shortcuts/v1/custom");
        if let Ok(text) = fs::read_to_string(custom) {
            for part in text.split("):") {
                if part.contains("WindowSwitcher") {
                    if part.contains("Ctrl") {
                        return KEY_LEFTCTRL;
                    } else if part.contains("Super") {
                        return KEY_LEFTMETA;
                    } else if part.contains("Alt") {
                        return KEY_LEFTALT;
                    }
                }
            }
        }
    }
    KEY_LEFTALT
}

pub struct VirtualKeyboard {
    file: File,
}

impl VirtualKeyboard {
    pub fn open() -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/uinput")
            .context("open /dev/uinput")?;

        let fd = file.as_raw_fd();
        unsafe {
            if libc::ioctl(
                fd,
                0x40045564, /* UI_SET_EVBIT */
                EV_KEY as libc::c_int,
            ) < 0
            {
                bail!("ioctl UI_SET_EVBIT EV_KEY failed");
            }
            if libc::ioctl(
                fd,
                0x40045564, /* UI_SET_EVBIT */
                EV_SYN as libc::c_int,
            ) < 0
            {
                bail!("ioctl UI_SET_EVBIT EV_SYN failed");
            }
            for &k in &[
                KEY_TAB,
                KEY_ENTER,
                KEY_LEFTCTRL,
                KEY_LEFTSHIFT,
                KEY_LEFTALT,
                KEY_LEFTMETA,
            ] {
                if libc::ioctl(fd, 0x40045565 /* UI_SET_KEYBIT */, k as libc::c_int) < 0 {
                    bail!("ioctl UI_SET_KEYBIT {k} failed");
                }
            }

            let mut setup = UInputSetup {
                id: libc::input_id {
                    bustype: 0x03, // BUS_USB
                    vendor: 0x1234,
                    product: 0x5678,
                    version: 1,
                },
                name: [0; 80],
                ff_effects_max: 0,
            };
            let name_bytes = b"Cosmic Macos Four Finger Virtual Keyboard";
            for (dest, &src) in setup.name.iter_mut().zip(name_bytes.iter()) {
                *dest = src as libc::c_char;
            }

            if libc::ioctl(fd, 0x405c5503 /* UI_DEV_SETUP */, &setup) < 0 {
                bail!("ioctl UI_DEV_SETUP failed");
            }
            if libc::ioctl(fd, 0x5501 /* UI_DEV_CREATE */) < 0 {
                bail!("ioctl UI_DEV_CREATE failed");
            }
        }

        thread::sleep(Duration::from_millis(150));
        Ok(Self { file })
    }

    pub fn send_key(&mut self, code: u16, value: i32) -> Result<()> {
        let fd = self.file.as_raw_fd();
        let ev = InputEvent {
            time: libc::timeval {
                tv_sec: 0,
                tv_usec: 0,
            },
            type_: EV_KEY,
            code,
            value,
        };
        let syn = InputEvent {
            time: libc::timeval {
                tv_sec: 0,
                tv_usec: 0,
            },
            type_: EV_SYN,
            code: SYN_REPORT,
            value: 0,
        };
        unsafe {
            let ev_ptr = &ev as *const _ as *const libc::c_void;
            let syn_ptr = &syn as *const _ as *const libc::c_void;
            if libc::write(fd, ev_ptr, std::mem::size_of::<InputEvent>()) < 0 {
                bail!("write key event failed");
            }
            if libc::write(fd, syn_ptr, std::mem::size_of::<InputEvent>()) < 0 {
                bail!("write syn event failed");
            }
        }
        Ok(())
    }

    pub fn tap_key(&mut self, code: u16) -> Result<()> {
        self.send_key(code, 1)?;
        thread::sleep(Duration::from_millis(20));
        self.send_key(code, 0)?;
        Ok(())
    }
}

impl Drop for VirtualKeyboard {
    fn drop(&mut self) {
        unsafe {
            libc::ioctl(self.file.as_raw_fd(), 0x5502 /* UI_DEV_DESTROY */);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwipeDirection {
    Right,
    Left,
    Release,
}

#[derive(Debug, Clone, Copy, Default)]
struct Slot {
    active: bool,
    x: i32,
    y: i32,
}

#[derive(Debug)]
pub struct GestureTracker {
    slots: [Slot; MAX_SLOTS],
    current_slot: usize,
    quadtap: bool,
    threshold: i32,

    // Gesture tracking state
    pub in_gesture: bool,
    start_x: i32,
    start_y: i32,
    last_trigger_x: i32,
    pub swiped: bool,
    pub last_four_finger_time: Option<Instant>,
}

impl Default for GestureTracker {
    fn default() -> Self {
        Self::new(DEFAULT_THRESHOLD)
    }
}

impl GestureTracker {
    #[must_use]
    pub fn new(threshold: i32) -> Self {
        Self {
            slots: [Slot::default(); MAX_SLOTS],
            current_slot: 0,
            quadtap: false,
            threshold: threshold.max(100),
            in_gesture: false,
            start_x: 0,
            start_y: 0,
            last_trigger_x: 0,
            swiped: false,
            last_four_finger_time: None,
        }
    }

    pub fn handle_event(&mut self, event: &InputEvent, now: Instant) -> Option<SwipeDirection> {
        match event.type_ {
            EV_ABS => {
                match event.code {
                    ABS_MT_SLOT => {
                        let slot = event.value as usize;
                        if slot < MAX_SLOTS {
                            self.current_slot = slot;
                        }
                    }
                    ABS_MT_TRACKING_ID => {
                        self.slots[self.current_slot].active = event.value >= 0;
                    }
                    ABS_MT_POSITION_X => {
                        self.slots[self.current_slot].x = event.value;
                    }
                    ABS_MT_POSITION_Y => {
                        self.slots[self.current_slot].y = event.value;
                    }
                    _ => {}
                }
                None
            }
            EV_KEY => {
                if event.code == BTN_TOOL_QUADTAP {
                    self.quadtap = event.value > 0;
                }
                None
            }
            EV_SYN if event.code == SYN_REPORT => self.on_sync(now),
            _ => None,
        }
    }

    fn on_sync(&mut self, now: Instant) -> Option<SwipeDirection> {
        let active_count = self.slots.iter().filter(|s| s.active).count();
        let is_four_fingers = active_count == 4 || (self.quadtap && active_count >= 3);

        if is_four_fingers {
            self.last_four_finger_time = Some(now);

            let (sum_x, sum_y, count) = self
                .slots
                .iter()
                .filter(|s| s.active)
                .fold((0i64, 0i64, 0i64), |(sx, sy, c), s| {
                    (sx + s.x as i64, sy + s.y as i64, c + 1)
                });

            if count > 0 {
                let avg_x = (sum_x / count) as i32;
                let avg_y = (sum_y / count) as i32;

                if !self.in_gesture {
                    self.in_gesture = true;
                    self.start_x = avg_x;
                    self.start_y = avg_y;
                    self.last_trigger_x = avg_x;
                    self.swiped = false;
                    None
                } else {
                    let total_dx = avg_x - self.start_x;
                    let total_dy = avg_y - self.start_y;
                    let step_dx = avg_x - self.last_trigger_x;

                    // Only consider predominantly horizontal swipes to preserve COSMIC's
                    // 4-finger vertical swipe gestures for workspace switching.
                    if total_dx.abs() > (total_dy.abs() * 5 / 4) {
                        if step_dx > self.threshold {
                            self.last_trigger_x = avg_x;
                            self.swiped = true;
                            Some(SwipeDirection::Right)
                        } else if step_dx < -self.threshold {
                            self.last_trigger_x = avg_x;
                            self.swiped = true;
                            Some(SwipeDirection::Left)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                }
            } else {
                None
            }
        } else {
            // Check contact grace period before declaring gesture ended
            if self.in_gesture {
                if let Some(last_seen) = self.last_four_finger_time
                    && now.duration_since(last_seen) < CONTACT_GRACE
                {
                    return None;
                }
                let was_swiped = self.swiped;
                self.in_gesture = false;
                self.swiped = false;
                self.last_four_finger_time = None;
                if was_swiped {
                    return Some(SwipeDirection::Release);
                }
            }
            None
        }
    }

    /// Checks if contact grace expired while no events arrived.
    pub fn check_timeout(&mut self, now: Instant) -> Option<SwipeDirection> {
        if self.in_gesture
            && let Some(last_seen) = self.last_four_finger_time
            && now.duration_since(last_seen) >= CONTACT_GRACE
        {
            let was_swiped = self.swiped;
            self.in_gesture = false;
            self.swiped = false;
            self.last_four_finger_time = None;
            if was_swiped {
                return Some(SwipeDirection::Release);
            }
        }
        None
    }
}

/// Runs the four-finger swipe monitor service.
pub fn watch() -> Result<()> {
    eprintln!("cosmic-macos-setup four-finger-swipe-watch starting");

    let mut keyboard = VirtualKeyboard::open().context("open virtual keyboard on /dev/uinput")?;
    let event_size = std::mem::size_of::<InputEvent>();
    let modifier_key = detect_switcher_modifier();
    eprintln!("detected switcher modifier keycode: {modifier_key}");

    loop {
        let Some(device_path) = find_touchpad_device() else {
            eprintln!("no touchpad found; retrying in 2 seconds...");
            thread::sleep(Duration::from_secs(2));
            continue;
        };

        eprintln!("monitoring touchpad: {}", device_path.display());

        let mut file = match File::open(&device_path) {
            Ok(f) => f,
            Err(e) => {
                eprintln!(
                    "failed to open {}: {e}; retrying in 2s...",
                    device_path.display()
                );
                thread::sleep(Duration::from_secs(2));
                continue;
            }
        };

        let fd = file.as_raw_fd();
        let width = query_abs_width(fd).unwrap_or(4000);
        let threshold = (width / 10).clamp(250, 500);
        let mut tracker = GestureTracker::new(threshold);

        let mut modifier_held = false;
        let mut pending_release = false;
        let mut release_time = Instant::now();

        let mut buf = vec![0u8; event_size * 32];
        loop {
            let timeout_ms = if modifier_held && pending_release {
                let remaining = release_time.saturating_duration_since(Instant::now());
                (remaining.as_millis().min(1000) as libc::c_int).max(1)
            } else if tracker.in_gesture && tracker.last_four_finger_time.is_some() {
                150
            } else {
                -1
            };

            let mut pfd = libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            };

            let poll_res = unsafe { libc::poll(&mut pfd, 1, timeout_ms) };
            let now = Instant::now();

            if poll_res == 0 {
                // Timeout fired
                if modifier_held && pending_release && now >= release_time {
                    let _ = keyboard.send_key(modifier_key, 0);
                    modifier_held = false;
                    pending_release = false;
                }
                if let Some(SwipeDirection::Release) = tracker.check_timeout(now)
                    && modifier_held
                {
                    pending_release = true;
                    release_time = now + RELEASE_GRACE;
                }
                continue;
            } else if poll_res < 0 {
                eprintln!("poll error on touchpad device; re-detecting in 1s...");
                break;
            }

            let bytes_read = match file.read(&mut buf) {
                Ok(0) => {
                    eprintln!("device closed (EOF); re-detecting in 1s...");
                    break;
                }
                Ok(n) => n,
                Err(e) => {
                    eprintln!("device read error: {e}; re-detecting in 1s...");
                    break;
                }
            };

            let count = bytes_read / event_size;
            for i in 0..count {
                let offset = i * event_size;
                let event = unsafe {
                    std::ptr::read_unaligned(buf[offset..].as_ptr() as *const InputEvent)
                };

                if let Some(direction) = tracker.handle_event(&event, now) {
                    match direction {
                        SwipeDirection::Right => {
                            if !modifier_held {
                                let _ = keyboard.send_key(modifier_key, 1);
                                modifier_held = true;
                                thread::sleep(Duration::from_millis(25));
                            }
                            let _ = keyboard.tap_key(KEY_TAB);
                            pending_release = false;
                        }
                        SwipeDirection::Left => {
                            if !modifier_held {
                                let _ = keyboard.send_key(modifier_key, 1);
                                modifier_held = true;
                                thread::sleep(Duration::from_millis(25));
                            }
                            let _ = keyboard.send_key(KEY_LEFTSHIFT, 1);
                            thread::sleep(Duration::from_millis(15));
                            let _ = keyboard.tap_key(KEY_TAB);
                            let _ = keyboard.send_key(KEY_LEFTSHIFT, 0);
                            pending_release = false;
                        }
                        SwipeDirection::Release => {
                            if modifier_held {
                                pending_release = true;
                                release_time = now + RELEASE_GRACE;
                            }
                        }
                    }
                }
            }
        }

        if modifier_held {
            let _ = keyboard.send_key(modifier_key, 0);
        }

        thread::sleep(Duration::from_secs(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_missing_prerequisites_properly() {
        assert!(missing_prerequisites(true, true).is_empty());
        assert_eq!(missing_prerequisites(false, false).len(), 2);
        assert_eq!(missing_prerequisites(true, false).len(), 1);
        assert_eq!(missing_prerequisites(false, true).len(), 1);
    }

    #[test]
    fn unit_runs_the_watcher() {
        let contents = unit_contents(Path::new("/usr/bin/cosmic-macos-setup"));
        assert!(contents.contains("ExecStart=/usr/bin/cosmic-macos-setup four-finger-swipe-watch"));
        assert!(contents.contains("PartOf=graphical-session.target"));
    }

    #[test]
    fn ignores_one_to_three_fingers() {
        let mut tracker = GestureTracker::new(200);
        let now = Instant::now();

        // Put 3 fingers down
        for slot in 0..3 {
            tracker.handle_event(
                &InputEvent {
                    type_: EV_ABS,
                    code: ABS_MT_SLOT,
                    value: slot,
                    ..Default::default()
                },
                now,
            );
            tracker.handle_event(
                &InputEvent {
                    type_: EV_ABS,
                    code: ABS_MT_TRACKING_ID,
                    value: 10 + slot,
                    ..Default::default()
                },
                now,
            );
            tracker.handle_event(
                &InputEvent {
                    type_: EV_ABS,
                    code: ABS_MT_POSITION_X,
                    value: 1000,
                    ..Default::default()
                },
                now,
            );
        }
        assert_eq!(
            tracker.handle_event(
                &InputEvent {
                    type_: EV_SYN,
                    code: SYN_REPORT,
                    value: 0,
                    ..Default::default()
                },
                now,
            ),
            None
        );

        // Move 3 fingers by 500 units
        for slot in 0..3 {
            tracker.handle_event(
                &InputEvent {
                    type_: EV_ABS,
                    code: ABS_MT_SLOT,
                    value: slot,
                    ..Default::default()
                },
                now,
            );
            tracker.handle_event(
                &InputEvent {
                    type_: EV_ABS,
                    code: ABS_MT_POSITION_X,
                    value: 1500,
                    ..Default::default()
                },
                now,
            );
        }
        assert_eq!(
            tracker.handle_event(
                &InputEvent {
                    type_: EV_SYN,
                    code: SYN_REPORT,
                    value: 0,
                    ..Default::default()
                },
                now,
            ),
            None
        );
    }

    #[test]
    fn four_finger_swipe_right_and_release() {
        let mut tracker = GestureTracker::new(200);
        let now = Instant::now();

        // Put 4 fingers down at x=1000, y=1000
        for slot in 0..4 {
            tracker.handle_event(
                &InputEvent {
                    type_: EV_ABS,
                    code: ABS_MT_SLOT,
                    value: slot,
                    ..Default::default()
                },
                now,
            );
            tracker.handle_event(
                &InputEvent {
                    type_: EV_ABS,
                    code: ABS_MT_TRACKING_ID,
                    value: 20 + slot,
                    ..Default::default()
                },
                now,
            );
            tracker.handle_event(
                &InputEvent {
                    type_: EV_ABS,
                    code: ABS_MT_POSITION_X,
                    value: 1000,
                    ..Default::default()
                },
                now,
            );
            tracker.handle_event(
                &InputEvent {
                    type_: EV_ABS,
                    code: ABS_MT_POSITION_Y,
                    value: 1000,
                    ..Default::default()
                },
                now,
            );
        }
        assert_eq!(
            tracker.handle_event(
                &InputEvent {
                    type_: EV_SYN,
                    code: SYN_REPORT,
                    value: 0,
                    ..Default::default()
                },
                now,
            ),
            None
        );

        // Move 4 fingers to x=1300 (+300 units > 200 threshold)
        for slot in 0..4 {
            tracker.handle_event(
                &InputEvent {
                    type_: EV_ABS,
                    code: ABS_MT_SLOT,
                    value: slot,
                    ..Default::default()
                },
                now,
            );
            tracker.handle_event(
                &InputEvent {
                    type_: EV_ABS,
                    code: ABS_MT_POSITION_X,
                    value: 1300,
                    ..Default::default()
                },
                now,
            );
        }
        let dir = tracker.handle_event(
            &InputEvent {
                type_: EV_SYN,
                code: SYN_REPORT,
                value: 0,
                ..Default::default()
            },
            now,
        );
        assert_eq!(dir, Some(SwipeDirection::Right));

        // Lift fingers (tracking_id = -1) after contact grace
        let lift_time = now + CONTACT_GRACE + Duration::from_millis(10);
        for slot in 0..4 {
            tracker.handle_event(
                &InputEvent {
                    type_: EV_ABS,
                    code: ABS_MT_SLOT,
                    value: slot,
                    ..Default::default()
                },
                lift_time,
            );
            tracker.handle_event(
                &InputEvent {
                    type_: EV_ABS,
                    code: ABS_MT_TRACKING_ID,
                    value: -1,
                    ..Default::default()
                },
                lift_time,
            );
        }
        let rel = tracker.handle_event(
            &InputEvent {
                type_: EV_SYN,
                code: SYN_REPORT,
                value: 0,
                ..Default::default()
            },
            lift_time,
        );
        assert_eq!(rel, Some(SwipeDirection::Release));
    }

    #[test]
    fn four_finger_swipe_left_and_release() {
        let mut tracker = GestureTracker::new(200);
        let now = Instant::now();

        // Put 4 fingers down at x=2000, y=1000
        for slot in 0..4 {
            tracker.handle_event(
                &InputEvent {
                    type_: EV_ABS,
                    code: ABS_MT_SLOT,
                    value: slot,
                    ..Default::default()
                },
                now,
            );
            tracker.handle_event(
                &InputEvent {
                    type_: EV_ABS,
                    code: ABS_MT_TRACKING_ID,
                    value: 30 + slot,
                    ..Default::default()
                },
                now,
            );
            tracker.handle_event(
                &InputEvent {
                    type_: EV_ABS,
                    code: ABS_MT_POSITION_X,
                    value: 2000,
                    ..Default::default()
                },
                now,
            );
            tracker.handle_event(
                &InputEvent {
                    type_: EV_ABS,
                    code: ABS_MT_POSITION_Y,
                    value: 1000,
                    ..Default::default()
                },
                now,
            );
        }
        tracker.handle_event(
            &InputEvent {
                type_: EV_SYN,
                code: SYN_REPORT,
                value: 0,
                ..Default::default()
            },
            now,
        );

        // Move 4 fingers to x=1700 (-300 units)
        for slot in 0..4 {
            tracker.handle_event(
                &InputEvent {
                    type_: EV_ABS,
                    code: ABS_MT_SLOT,
                    value: slot,
                    ..Default::default()
                },
                now,
            );
            tracker.handle_event(
                &InputEvent {
                    type_: EV_ABS,
                    code: ABS_MT_POSITION_X,
                    value: 1700,
                    ..Default::default()
                },
                now,
            );
        }
        let dir = tracker.handle_event(
            &InputEvent {
                type_: EV_SYN,
                code: SYN_REPORT,
                value: 0,
                ..Default::default()
            },
            now,
        );
        assert_eq!(dir, Some(SwipeDirection::Left));

        // Lift fingers after contact grace
        let lift_time = now + CONTACT_GRACE + Duration::from_millis(10);
        for slot in 0..4 {
            tracker.handle_event(
                &InputEvent {
                    type_: EV_ABS,
                    code: ABS_MT_SLOT,
                    value: slot,
                    ..Default::default()
                },
                lift_time,
            );
            tracker.handle_event(
                &InputEvent {
                    type_: EV_ABS,
                    code: ABS_MT_TRACKING_ID,
                    value: -1,
                    ..Default::default()
                },
                lift_time,
            );
        }
        let rel = tracker.handle_event(
            &InputEvent {
                type_: EV_SYN,
                code: SYN_REPORT,
                value: 0,
                ..Default::default()
            },
            lift_time,
        );
        assert_eq!(rel, Some(SwipeDirection::Release));
    }

    #[test]
    fn vertical_swipes_are_ignored() {
        let mut tracker = GestureTracker::new(200);
        let now = Instant::now();

        // Put 4 fingers down at x=1000, y=1000
        for slot in 0..4 {
            tracker.handle_event(
                &InputEvent {
                    type_: EV_ABS,
                    code: ABS_MT_SLOT,
                    value: slot,
                    ..Default::default()
                },
                now,
            );
            tracker.handle_event(
                &InputEvent {
                    type_: EV_ABS,
                    code: ABS_MT_TRACKING_ID,
                    value: 40 + slot,
                    ..Default::default()
                },
                now,
            );
            tracker.handle_event(
                &InputEvent {
                    type_: EV_ABS,
                    code: ABS_MT_POSITION_X,
                    value: 1000,
                    ..Default::default()
                },
                now,
            );
            tracker.handle_event(
                &InputEvent {
                    type_: EV_ABS,
                    code: ABS_MT_POSITION_Y,
                    value: 1000,
                    ..Default::default()
                },
                now,
            );
        }
        tracker.handle_event(
            &InputEvent {
                type_: EV_SYN,
                code: SYN_REPORT,
                value: 0,
                ..Default::default()
            },
            now,
        );

        // Move primarily in Y: y=1600 (dy=600), x=1050 (dx=50)
        for slot in 0..4 {
            tracker.handle_event(
                &InputEvent {
                    type_: EV_ABS,
                    code: ABS_MT_SLOT,
                    value: slot,
                    ..Default::default()
                },
                now,
            );
            tracker.handle_event(
                &InputEvent {
                    type_: EV_ABS,
                    code: ABS_MT_POSITION_X,
                    value: 1050,
                    ..Default::default()
                },
                now,
            );
            tracker.handle_event(
                &InputEvent {
                    type_: EV_ABS,
                    code: ABS_MT_POSITION_Y,
                    value: 1600,
                    ..Default::default()
                },
                now,
            );
        }
        let dir = tracker.handle_event(
            &InputEvent {
                type_: EV_SYN,
                code: SYN_REPORT,
                value: 0,
                ..Default::default()
            },
            now,
        );
        assert_eq!(dir, None);
    }
}
