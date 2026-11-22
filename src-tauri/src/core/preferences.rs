use crate::{
    core::{desktop, monitor},
    providers::discovery,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{atomic::AtomicU64, Mutex},
};
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

#[derive(Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Preferences {
    pub autostart: bool,
    pub minimize_to_tray: bool,
    pub open_panel_at_startup: bool,
    pub pin_panel: bool,
    pub shortcut: String,
    pub claude_enabled: bool,
    pub codex_enabled: bool,
    pub codex_executable: Option<String>,
    pub antigravity_enabled: bool,
    pub antigravity_executable: Option<String>,
    pub notifications: bool,
    pub claude_alerts: bool,
    pub codex_alerts: bool,
    pub antigravity_alerts: bool,
    pub thresholds: Vec<u8>,
    pub local_providers: Vec<LocalProvider>,
}

#[derive(Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalProvider {
    pub id: String,
    pub name: String,
    pub path: String,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            autostart: false,
            minimize_to_tray: true,
            open_panel_at_startup: false,
            pin_panel: false,
            shortcut: "Ctrl+Alt+W".into(),
            claude_enabled: false,
            codex_enabled: false,
            codex_executable: None,
            antigravity_enabled: false,
            antigravity_executable: None,
            notifications: false,
            claude_alerts: true,
            codex_alerts: true,
            antigravity_alerts: true,
            thresholds: vec![80, 90, 95, 100],
            local_providers: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickBounds {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct UiState {
    pub language: Option<String>,
    pub theme: String,
    pub font_scale: u16,
    pub percent_style: String,
    pub per_tool_percent: HashMap<String, String>,
    pub collapsed_overview: Vec<String>,
    pub collapsed_quick: Vec<String>,
    pub last_route: String,
    pub activity_period: u16,
    pub heatmap_days: u16,
    pub dismissed_guidance: String,
    pub quick_bounds: Option<QuickBounds>,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            language: None,
            theme: "system".into(),
            font_scale: 100,
            percent_style: "remaining".into(),
            per_tool_percent: HashMap::new(),
            collapsed_overview: Vec::new(),
            collapsed_quick: Vec::new(),
            last_route: "/".into(),
            activity_period: 7,
            heatmap_days: 90,
            dismissed_guidance: String::new(),
            quick_bounds: None,
        }
    }
}

#[derive(Clone, Default, Debug, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct UiStatePatch {
    pub language: Option<String>,
    pub theme: Option<String>,
    pub font_scale: Option<u16>,
    pub percent_style: Option<String>,
    pub per_tool_percent: Option<HashMap<String, String>>,
    pub per_tool_override: Option<(String, Option<String>)>,
    pub collapsed_overview: Option<Vec<String>>,
    pub collapsed_quick: Option<Vec<String>>,
    pub last_route: Option<String>,
    pub activity_period: Option<u16>,
    pub heatmap_days: Option<u16>,
    pub dismissed_guidance: Option<String>,
}

fn clamp_font_scale(scale: u16) -> u16 {
    let clamped = scale.clamp(85, 125);
    ((clamped + 2) / 5) * 5
}

fn clean_id_list(items: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    for item in items {
        if !item.is_empty() && item.len() <= 80 && !item.chars().any(char::is_control) && !out.contains(&item) {
            out.push(item);
            if out.len() >= 50 {
                break;
            }
        }
    }
    out
}

fn clean_style_map(map: HashMap<String, String>) -> HashMap<String, String> {
    map.into_iter()
        .filter(|(id, style)| {
            !id.is_empty() && id.len() <= 80 && !id.chars().any(char::is_control) && matches!(style.as_str(), "remaining" | "used")
        })
        .take(50)
        .collect()
}

fn sanitize_ui_state(mut incoming: UiState, existing_bounds: Option<QuickBounds>) -> UiState {
    incoming.language = incoming.language.filter(|lang| lang == "en-US" || lang == "pt-BR");
    if !matches!(incoming.theme.as_str(), "system" | "light" | "dark") {
        incoming.theme = "system".into();
    }
    incoming.font_scale = if (85..=125).contains(&incoming.font_scale) { clamp_font_scale(incoming.font_scale) } else { 100 };
    if !matches!(incoming.percent_style.as_str(), "remaining" | "used") {
        incoming.percent_style = "remaining".into();
    }
    incoming.per_tool_percent = clean_style_map(incoming.per_tool_percent);
    incoming.collapsed_overview = clean_id_list(incoming.collapsed_overview);
    incoming.collapsed_quick = clean_id_list(incoming.collapsed_quick);
    if !matches!(incoming.last_route.as_str(), "/" | "/activity" | "/settings") {
        incoming.last_route = "/".into();
    }
    if !matches!(incoming.activity_period, 7 | 30 | 90) {
        incoming.activity_period = 7;
    }
    if !matches!(incoming.heatmap_days, 30 | 90) {
        incoming.heatmap_days = 90;
    }
    if incoming.dismissed_guidance.len() > 300 || incoming.dismissed_guidance.chars().any(char::is_control) {
        incoming.dismissed_guidance.clear();
    }
    // Window geometry is authoritative in Rust; frontend saves never clear it.
    incoming.quick_bounds = existing_bounds;
    incoming
}

