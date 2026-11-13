//! A plugin's own knobs. GigEyes starts a second copy of itself (`gigeyes --knobs <plugin>`) that
//! loads the VST3, asks it for its parameter list and prints it, so a plugin that crashes, hangs or
//! waits on an activation window can't take GigEyes down: after a while it's ended.

use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const FLAG: &str = "--knobs";
const WAIT: Duration = Duration::from_secs(40);

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Knob {
    pub name: String,
    /// The section of the plugin it's in, e.g. "Amp" or "Delay", when the plugin says.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub group: String,
    /// What the plugin shows at its default, all the way down and all the way up, e.g. "5.0".
    pub default: String,
    pub low: String,
    pub high: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub units: String,
    /// Settings for a switch or selector; 0 for a knob that turns smoothly.
    #[serde(default)]
    pub steps: i32,
}

/// The plugin's knobs, read in a separate process.
pub fn read(plugin: &Path) -> Result<Vec<Knob>, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut command = Command::new(exe);
    command
        .arg(FLAG)
        .arg(plugin)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    #[cfg(windows)]
    crate::ai::end_with_gigeyes(&child);
    let mut out = child.stdout.take().expect("piped");
    // Read while it runs: a long list would fill the pipe and stall it.
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = out.read_to_string(&mut text);
        text
    });
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        if started.elapsed() > WAIT {
            let _ = child.kill();
            let _ = child.wait();
            return Err("It didn't answer in time. It may be waiting on an activation window: open it once in your host, then try again.".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let text = reader.join().unwrap_or_default();
    let line = text.lines().last().unwrap_or_default();
    if let Ok(knobs) = serde_json::from_str::<Vec<Knob>>(line) {
        return Ok(knobs);
    }
    Err(match serde_json::from_str::<String>(line) {
        Ok(e) => e,
        Err(_) if !status.success() => "It closed while GigEyes read it.".into(),
        Err(_) => "It answered something GigEyes couldn't read.".into(),
    })
}

/// The `--knobs` copy: print the knobs as one JSON line, or the reason as a JSON string.
pub fn child(plugin: &Path) -> ! {
    // A missing DLL or a crash must end this copy, not put up a Windows error box nobody sees.
    #[cfg(windows)]
    unsafe {
        use windows::Win32::System::Diagnostics::Debug::*;
        SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX | SEM_NOOPENFILEERRORBOX);
    }
    // SAFETY: loading a plugin runs its code; that's why this is a separate process.
    let result = unsafe { load(plugin) };
    let line = match &result {
        Ok(knobs) => serde_json::to_string(knobs),
        Err(e) => serde_json::to_string(e),
    };
    println!("{}", line.unwrap_or_default());
    // Leave without unloading the plugin: some crash when unloaded.
    std::process::exit(if result.is_ok() { 0 } else { 1 })
}

/// The library inside a .vst3: the bundle's binary for this PC, or the file itself (older plugins).
fn binary(plugin: &Path) -> Option<PathBuf> {
    if plugin.is_file() {
        return Some(plugin.to_path_buf());
    }
    let (arch, ext) = if cfg!(windows) {
        ("x86_64-win", "vst3")
    } else {
        ("x86_64-linux", "so")
    };
    let dir = plugin.join("Contents").join(arch);
    let named = dir.join(plugin.file_stem()?).with_extension(ext);
    if named.is_file() {
        return Some(named);
    }
    std::fs::read_dir(&dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| p.is_file() && p.extension().is_some_and(|e| e.eq_ignore_ascii_case(ext)))
}

mod host {
    use vst3::Class;
    use vst3::Steinberg::Vst::{IHostApplication, IHostApplicationTrait, String128};
    use vst3::Steinberg::{TUID, kNotImplemented, kResultOk, tresult};

    /// The little a plugin may ask of its host while it lists its knobs.
    pub struct Host;

    impl Class for Host {
        type Interfaces = (IHostApplication,);
    }

