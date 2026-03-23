/// Opaque data channel — reads SpatialControllerPacket from CloudXR.
/// Simplified port from clear-xr/src/opaque_channel.rs for use in the API layer.

use openxr_sys as xr;
use serde::Deserialize;
use std::ffi::c_char;
use std::path::PathBuf;

macro_rules! opaque_log {
    (warn, $($arg:tt)*) => {{
        let message = format!($($arg)*);
        crate::debug_log(log::Level::Warn, &message);
    }};
    (info, $($arg:tt)*) => {{
        let message = format!($($arg)*);
        crate::debug_log(log::Level::Info, &message);
    }};
}

// ============================================================
// Spatial controller packet (100 bytes, packed, little-endian)
// ============================================================

#[repr(C, packed)]
#[derive(Copy, Clone, Default)]
pub struct SpatialControllerHand {
    pub buttons: u16,
    pub _reserved: u16,
    pub thumbstick_x: f32,
    pub thumbstick_y: f32,
    pub trigger: f32,
    pub grip: f32,
    pub pos_x: f32,
    pub pos_y: f32,
    pub pos_z: f32,
    pub rot_x: f32,
    pub rot_y: f32,
    pub rot_z: f32,
    pub rot_w: f32,
}

#[repr(C, packed)]
#[derive(Copy, Clone, Default)]
pub struct SpatialControllerPacket {
    pub magic: u16,
    pub version: u8,
    pub active_hands: u8,
    pub left: SpatialControllerHand,
    pub right: SpatialControllerHand,
}

const PACKET_MAGIC: u16 = 0x5343;

// Button bitmask
pub const SC_BTN_A: u16            = 1 << 0;
pub const SC_BTN_B: u16            = 1 << 1;
pub const SC_BTN_GRIP: u16         = 1 << 2;
pub const SC_BTN_TRIGGER: u16      = 1 << 3;
pub const SC_BTN_THUMBSTICK: u16   = 1 << 4;
pub const SC_BTN_MENU: u16         = 1 << 5;
pub const SC_TOUCH_A: u16          = 1 << 6;
pub const SC_TOUCH_B: u16          = 1 << 7;
pub const SC_TOUCH_TRIGGER: u16    = 1 << 8;
pub const SC_TOUCH_GRIP: u16       = 1 << 9;
pub const SC_TOUCH_THUMBSTICK: u16 = 1 << 10;


// ============================================================
// Haptic event packet (20 bytes, packed, little-endian)
// Sent PC → headset over the opaque data channel.
// ============================================================

#[repr(C, packed)]
#[derive(Copy, Clone, Debug)]
pub struct HapticEventPacket {
    pub magic: u16,       // 0x4856 ("HV")
    pub version: u8,      // 1
    pub hand: u8,         // 0 = left, 1 = right
    pub duration_ns: u64, // nanoseconds (0 = minimum)
    pub frequency: f32,   // Hz (0 = default)
    pub amplitude: f32,   // 0.0–1.0
}

const HAPTIC_MAGIC: u16 = 0x4856; // "HV"

// ============================================================
// Stream configuration packet (variable length)
// Sent headset → PC over the opaque data channel.
// 6-byte header followed by a UTF-8 JSON payload.
// ============================================================

const CONFIG_MAGIC: u16 = 0x4346; // "CF"
const CONFIG_HEADER_SIZE: usize = 6; // 2 (magic) + 4 (jsonLength)

#[repr(C, packed)]
#[derive(Copy, Clone)]
struct StreamConfigHeader {
    magic: u16,        // 0x4346
    json_length: u32,  // little-endian byte count of trailing JSON
}

/// Parsed stream configuration received from the headset.
#[derive(Clone, Debug, Deserialize)]
pub struct StreamConfig {
    #[serde(rename = "RenderedResolution")]
    pub rendered_resolution: i64,
    #[serde(rename = "EncodedResolution")]
    pub encoded_resolution: i64,
    #[serde(rename = "FoveationInsetRatio")]
    pub foveation_inset_ratio: f64,
    #[serde(rename = "DefaultAppEnabled")]
    pub default_app_enabled: bool,
    #[serde(rename = "AlphaTransparencyEnabled")]
    pub alpha_transparency_enabled: bool,
}