/// Applies only the edited fields over the authoritative in-memory UI state to prevent cross-window overwrites.
fn apply_ui_patch(current: &mut UiState, patch: UiStatePatch) {
    if let Some(language) = patch.language.filter(|lang| lang == "en-US" || lang == "pt-BR") {
        current.language = Some(language);
    }
    if let Some(theme) = patch.theme.filter(|t| matches!(t.as_str(), "system" | "light" | "dark")) {
        current.theme = theme;
    }
    if let Some(scale) = patch.font_scale {
        current.font_scale = clamp_font_scale(scale);
    }
    if let Some(style) = patch.percent_style.filter(|s| matches!(s.as_str(), "remaining" | "used")) {
        current.percent_style = style;
    }
    if let Some(map) = patch.per_tool_percent {
        current.per_tool_percent = clean_style_map(map);
    }
    if let Some((tool_id, style)) = patch.per_tool_override {
        if !tool_id.is_empty() && tool_id.len() <= 80 && !tool_id.chars().any(char::is_control) {
            match style.filter(|s| matches!(s.as_str(), "remaining" | "used")) {
                Some(valid) if current.per_tool_percent.len() < 50 || current.per_tool_percent.contains_key(&tool_id) => {
                    current.per_tool_percent.insert(tool_id, valid);
                }
                None => {
                    current.per_tool_percent.remove(&tool_id);
                }
                _ => {}
            }
        }
    }
    if let Some(list) = patch.collapsed_overview {
        current.collapsed_overview = clean_id_list(list);
    }
    if let Some(list) = patch.collapsed_quick {
        current.collapsed_quick = clean_id_list(list);
    }
    if let Some(route) = patch.last_route.filter(|r| matches!(r.as_str(), "/" | "/activity" | "/settings")) {
        current.last_route = route;
    }
    if let Some(period) = patch.activity_period.filter(|p| matches!(p, 7 | 30 | 90)) {
        current.activity_period = period;
    }
    if let Some(days) = patch.heatmap_days.filter(|d| matches!(d, 30 | 90)) {
        current.heatmap_days = days;
    }
    if let Some(dismissed) = patch.dismissed_guidance {
        if dismissed.len() <= 300 && !dismissed.chars().any(char::is_control) {
            current.dismissed_guidance = dismissed;
        }
    }
}

#[derive(Default)]
pub struct DesktopState {
    // Serializes every read-modify-write cycle on preferences.json so concurrent saves never lose updates.
    store_lock: Mutex<()>,
    pub config: Mutex<Preferences>,
    pub ui: Mutex<UiState>,
    pub portuguese: Mutex<Option<bool>>,
    pub shortcut: Mutex<Option<Shortcut>>,
    pub shortcut_issue: Mutex<bool>,
    pub quick_position: Mutex<desktop::PanelPosition>,
    pub quick_save_seq: AtomicU64,
    recovery: Mutex<Recovery>,
}

impl DesktopState {
    /// Serialized UI patch write that always pairs the latest authoritative settings and UI state on disk.
    pub fn persist_ui_patch(&self, path: &Path, patch: UiStatePatch) -> Result<UiState, &'static str> {
        let _guard = self.store_lock.lock().unwrap_or_else(|error| error.into_inner());
        if self.recovery.lock().unwrap_or_else(|error| error.into_inner()).block_save {
            return Err("Local preferences could not be recovered.");
        }
        let settings = self.config.lock().unwrap_or_else(|error| error.into_inner()).clone();
        let mut current_ui = self.ui.lock().unwrap_or_else(|error| error.into_inner());
        let mut candidate = current_ui.clone();
        apply_ui_patch(&mut candidate, patch);
        write_store_file(path, &settings, &candidate).map_err(|_| "Could not save display preferences.")?;
        *current_ui = candidate.clone();
        Ok(candidate)
    }

    /// Serialized Quick Monitor bounds write that cannot overwrite concurrent provider changes.
    pub fn persist_bounds(&self, path: &Path, bounds: QuickBounds) {
        let _guard = self.store_lock.lock().unwrap_or_else(|error| error.into_inner());
        if self.recovery.lock().unwrap_or_else(|error| error.into_inner()).block_save {
            return;
        }
        let settings = self.config.lock().unwrap_or_else(|error| error.into_inner()).clone();
        let mut ui = self.ui.lock().unwrap_or_else(|error| error.into_inner());
        if ui.quick_bounds == Some(bounds) {
            return;
        }
        let mut candidate = ui.clone();
        candidate.quick_bounds = Some(bounds);
        if write_store_file(path, &settings, &candidate).is_ok() {
            *ui = candidate;
        }
    }

    pub fn mutate_settings<F>(&self, path: &Path, mutate: F) -> Result<(Preferences, Preferences), &'static str>
    where
        F: FnOnce(&mut Preferences) -> Result<(), &'static str>,
    {
        let _guard = self.store_lock.lock().unwrap_or_else(|error| error.into_inner());
        let mut current = self.config.lock().unwrap_or_else(|error| error.into_inner());
        let ui = self.ui.lock().unwrap_or_else(|error| error.into_inner()).clone();
        let mut recovery = self.recovery.lock().unwrap_or_else(|error| error.into_inner());
        if recovery.block_save {
            return Err("Local preferences could not be recovered. Fix access to local storage and restart to save changes.");
        }
        let mut next = current.clone();
        mutate(&mut next)?;
        write_store_file(path, &next, &ui).map_err(|_| "Could not save preferences.")?;
        let previous = std::mem::replace(&mut *current, next.clone());
        recovery.warning = None;
        Ok((previous, next))
    }
}

