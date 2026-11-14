//! English while a game has the focus: WASD, Q, E and the rest must reach the game as typed,
//! not as Telex marks. When a game window comes to the front in Vietnamese mode, the global mode
//! switches to English; when the focus leaves for a program that is not a game, the mode the
//! user had before comes back. Ctrl+Shift still works in the game, for its chat.
//!
//! A game is recognised with no setup and without guessing from the screen: full screen is no
//! sign of a game (a browser, a video, a slide show), so only two facts count.
//! - Windows' own list of the games on this machine: Xbox Game Bar records every game it
//!   matched in `HKCU\System\GameConfigStore\Children`, with `MatchedExeFullPath`.
//! - The folders the big stores install games into (Steam, Epic, GOG, Riot, EA, Ubisoft, Xbox).
//!
//! Launchers and editors that live in those folders are not games: their chat and search fields
//! keep Vietnamese.
//!
//! The watch runs on the tray thread through an out-of-context `SetWinEventHook`, so it costs
//! the keyboard hook nothing and the tray icon changes as the game appears, before any key.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use oh_key_ipc::{SharedState, load_settings};
use windows::Win32::Foundation::{ERROR_SUCCESS, HWND};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, RRF_RT_REG_SZ, RegCloseKey, RegEnumKeyExW, RegGetValueW,
    RegOpenKeyExW,
};
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook};
use windows::Win32::UI::WindowsAndMessaging::{
    EVENT_SYSTEM_FOREGROUND, GetForegroundWindow, GetWindowThreadProcessId, WINEVENT_OUTOFCONTEXT,
    WINEVENT_SKIPOWNPROCESS,
};
use windows::core::{PWSTR, w};

use crate::hook::process_image_path;

/// Folders that only hold installed games, lower case with their backslashes.
const GAME_FOLDERS: [&str; 9] = [
    "\\steamapps\\common\\",
    "\\epic games\\",
    "\\gog galaxy\\games\\",
    "\\gog games\\",
    "\\riot games\\",
    "\\ea games\\",
    "\\ubisoft game launcher\\games\\",
    "\\xboxgames\\",
    "\\program files\\windowsapps\\microsoft.minecraft",
];

/// Parts of those folders that hold launchers, engines and redistributables, not games.
const NOT_GAME_FOLDERS: [&str; 4] = [
    "\\epic games\\launcher\\",
    "\\epic games\\ue_",
    "\\riot games\\riot client\\",
    "\\steamapps\\common\\steamworks shared\\",
];

/// Programs found in those folders, or on Windows' game list, where people type text: store
/// launchers with chat, and game editors.
const NOT_GAMES: [&str; 11] = [
    "epicgameslauncher.exe",
    "unrealeditor.exe",
    "riotclientservices.exe",
    "riotclientux.exe",
    "riotclientuxrender.exe",
    "leagueclient.exe",
    "leagueclientux.exe",
    "leagueclientuxrender.exe",
    "robloxstudiobeta.exe",
    "robloxstudio.exe",
    "steamwebhelper.exe",
];

/// Windows' game list is read again at most this often, when a program not seen before comes
/// to the front, so a game installed while the app runs is known from its next launch.
const GAME_LIST_REFRESH_MILLISECONDS: u64 = 60_000;
/// The most program paths remembered by process id; a full cache starts over.
const PATH_CACHE_LIMIT: usize = 64;

/// Whether the program at `path` is a game. `known_games` holds lower case full paths.
pub fn is_game_path(path: &str, known_games: &HashSet<String>) -> bool {
    let path = path.to_lowercase();
    let file_name = path.rsplit(['\\', '/']).next().unwrap_or(&path);
    let not_a_game = NOT_GAMES.contains(&file_name)
        || NOT_GAME_FOLDERS.iter().any(|folder| path.contains(folder));
    if path.is_empty() || not_a_game {
        return false;
    }
    known_games.contains(&path) || GAME_FOLDERS.iter().any(|folder| path.contains(folder))
}

/// The mode switch that follows the focus in and out of games.
#[derive(Debug, Default)]
pub struct GameSwitch {
    /// The mode the user had when the focus went into a game; `None` outside games.
    mode_before_game: Option<bool>,
}

impl GameSwitch {
    /// The focus moved to a window; returns the mode to set, if it has to change.
    pub fn focus_moved(&mut self, is_game: bool, vietnamese: bool, enabled: bool) -> Option<bool> {
        if !enabled {
            self.mode_before_game = None;
            return None;
        }
        match (self.mode_before_game, is_game) {
            (None, true) => {
                self.mode_before_game = Some(vietnamese);
                vietnamese.then_some(false)
            }
            (Some(before), false) => {
                self.mode_before_game = None;
                (before != vietnamese).then_some(before)
            }
            _ => None,
        }
    }
}