// ============================================================
// FFI types for XR_NV_opaque_data_channel
// ============================================================

const XR_TYPE_CREATE_INFO: u64 = 1000500000;
const XR_TYPE_STATE: u64       = 1000500001;
const STATUS_CONNECTED: i32    = 1;
const STATUS_DISCONNECTED: i32 = 3;

#[repr(C)]
struct XrGuid { data1: u32, data2: u16, data3: u16, data4: [u8; 8] }

#[repr(C)]
struct CreateInfoNV {
    ty: u64,
    next: *const std::ffi::c_void,
    system_id: u64,
    uuid: XrGuid,
}

#[repr(C)]
struct StateNV {
    ty: u64,
    next: *mut std::ffi::c_void,
    state: i32,
}

type FnCreate   = unsafe extern "system" fn(xr::Instance, *const CreateInfoNV, *mut u64) -> xr::Result;
type FnDestroy  = unsafe extern "system" fn(u64) -> xr::Result;
type FnGetState = unsafe extern "system" fn(u64, *mut StateNV) -> xr::Result;
type FnShutdown = unsafe extern "system" fn(u64) -> xr::Result;
type FnSend     = unsafe extern "system" fn(u64, u32, *const u8) -> xr::Result;
type FnReceive  = unsafe extern "system" fn(u64, u32, *mut u32, *mut u8) -> xr::Result;

// ============================================================
// OpaqueChannel
// ============================================================

pub struct OpaqueChannel {
    fn_create: FnCreate,
    fn_destroy: FnDestroy,
    fn_get_state: FnGetState,
    fn_shutdown: FnShutdown,
    fn_send: FnSend,
    fn_receive: FnReceive,
    channel: u64,
    instance: xr::Instance,
    system_id: u64,
    connected: bool,
    recv_buf: [u8; 4096],
    pub latest: Option<SpatialControllerPacket>,
    pub latest_config: Option<StreamConfig>,
    reconnect_after: Option<std::time::Instant>,
}

impl OpaqueChannel {
    /// Load extension function pointers. Returns None if not available.
    pub unsafe fn load(
        get_proc: xr::pfn::GetInstanceProcAddr,
        instance: xr::Instance,
        system_id: u64,
    ) -> Option<Self> {
        let load = |name: &[u8]| -> Option<xr::pfn::VoidFunction> {
            let mut fp: Option<xr::pfn::VoidFunction> = None;
            (get_proc)(instance, name.as_ptr() as *const c_char, &mut fp);
            fp
        };

        Some(Self {
            fn_create:    std::mem::transmute(load(b"xrCreateOpaqueDataChannelNV\0")?),
            fn_destroy:   std::mem::transmute(load(b"xrDestroyOpaqueDataChannelNV\0")?),
            fn_get_state: std::mem::transmute(load(b"xrGetOpaqueDataChannelStateNV\0")?),
            fn_shutdown:  std::mem::transmute(load(b"xrShutdownOpaqueDataChannelNV\0")?),
            fn_send:      std::mem::transmute(load(b"xrSendOpaqueDataChannelNV\0")?),
            fn_receive:   std::mem::transmute(load(b"xrReceiveOpaqueDataChannelNV\0")?),
            channel: 0,
            instance,
            system_id,
            connected: false,
            recv_buf: [0u8; 4096],
            latest: None,
            latest_config: None,
            reconnect_after: None,
        })
    }

