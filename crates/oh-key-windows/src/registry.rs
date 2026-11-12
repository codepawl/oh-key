//! COM class registration, TSF profile registration and the stable GUIDs.

use std::sync::atomic::{AtomicUsize, Ordering};
use windows::Win32::Foundation::{HMODULE, MAX_PATH};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::System::LibraryLoader::GetModuleFileNameW;
use windows::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey,
    RegCreateKeyExW, RegDeleteTreeW, RegSetValueExW,
};
use windows::Win32::UI::Input::KeyboardAndMouse::HKL;
use windows::Win32::UI::TextServices::{
    CLSID_TF_CategoryMgr, CLSID_TF_InputProcessorProfiles, GUID_TFCAT_DISPLAYATTRIBUTEPROVIDER,
    GUID_TFCAT_TIP_KEYBOARD, GUID_TFCAT_TIPCAP_COMLESS, GUID_TFCAT_TIPCAP_IMMERSIVESUPPORT,
    GUID_TFCAT_TIPCAP_INPUTMODECOMPARTMENT, GUID_TFCAT_TIPCAP_SYSTRAYSUPPORT, ITfCategoryMgr,
    ITfInputProcessorProfileMgr,
};
use windows::core::{BOOL, GUID, HSTRING, PCWSTR, Result};

pub const OHKEY_CLSID: GUID = GUID::from_u128(0x6d5a7f1e_3b2c_4e8a_9c41_7a1e5b0d2f63);
pub const OHKEY_PROFILE_GUID: GUID = GUID::from_u128(0xb8e0c4a2_5f17_4d93_a6b1_2c9e7d4f8a15);
pub const DISPLAY_ATTRIBUTE_GUID: GUID = GUID::from_u128(0x1f3d9a67_84c2_4b5e_8e0a_d6c27b93f451);
pub const LANG_BAR_ITEM_GUID: GUID = GUID::from_u128(0xa47c2e19_6d83_4f0b_b5e2_93c1d8f7a604);

pub const VIETNAMESE_LANGUAGE_ID: u16 = 0x042A;
const TIP_NAME: &str = "oh-key";

static MODULE_HANDLE: AtomicUsize = AtomicUsize::new(0);

windows_link::link!("input.dll" "system" fn InstallLayoutOrTip(layout_or_tip: PCWSTR, flags: u32) -> BOOL);

pub fn remember_module(module: HMODULE) {
    MODULE_HANDLE.store(module.0 as usize, Ordering::Relaxed);
}

fn module_path() -> Result<String> {
    let mut buffer = [0u16; MAX_PATH as usize];
    let module = HMODULE(MODULE_HANDLE.load(Ordering::Relaxed) as *mut core::ffi::c_void);
    let length = unsafe { GetModuleFileNameW(Some(module), &mut buffer) } as usize;
    if length == 0 {
        return Err(windows::core::Error::from_thread());
    }
    Ok(String::from_utf16_lossy(&buffer[..length]))
}

fn braced(guid: &GUID) -> String {
    format!("{{{guid:?}}}")
}

fn class_key_path() -> String {
    format!("SOFTWARE\\Classes\\CLSID\\{}", braced(&OHKEY_CLSID))
}

pub fn register_server() -> Result<()> {
    let dll_path = module_path()?;
    write_class_keys(&dll_path)?;
    register_profile(&dll_path)?;
    register_categories()?;
    enable_for_current_user();
    Ok(())
}

pub fn unregister_server() -> Result<()> {
    // Each step runs even when an earlier one fails, so a half-registered
    // install can still be removed.
    let profile = unregister_profile();
    let categories = unregister_categories();
    let class = delete_class_keys();
    profile.and(categories).and(class)
}

fn write_class_keys(dll_path: &str) -> Result<()> {
    let class_path = class_key_path();
    set_string_value(&class_path, None, TIP_NAME)?;
    let server_path = format!("{class_path}\\InprocServer32");
    set_string_value(&server_path, None, dll_path)?;
    set_string_value(&server_path, Some("ThreadingModel"), "Apartment")
}