#[derive(Default)]
struct Recovery {
    warning: Option<String>,
    block_save: bool,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct PreferencesFile {
    settings: Preferences,
    ui: UiState,
    #[serde(flatten)]
    other: serde_json::Map<String, serde_json::Value>,
}

fn store_path(app: &AppHandle) -> PathBuf {
    app.state::<monitor::Monitor>().preferences_path()
}

pub fn require_window(label: &str, allowed: &[&str], message: &'static str) -> Result<(), &'static str> {
    if allowed.contains(&label) { Ok(()) } else { Err(message) }
}

pub fn is_local_path(path: &Path) -> bool {
    let Some(text) = path.to_str() else { return false; };
    if !path.is_absolute() || text.starts_with("//") || text.starts_with(r"\\") || text.len() > 1024 || text.contains('\0') {
        return false;
    }
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        use windows_sys::Win32::Storage::FileSystem::GetDriveTypeW;
        let Some(Component::Prefix(prefix)) = path.components().next() else { return false; };
        let Prefix::Disk(letter) = prefix.kind() else { return false; };
        let root = [u16::from(letter), b':' as u16, b'\\' as u16, 0];
        let drive = unsafe { GetDriveTypeW(root.as_ptr()) };
        local_drive_type(drive)
    }
    #[cfg(not(windows))]
    true
}

#[cfg(windows)]
fn local_drive_type(drive: u32) -> bool {
    use windows_sys::Win32::System::WindowsProgramming::{DRIVE_CDROM, DRIVE_FIXED, DRIVE_RAMDISK, DRIVE_REMOVABLE};
    matches!(drive, DRIVE_FIXED | DRIVE_REMOVABLE | DRIVE_CDROM | DRIVE_RAMDISK)
}

pub fn strip_windows_verbatim(path: &Path) -> Option<PathBuf> {
    let text = path.to_str()?;
    #[cfg(windows)]
    let normalized = if let Some(rest) = text.strip_prefix(r"\\?\") {
        // Verbatim UNC paths (`\\?\UNC\server\share`) must never be treated as local drives.
        if rest.len() >= 4 && rest[..4].eq_ignore_ascii_case(r"UNC\") {
            return None;
        }
        rest
    } else {
        text
    };
    #[cfg(not(windows))]
    let normalized = text;
    let candidate = PathBuf::from(normalized);
    is_local_path(&candidate).then_some(candidate)
}

/// Resolves a local file, rejecting UNC/network paths, symlinks, and redirects to non-local drives.
pub fn resolve_local_file(path: &Path) -> Option<PathBuf> {
    if !is_local_path(path) {
        return None;
    }
    let meta = fs::symlink_metadata(path).ok()?;
    if meta.file_type().is_symlink() || !meta.file_type().is_file() {
        return None;
    }
    let canonical = fs::canonicalize(path).ok()?;
    let resolved = strip_windows_verbatim(&canonical)?;
    let target_meta = fs::symlink_metadata(&resolved).ok()?;
    (target_meta.file_type().is_file() && !target_meta.file_type().is_symlink()).then_some(resolved)
}

/// Resolves a local directory, rejecting UNC/network paths and redirects whose final target leaves local drives.
pub fn resolve_local_dir(path: &Path) -> Option<PathBuf> {
    if !is_local_path(path) {
        return None;
    }
    let meta = fs::symlink_metadata(path).ok()?;
    if meta.file_type().is_symlink() || !meta.file_type().is_dir() {
        return None;
    }
    let canonical = fs::canonicalize(path).ok()?;
    let resolved = strip_windows_verbatim(&canonical)?;
    resolved.is_dir().then_some(resolved)
}

fn is_disallowed_shared_dir(path: &Path) -> bool {
    let lower = path.to_string_lossy().replace('/', "\\").to_ascii_lowercase();
    for var in ["TEMP", "TMP", "PUBLIC"] {
        if let Some(root) = std::env::var_os(var) {
            let root_norm = PathBuf::from(root).to_string_lossy().replace('/', "\\").to_ascii_lowercase();
            let root_trimmed = root_norm.trim_end_matches('\\');
            if !root_trimmed.is_empty() && (lower == root_trimmed || lower.starts_with(&format!("{root_trimmed}\\"))) {
                return true;
            }
        }
    }
    false
}

#[cfg(windows)]
fn is_writable_by_other_users(path: &Path) -> bool {
    use std::{ffi::c_void, os::windows::ffi::OsStrExt, ptr};
    use windows_sys::Win32::{
        Foundation::{ERROR_SUCCESS, GENERIC_ALL, GENERIC_WRITE, LocalFree},
        Security::{
            Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT},
            AclSizeInformation, CreateWellKnownSid, EqualSid, GetAce, GetAclInformation,
            WinAuthenticatedUserSid, WinBuiltinUsersSid, WinWorldSid,
            ACCESS_ALLOWED_ACE, ACL, ACL_SIZE_INFORMATION, DACL_SECURITY_INFORMATION,
            INHERIT_ONLY_ACE, PSECURITY_DESCRIPTOR, SECURITY_MAX_SID_SIZE,
        },
    };