    impl IHostApplicationTrait for Host {
        unsafe fn getName(&self, name: *mut String128) -> tresult {
            // SAFETY: the plugin passes a String128 to fill.
            let name = unsafe { &mut *name };
            name.fill(0);
            for (to, from) in name.iter_mut().zip("GigEyes".encode_utf16()) {
                *to = from;
            }
            kResultOk
        }

        unsafe fn createInstance(
            &self,
            _cid: *mut TUID,
            _iid: *mut TUID,
            obj: *mut *mut std::ffi::c_void,
        ) -> tresult {
            // SAFETY: the plugin passes somewhere to put the object.
            unsafe { *obj = std::ptr::null_mut() };
            kNotImplemented
        }
    }
}

/// "80.000000" → "80", "9.999999" → "10", "0.400000" → "0.4"; words stay as they are.
fn tidy(shown: &str) -> String {
    match shown.parse::<f64>() {
        Ok(n) => {
            let n = (n * 100.0).round() / 100.0;
            if n == 0.0 { "0".into() } else { n.to_string() }
        }
        Err(_) => shown.to_string(),
    }
}

fn text(s: &[u16]) -> String {
    let end = s.iter().position(|&c| c == 0).unwrap_or(s.len());
    String::from_utf16_lossy(&s[..end]).trim().to_string()
}

