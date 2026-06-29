use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::Manager;

const DETECTABLE_URL: &str = "https://discord.com/api/v9/applications/detectable";
const MAX_AGE: Duration = Duration::from_secs(7 * 24 * 3600);

// Runtimes compartidos por muchos juegos: su nombre de ejecutable no identifica nada.
const GENERIC: &[&str] = &["javaw.exe", "java.exe", "python.exe", "pythonw.exe"];

// Minecraft corre sobre Java y sus clientes (Lunar, Badlion…) ni están en la lista
// de Discord: se reconoce por la ruta del proceso, que delata el cliente usado.
const MINECRAFT_HINTS: &[&str] = &[
    "minecraft",
    "lunarclient",
    "badlion",
    "feather",
    "labymod",
    "tlauncher",
    "prismlauncher",
    "multimc",
    "modrinth",
    "salwyrr",
    "pojav",
];

type GameMap = HashMap<String, String>;

static MAP: Mutex<Option<Arc<GameMap>>> = Mutex::new(None);
// Último juego detectado en primer plano; se mantiene mientras su proceso viva.
static CURRENT: Mutex<Option<(u32, DetectedGame)>> = Mutex::new(None);

#[derive(Clone, Serialize)]
pub struct DetectedGame {
    pub name: String,
    // AppID de Steam si lo conocemos: permite arte oficial exacto (distingue
    // remasters/secuelas que comparten nombre, p. ej. The Last of Us Part I vs II).
    pub steam_appid: Option<u32>,
}

#[derive(Deserialize)]
struct Detectable {
    name: String,
    #[serde(default)]
    executables: Vec<Executable>,
}

#[derive(Deserialize)]
struct Executable {
    name: String,
    #[serde(default)]
    os: String,
    #[serde(default)]
    is_launcher: bool,
    #[serde(default)]
    arguments: Option<String>,
}

fn basename(name: &str) -> String {
    let stripped = name.trim_start_matches('>');
    stripped
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(stripped)
        .trim()
        .to_lowercase()
}

fn build_map(list: Vec<Detectable>) -> GameMap {
    // Un basename solo sirve si pertenece a UN único juego. Si lo comparten varios
    // (game.exe, nw.exe, anti-cheats, helpers de motor…) se descarta: marca None.
    let mut owners: HashMap<String, Option<String>> = HashMap::new();
    for game in list {
        for exe in game.executables {
            if exe.is_launcher || exe.arguments.is_some() {
                continue;
            }
            if !exe.os.is_empty() && exe.os != "win32" {
                continue;
            }
            let base = basename(&exe.name);
            if !base.ends_with(".exe") || GENERIC.contains(&base.as_str()) {
                continue;
            }
            match owners.entry(base) {
                Entry::Vacant(v) => {
                    v.insert(Some(game.name.clone()));
                }
                Entry::Occupied(mut o) => {
                    let slot = o.get_mut();
                    if slot.as_deref() != Some(game.name.as_str()) {
                        *slot = None;
                    }
                }
            }
        }
    }
    owners
        .into_iter()
        .filter_map(|(base, owner)| owner.map(|game| (base, game)))
        .collect()
}

async fn fetch() -> Option<Vec<u8>> {
    let client = reqwest::Client::new();
    let resp = client
        .get(DETECTABLE_URL)
        .header("User-Agent", "Flashback")
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    resp.bytes().await.ok().map(|b| b.to_vec())
}

async fn load_or_fetch(app: &tauri::AppHandle) -> Option<Vec<u8>> {
    let path = app.path().app_cache_dir().ok()?.join("detectable.json");
    let fresh = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .map(|age| age < MAX_AGE)
        .unwrap_or(false);
    if fresh {
        if let Ok(bytes) = std::fs::read(&path) {
            return Some(bytes);
        }
    }
    match fetch().await {
        Some(bytes) => {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(&path, &bytes);
            Some(bytes)
        }
        None => std::fs::read(&path).ok(),
    }
}

async fn ensure_map(app: &tauri::AppHandle) -> Option<Arc<GameMap>> {
    if let Some(map) = MAP.lock().unwrap().as_ref() {
        return Some(map.clone());
    }
    let bytes = load_or_fetch(app).await?;
    let list: Vec<Detectable> = serde_json::from_slice(&bytes).ok()?;
    let map = Arc::new(build_map(list));
    *MAP.lock().unwrap() = Some(map.clone());
    Some(map)
}

pub async fn detect_game(app: &tauri::AppHandle) -> Option<DetectedGame> {
    let map = ensure_map(app).await?;
    let game = detect_with(&map)?;
    crate::config::record_seen_game(app, &game.name, game.steam_appid);
    Some(game)
}

// PID del juego actualmente rastreado (el de primer plano, persistido mientras viva).
// Lo usa la captura para encontrar la ventana del juego en modo Aplicación.
pub fn current_game_pid() -> Option<u32> {
    CURRENT.lock().unwrap().as_ref().map(|(pid, _)| *pid)
}