    const WRITE_MASK: u32 = 0x0002 // FILE_WRITE_DATA / FILE_ADD_FILE
        | 0x0004 // FILE_APPEND_DATA / FILE_ADD_SUBDIRECTORY
        | 0x0010 // FILE_WRITE_EA
        | 0x0040 // FILE_DELETE_CHILD
        | 0x0100 // FILE_WRITE_ATTRIBUTES
        | 0x0001_0000 // DELETE
        | 0x0004_0000 // WRITE_DAC
        | 0x0008_0000 // WRITE_OWNER
        | GENERIC_WRITE
        | GENERIC_ALL;

    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let mut dacl: *mut ACL = ptr::null_mut();
    let mut sd: PSECURITY_DESCRIPTOR = ptr::null_mut();
    let status = unsafe {
        GetNamedSecurityInfoW(
            wide.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut dacl,
            ptr::null_mut(),
            &mut sd,
        )
    };
    if status != ERROR_SUCCESS {
        return true;
    }
    struct SdGuard(PSECURITY_DESCRIPTOR);
    impl Drop for SdGuard {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { LocalFree(self.0); }
            }
        }
    }
    let _guard = SdGuard(sd);
    // A NULL DACL grants full access to Everyone on Windows.
    if dacl.is_null() {
        return true;
    }

    let mut untrusted_sids: Vec<Vec<u8>> = Vec::with_capacity(3);
    for sid_kind in [WinWorldSid, WinAuthenticatedUserSid, WinBuiltinUsersSid] {
        let mut buf = vec![0u8; SECURITY_MAX_SID_SIZE as usize];
        let mut size = buf.len() as u32;
        if unsafe { CreateWellKnownSid(sid_kind, ptr::null_mut(), buf.as_mut_ptr().cast(), &mut size) } != 0 {
            untrusted_sids.push(buf);
        }
    }
    if untrusted_sids.is_empty() {
        return true;
    }

    let mut info = ACL_SIZE_INFORMATION::default();
    let ok = unsafe {
        GetAclInformation(
            dacl,
            (&mut info as *mut ACL_SIZE_INFORMATION).cast::<c_void>(),
            std::mem::size_of::<ACL_SIZE_INFORMATION>() as u32,
            AclSizeInformation,
        )
    };
    if ok == 0 {
        return true;
    }

    for index in 0..info.AceCount {
        let mut ace_ptr: *mut c_void = ptr::null_mut();
        if unsafe { GetAce(dacl, index, &mut ace_ptr) } == 0 || ace_ptr.is_null() {
            continue;
        }
        let ace = unsafe { &*(ace_ptr.cast::<ACCESS_ALLOWED_ACE>()) };
        if ace.Header.AceType != 0 || u32::from(ace.Header.AceFlags) & INHERIT_ONLY_ACE != 0 {
            continue;
        }
        if ace.Mask & WRITE_MASK == 0 {
            continue;
        }
        let ace_sid = (&ace.SidStart as *const u32).cast::<c_void>().cast_mut();
        if untrusted_sids.iter().any(|sid| unsafe { EqualSid(ace_sid, sid.as_ptr().cast::<c_void>().cast_mut()) } != 0) {
            return true;
        }
    }
    false
}

#[cfg(not(windows))]
fn is_writable_by_other_users(_path: &Path) -> bool {
    false
}

/// Resolves and validates a local `.exe` outside temp/public folders and not writable by other local users.
pub fn is_trusted_executable_path(path: &Path) -> Option<PathBuf> {
    let resolved = resolve_local_file(path)?;
    if !resolved.extension().and_then(|ext| ext.to_str()).is_some_and(|ext| ext.eq_ignore_ascii_case("exe")) {
        return None;
    }
    let parent = resolved.parent()?;
    if is_disallowed_shared_dir(&resolved) || is_disallowed_shared_dir(parent) {
        return None;
    }
    if is_writable_by_other_users(&resolved) || is_writable_by_other_users(parent) {
        return None;
    }
    Some(resolved)
}

fn is_valid_saved_executable_syntax(path: &str) -> bool {
    let candidate = Path::new(path);
    is_local_path(candidate)
        && candidate.extension().and_then(|ext| ext.to_str()).is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
}

fn write_store_file(path: &Path, settings: &Preferences, ui: &UiState) -> io::Result<()> {
    let mut store = match fs::read(path) {
        Ok(bytes) => match serde_json::from_slice::<PreferencesFile>(&bytes) {
            Ok(store) => store,
            Err(_) => {
                backup_preferences(path, &bytes)?;
                PreferencesFile::default()
            }
        },
        Err(error) if error.kind() == io::ErrorKind::NotFound => PreferencesFile::default(),
        Err(error) => return Err(error),
    };
    store.settings = settings.clone();
    store.ui = ui.clone();
    let mut temp = tempfile::NamedTempFile::new_in(path.parent().ok_or(io::ErrorKind::InvalidInput)?)?;
    serde_json::to_writer_pretty(temp.as_file_mut(), &store)?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|error| error.error)?;
    Ok(())
}

fn backup_preferences(path: &Path, bytes: &[u8]) -> io::Result<PathBuf> {
    let parent = path.parent().ok_or(io::ErrorKind::InvalidInput)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    for index in 0..100 {
        let suffix = if index == 0 { String::new() } else { format!(".{index}") };
        let backup = parent.join(format!("preferences.json.bak{suffix}"));
        match temp.persist_noclobber(&backup) {
            Ok(_) => return Ok(backup),
            Err(error) if error.error.kind() == io::ErrorKind::AlreadyExists => {
                if fs::read(&backup).is_ok_and(|existing| existing == bytes) { return Ok(backup); }
                temp = error.file;
            }
            Err(error) => return Err(error.error),
        }
    }
    Err(io::ErrorKind::AlreadyExists.into())
}

fn migrate_loaded_preferences(settings: &mut Preferences) {
    // Migrate file-based Antigravity entries to native CLI integration.
    let had_legacy = settings.local_providers.iter().any(|item| item.id == "local-antigravity");
    if had_legacy {
        settings.antigravity_enabled = true;
        settings.local_providers.retain(|item| item.id != "local-antigravity");
    }
}

