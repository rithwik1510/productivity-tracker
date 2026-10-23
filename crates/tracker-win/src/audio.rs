//! "Is the foreground app making sound?" from WASAPI sessions on every active render endpoint.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use tracker_api::AppIdentity;
use windows::Win32::Foundation::S_OK;
use windows::Win32::Media::Audio::Endpoints::{IAudioEndpointVolume, IAudioMeterInformation};
use windows::Win32::Media::Audio::{
    DEVICE_STATE_ACTIVE, IAudioSessionControl2, IAudioSessionManager2, IMMDevice,
    IMMDeviceEnumerator, ISimpleAudioVolume, MMDeviceEnumerator, eRender,
};
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
use windows::core::{Interface, Result};

use crate::process;

/// Endpoints, sessions and the process snapshot are re-acquired this often: a session
/// enumerator is a snapshot and never shows sessions created after it.
const REFRESH: Duration = Duration::from_secs(10);
/// Peak meter level above which a session is making sound.
const PEAK_THRESHOLD: f32 = 0.001;

struct Session {
    pid: u32,
    meter: IAudioMeterInformation,
    volume: ISimpleAudioVolume,
}

struct Endpoint {
    volume: IAudioEndpointVolume,
    sessions: Vec<Session>,
}

struct Snapshot {
    endpoints: Vec<Endpoint>,
    parents: HashMap<u32, u32>,
    taken: Instant,
}

/// Holds COM objects, so it lives on the thread that created it.
pub(crate) struct AudioMonitor {
    snapshot: Option<Snapshot>,
}

impl AudioMonitor {
    pub(crate) fn new() -> Self {
        Self { snapshot: None }
    }

    /// True if a session that is actually heard belongs to `pid`, a descendant of it (WebView2
    /// call audio), or any process with the same identity (Chrome's audio utility process).
    pub(crate) fn audible(&mut self, pid: u32, identity: &AppIdentity) -> bool {
        let snapshot = match self.snapshot.take() {
            Some(snapshot) if snapshot.taken.elapsed() < REFRESH => snapshot,
            _ => acquire(),
        };
        match snapshot.audible(pid, identity) {
            Ok(audible) => {
                self.snapshot = Some(snapshot);
                audible
            }
            Err(err) => {
                // Usually a device change (unplugged headset). Re-acquire on the next tick.
                tracing::debug!("audio session read failed: {err}");
                false
            }
        }
    }
}

impl Snapshot {
    fn audible(&self, pid: u32, identity: &AppIdentity) -> Result<bool> {
        for endpoint in &self.endpoints {
            // SAFETY: COM calls on interfaces created on this thread.
            let silent_endpoint = unsafe {
                endpoint.volume.GetMute()?.as_bool()
                    || endpoint.volume.GetMasterVolumeLevelScalar()? <= 0.0
            };
            if silent_endpoint {
                continue;
            }
            for session in &endpoint.sessions {
                // The peak is measured before session and endpoint volume, hence both mute checks.
                // SAFETY: as above.
                let heard = unsafe {
                    session.meter.GetPeakValue()? > PEAK_THRESHOLD
                        && !session.volume.GetMute()?.as_bool()
                };
                if heard && self.belongs_to(session.pid, pid, identity) {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    fn belongs_to(&self, session_pid: u32, pid: u32, identity: &AppIdentity) -> bool {
        session_pid == pid
            || process::descends_from(&self.parents, session_pid, pid)
            || process::identity(session_pid).as_ref() == Some(identity)
    }
}

/// A fresh snapshot. Failures leave it empty (nothing audible) until the next refresh, so a
/// broken audio stack costs one attempt per `REFRESH`, not one per tick.
fn acquire() -> Snapshot {
    let endpoints = endpoints().unwrap_or_else(|err| {
        tracing::debug!("audio endpoint enumeration failed: {err}");
        Vec::new()
    });
    Snapshot {
        endpoints,
        parents: process::parent_map(),
        taken: Instant::now(),
    }
}

fn endpoints() -> Result<Vec<Endpoint>> {
    // SAFETY: COM is initialized on this thread (the sampler owns an `Apartment`).
    let enumerator: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }?;
    // SAFETY: COM calls on interfaces created on this thread.
    let devices = unsafe { enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) }?;
    let mut endpoints = Vec::new();
    // SAFETY: as above.
    for i in 0..unsafe { devices.GetCount() }? {
        // SAFETY: `i` is below the collection's count.
        let device = unsafe { devices.Item(i) }?;
        match endpoint(&device) {
            Ok(endpoint) => endpoints.push(endpoint),
            Err(err) => tracing::debug!("skipping audio endpoint: {err}"),
        }
    }
    Ok(endpoints)
}

fn endpoint(device: &IMMDevice) -> Result<Endpoint> {
    // SAFETY: COM calls on interfaces created on this thread; no activation params needed.
    let (volume, manager) = unsafe {
        (
            device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None)?,
            device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None)?,
        )
    };
    // SAFETY: as above.
    let list = unsafe { manager.GetSessionEnumerator() }?;
    let mut sessions = Vec::new();
    // SAFETY: as above.
    for i in 0..unsafe { list.GetCount() }? {
        // SAFETY: `i` is below the enumerator's count.
        let Ok(control) = (unsafe { list.GetSession(i) }) else {
            continue;
        };
        let Ok(control) = control.cast::<IAudioSessionControl2>() else {
            continue;
        };
        // SAFETY: as above. S_OK means "this is the system sounds session".
        if unsafe { control.IsSystemSoundsSession() } == S_OK {
            continue;
        }
        // AUDCLNT_S_NO_SINGLE_PROCESS is a success code: the PID is still the session's creator.
        // SAFETY: as above.
        let Ok(pid) = (unsafe { control.GetProcessId() }) else {
            continue;
        };
        if let (Ok(meter), Ok(volume)) = (control.cast(), control.cast()) {
            sessions.push(Session { pid, meter, volume });
        }
    }
    Ok(Endpoint { volume, sessions })
}