/// Everything the watch needs between two focus changes, on the tray thread.
struct Watch {
    shared: SharedState,
    switch: GameSwitch,
    known_games: HashSet<String>,
    game_list_read_at: Option<u64>,
    paths: HashMap<u32, String>,
}

thread_local! {
    static WATCH: RefCell<Option<Watch>> = const { RefCell::new(None) };
}

/// Starts the watch on the calling thread, which must run a message loop (the tray's). Measured
/// builds without the keyboard hook leave the mode alone.
pub fn start() {
    if std::env::var_os("OHKEY_NO_HOOK").is_some_and(|value| value == "1") {
        return;
    }
    let Ok(shared) = SharedState::open() else {
        return;
    };
    let mut watch = Watch {
        shared,
        switch: GameSwitch::default(),
        known_games: HashSet::new(),
        game_list_read_at: None,
        paths: HashMap::new(),
    };
    watch.refresh_game_list();
    WATCH.with(|cell| *cell.borrow_mut() = Some(watch));
    let flags = WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS;
    let hook = unsafe {
        SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            None,
            Some(foreground_changed),
            0,
            0,
            flags,
        )
    };
    if hook.is_invalid() {
        return;
    }
    // A game that was already in front when the app started.
    focus_moved_to(unsafe { GetForegroundWindow() });
}

unsafe extern "system" fn foreground_changed(
    _hook: HWINEVENTHOOK,
    _event: u32,
    window: HWND,
    _object: i32,
    _child: i32,
    _thread: u32,
    _time: u32,
) {
    focus_moved_to(window);
}

fn focus_moved_to(window: HWND) {
    if window.is_invalid() {
        return;
    }
    WATCH.with(|cell| {
        if let Ok(mut guard) = cell.try_borrow_mut()
            && let Some(watch) = guard.as_mut()
        {
            watch.follow(window);
        }
    });
}

impl Watch {
    fn follow(&mut self, window: HWND) {
        let mut process_id = 0u32;
        unsafe { GetWindowThreadProcessId(window, Some(&mut process_id)) };
        if process_id == 0 {
            return;
        }
        // An elevated program cannot be read; the keyboard hook does not reach it either.
        let path = self.path_of(process_id);
        let is_game = is_game_path(&path, &self.known_games);
        let enabled = self.enabled();
        let vietnamese = self.shared.vietnamese();
        if let Some(mode) = self.switch.focus_moved(is_game, vietnamese, enabled) {
            // The tray wakes on the mode event, redraws the icon and plays the switch effects.
            self.shared.set_vietnamese(mode);
        }
    }

    fn enabled(&self) -> bool {
        match self.shared.read_settings() {
            Some(settings) => settings.english_in_games,
            None => load_settings().english_in_games,
        }
    }

    fn path_of(&mut self, process_id: u32) -> String {
        if let Some(known) = self.paths.get(&process_id) {
            return known.clone();
        }
        self.refresh_game_list_when_due();
        let path = process_image_path(process_id).unwrap_or_default();
        if self.paths.len() >= PATH_CACHE_LIMIT {
            self.paths.clear();
        }
        self.paths.insert(process_id, path.clone());
        path
    }

    fn refresh_game_list_when_due(&mut self) {
        let now = unsafe { GetTickCount64() };
        let due = self
            .game_list_read_at
            .is_none_or(|read_at| now.saturating_sub(read_at) >= GAME_LIST_REFRESH_MILLISECONDS);
        if due {
            self.refresh_game_list();
        }
    }

    fn refresh_game_list(&mut self) {
        self.known_games = read_windows_game_list();
        self.game_list_read_at = Some(unsafe { GetTickCount64() });
    }
}

/// The executables Xbox Game Bar matched as games, lower case. Empty when the list is missing.
fn read_windows_game_list() -> HashSet<String> {
    let mut games = HashSet::new();
    let mut key = HKEY::default();
    let opened = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            w!("System\\GameConfigStore\\Children"),
            None,
            KEY_READ,
            &mut key,
        )
    };
    if opened != ERROR_SUCCESS {
        return games;
    }
    let mut index = 0u32;
    while let Some(child) = child_key_name(key, index) {
        if let Some(path) = matched_exe_path(key, &child) {
            games.insert(path.to_lowercase());
        }
        index += 1;
    }
    let _ = unsafe { RegCloseKey(key) };
    games
}

fn child_key_name(key: HKEY, index: u32) -> Option<Vec<u16>> {
    let mut name = [0u16; 256];
    let mut length = name.len() as u32;
    let enumerated = unsafe {
        RegEnumKeyExW(
            key,
            index,
            Some(PWSTR(name.as_mut_ptr())),
            &mut length,
            None,
            None,
            None,
            None,
        )
    };
    if enumerated != ERROR_SUCCESS {
        return None;
    }
    let mut child: Vec<u16> = name[..length as usize].to_vec();
    child.push(0);
    Some(child)
}