fn load_store_file(path: &Path) -> (Preferences, UiState, Recovery) {
    let recovery = match fs::read(path) {
        Ok(bytes) => match serde_json::from_slice::<PreferencesFile>(&bytes) {
            Ok(mut store) => {
                migrate_loaded_preferences(&mut store.settings);
                let bounds = store.ui.quick_bounds;
                let ui = sanitize_ui_state(store.ui, bounds);
                return (store.settings, ui, Recovery::default());
            }
            Err(_) => match backup_preferences(path, &bytes) {
                Ok(backup) => Recovery {
                    warning: Some(format!(
                        "Preferences were damaged. Defaults are in use and the original was saved as {}.",
                        backup.file_name().unwrap_or_default().to_string_lossy()
                    )),
                    block_save: false,
                },
                Err(_) => Recovery {
                    warning: Some("Preferences were damaged and could not be backed up. Defaults are in use. Fix access to local storage and restart to save changes.".into()),
                    block_save: true,
                },
            },
        },
        Err(error) if error.kind() == io::ErrorKind::NotFound => Recovery::default(),
        Err(_) => Recovery {
            warning: Some("Could not read local preferences. Defaults are in use. Fix access to local storage and restart to save changes.".into()),
            block_save: true,
        },
    };
    (Preferences::default(), UiState::default(), recovery)
}

pub fn initialize(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let (mut settings, ui, recovery) = load_store_file(&store_path(app));
    settings.autostart = app.autolaunch().is_enabled().unwrap_or(false);
    let state = app.state::<DesktopState>();
    let _guard = state.store_lock.lock().unwrap_or_else(|error| error.into_inner());
    *state.recovery.lock().unwrap_or_else(|error| error.into_inner()) = recovery;
    *state.config.lock().unwrap_or_else(|error| error.into_inner()) = settings.clone();
    if let Some(portuguese) = ui.language.as_deref().map(|lang| lang == "pt-BR") {
        *state.portuguese.lock().unwrap_or_else(|error| error.into_inner()) = Some(portuguese);
    }
    if let Some(bounds) = ui.quick_bounds {
        state.quick_position.lock().unwrap_or_else(|error| error.into_inner()).seed(bounds);
    }
    *state.ui.lock().unwrap_or_else(|error| error.into_inner()) = ui;
    let registered = settings.shortcut.parse::<Shortcut>().ok().filter(|shortcut| app.global_shortcut().register(*shortcut).is_ok());
    *state.shortcut_issue.lock().unwrap_or_else(|error| error.into_inner()) = registered.is_none();
    *state.shortcut.lock().unwrap_or_else(|error| error.into_inner()) = registered;
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreferencesState {
    settings: Preferences,
    shortcut_conflict: bool,
    recovery_warning: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PreferenceError {
    #[serde(skip_serializing_if = "Option::is_none")]
    field: Option<&'static str>,
    message: &'static str,
}

impl PreferenceError {
    fn field(field: &'static str, message: &'static str) -> Self { Self { field: Some(field), message } }
}

impl From<&'static str> for PreferenceError {
    fn from(message: &'static str) -> Self { Self { field: None, message } }
}

#[tauri::command]
pub fn get_preferences(state: State<'_, DesktopState>) -> PreferencesState {
    PreferencesState {
        settings: state.config.lock().unwrap_or_else(|error| error.into_inner()).clone(),
        shortcut_conflict: *state.shortcut_issue.lock().unwrap_or_else(|error| error.into_inner()),
        recovery_warning: state.recovery.lock().unwrap_or_else(|error| error.into_inner()).warning.clone(),
    }
}

#[tauri::command]
pub fn get_ui_state(state: State<'_, DesktopState>) -> UiState {
    state.ui.lock().unwrap_or_else(|error| error.into_inner()).clone()
}

#[tauri::command]
pub fn save_ui_state(app: AppHandle, window: WebviewWindow, state: State<'_, DesktopState>, ui: UiStatePatch) -> Result<UiState, String> {
    require_window(window.label(), &["main", "quick"], "Open TempoAI to save display preferences.")?;
    let language_changed = ui.language.is_some();
    let next = state.persist_ui_patch(&store_path(&app), ui).map_err(str::to_owned)?;
    if language_changed {
        if let Some(portuguese) = next.language.as_deref().map(|lang| lang == "pt-BR") {
            let _ = desktop::apply_desktop_language(&app, portuguese);
        }
    }
    let _ = app.emit("ui-state-updated", ());
    Ok(next)
}

pub fn persist_quick_bounds(app: &AppHandle, bounds: QuickBounds) {
    app.state::<DesktopState>().persist_bounds(&store_path(app), bounds);
}

fn validate_preferences(settings: &Preferences) -> Result<Shortcut, PreferenceError> {
    discovery::validate_local_providers(&settings.local_providers)?;
    if settings.codex_executable.as_deref().is_some_and(|path| !is_valid_saved_executable_syntax(path)) {
        return Err("The Codex executable path is not valid.".into());
    }
    if settings.antigravity_executable.as_deref().is_some_and(|path| !is_valid_saved_executable_syntax(path)) {
        return Err("The Antigravity executable path is not valid.".into());
    }
    if settings.thresholds.iter().any(|value| ![80, 90, 95, 100].contains(value)) {
        return Err("Unsupported alert threshold.".into());
    }
    settings.shortcut.parse().map_err(|_| PreferenceError::field("shortcut", "Enter a valid shortcut, such as Ctrl+Alt+W."))
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ProviderConnection {
    ClaudeAuto,
    CodexAuto {
        #[serde(default)]
        executable: Option<String>,
    },
    AntigravityAuto {
        #[serde(default)]
        executable: Option<String>,
    },
    Local { source: LocalProvider },
}

fn pin_executable(raw: Option<String>, invalid_msg: &'static str) -> Result<Option<String>, &'static str> {
    match raw.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(path) => {
            let trusted = is_trusted_executable_path(Path::new(path)).ok_or(invalid_msg)?;
            let text = trusted.to_str().ok_or(invalid_msg)?;
            Ok(Some(text.to_owned()))
        }
        None => Ok(None),
    }
}

fn apply_connection(settings: &mut Preferences, connection: ProviderConnection) -> Result<(), &'static str> {
    match connection {
        ProviderConnection::ClaudeAuto => settings.claude_enabled = true,
        ProviderConnection::CodexAuto { executable } => {
            let pinned = pin_executable(executable, "The Codex executable is not in a trusted local folder.")?;
            settings.codex_enabled = true;
            settings.codex_executable = pinned;
        }
        ProviderConnection::AntigravityAuto { executable } => {
            let pinned = pin_executable(executable, "The Antigravity CLI is not in a trusted local folder.")?;
            settings.antigravity_enabled = true;
            settings.antigravity_executable = pinned
                .or_else(|| discovery::antigravity_executable().and_then(|p| p.to_str().map(str::to_owned)));
        }
        ProviderConnection::Local { source } => {
            settings.local_providers.retain(|item| item.id != source.id);
            settings.local_providers.push(source);
            discovery::validate_local_providers(&settings.local_providers)?;
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn connect_provider(app: AppHandle, window: WebviewWindow, connection: ProviderConnection) -> Result<PreferencesState, String> {
    require_window(window.label(), &["main"], "Use the dashboard to change these preferences.")?;
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<DesktopState>();
        let monitor = app.state::<monitor::Monitor>();
        let (previous, next) = state.mutate_settings(&monitor.preferences_path(), |settings| apply_connection(settings, connection))?;
        monitor.reset_changed_sources(&previous, &next);
        let _ = app.emit("preferences-updated", ());
        monitor::queue(&app, true);
        Ok(get_preferences(state))
    }).await.map_err(|_| "Could not save preferences.")?.map_err(str::to_owned)
}

fn remove_connection(settings: &mut Preferences, provider_id: &str) -> Result<(), &'static str> {
    match provider_id {
        "claude" => settings.claude_enabled = false,
        "codex" => {
            settings.codex_enabled = false;
            settings.codex_executable = None;
        }
        "antigravity" | "local-antigravity" => {
            settings.antigravity_enabled = false;
            settings.antigravity_executable = None;
            settings.local_providers.retain(|source| source.id != "local-antigravity");
        }
        id if id.starts_with("local-") && settings.local_providers.iter().any(|source| source.id == id) => {
            settings.local_providers.retain(|source| source.id != id);
        }
        _ => return Err("This tool is no longer connected."),
    }
    Ok(())
}

#[tauri::command]
pub async fn remove_provider(app: AppHandle, window: WebviewWindow, provider_id: String) -> Result<PreferencesState, String> {
    require_window(window.label(), &["main"], "Use the dashboard to change these preferences.")?;
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<DesktopState>();
        let monitor = app.state::<monitor::Monitor>();
        let (previous, next) = state.mutate_settings(&monitor.preferences_path(), |settings| remove_connection(settings, &provider_id))?;
        monitor.reset_changed_sources(&previous, &next);
        let _ = app.emit("preferences-updated", ());
        monitor::queue(&app, true);
        Ok(get_preferences(state))
    }).await.map_err(|_| "Could not save preferences.")?.map_err(str::to_owned)
}

