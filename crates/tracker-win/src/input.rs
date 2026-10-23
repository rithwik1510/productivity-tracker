//! Idle time: keyboard/mouse from `GetLastInputInfo`, plus XInput gamepads, which it doesn't see
//! (a controller-only game would otherwise go Away after 5 min, or hit the 2 h sound cap).

use std::time::{Duration, Instant};

use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::SystemInformation::GetTickCount;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
use windows::Win32::UI::Input::XboxController::{
    XINPUT_GAMEPAD, XINPUT_GAMEPAD_LEFT_THUMB_DEADZONE, XINPUT_GAMEPAD_RIGHT_THUMB_DEADZONE,
    XINPUT_GAMEPAD_TRIGGER_THRESHOLD, XINPUT_STATE, XInputGetState, XUSER_MAX_COUNT,
};

/// Empty controller slots are probed this often. XInput is slow on an empty slot (it looks for a
/// device each time), so only connected slots are read every tick.
const RESCAN: Duration = Duration::from_secs(5);

/// Milliseconds since the last keyboard, mouse or gamepad input. Lives on the poll thread.
pub(crate) struct Idle {
    pads: [Option<XINPUT_GAMEPAD>; XUSER_MAX_COUNT as usize],
    /// Per slot, the packet number of the last read; XInput bumps it on any state change.
    packets: [u32; XUSER_MAX_COUNT as usize],
    rescan_at: Instant,
    /// `GetTickCount` at the tick that saw gamepad input, on the same clock as `GetLastInputInfo`.
    pad_input_tick: Option<u32>,
}

impl Idle {
    pub(crate) fn new() -> Self {
        Self {
            pads: [None; XUSER_MAX_COUNT as usize],
            packets: [0; XUSER_MAX_COUNT as usize],
            rescan_at: Instant::now(),
            pad_input_tick: None,
        }
    }

    pub(crate) fn idle_ms(&mut self) -> u32 {
        // SAFETY: no preconditions.
        let now = unsafe { GetTickCount() };
        if self.poll_pads() {
            self.pad_input_tick = Some(now);
        }
        // Both are 32-bit tick counts that wrap every 49.7 days; wrapping_sub stays correct.
        let keyboard = last_input_tick().map_or(u32::MAX, |tick| now.wrapping_sub(tick));
        self.pad_input_tick
            .map_or(keyboard, |tick| keyboard.min(now.wrapping_sub(tick)))
    }

    /// Reads connected pads (and empty slots every `RESCAN`); true if one was used since the last
    /// read. A pad's first read after connecting is only a baseline, never input.
    fn poll_pads(&mut self) -> bool {
        let rescan = Instant::now() >= self.rescan_at;
        if rescan {
            self.rescan_at = Instant::now() + RESCAN;
        }
        let mut used = false;
        for slot in 0..XUSER_MAX_COUNT {
            let i = slot as usize;
            if self.pads[i].is_none() && !rescan {
                continue;
            }
            let mut state = XINPUT_STATE::default();
            // SAFETY: `state` is a valid out pointer; the slot index is below XUSER_MAX_COUNT.
            if unsafe { XInputGetState(slot, &mut state) } != ERROR_SUCCESS.0 {
                self.pads[i] = None;
                continue;
            }
            if let Some(before) = self.pads[i] {
                used |=
                    state.dwPacketNumber != self.packets[i] && deliberate(&before, &state.Gamepad);
            }
            self.pads[i] = Some(state.Gamepad);
            self.packets[i] = state.dwPacketNumber;
        }
        used
    }
}

/// A changed state is input if a button changed or a stick or trigger is pushed past its dead
/// zone, so the noise of a resting stick (which also bumps the packet number) isn't presence.
fn deliberate(before: &XINPUT_GAMEPAD, now: &XINPUT_GAMEPAD) -> bool {
    let past = |x: i16, y: i16, zone: u16| x.unsigned_abs() > zone || y.unsigned_abs() > zone;
    let trigger = XINPUT_GAMEPAD_TRIGGER_THRESHOLD.0;
    before.wButtons != now.wButtons
        || u16::from(now.bLeftTrigger) > trigger
        || u16::from(now.bRightTrigger) > trigger
        || past(
            now.sThumbLX,
            now.sThumbLY,
            XINPUT_GAMEPAD_LEFT_THUMB_DEADZONE.0,
        )
        || past(
            now.sThumbRX,
            now.sThumbRY,
            XINPUT_GAMEPAD_RIGHT_THUMB_DEADZONE.0,
        )
}

/// `GetTickCount` at the last keyboard or mouse input in this session. `None` on failure, which
/// fails closed: "no input for ages" can only miss time, "input just now" would invent it.
fn last_input_tick() -> Option<u32> {
    let mut info = LASTINPUTINFO {
        cbSize: size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    // SAFETY: `info.cbSize` is set as the API requires.
    unsafe { GetLastInputInfo(&mut info) }
        .as_bool()
        .then_some(info.dwTime)
}