fn delete_class_keys() -> Result<()> {
    let path = HSTRING::from(class_key_path());
    unsafe { RegDeleteTreeW(HKEY_LOCAL_MACHINE, &path).ok() }
}

fn set_string_value(key_path: &str, value_name: Option<&str>, value: &str) -> Result<()> {
    let key_path = HSTRING::from(key_path);
    let mut key = HKEY::default();
    unsafe {
        RegCreateKeyExW(
            HKEY_LOCAL_MACHINE,
            &key_path,
            None,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut key,
            None,
        )
        .ok()?;
    }
    let name = value_name.map(HSTRING::from);
    let name_pointer = name
        .as_ref()
        .map_or(PCWSTR::null(), |name| PCWSTR(name.as_ptr()));
    let wide: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
    let bytes = unsafe { std::slice::from_raw_parts(wide.as_ptr().cast::<u8>(), wide.len() * 2) };
    let result = unsafe { RegSetValueExW(key, name_pointer, None, REG_SZ, Some(bytes)).ok() };
    unsafe {
        let _ = RegCloseKey(key);
    }
    result
}

fn profile_manager() -> Result<ITfInputProcessorProfileMgr> {
    unsafe { CoCreateInstance(&CLSID_TF_InputProcessorProfiles, None, CLSCTX_INPROC_SERVER) }
}

fn register_profile(icon_path: &str) -> Result<()> {
    let description: Vec<u16> = TIP_NAME.encode_utf16().collect();
    let icon: Vec<u16> = icon_path.encode_utf16().collect();
    unsafe {
        profile_manager()?.RegisterProfile(
            &OHKEY_CLSID,
            VIETNAMESE_LANGUAGE_ID,
            &OHKEY_PROFILE_GUID,
            &description,
            &icon,
            0,
            HKL::default(),
            0,
            false,
            0,
        )
    }
}

fn unregister_profile() -> Result<()> {
    unsafe {
        profile_manager()?.UnregisterProfile(
            &OHKEY_CLSID,
            VIETNAMESE_LANGUAGE_ID,
            &OHKEY_PROFILE_GUID,
            0,
        )
    }
}

/// Without these the TIP is missing in Store apps, the tray or underlines.
fn registered_categories() -> [GUID; 6] {
    [
        GUID_TFCAT_TIP_KEYBOARD,
        GUID_TFCAT_DISPLAYATTRIBUTEPROVIDER,
        GUID_TFCAT_TIPCAP_IMMERSIVESUPPORT,
        GUID_TFCAT_TIPCAP_SYSTRAYSUPPORT,
        GUID_TFCAT_TIPCAP_COMLESS,
        GUID_TFCAT_TIPCAP_INPUTMODECOMPARTMENT,
    ]
}

fn category_manager() -> Result<ITfCategoryMgr> {
    unsafe { CoCreateInstance(&CLSID_TF_CategoryMgr, None, CLSCTX_INPROC_SERVER) }
}

fn register_categories() -> Result<()> {
    let manager = category_manager()?;
    for category in registered_categories() {
        unsafe { manager.RegisterCategory(&OHKEY_CLSID, &category, &OHKEY_CLSID)? };
    }
    Ok(())
}

fn unregister_categories() -> Result<()> {
    let manager = category_manager()?;
    let mut outcome = Ok(());
    for category in registered_categories() {
        let result = unsafe { manager.UnregisterCategory(&OHKEY_CLSID, &category, &OHKEY_CLSID) };
        outcome = outcome.and(result);
    }
    outcome
}

/// Adds the profile to the current user's language list. A failure only means
/// the user turns it on by hand in Settings, so it is not an error.
fn enable_for_current_user() {
    let tip = format!(
        "0x{:04X}:{}{}",
        VIETNAMESE_LANGUAGE_ID,
        braced(&OHKEY_CLSID),
        braced(&OHKEY_PROFILE_GUID)
    );
    let tip = HSTRING::from(tip);
    unsafe {
        let _ = InstallLayoutOrTip(PCWSTR(tip.as_ptr()), 0);
    }
}