#[tauri::command]
pub fn save_preferences(app: AppHandle, window: WebviewWindow, state: State<'_, DesktopState>, settings: Preferences) -> Result<PreferencesState, PreferenceError> {
    require_window(window.label(), &["main"], "Use the dashboard to change these preferences.")?;
    let candidate = validate_preferences(&settings)?;
    let _store_guard = state.store_lock.lock().unwrap_or_else(|error| error.into_inner());
    let mut current = state.config.lock().unwrap_or_else(|error| error.into_inner());
    let ui = state.ui.lock().unwrap_or_else(|error| error.into_inner()).clone();
    if state.recovery.lock().unwrap_or_else(|error| error.into_inner()).block_save {
        return Err("Local preferences could not be recovered. Fix access to local storage and restart to save changes.".into());
    }
    let mut active = state.shortcut.lock().unwrap_or_else(|error| error.into_inner());
    let previous = *active;
    let changed = previous != Some(candidate);
    let registered = if changed {
        match app.global_shortcut().register(candidate) {
            Ok(()) => true,
            Err(_) if settings.shortcut == current.shortcut => false,
            Err(_) => return Err(PreferenceError::field("shortcut", "This shortcut could not be registered. Try a different combination.")),
        }
    } else { false };
    let autostart_changed = current.autostart != settings.autostart;
    let set_autostart = |enabled| if enabled { app.autolaunch().enable() } else { app.autolaunch().disable() };
    if autostart_changed && set_autostart(settings.autostart).is_err() {
        if registered { let _ = app.global_shortcut().unregister(candidate); }
        return Err("Could not change Windows startup settings.".into());
    }
    if write_store_file(&store_path(&app), &settings, &ui).is_err() {
        if autostart_changed { let _ = set_autostart(current.autostart); }
        if registered { let _ = app.global_shortcut().unregister(candidate); }
        return Err("Could not save preferences.".into());
    }
    if registered {
        if let Some(old) = previous { let _ = app.global_shortcut().unregister(old); }
        *active = Some(candidate);
        *state.shortcut_issue.lock().unwrap_or_else(|error| error.into_inner()) = false;
    }
    let sources_changed = app.state::<monitor::Monitor>().reset_changed_sources(&current, &settings);
    *current = settings;
    let mut recovery = state.recovery.lock().unwrap_or_else(|error| error.into_inner());
    recovery.warning = None;
    drop(recovery);
    drop(active);
    drop(current);
    drop(_store_guard);
    let _ = app.emit("preferences-updated", ());
    if sources_changed {
        let _ = app.emit("usage-updated", ());
        monitor::queue(&app, true);
    }
    Ok(get_preferences(state))
}