    pub unsafe fn create_channel(&mut self) -> bool {
        let ci = CreateInfoNV {
            ty: XR_TYPE_CREATE_INFO,
            next: std::ptr::null(),
            system_id: self.system_id,
            uuid: XrGuid {
                data1: 0x12345678, data2: 0x1234, data3: 0x1234,
                data4: [0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xF0],
            },
        };
        let mut handle: u64 = 0;
        let r = (self.fn_create)(self.instance, &ci, &mut handle);
        if r != xr::Result::SUCCESS {
            opaque_log!(warn, "[ClearXR Layer] Failed to create opaque channel: {:?}", r);
            return false;
        }
        self.channel = handle;
        opaque_log!(info, "[ClearXR Layer] Opaque data channel created.");
        true
    }

    /// Poll once per frame. Returns the latest valid packet if any.
    pub unsafe fn poll(&mut self) -> Option<SpatialControllerPacket> {
        if self.channel == 0 { return None; }

        let state = self.channel_state();
        if state == STATUS_CONNECTED {
            if !self.connected {
                opaque_log!(info, "[ClearXR Layer] Opaque data channel connected!");
            }
            self.connected = true;
            self.reconnect_after = None;
        } else if state == STATUS_DISCONNECTED {
            if self.connected {
                opaque_log!(info, "[ClearXR Layer] Opaque channel disconnected. Will reconnect.");
                self.connected = false;
                self.latest = None;
            }
            self.try_reconnect();
        }

        if !self.connected { return None; }

        // Drain pending data — dispatch based on magic number
        loop {
            let mut received: u32 = 0;
            let r = (self.fn_receive)(self.channel, self.recv_buf.len() as u32,
                                       &mut received, self.recv_buf.as_mut_ptr());
            if r != xr::Result::SUCCESS || received == 0 { break; }

            if (received as usize) < 2 { continue; }

            let magic = u16::from_le_bytes([self.recv_buf[0], self.recv_buf[1]]);
            match magic {
                PACKET_MAGIC => {
                    let pkt_size = std::mem::size_of::<SpatialControllerPacket>();
                    if received as usize >= pkt_size {
                        let pkt: SpatialControllerPacket =
                            std::ptr::read_unaligned(self.recv_buf.as_ptr() as *const _);
                        if pkt.version == 1 {
                            self.latest = Some(pkt);
                        }
                    }
                }
                CONFIG_MAGIC => {
                    self.handle_config_packet(received as usize);
                }
                _ => {
                    opaque_log!(warn,
                        "[ClearXR Layer] Unknown opaque packet magic 0x{:04X} ({} bytes)",
                        magic, received
                    );
                }
            }
        }
        self.latest
    }

    
    /// Send a haptic event over the opaque channel to the headset.
    /// `hand`: 0 = left, 1 = right.
    pub fn send_haptic(&mut self, hand: u8, duration_ns: u64, frequency: f32, amplitude: f32) -> bool {
        if !self.connected || self.channel == 0 {
            return false;
        }

        let pkt = HapticEventPacket {
            magic: HAPTIC_MAGIC,
            version: 1,
            hand,
            duration_ns,
            frequency,
            amplitude,
        };

        let bytes = unsafe {
            std::slice::from_raw_parts(
                &pkt as *const HapticEventPacket as *const u8,
                std::mem::size_of::<HapticEventPacket>(),
            )
        };

        let result = unsafe {
            (self.fn_send)(self.channel, bytes.len() as u32, bytes.as_ptr())
        };

        if result != xr::Result::SUCCESS {
            opaque_log!(warn, "[ClearXR Layer] Haptic send failed: {:?}", result);
            return false;
        }
        opaque_log!(
            info,
            "[ClearXR Layer] Forwarded haptic packet: hand={} duration_ns={} frequency={} amplitude={}",
            hand,
            duration_ns,
            frequency,
            amplitude
        );
        true
    }


    unsafe fn try_reconnect(&mut self) {
        let now = std::time::Instant::now();
        if let Some(after) = self.reconnect_after {
            if now < after { return; }
        }
        self.reconnect_after = Some(now + std::time::Duration::from_secs(2));
        if self.channel != 0 {
            (self.fn_shutdown)(self.channel);
            (self.fn_destroy)(self.channel);
            self.channel = 0;
        }
        if self.create_channel() {
            opaque_log!(info, "[ClearXR Layer] Opaque channel recreated, waiting for connection...");
        }
    }

