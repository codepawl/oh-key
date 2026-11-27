//! Self-update from GitHub Releases of the public `anxuanng/oh-key` repository (the
//! source stays private): find the latest tag, download the exe for this architecture and
//! `SHA256SUMS.txt`, verify the hash, try the new exe, swap it in place and restart.
//!
//! The latest tag comes from where `releases/latest` redirects, not from the REST API, whose
//! 60 calls an hour per address run out for people behind a shared one.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::{app_log, http};

const RELEASES: &str = "https://github.com/anxuanng/oh-key/releases";
const TAG_PAGE_PREFIX: &str = "https://github.com/anxuanng/oh-key/releases/tag/";
/// Set while a freshly installed version has not yet proved it keeps running.
const PROBATION_FILE: &str = "update-unconfirmed.txt";
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const PROGRAM_NAME: &str = "oh-key";
/// The new exe has this many seconds to print its version.
const TRY_SECONDS: u64 = 10;
const MAX_SUMS_BYTES: usize = 64 * 1024;
const MAX_EXE_BYTES: usize = 64 * 1024 * 1024;
const MIN_EXE_BYTES: usize = 100 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    pub asset_name: String,
    exe_url: String,
    sums_url: String,
}

pub enum Check {
    UpToDate,
    Available(Release),
}

/// The exe path from before any swap: once this exe is renamed aside, the system may report
/// the new name for the running image.
static EXE_PATH: OnceLock<PathBuf> = OnceLock::new();

pub fn remember_exe_path() {
    if let Ok(path) = std::env::current_exe() {
        let _ = EXE_PATH.set(path);
    }
}

fn exe_path() -> Result<PathBuf, String> {
    if let Some(path) = EXE_PATH.get() {
        return Ok(path.clone());
    }
    std::env::current_exe().map_err(|error| error.to_string())
}

/// The release file for a CPU architecture, as `std::env::consts::ARCH` names it.
pub fn asset_name_for_arch(architecture: &str) -> Option<&'static str> {
    match architecture {
        "x86_64" => Some("oh-key-windows-x64.exe"),
        "x86" => Some("oh-key-windows-x86.exe"),
        "aarch64" => Some("oh-key-windows-arm64.exe"),
        _ => None,
    }
}

/// Asks GitHub for the latest release and compares it with this build.
pub fn check() -> Result<Check, String> {
    let asset_name = asset_name_for_arch(std::env::consts::ARCH)
        .ok_or_else(|| format!("no release file for {}", std::env::consts::ARCH))?;
    let latest = http::head(&format!("{RELEASES}/latest"))?;
    if latest.status != 200 {
        return Err(format!("the releases page answered {}", latest.status));
    }
    let tag = tag_from_release_url(&latest.final_url)
        .ok_or_else(|| "the releases page named no release".to_string())?;
    match newer_version(&tag, CURRENT_VERSION) {
        None => Ok(Check::UpToDate),
        Some(version) => Ok(Check::Available(Release {
            version,
            asset_name: asset_name.to_string(),
            exe_url: format!("{RELEASES}/download/{tag}/{asset_name}"),
            sums_url: format!("{RELEASES}/download/{tag}/SHA256SUMS.txt"),
        })),
    }
}

/// "v0.5.0" from "https://github.com/anxuanng/oh-key/releases/tag/v0.5.0". Only that
/// repository's tag page counts, and only a plain version number is accepted as the tag.
pub fn tag_from_release_url(url: &str) -> Option<String> {
    let url = url.trim_end_matches('\0').trim();
    let tag = url.strip_prefix(TAG_PAGE_PREFIX)?.trim_end_matches('/');
    is_plain_version_tag(tag).then(|| tag.to_string())
}

/// `v1.2.3` or `1.2.3`: nothing that could change the download link or the version compare.
pub fn is_plain_version_tag(tag: &str) -> bool {
    let version = tag.strip_prefix('v').unwrap_or(tag);
    let parts: Vec<&str> = version.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty() && part.len() <= 9 && part.bytes().all(|byte| byte.is_ascii_digit())
        })
}

/// The version of `tag` when it is a release higher than `current`; never a downgrade, never
/// a tag that is not a version.
pub fn newer_version(tag: &str, current: &str) -> Option<String> {
    if !is_plain_version_tag(tag) {
        return None;
    }
    let version = tag.trim_start_matches('v');
    is_newer(version, current).then(|| version.to_string())
}

/// True when `candidate` (like "0.5.0") is a higher version than `current`. Both are split on
/// `.` and `-`, and the first three parts are compared as numbers.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    version_numbers(candidate) > version_numbers(current)
}