#[tauri::command]
pub fn set_pin(app: AppHandle, window: WebviewWindow, state: State<'_, DesktopState>, pinned: bool) -> Result<(), String> {
    require_window(window.label(), &["quick"], "Use Quick Monitor to change the panel pin.")?;
    state.mutate_settings(&store_path(&app), |settings| {
        settings.pin_panel = pinned;
        Ok(())
    }).map_err(str::to_owned)?;
    let _ = app.emit("preferences-updated", ());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};

    #[test]
    fn preserves_corrupt_preferences_before_saving_defaults() {
        let directory = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
        let path = directory.path().join("preferences.json");
        let damaged = b"{\"settings\": {\"shortcut\":";
        fs::write(&path, damaged).unwrap();

        let (mut settings, ui, recovery) = load_store_file(&path);
        let backup = path.with_file_name("preferences.json.bak");
        assert_eq!(fs::read(&backup).unwrap(), damaged);
        assert_eq!(fs::read(&path).unwrap(), damaged);
        assert!(recovery.warning.unwrap().contains("preferences.json.bak"));
        assert!(!recovery.block_save);

        settings.pin_panel = true;
        write_store_file(&path, &settings, &ui).unwrap();
        let (saved, _, recovery) = load_store_file(&path);
        assert!(saved.pin_panel);
        assert!(recovery.warning.is_none());
        assert_eq!(fs::read(&backup).unwrap(), damaged);
        let later_damage = b"{\"settings\":null}";
        fs::write(&path, later_damage).unwrap();
        assert!(load_store_file(&path).2.warning.unwrap().contains("preferences.json.bak.1"));
        assert_eq!(fs::read(&backup).unwrap(), damaged);
        assert_eq!(fs::read(path.with_file_name("preferences.json.bak.1")).unwrap(), later_damage);
    }

    #[test]
    fn concurrent_provider_removal_with_ui_and_bounds_saves_never_re_enables_removed_provider() {
        let directory = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
        let path = Arc::new(directory.path().join("preferences.json"));
        let state = Arc::new(DesktopState::default());

        for round in 0..25 {
            state.mutate_settings(&path, |s| {
                s.claude_enabled = true;
                s.codex_enabled = true;
                Ok(())
            }).unwrap();

            let barrier = Arc::new(Barrier::new(3));
            let s1 = Arc::clone(&state);
            let p1 = Arc::clone(&path);
            let b1 = Arc::clone(&barrier);
            let t_remove = std::thread::spawn(move || {
                b1.wait();
                s1.mutate_settings(&p1, |s| remove_connection(s, "claude")).unwrap();
            });

            let s2 = Arc::clone(&state);
            let p2 = Arc::clone(&path);
            let b2 = Arc::clone(&barrier);
            let t_ui = std::thread::spawn(move || {
                b2.wait();
                s2.persist_ui_patch(&p2, UiStatePatch {
                    theme: Some(if round % 2 == 0 { "dark" } else { "light" }.into()),
                    font_scale: Some(115),
                    ..UiStatePatch::default()
                }).unwrap();
            });

            let s3 = Arc::clone(&state);
            let p3 = Arc::clone(&path);
            let b3 = Arc::clone(&barrier);
            let bounds = QuickBounds { x: 40 + round, y: 80, width: 350, height: 430 };
            let t_bounds = std::thread::spawn(move || {
                b3.wait();
                s3.persist_bounds(&p3, bounds);
            });

            t_remove.join().unwrap();
            t_ui.join().unwrap();
            t_bounds.join().unwrap();

            let (reloaded_settings, reloaded_ui, recovery) = load_store_file(&path);
            assert!(recovery.warning.is_none());
            assert!(!reloaded_settings.claude_enabled, "round {round}: removed provider returned on disk");
            assert!(reloaded_settings.codex_enabled);
            assert_eq!(reloaded_ui.font_scale, 115);
            assert_eq!(reloaded_ui.quick_bounds, Some(bounds));
        }
    }

    #[test]
    fn persists_ui_state_and_migrates_legacy_preferences_without_data_loss() {
        let directory = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
        let path = directory.path().join("preferences.json");
        fs::write(
            &path,
            br#"{"settings":{"shortcut":"Ctrl+Alt+Q","reduceMotion":true,"claudeFolder":"C:/old","localProviders":[{"id":"local-antigravity","name":"Google Antigravity","path":"C:/a.json"}]},"other":true}"#,
        )
        .unwrap();
        let (settings, mut ui, recovery) = load_store_file(&path);
        assert!(recovery.warning.is_none());
        assert_eq!(settings.shortcut, "Ctrl+Alt+Q");
        assert!(settings.antigravity_enabled);
        assert!(settings.local_providers.is_empty());

        ui.language = Some("pt-BR".into());
        ui.percent_style = "used".into();
        ui.per_tool_percent.insert("claude".into(), "remaining".into());
        ui.collapsed_quick.push("codex".into());
        ui.quick_bounds = Some(QuickBounds { x: 100, y: 200, width: 360, height: 480 });
        write_store_file(&path, &settings, &ui).unwrap();

        let (_, reloaded_ui, _) = load_store_file(&path);
        assert_eq!(reloaded_ui.language.as_deref(), Some("pt-BR"));
        assert_eq!(reloaded_ui.percent_style, "used");
        assert_eq!(reloaded_ui.per_tool_percent.get("claude").map(String::as_str), Some("remaining"));
        assert_eq!(reloaded_ui.collapsed_quick, vec!["codex"]);
        assert_eq!(reloaded_ui.quick_bounds, Some(QuickBounds { x: 100, y: 200, width: 360, height: 480 }));
        let stored: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        assert_eq!(stored["other"], true);
    }

    #[test]
    fn validates_shortcut_and_thresholds() {
        let mut settings = Preferences { claude_enabled: true, codex_enabled: true, antigravity_enabled: true, ..Preferences::default() };
        assert!(validate_preferences(&settings).is_ok());
        settings.shortcut = "NotAKey".into();
        assert_eq!(validate_preferences(&settings).unwrap_err().field, Some("shortcut"));
    }

    #[test]
    fn zero_config_connections_pin_and_clear_trusted_executables() {
        let dir = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
        let codex_exe = dir.path().join("codex.exe");
        let agy_exe = dir.path().join("agy.exe");
        fs::write(&codex_exe, "synthetic").unwrap();
        fs::write(&agy_exe, "synthetic").unwrap();
        let source = LocalProvider { id: "local-a".into(), name: "A".into(), path: "C:/a.json".into() };
        let mut settings = Preferences::default();

        apply_connection(&mut settings, ProviderConnection::ClaudeAuto).unwrap();
        apply_connection(&mut settings, ProviderConnection::CodexAuto { executable: Some(codex_exe.to_str().unwrap().into()) }).unwrap();
        apply_connection(&mut settings, ProviderConnection::AntigravityAuto { executable: Some(agy_exe.to_str().unwrap().into()) }).unwrap();
        apply_connection(&mut settings, ProviderConnection::Local { source }).unwrap();

        assert!(settings.claude_enabled && settings.codex_enabled && settings.antigravity_enabled && settings.local_providers.len() == 1);
        assert!(settings.codex_executable.is_some());
        assert!(settings.antigravity_executable.is_some());

        remove_connection(&mut settings, "claude").unwrap();
        assert!(!settings.claude_enabled && settings.codex_enabled && settings.antigravity_enabled);
        remove_connection(&mut settings, "codex").unwrap();
        assert!(!settings.codex_enabled && settings.codex_executable.is_none());
        remove_connection(&mut settings, "antigravity").unwrap();
        assert!(!settings.antigravity_enabled && settings.antigravity_executable.is_none());
        remove_connection(&mut settings, "local-a").unwrap();
        assert!(settings.local_providers.is_empty());
        assert!(remove_connection(&mut settings, "local-a").is_err());
    }

    #[test]
    fn partial_ui_patches_merge_without_clobbering_other_fields() {
        let mut ui = UiState {
            quick_bounds: Some(QuickBounds { x: 10, y: 20, width: 340, height: 420 }),
            ..UiState::default()
        };
        apply_ui_patch(&mut ui, UiStatePatch { theme: Some("dark".into()), ..UiStatePatch::default() });
        apply_ui_patch(&mut ui, UiStatePatch { font_scale: Some(115), ..UiStatePatch::default() });
        apply_ui_patch(&mut ui, UiStatePatch { collapsed_quick: Some(vec!["codex".into()]), ..UiStatePatch::default() });
        apply_ui_patch(&mut ui, UiStatePatch { per_tool_override: Some(("claude".into(), Some("used".into()))), ..UiStatePatch::default() });
        apply_ui_patch(&mut ui, UiStatePatch { per_tool_override: Some(("codex".into(), Some("remaining".into()))), ..UiStatePatch::default() });
        assert_eq!(ui.theme, "dark");
        assert_eq!(ui.font_scale, 115);
        assert_eq!(ui.collapsed_quick, vec!["codex"]);
        assert_eq!(ui.per_tool_percent.get("claude").map(String::as_str), Some("used"));
        assert_eq!(ui.per_tool_percent.get("codex").map(String::as_str), Some("remaining"));
        assert_eq!(ui.quick_bounds, Some(QuickBounds { x: 10, y: 20, width: 340, height: 420 }));
        // Out-of-range scales clamp to 85..=125 in steps of 5.
        apply_ui_patch(&mut ui, UiStatePatch { font_scale: Some(200), ..UiStatePatch::default() });
        assert_eq!(ui.font_scale, 125);
        apply_ui_patch(&mut ui, UiStatePatch { font_scale: Some(40), ..UiStatePatch::default() });
        assert_eq!(ui.font_scale, 85);
        // Removing one tool override keeps the other untouched.
        apply_ui_patch(&mut ui, UiStatePatch { per_tool_override: Some(("claude".into(), None)), ..UiStatePatch::default() });
        assert!(!ui.per_tool_percent.contains_key("claude"));
        assert_eq!(ui.per_tool_percent.get("codex").map(String::as_str), Some("remaining"));
    }

    #[test]
    fn history_and_panel_commands_allow_only_their_respective_windows() {
        assert!(require_window("main", &["main"], "Dashboard only").is_ok());
        assert!(require_window("quick", &["main"], "Dashboard only").is_err());
        assert!(require_window("quick", &["quick"], "Quick Monitor only").is_ok());
        assert!(require_window("main", &["quick"], "Quick Monitor only").is_err());
    }

    #[test]
    #[cfg(windows)]
    fn rejects_unc_verbatim_unc_temp_dirs_and_network_drives() {
        use windows_sys::Win32::System::WindowsProgramming::{DRIVE_FIXED, DRIVE_REMOTE};
        assert!(!is_local_path(Path::new(r"\\server\share\export")));
        assert!(strip_windows_verbatim(Path::new(r"\\?\UNC\server\share\file.json")).is_none());
        assert!(!local_drive_type(DRIVE_REMOTE));
        assert!(local_drive_type(DRIVE_FIXED));

        let temp_file = std::env::temp_dir().join("tempoai-untrusted-test.exe");
        let _ = fs::write(&temp_file, "test");
        assert!(is_trusted_executable_path(&temp_file).is_none());
        let _ = fs::remove_file(&temp_file);
    }
}