    unsafe fn channel_state(&self) -> i32 {
        let mut s = StateNV { ty: XR_TYPE_STATE, next: std::ptr::null_mut(), state: -1 };
        (self.fn_get_state)(self.channel, &mut s);
        s.state
    }

    /// Parse and apply a stream configuration packet from the headset.
    fn handle_config_packet(&mut self, received: usize) {
        if received < CONFIG_HEADER_SIZE {
            opaque_log!(warn, "[ClearXR Layer] Config packet too short ({} bytes)", received);
            return;
        }

        let header: StreamConfigHeader = unsafe {
            std::ptr::read_unaligned(self.recv_buf.as_ptr() as *const StreamConfigHeader)
        };

        let json_len = header.json_length as usize;
        let total = CONFIG_HEADER_SIZE + json_len;
        if received < total {
            opaque_log!(warn,
                "[ClearXR Layer] Config packet truncated: expected {} bytes, got {}",
                total, received
            );
            return;
        }

        let json_bytes = &self.recv_buf[CONFIG_HEADER_SIZE..CONFIG_HEADER_SIZE + json_len];
        let json_str = match std::str::from_utf8(json_bytes) {
            Ok(s) => s,
            Err(e) => {
                opaque_log!(warn, "[ClearXR Layer] Config JSON is not valid UTF-8: {}", e);
                return;
            }
        };

        opaque_log!(info, "[ClearXR Layer] Received stream config: {}", json_str);

        let config = match parse_stream_config(json_str) {
            Some(c) => c,
            None => {
                opaque_log!(warn, "[ClearXR Layer] Failed to parse stream config JSON");
                return;
            }
        };

        apply_stream_config(&config);
        self.latest_config = Some(config);
    }
}


impl Drop for OpaqueChannel {
    fn drop(&mut self) {
        if self.channel != 0 {
            unsafe {
                (self.fn_shutdown)(self.channel);
                (self.fn_destroy)(self.channel);
            }
        }
    }
}

// ============================================================
// Stream configuration — JSON parsing and file updates
// ============================================================

fn parse_stream_config(json: &str) -> Option<StreamConfig> {
    serde_json::from_str(json).ok()
}

/// Apply the received stream configuration to disk.
fn apply_stream_config(config: &StreamConfig) {
    // Update cloudxr-runtime.yaml (relative to this DLL)
    if let Some(dll_dir) = get_dll_dir() {
        let yaml_path = dll_dir.join("Server").join("cloudxr-runtime.yaml");
        match update_runtime_yaml(&yaml_path, config) {
            Ok(()) => opaque_log!(info, "[ClearXR Layer] Updated {}", yaml_path.display()),
            Err(e) => opaque_log!(warn, "[ClearXR Layer] Failed to update {}: {}", yaml_path.display(), e),
        }
    } else {
        opaque_log!(warn, "[ClearXR Layer] Could not determine DLL directory for YAML update");
    }

    // Update clearxr-settings.json (default app toggle)
    match update_settings_json(config.default_app_enabled) {
        Ok(path) => opaque_log!(info,
            "[ClearXR Layer] Updated default app setting to {} in {}",
            config.default_app_enabled, path.display()
        ),
        Err(e) => opaque_log!(warn, "[ClearXR Layer] Failed to update settings: {}", e),
    }
}

