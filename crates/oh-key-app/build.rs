//! Embeds `assets/oh-key.ico` in the Windows exe, so Explorer, the taskbar and the Start
//! menu show the oh-key mark. The icon goes in as a compiled resource (`.res`) file written
//! here and handed to the MSVC linker, which needs no resource compiler on the build machine.
//! Other targets build no resources.

use std::path::{Path, PathBuf};

const ICON_PATH: &str = "assets/oh-key.ico";
const RT_ICON: u16 = 3;
const RT_GROUP_ICON: u16 = 14;
const GROUP_ICON_ID: u16 = 1;
const MOVEABLE_PURE_DISCARDABLE: u16 = 0x1030;
const MOVEABLE_DISCARDABLE: u16 = 0x1010;
const ICO_HEADER_SIZE: usize = 6;
const ICO_ENTRY_SIZE: usize = 16;

struct IconImage<'icon> {
    /// The 12 directory bytes before the image offset: size, colours, planes, bits, length.
    description: &'icon [u8],
    data: &'icon [u8],
}

fn main() {
    println!("cargo:rerun-if-changed={ICON_PATH}");
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_os != "windows" || target_env != "msvc" {
        return;
    }
    let icon = std::fs::read(ICON_PATH).expect("assets/oh-key.ico must be readable");
    let images = parse_ico(&icon);
    let output_folder = PathBuf::from(std::env::var("OUT_DIR").expect("cargo sets OUT_DIR"));
    let resource_path = output_folder.join("oh-key-icon.res");
    write_resource_file(&resource_path, &images);
    println!("cargo:rustc-link-arg-bins={}", resource_path.display());
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn read_u32(bytes: &[u8], offset: usize) -> usize {
    let word = [
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ];
    u32::from_le_bytes(word) as usize
}

fn parse_ico(icon: &[u8]) -> Vec<IconImage<'_>> {
    let count = read_u16(icon, 4) as usize;
    let mut images = Vec::with_capacity(count);
    for index in 0..count {
        let entry = ICO_HEADER_SIZE + index * ICO_ENTRY_SIZE;
        let length = read_u32(icon, entry + 8);
        let offset = read_u32(icon, entry + 12);
        images.push(IconImage {
            description: &icon[entry..entry + 12],
            data: &icon[offset..offset + length],
        });
    }
    images
}

/// One resource entry with a numeric type and name, padded to four bytes.
fn push_resource(file: &mut Vec<u8>, kind: u16, name: u16, memory_flags: u16, data: &[u8]) {
    const HEADER_SIZE: u32 = 32;
    file.extend_from_slice(&(data.len() as u32).to_le_bytes());
    file.extend_from_slice(&HEADER_SIZE.to_le_bytes());
    file.extend_from_slice(&0xFFFFu16.to_le_bytes());
    file.extend_from_slice(&kind.to_le_bytes());
    file.extend_from_slice(&0xFFFFu16.to_le_bytes());
    file.extend_from_slice(&name.to_le_bytes());
    // Data version, then memory flags, language (neutral), version and characteristics.
    file.extend_from_slice(&0u32.to_le_bytes());
    file.extend_from_slice(&memory_flags.to_le_bytes());
    file.extend_from_slice(&0u16.to_le_bytes());
    file.extend_from_slice(&0u32.to_le_bytes());
    file.extend_from_slice(&0u32.to_le_bytes());
    file.extend_from_slice(data);
    while !file.len().is_multiple_of(4) {
        file.push(0);
    }
}

fn write_resource_file(path: &Path, images: &[IconImage]) {
    let mut file = Vec::new();
    // A .res file starts with an empty entry that marks it as 32-bit.
    push_resource(&mut file, 0, 0, 0, &[]);
    let mut group = Vec::new();
    group.extend_from_slice(&0u16.to_le_bytes());
    group.extend_from_slice(&1u16.to_le_bytes());
    group.extend_from_slice(&(images.len() as u16).to_le_bytes());
    for (index, image) in images.iter().enumerate() {
        let icon_id = index as u16 + 1;
        push_resource(
            &mut file,
            RT_ICON,
            icon_id,
            MOVEABLE_DISCARDABLE,
            image.data,
        );
        group.extend_from_slice(image.description);
        group.extend_from_slice(&icon_id.to_le_bytes());
    }
    push_resource(
        &mut file,
        RT_GROUP_ICON,
        GROUP_ICON_ID,
        MOVEABLE_PURE_DISCARDABLE,
        &group,
    );
    std::fs::write(path, file).expect("the icon resource file must be writable");
}