fn version_numbers(text: &str) -> Vec<u64> {
    text.split(['.', '-'])
        .take(3)
        .map(|part| part.parse::<u64>().unwrap_or(0))
        .collect()
}

/// The checksum of `asset` in a `sha256sum`-style file: `<64 hex digits>  <name>` per line.
pub fn checksum_for(sums: &str, asset: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (hash, name) = line.split_once(char::is_whitespace)?;
        let name = name.trim().trim_start_matches('*');
        let is_hash = hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit());
        (is_hash && name == asset).then(|| hash.to_ascii_lowercase())
    })
}

/// A Windows program of a believable size, not an error page saved as an exe.
pub fn looks_like_windows_program(bytes: &[u8]) -> bool {
    bytes.len() >= MIN_EXE_BYTES && bytes.starts_with(b"MZ")
}

/// Downloads the release's exe and returns it only when its SHA256 matches the release's list.
pub fn download(release: &Release) -> Result<Vec<u8>, String> {
    let sums = ok_body(http::get(&release.sums_url, MAX_SUMS_BYTES)?)?;
    let sums = String::from_utf8_lossy(&sums).to_string();
    let expected = checksum_for(&sums, &release.asset_name)
        .ok_or_else(|| format!("SHA256SUMS.txt has no line for {}", release.asset_name))?;
    let exe = ok_body(http::get(&release.exe_url, MAX_EXE_BYTES)?)?;
    let actual = http::sha256_hex(&exe)?;
    if actual != expected {
        return Err("the download does not match its checksum".into());
    }
    if !looks_like_windows_program(&exe) {
        return Err("the download is not a Windows program".into());
    }
    Ok(exe)
}

fn ok_body(response: http::Response) -> Result<Vec<u8>, String> {
    if response.status == 200 {
        Ok(response.body)
    } else {
        Err(format!("the server answered {}", response.status))
    }
}

/// A new exe that has been written next to this one and has run.
pub struct TriedExe {
    path: PathBuf,
    sha256: String,
}