fn update_runtime_yaml(path: &std::path::Path, config: &StreamConfig) -> Result<(), String> {
    let contents = std::fs::read_to_string(path)
        .map_err(|e| format!("read {}: {}", path.display(), e))?;

    let mut doc: serde_yaml::Value = serde_yaml::from_str(&contents)
        .map_err(|e| format!("parse {}: {}", path.display(), e))?;

    let inset = (config.foveation_inset_ratio * 100.0).round() as i64;

    set_yaml_value(&mut doc, "runtimeFoveationUnwarpedWidth", config.rendered_resolution.into());
    set_yaml_value(&mut doc, "runtimeFoveationWarpedWidth", config.encoded_resolution.into());
    set_yaml_value(&mut doc, "runtimeFoveationInset", inset.into());
    set_yaml_value(&mut doc, "disableAlpha", (!config.alpha_transparency_enabled).into());

    let output = serde_yaml::to_string(&doc)
        .map_err(|e| format!("serialize: {}", e))?;
    std::fs::write(path, output)
        .map_err(|e| format!("write {}: {}", path.display(), e))
}

/// Recursively find and update a key in a YAML document.
fn set_yaml_value(doc: &mut serde_yaml::Value, key: &str, val: serde_yaml::Value) {
    if let Some(mapping) = doc.as_mapping_mut() {
        let yaml_key = serde_yaml::Value::String(key.to_string());
        if mapping.contains_key(&yaml_key) {
            mapping[&yaml_key] = val;
            return;
        }
        for (_, v) in mapping.iter_mut() {
            set_yaml_value(v, key, val.clone());
        }
    }
}

fn update_settings_json(launch_default_app: bool) -> Result<PathBuf, String> {
    let path = settings_json_path()
        .ok_or_else(|| "could not determine settings path".to_string())?;

    let mut settings: serde_json::Value = if path.exists() {
        let contents = std::fs::read_to_string(&path)
            .map_err(|e| format!("read {}: {}", path.display(), e))?;
        serde_json::from_str(&contents)
            .map_err(|e| format!("parse {}: {}", path.display(), e))?
    } else {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("create {}: {}", parent.display(), e))?;
        }
        serde_json::json!({
            "clearxrExePath": "clear-xr.exe",
            "clearxrLaunchDelaySeconds": 3
        })
    };

    settings["launchDefaultApp"] = serde_json::json!(launch_default_app);

    let output = serde_json::to_string_pretty(&settings)
        .map_err(|e| format!("serialize: {}", e))?;
    std::fs::write(&path, output + "\n")
        .map_err(|e| format!("write {}: {}", path.display(), e))?;
    Ok(path)
}

fn settings_json_path() -> Option<PathBuf> {
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        return Some(PathBuf::from(local).join("ClearXR").join("clearxr-settings.json"));
    }
    if let Some(appdata) = std::env::var_os("APPDATA") {
        return Some(PathBuf::from(appdata).join("ClearXR").join("clearxr-settings.json"));
    }
    None
}

#[cfg(windows)]
fn get_dll_dir() -> Option<PathBuf> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;

    unsafe extern "system" {
        fn GetModuleHandleExW(
            flags: u32,
            module_name: *const u16,
            module: *mut *mut std::ffi::c_void,
        ) -> i32;
        fn GetModuleFileNameW(
            module: *mut std::ffi::c_void,
            filename: *mut u16,
            size: u32,
        ) -> u32;
    }

    const FLAG_FROM_ADDRESS: u32 = 0x00000004;
    const FLAG_UNCHANGED_REFCOUNT: u32 = 0x00000002;

    unsafe {
        let mut module: *mut std::ffi::c_void = std::ptr::null_mut();
        // Use an address inside this DLL as the lookup reference
        let addr = get_dll_dir as *const u16;
        if GetModuleHandleExW(FLAG_FROM_ADDRESS | FLAG_UNCHANGED_REFCOUNT, addr, &mut module) == 0 {
            return None;
        }

        let mut buf = [0u16; 512];
        let len = GetModuleFileNameW(module, buf.as_mut_ptr(), buf.len() as u32);
        if len == 0 || len >= buf.len() as u32 {
            return None;
        }

        let path = PathBuf::from(OsString::from_wide(&buf[..len as usize]));
        path.parent().map(|p| p.to_path_buf())
    }
}

#[cfg(not(windows))]
fn get_dll_dir() -> Option<PathBuf> {
    None
}