#[cfg(target_os = "windows")]
fn detect_with(map: &GameMap) -> Option<DetectedGame> {
    let procs = running_processes();

    // La ventana en primer plano manda: es lo que el usuario está jugando.
    if let Some(pid) = foreground_pid() {
        let base = procs.iter().find(|(p, _)| *p == pid).map(|(_, b)| b.clone());
        let path = process_path(pid);

        // 1. Juego de Steam por ruta del proceso → AppID exacto (sin ambigüedad).
        if let Some(path) = &path {
            if let Some(game) = steam_game_from_path(path) {
                *CURRENT.lock().unwrap() = Some((pid, game.clone()));
                return Some(game);
            }
        }

        if let Some(base) = &base {
            // 2. Ejecutable dedicado (lista de Discord).
            if let Some(name) = map.get(base) {
                let game = DetectedGame {
                    name: name.clone(),
                    steam_appid: None,
                };
                *CURRENT.lock().unwrap() = Some((pid, game.clone()));
                return Some(game);
            }
            // 3. Minecraft (clientes Java).
            if GENERIC.contains(&base.as_str()) {
                let is_minecraft = path.as_ref().is_some_and(|p| {
                    MINECRAFT_HINTS.iter().any(|hint| p.contains(hint))
                }) || procs.iter().any(|(p, _)| {
                    *p != pid
                        && process_path(*p).is_some_and(|p2| {
                            MINECRAFT_HINTS.iter().any(|hint| p2.contains(hint))
                        })
                }) || foreground_class()
                    .zip(foreground_title())
                    .is_some_and(|(c, t)| c == "glfw30" && t.contains("minecraft"));
                if is_minecraft {
                    let game = DetectedGame {
                        name: "Minecraft".to_string(),
                        steam_appid: None,
                    };
                    *CURRENT.lock().unwrap() = Some((pid, game.clone()));
                    return Some(game);
                }
            }
        }
    }

    // Si no hay juego en primer plano, mantener el último mientras su proceso viva.
    let mut current = CURRENT.lock().unwrap();
    if let Some((pid, game)) = current.as_ref() {
        if procs.iter().any(|(p, _)| p == pid) {
            return Some(game.clone());
        }
        *current = None;
    }
    None
}

#[cfg(not(target_os = "windows"))]
fn detect_with(_map: &GameMap) -> Option<DetectedGame> {
    None
}

// Extrae appid + nombre del juego de Steam a partir de la ruta del ejecutable,
// leyendo el appmanifest de la biblioteca (`…/steamapps/common/<dir>/…`).
#[cfg(target_os = "windows")]
fn steam_game_from_path(path: &str) -> Option<DetectedGame> {
    const MARKER: &str = "/steamapps/common/";
    let idx = path.find(MARKER)?;
    let lib_root = format!("{}/steamapps", &path[..idx]);
    let installdir = path[idx + MARKER.len()..].split('/').next()?;
    if installdir.is_empty() {
        return None;
    }
    for entry in std::fs::read_dir(&lib_root).ok()?.flatten() {
        let file = entry.path();
        let is_manifest = file
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.starts_with("appmanifest_") && n.ends_with(".acf"))
            .unwrap_or(false);
        if !is_manifest {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&file) else {
            continue;
        };
        let Some(dir) = acf_value(&content, "installdir") else {
            continue;
        };
        if dir.eq_ignore_ascii_case(installdir) {
            let appid = acf_value(&content, "appid").and_then(|a| a.parse().ok())?;
            let name = acf_value(&content, "name")
                .unwrap_or(dir)
                .replace(['™', '®', '©'], "")
                .trim()
                .to_string();
            return Some(DetectedGame {
                name,
                steam_appid: Some(appid),
            });
        }
    }
    None
}

// Valor del primer `"key"  "valor"` en un fichero ACF (KeyValues de Valve).
#[cfg(target_os = "windows")]
fn acf_value(content: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let after = &content[content.find(&needle)? + needle.len()..];
    let start = after.find('"')? + 1;
    let rest = &after[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

#[cfg(target_os = "windows")]
fn foreground_pid() -> Option<u32> {
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return None;
        }
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        (pid != 0).then_some(pid)
    }
}

#[cfg(target_os = "windows")]
fn foreground_title() -> Option<String> {
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW};
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return None;
        }
        let mut buf = [0u16; 512];
        let len = GetWindowTextW(hwnd, &mut buf);
        if len == 0 {
            return None;
        }
        Some(String::from_utf16_lossy(&buf[..len as usize]).to_lowercase())
    }
}

#[cfg(target_os = "windows")]
fn foreground_class() -> Option<String> {
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetClassNameW};
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return None;
        }
        let mut buf = [0u16; 512];
        let len = GetClassNameW(hwnd, &mut buf);
        if len == 0 {
            return None;
        }
        Some(String::from_utf16_lossy(&buf[..len as usize]).to_lowercase())
    }
}

#[cfg(target_os = "windows")]
fn process_path(pid: u32) -> Option<String> {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false.into(), pid).ok()?;
        let mut buf = [0u16; 512];
        let mut size = buf.len() as u32;
        let res = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut size,
        );
        let _ = CloseHandle(handle);
        res.ok()?;
        Some(
            String::from_utf16_lossy(&buf[..size as usize])
                .to_lowercase()
                .replace('\\', "/"),
        )
    }
}

#[cfg(target_os = "windows")]
fn running_processes() -> Vec<(u32, String)> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    let mut out = Vec::new();
    unsafe {
        let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return out;
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                let len = entry
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(entry.szExeFile.len());
                let name = String::from_utf16_lossy(&entry.szExeFile[..len]).to_lowercase();
                out.push((entry.th32ProcessID, name));
                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snapshot);
    }
    out
}