impl TriedExe {
    /// Throws the tried exe away, for an update that is not going ahead.
    pub fn discard(self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Writes the new exe next to this one and runs it once. A release that does not start, or
/// that Windows blocks, stops here, while the working program is still untouched.
pub fn try_new_exe(new_exe: &[u8], version: &str) -> Result<TriedExe, String> {
    let current = exe_path()?;
    let sha256 = http::sha256_hex(new_exe)?;
    write_and_try(&new_exe_path(&current), new_exe, sha256, |path| {
        reports_version(path, version)
    })
}

/// Puts the tried exe where this one is. Windows lets a running exe be renamed, so this one
/// moves aside to `oh-key.old.exe`, which the next start deletes.
pub fn install(tried: TriedExe) -> Result<(), String> {
    if let Err(error) = verify_unchanged(&tried) {
        tried.discard();
        return Err(error);
    }
    swap_in(&tried, &exe_path()?)
}

/// The file on disk is still the bytes that were checked.
fn verify_unchanged(tried: &TriedExe) -> Result<(), String> {
    let bytes = std::fs::read(&tried.path)
        .map_err(|error| format!("could not read the new program again: {error}"))?;
    if http::sha256_hex(&bytes)? != tried.sha256 {
        return Err("the new program changed on disk before it was installed".into());
    }
    Ok(())
}

/// Starts the new version. When it cannot start, the old files are put back.
pub fn restart_into_new_version() -> Result<(), String> {
    let current = exe_path()?;
    match std::process::Command::new(&current)
        .arg("--after-update")
        .spawn()
    {
        Ok(_) => {
            HANDING_OVER.store(true, Ordering::SeqCst);
            Ok(())
        }
        Err(error) => {
            roll_back(&current)?;
            Err(format!("the new version would not start: {error}"))
        }
    }
}

fn write_and_try(
    path: &Path,
    new_exe: &[u8],
    sha256: String,
    try_it: impl Fn(&Path) -> Result<(), String>,
) -> Result<TriedExe, String> {
    std::fs::write(path, new_exe)
        .map_err(|error| format!("could not write the new program: {error}"))?;
    if let Err(error) = try_it(path) {
        let _ = std::fs::remove_file(path);
        return Err(error);
    }
    Ok(TriedExe {
        path: path.to_path_buf(),
        sha256,
    })
}

fn swap_in(tried: &TriedExe, current: &Path) -> Result<(), String> {
    let old = old_exe_path(current);
    let _ = std::fs::remove_file(&old);
    std::fs::rename(current, &old)
        .map_err(|error| format!("could not move the running program aside: {error}"))?;
    if let Err(error) = std::fs::rename(&tried.path, current) {
        return Err(match std::fs::rename(&old, current) {
            Ok(()) => format!("could not put the new program in place: {error}"),
            Err(restore_error) => format!(
                "could not put the new program in place ({error}) and could not restore the old one ({restore_error})"
            ),
        });
    }
    Ok(())
}

/// Undoes `swap_in`: the program that was moved aside takes its place again. The bad program
/// is renamed out of the way instead of deleted, so a lock on it cannot leave no program at
/// all; the old one is never removed before the rename that restores it.
fn roll_back(current: &Path) -> Result<(), String> {
    let old = old_exe_path(current);
    if !old.exists() {
        return Err("the old program is gone, nothing to roll back to".into());
    }
    let failed = failed_exe_path(current);
    let _ = std::fs::remove_file(&failed);
    if let Err(error) = std::fs::rename(current, &failed) {
        app_log::write(&format!(
            "rollback: could not move the new program aside: {error}"
        ));
    }
    std::fs::rename(&old, current)
        .map_err(|error| format!("could not roll back to the old program: {error}"))
}

/// Called when the tray app starts: a version that was installed but died before it was
/// confirmed gives way to the one it replaced. `Some(Ok(message))`: the old program is back in
/// place and this process should hand over to it; `Some(Err(message))`: the rollback failed
/// and this version keeps running. Both messages are for the log and a report.
pub fn roll_back_unconfirmed_update(started_after_update: bool) -> Option<Result<String, String>> {
    let marker = app_log::data_folder().join(PROBATION_FILE);
    if started_after_update || !marker.exists() {
        return None;
    }
    let _ = std::fs::remove_file(&marker);
    let current = exe_path().ok()?;
    Some(match roll_back(&current) {
        Ok(()) => Ok(
            "the new version stopped within minutes of an update; went back to the old one".into(),
        ),
        Err(error) => Err(format!(
            "the new version stopped within minutes of an update and the rollback failed: {error}"
        )),
    })
}

/// Starts the program a rollback restored. It waits for this process to let go of the lock.
pub fn restart_after_rollback() {
    if let Ok(current) = exe_path() {
        let _ = std::process::Command::new(current)
            .arg("--after-rollback")
            .spawn();
    }
}

/// Set once the next version has been started, so this one does not confirm itself.
static HANDING_OVER: AtomicBool = AtomicBool::new(false);

/// Marks a fresh update as unproven. The old program stays until `confirm_update`.
pub fn begin_probation() {
    let _ = std::fs::create_dir_all(app_log::data_folder());
    let _ = std::fs::write(app_log::data_folder().join(PROBATION_FILE), CURRENT_VERSION);
}

/// The new version has kept running: the old program is no longer needed.
pub fn confirm_update() {
    if HANDING_OVER.load(Ordering::SeqCst) {
        return;
    }
    let _ = std::fs::remove_file(app_log::data_folder().join(PROBATION_FILE));
    if let Ok(current) = exe_path() {
        let _ = std::fs::remove_file(old_exe_path(&current));
        let _ = std::fs::remove_file(failed_exe_path(&current));
    }
}

/// Runs `<exe> --version` and wants this version back within a few seconds.
fn reports_version(exe: &Path, version: &str) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut child = std::process::Command::new(exe)
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|error| format!("the new program would not start: {error}"))?;
    let started = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed().as_secs() > TRY_SECONDS => {
                let _ = child.kill();
                return Err("the new program did not answer".into());
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(20)),
            Err(error) => return Err(error.to_string()),
        }
    }
    let output = child
        .wait_with_output()
        .map_err(|error| error.to_string())?;
    let answer = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() || answer.trim() != format!("{PROGRAM_NAME} {version}") {
        return Err(format!(
            "the new program answered with exit code {:?}",
            output.status.code()
        ));
    }
    Ok(())
}

/// Removes what an update left behind. The old program stays while the version that replaced
/// it is on probation.
pub fn clean_up_old_exe(keep_old_program: bool) {
    let Ok(current) = exe_path() else {
        return;
    };
    let _ = std::fs::remove_file(new_exe_path(&current));
    if !keep_old_program {
        let _ = std::fs::remove_file(old_exe_path(&current));
        let _ = std::fs::remove_file(failed_exe_path(&current));
    }
}

fn failed_exe_path(current: &Path) -> PathBuf {
    current.with_file_name(format!("{PROGRAM_NAME}.failed.exe"))
}

fn new_exe_path(current: &Path) -> PathBuf {
    current.with_file_name(format!("{PROGRAM_NAME}.new.exe"))
}