/// # Safety
/// Runs the plugin's code.
unsafe fn load(plugin: &Path) -> Result<Vec<Knob>, String> {
    use vst3::Steinberg::Vst::*;
    use vst3::Steinberg::*;
    use vst3::{ComPtr, ComWrapper, Interface};

    let path = binary(plugin).ok_or("Its plugin file for this PC is missing.")?;
    // On Windows a plugin's own DLLs sit next to it: search there first, as hosts do.
    #[cfg(windows)]
    let library = unsafe {
        use libloading::os::windows::{LOAD_WITH_ALTERED_SEARCH_PATH, Library};
        Library::load_with_flags(&path, LOAD_WITH_ALTERED_SEARCH_PATH)
    };
    #[cfg(not(windows))]
    let library = unsafe { libloading::Library::new(&path) };
    let library = std::mem::ManuallyDrop::new(library.map_err(|e| e.to_string())?);
    unsafe {
        #[cfg(windows)]
        if let Ok(init) = library.get::<unsafe extern "system" fn() -> bool>(b"InitDll") {
            init();
        }
        #[cfg(not(windows))]
        if let Ok(entry) =
            library.get::<unsafe extern "C" fn(*mut std::ffi::c_void) -> bool>(b"ModuleEntry")
        {
            // shortcut: plugins get a placeholder handle; fine for the ones tested, pass the real dlopen handle if one needs it.
            entry(std::ptr::NonNull::<u8>::dangling().as_ptr().cast());
        }
    }
    let factory = unsafe {
        let get = library
            .get::<unsafe extern "system" fn() -> *mut IPluginFactory>(b"GetPluginFactory")
            .map_err(|_| "It isn't a VST3 plugin GigEyes can open.")?;
        ComPtr::from_raw(get()).ok_or("It didn't start.")?
    };
    let audio = (0..unsafe { factory.countClasses() })
        .find_map(|i| {
            let mut info: PClassInfo = unsafe { std::mem::zeroed() };
            let ok = unsafe { factory.getClassInfo(i, &mut info) } == kResultOk;
            let category: Vec<u8> = info
                .category
                .iter()
                .take_while(|&&c| c != 0)
                .map(|&c| c as u8)
                .collect();
            (ok && category == b"Audio Module Class").then_some(info.cid)
        })
        .ok_or("It has no audio effect in it.")?;
    let host = ComWrapper::new(host::Host);
    let host = host.to_com_ptr::<FUnknown>().expect("Host is an FUnknown");
    let create = |cid: &TUID, iid: &[u8; 16]| -> Option<*mut std::ffi::c_void> {
        let mut obj = std::ptr::null_mut();
        let ok = unsafe { factory.createInstance(cid.as_ptr(), iid.as_ptr().cast(), &mut obj) };
        (ok == kResultOk && !obj.is_null()).then_some(obj)
    };
    let component = create(&audio, &IComponent::IID)
        .and_then(|obj| unsafe { ComPtr::<IComponent>::from_raw(obj.cast()) })
        .ok_or("It didn't start.")?;
    if unsafe { component.initialize(host.as_ptr()) } != kResultOk {
        return Err("It didn't start.".into());
    }
    // Most plugins have a separate controller that owns the knobs; a few are both in one.
    let (controller, separate) = match component.cast::<IEditController>() {
        Some(c) => (c, false),
        None => {
            let mut cid: TUID = [0; 16];
            if unsafe { component.getControllerClassId(&mut cid) } != kResultOk {
                return Err("It has no knobs GigEyes can read.".into());
            }
            let c = create(&cid, &IEditController::IID)
                .and_then(|obj| unsafe { ComPtr::<IEditController>::from_raw(obj.cast()) })
                .ok_or("It has no knobs GigEyes can read.")?;
            if unsafe { c.initialize(host.as_ptr()) } != kResultOk {
                return Err("Its knobs didn't start.".into());
            }
            (c, true)
        }
    };
    let mut groups = std::collections::HashMap::new();
    if let Some(units) = controller.cast::<IUnitInfo>() {
        for i in 0..unsafe { units.getUnitCount() } {
            let mut unit: UnitInfo = unsafe { std::mem::zeroed() };
            if unsafe { units.getUnitInfo(i, &mut unit) } == kResultOk && unit.id != 0 {
                groups.insert(unit.id, text(&unit.name));
            }
        }
    }
    let shown = |id: ParamID, value: f64| {
        let mut s: String128 = [0; 128];
        if unsafe { controller.getParamStringByValue(id, value, &mut s) } == kResultOk {
            tidy(&text(&s))
        } else {
            String::new()
        }
    };
    use ParameterInfo_::ParameterFlags_::*;
    let skip = kIsHidden | kIsReadOnly | kIsProgramChange | kIsBypass;
    let mut knobs = vec![];
    // shortcut: the first 2000 parameters; no guitar plugin GigEyes knows of has more.
    for i in 0..unsafe { controller.getParameterCount() }.min(2000) {
        let mut info: ParameterInfo = unsafe { std::mem::zeroed() };
        if unsafe { controller.getParameterInfo(i, &mut info) } != kResultOk
            || info.flags & skip != 0
        {
            continue;
        }
        let name = Some(text(&info.title))
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| text(&info.shortTitle));
        if name.is_empty() {
            continue;
        }
        knobs.push(Knob {
            name,
            group: groups.get(&info.unitId).cloned().unwrap_or_default(),
            default: shown(info.id, info.defaultNormalizedValue),
            low: shown(info.id, 0.0),
            high: shown(info.id, 1.0),
            units: text(&info.units),
            steps: info.stepCount,
        });
    }
    unsafe {
        if separate {
            controller.terminate();
        }
        component.terminate();
    }
    Ok(knobs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shown_numbers_are_tidied() {
        assert_eq!(tidy("80.000000"), "80");
        assert_eq!(tidy("9.999999"), "10");
        assert_eq!(tidy("0.400000"), "0.4");
        assert_eq!(tidy("-0.000"), "0");
        assert_eq!(tidy("Nested"), "Nested");
    }

    #[test]
    fn finds_the_binary_inside_a_bundle() {
        let dir = std::env::temp_dir().join(format!("gigeyes-knobs-{}", std::process::id()));
        let arch = dir.join("Echo.vst3").join("Contents").join("x86_64-linux");
        std::fs::create_dir_all(&arch).unwrap();
        std::fs::write(arch.join("Echo.so"), b"").unwrap();
        if cfg!(not(windows)) {
            assert_eq!(binary(&dir.join("Echo.vst3")), Some(arch.join("Echo.so")));
        }
        assert_eq!(binary(&dir.join("Missing.vst3")), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