fn matched_exe_path(key: HKEY, child: &[u16]) -> Option<String> {
    let mut buffer = [0u16; 1024];
    let mut size = (buffer.len() * size_of::<u16>()) as u32;
    let read = unsafe {
        RegGetValueW(
            key,
            windows::core::PCWSTR(child.as_ptr()),
            w!("MatchedExeFullPath"),
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    if read != ERROR_SUCCESS {
        return None;
    }
    let units = (size as usize / size_of::<u16>()).saturating_sub(1);
    let path = String::from_utf16_lossy(&buffer[..units]);
    (!path.is_empty()).then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn windows_list(paths: &[&str]) -> HashSet<String> {
        paths.iter().map(|path| path.to_lowercase()).collect()
    }

    #[test]
    fn store_folders_and_windows_list_are_games() {
        let known = windows_list(&[
            "C:\\Users\\a\\AppData\\Local\\Roblox\\Versions\\v1\\RobloxPlayerBeta.exe",
        ]);
        for path in [
            "C:\\Program Files (x86)\\Steam\\steamapps\\common\\Factorio\\bin\\x64\\factorio.exe",
            "D:\\SteamLibrary\\steamapps\\common\\Apex Legends\\r5apex.exe",
            "C:\\Program Files\\Epic Games\\Fortnite\\FortniteGame\\Binaries\\Win64\\FortniteClient-Win64-Shipping.exe",
            "C:\\Riot Games\\VALORANT\\live\\VALORANT.exe",
            "C:\\Riot Games\\League of Legends\\Game\\League of Legends.exe",
            "C:\\XboxGames\\Halo Infinite\\Content\\HaloInfinite.exe",
            "C:\\Users\\a\\AppData\\Local\\Roblox\\Versions\\v1\\RobloxPlayerBeta.exe",
        ] {
            assert!(is_game_path(path, &known), "{path}");
        }
    }

    #[test]
    fn browsers_players_editors_and_launchers_are_not_games() {
        let known = windows_list(&[
            "C:\\Users\\a\\AppData\\Local\\Roblox\\Versions\\v1\\RobloxStudioBeta.exe",
        ]);
        for path in [
            "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
            "C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe",
            "C:\\Program Files\\VideoLAN\\VLC\\vlc.exe",
            "C:\\Program Files\\Microsoft Office\\root\\Office16\\POWERPNT.EXE",
            "C:\\Program Files (x86)\\Steam\\steam.exe",
            "C:\\Program Files (x86)\\Steam\\bin\\cef\\cef.win7x64\\steamwebhelper.exe",
            "C:\\Program Files (x86)\\Epic Games\\Launcher\\Portal\\Binaries\\Win64\\EpicGamesLauncher.exe",
            "C:\\Program Files\\Epic Games\\UE_5.4\\Engine\\Binaries\\Win64\\UnrealEditor.exe",
            "C:\\Riot Games\\League of Legends\\LeagueClientUx.exe",
            "C:\\Riot Games\\Riot Client\\RiotClientServices.exe",
            "C:\\Users\\a\\AppData\\Local\\Roblox\\Versions\\v1\\RobloxStudioBeta.exe",
            "",
        ] {
            assert!(!is_game_path(path, &known), "{path}");
        }
    }

    #[test]
    fn a_game_in_front_switches_to_english_and_leaving_restores_vietnamese() {
        let mut switch = GameSwitch::default();
        assert_eq!(switch.focus_moved(true, true, true), Some(false));
        assert_eq!(switch.focus_moved(true, false, true), None);
        assert_eq!(switch.focus_moved(false, false, true), Some(true));
        assert_eq!(switch.focus_moved(false, true, true), None);
    }

    #[test]
    fn english_before_the_game_stays_english() {
        let mut switch = GameSwitch::default();
        assert_eq!(switch.focus_moved(true, false, true), None);
        assert_eq!(switch.focus_moved(false, false, true), None);
    }

    #[test]
    fn vietnamese_turned_on_for_the_game_chat_goes_back_to_the_earlier_mode() {
        let mut switch = GameSwitch::default();
        assert_eq!(switch.focus_moved(true, false, true), None);
        // Ctrl+Shift in the game, to chat in Vietnamese.
        assert_eq!(switch.focus_moved(false, true, true), Some(false));
    }

    #[test]
    fn a_manual_switch_back_to_vietnamese_needs_no_restore() {
        let mut switch = GameSwitch::default();
        assert_eq!(switch.focus_moved(true, true, true), Some(false));
        assert_eq!(switch.focus_moved(false, true, true), None);
    }

    #[test]
    fn switched_off_it_never_changes_the_mode_and_forgets_the_game() {
        let mut switch = GameSwitch::default();
        assert_eq!(switch.focus_moved(true, true, true), Some(false));
        assert_eq!(switch.focus_moved(false, false, false), None);
        assert_eq!(switch.focus_moved(true, true, false), None);
        assert_eq!(switch.focus_moved(false, true, true), None);
    }
}