fn old_exe_path(current: &Path) -> PathBuf {
    current.with_file_name(format!("{PROGRAM_NAME}.old.exe"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_folder(name: &str) -> PathBuf {
        let folder =
            std::env::temp_dir().join(format!("oh-key-update-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        folder
    }

    #[test]
    fn versions_compare_by_number() {
        assert!(is_newer("0.5.0", "0.4.1"));
        assert!(is_newer("0.10.0", "0.9.9"));
        assert!(is_newer("1.0.0", "0.99.99"));
        assert!(!is_newer("0.4.1", "0.4.1"));
        assert!(!is_newer("0.4.0", "0.4.1"));
    }

    #[test]
    fn only_the_first_three_parts_count_and_a_suffix_is_cut_at_the_dash() {
        assert!(!is_newer("1.2.3.9", "1.2.3"));
        assert!(!is_newer("1.2.3-beta", "1.2.3"));
        assert!(is_newer("1.2.4-beta", "1.2.3"));
    }

    #[test]
    fn a_downgrade_or_the_same_version_is_never_offered() {
        assert_eq!(newer_version("v0.1.0", "0.2.0"), None);
        assert_eq!(newer_version("v0.2.0", "0.2.0"), None);
        assert_eq!(newer_version("0.2.0", "0.2.0"), None);
        assert_eq!(newer_version("v0.3.0", "0.2.0").as_deref(), Some("0.3.0"));
    }

    #[test]
    fn a_tag_that_is_not_a_version_is_ignored() {
        assert_eq!(newer_version("nightly", "0.2.0"), None);
        assert_eq!(newer_version("", "0.2.0"), None);
        assert_eq!(newer_version("v9.9.9?x=1", "0.2.0"), None);
        assert_eq!(newer_version("v9.9.9%00", "0.2.0"), None);
        assert_eq!(newer_version("v9.9.9-beta", "0.2.0"), None);
        assert_eq!(newer_version("v1.2", "0.2.0"), None);
    }

    #[test]
    fn only_a_plain_version_tag_passes() {
        assert!(is_plain_version_tag("v0.5.0"));
        assert!(is_plain_version_tag("10.20.30"));
        assert!(!is_plain_version_tag("v0.5.0\0"));
        assert!(!is_plain_version_tag("v0.5.0/../x"));
        assert!(!is_plain_version_tag("v.."));
        assert!(!is_plain_version_tag("v0..1"));
    }

    #[test]
    fn reads_the_tag_from_the_redirect() {
        let url = "https://github.com/anxuanng/oh-key/releases/tag/v0.5.0";
        assert_eq!(tag_from_release_url(url).as_deref(), Some("v0.5.0"));
        assert_eq!(
            tag_from_release_url("https://github.com/anxuanng/oh-key/releases"),
            None
        );
        assert_eq!(tag_from_release_url("https://x/releases/tag/a/b"), None);
        let with_nul = "https://github.com/anxuanng/oh-key/releases/tag/v0.5.0\0";
        assert_eq!(tag_from_release_url(with_nul).as_deref(), Some("v0.5.0"));
        let other_host = "https://evil.example/anxuanng/oh-key/releases/tag/v0.5.0";
        assert_eq!(tag_from_release_url(other_host), None);
        let other_repo = "https://github.com/someone/else/releases/tag/v0.5.0";
        assert_eq!(tag_from_release_url(other_repo), None);
    }

    #[test]
    fn each_architecture_has_its_own_release_file() {
        assert_eq!(
            asset_name_for_arch("x86_64"),
            Some("oh-key-windows-x64.exe")
        );
        assert_eq!(asset_name_for_arch("x86"), Some("oh-key-windows-x86.exe"));
        assert_eq!(
            asset_name_for_arch("aarch64"),
            Some("oh-key-windows-arm64.exe")
        );
        assert_eq!(asset_name_for_arch("riscv64"), None);
        assert!(asset_name_for_arch(std::env::consts::ARCH).is_some());
    }

    #[test]
    fn finds_the_checksum_of_the_right_file() {
        let sums = format!(
            "{}  oh-key-windows-x64.exe\n{} *oh-key-windows-x86.exe\nnot a line\n{}  oh-key-windows-arm64.exe.sig\n",
            "A".repeat(64),
            "b".repeat(64),
            "c".repeat(64),
        );
        assert_eq!(
            checksum_for(&sums, "oh-key-windows-x64.exe"),
            Some("a".repeat(64))
        );
        assert_eq!(
            checksum_for(&sums, "oh-key-windows-x86.exe"),
            Some("b".repeat(64))
        );
        assert_eq!(checksum_for(&sums, "oh-key-windows-arm64.exe"), None);
    }

    #[test]
    fn a_short_or_non_hex_hash_is_not_a_checksum() {
        assert_eq!(
            checksum_for("abc  oh-key-windows-x64.exe\n", "oh-key-windows-x64.exe"),
            None
        );
        let not_hex = format!("{}  oh-key-windows-x64.exe\n", "z".repeat(64));
        assert_eq!(checksum_for(&not_hex, "oh-key-windows-x64.exe"), None);
    }

    #[test]
    fn only_a_big_enough_mz_file_counts_as_a_program() {
        let mut program = vec![0u8; MIN_EXE_BYTES];
        program[0] = b'M';
        program[1] = b'Z';
        assert!(looks_like_windows_program(&program));
        assert!(!looks_like_windows_program(&program[..MIN_EXE_BYTES - 1]));
        program[0] = b'<';
        assert!(!looks_like_windows_program(&program));
    }

    #[test]
    fn a_new_exe_that_fails_its_try_leaves_everything_as_it_was() {
        let folder = test_folder("fails");
        let current = folder.join("oh-key.exe");
        std::fs::write(&current, b"working").unwrap();
        let new_path = new_exe_path(&current);
        let result = write_and_try(&new_path, b"broken", String::new(), |_| {
            Err("does not start".into())
        });
        assert!(result.is_err());
        assert!(!new_path.exists());
        assert_eq!(std::fs::read(&current).unwrap(), b"working");
        assert!(!old_exe_path(&current).exists());
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_tried_exe_takes_over_and_the_old_one_moves_aside() {
        let folder = test_folder("works");
        let current = folder.join("oh-key.exe");
        std::fs::write(&current, b"working").unwrap();
        let tried =
            write_and_try(&new_exe_path(&current), b"newer", String::new(), |_| Ok(())).unwrap();
        swap_in(&tried, &current).unwrap();
        assert_eq!(std::fs::read(&current).unwrap(), b"newer");
        assert_eq!(std::fs::read(old_exe_path(&current)).unwrap(), b"working");
        assert!(!new_exe_path(&current).exists());
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn rolling_back_puts_the_old_program_where_it_was() {
        let folder = test_folder("rollback");
        let current = folder.join("oh-key.exe");
        std::fs::write(&current, b"working").unwrap();
        let tried =
            write_and_try(&new_exe_path(&current), b"newer", String::new(), |_| Ok(())).unwrap();
        swap_in(&tried, &current).unwrap();
        roll_back(&current).unwrap();
        assert_eq!(std::fs::read(&current).unwrap(), b"working");
        assert!(!old_exe_path(&current).exists());
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_swap_that_cannot_place_the_new_exe_puts_the_old_one_back() {
        let folder = test_folder("swap-fails");
        let current = folder.join("oh-key.exe");
        std::fs::write(&current, b"working").unwrap();
        let missing = TriedExe {
            path: folder.join("never-written.exe"),
            sha256: String::new(),
        };
        assert!(swap_in(&missing, &current).is_err());
        assert_eq!(std::fs::read(&current).unwrap(), b"working");
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn rolling_back_without_an_old_program_is_an_error() {
        let folder = test_folder("no-old");
        let current = folder.join("oh-key.exe");
        std::fs::write(&current, b"working").unwrap();
        assert!(roll_back(&current).is_err());
        assert_eq!(std::fs::read(&current).unwrap(), b"working");
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_program_that_is_not_oh_key_fails_the_try() {
        let folder = test_folder("garbage");
        let fake = folder.join("fake.exe");
        std::fs::write(&fake, b"MZ this is not a program").unwrap();
        assert!(reports_version(&fake, "9.9.9").is_err());
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_rollback_sets_the_bad_program_aside_and_restores_the_old_one() {
        let folder = test_folder("rollback-aside");
        let current = folder.join("oh-key.exe");
        std::fs::write(&current, b"working").unwrap();
        let tried =
            write_and_try(&new_exe_path(&current), b"newer", String::new(), |_| Ok(())).unwrap();
        swap_in(&tried, &current).unwrap();
        roll_back(&current).unwrap();
        assert_eq!(std::fs::read(&current).unwrap(), b"working");
        assert_eq!(std::fs::read(failed_exe_path(&current)).unwrap(), b"newer");
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_changed_file_is_not_installed() {
        let folder = test_folder("changed");
        let path = folder.join("oh-key.new.exe");
        std::fs::write(&path, b"swapped").unwrap();
        let tried = TriedExe {
            path,
            sha256: http::sha256_hex(b"checked").unwrap(),
        };
        assert!(verify_unchanged(&tried).is_err());
        let _ = std::fs::remove_dir_all(&folder);
    }
}
