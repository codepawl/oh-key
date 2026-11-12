//! The named file mapping that carries the state block between processes.

use crate::block::StateBlock;
use crate::settings::Settings;
use windows::Win32::Foundation::{CloseHandle, HANDLE, HLOCAL, INVALID_HANDLE_VALUE, LocalFree};
use windows::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use windows::Win32::System::Memory::{
    CreateFileMappingW, FILE_MAP_READ, FILE_MAP_WRITE, MEMORY_MAPPED_VIEW_ADDRESS, MapViewOfFile,
    OpenFileMappingW, PAGE_READWRITE, UnmapViewOfFile,
};
use windows::core::{HSTRING, PCWSTR, Result, w};

const MAPPING_NAME: &str = "Local\\oh-key-state";

/// System and the signed-in user get read/write, and so do app containers
/// (UWP, Edge sandbox) through ALL APPLICATION PACKAGES. The low mandatory
/// label lets low-integrity processes write as well.
const SECURITY_DESCRIPTOR: PCWSTR =
    w!("D:(A;;GRGW;;;SY)(A;;GRGW;;;IU)(A;;GRGW;;;AC)S:(ML;;NW;;;LW)");

pub struct SharedState {
    mapping: HANDLE,
    view: MEMORY_MAPPED_VIEW_ADDRESS,
}

// The block is only touched through atomics.
unsafe impl Send for SharedState {}
unsafe impl Sync for SharedState {}

impl SharedState {
    /// Creates the mapping, or opens it when another process made it first.
    pub fn open() -> Result<Self> {
        Self::open_named(MAPPING_NAME)
    }

    pub(crate) fn open_named(name: &str) -> Result<Self> {
        let mapping = open_or_create_mapping(name)?;
        let view = unsafe {
            MapViewOfFile(
                mapping,
                FILE_MAP_READ | FILE_MAP_WRITE,
                0,
                0,
                size_of::<StateBlock>(),
            )
        };
        if view.Value.is_null() {
            let error = windows::core::Error::from_thread();
            let _ = unsafe { CloseHandle(mapping) };
            return Err(error);
        }
        Ok(SharedState { mapping, view })
    }

    fn block(&self) -> &StateBlock {
        // The view is page aligned, zero filled and lives as long as `self`.
        unsafe { &*self.view.Value.cast::<StateBlock>() }
    }

    /// Bumped on every settings write.
    pub fn generation(&self) -> u64 {
        self.block().generation()
    }

    pub fn vietnamese(&self) -> bool {
        self.block().vietnamese()
    }

    pub fn set_vietnamese(&self, on: bool) {
        self.block().set_vietnamese(on);
    }

    pub fn mode_counter(&self) -> u64 {
        self.block().mode_counter()
    }

    pub fn publish_settings(&self, settings: &Settings) {
        self.block().publish_settings(settings);
    }

    /// `None` until something was published.
    pub fn read_settings(&self) -> Option<Settings> {
        self.block().read_settings()
    }
}

impl Drop for SharedState {
    fn drop(&mut self) {
        let _ = unsafe { UnmapViewOfFile(self.view) };
        let _ = unsafe { CloseHandle(self.mapping) };
    }
}

/// Opening comes first: `CreateFileMappingW` on an existing mapping asks for
/// full access, which the descriptor deliberately does not grant. When two
/// processes race to create, the loser opens what the winner made.
fn open_or_create_mapping(name: &str) -> Result<HANDLE> {
    if let Ok(mapping) = open_mapping(name) {
        return Ok(mapping);
    }
    create_mapping(name).or_else(|create_error| open_mapping(name).map_err(|_| create_error))
}

fn open_mapping(name: &str) -> Result<HANDLE> {
    let access = (FILE_MAP_READ | FILE_MAP_WRITE).0;
    unsafe { OpenFileMappingW(access, false, &HSTRING::from(name)) }
}

fn create_mapping(name: &str) -> Result<HANDLE> {
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            SECURITY_DESCRIPTOR,
            SDDL_REVISION_1,
            &mut descriptor,
            None,
        )?;
    }
    let attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: false.into(),
    };
    let mapping = unsafe {
        CreateFileMappingW(
            INVALID_HANDLE_VALUE,
            Some(&attributes),
            PAGE_READWRITE,
            0,
            size_of::<StateBlock>() as u32,
            &HSTRING::from(name),
        )
    };
    // The kernel copied the descriptor into the object.
    let _ = unsafe { LocalFree(Some(HLOCAL(descriptor.0))) };
    mapping
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_handles_in_one_process_see_each_others_writes() {
        let name = format!("Local\\oh-key-state-test-{}", std::process::id());
        let writer = SharedState::open_named(&name).expect("create mapping");
        let reader = SharedState::open_named(&name).expect("open mapping");
        assert!(reader.read_settings().is_none());

        let settings = Settings {
            excluded_apps: vec!["game.exe".to_owned()],
            ..Settings::default()
        };
        writer.publish_settings(&settings);
        assert_eq!(reader.generation(), 1);
        assert_eq!(reader.read_settings(), Some(settings));

        reader.set_vietnamese(false);
        assert!(!writer.vietnamese());
        assert_eq!(writer.mode_counter(), 1);
    }
}
