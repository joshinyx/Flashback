use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::Manager;

#[derive(Clone, Serialize, Deserialize)]
pub struct SeenGame {
    pub name: String,
    pub steam_appid: Option<u32>,
    pub last_seen: u64,
}

fn settings_path(app: &tauri::AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join("settings.json"))
}

fn read_setting(app: &tauri::AppHandle, key: &str) -> Option<String> {
    let s = std::fs::read_to_string(settings_path(app)?).ok()?;
    let v: serde_json::Value = serde_json::from_str(&s).ok()?;
    v.get(key)?.as_str().map(String::from)
}

fn read_array(app: &tauri::AppHandle, key: &str) -> Vec<String> {
    let Some(path) = settings_path(app) else { return Vec::new() };
    let Ok(s) = std::fs::read_to_string(path) else { return Vec::new() };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) else { return Vec::new() };
    v.get(key)
        .and_then(|a| a.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
        .unwrap_or_default()
}

fn write_setting(app: &tauri::AppHandle, key: &str, val: serde_json::Value) -> Result<(), String> {
    let path = settings_path(app).ok_or("No se pudo resolver el directorio de la app")?;
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut v: serde_json::Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    v[key] = val;
    std::fs::write(&path, v.to_string()).map_err(|e| e.to_string())
}

// Solo los clips y las capturas de fotograma se guardan en Vídeos\Flashback (visibles para el
// usuario); el resto (miniaturas, audio, índices, settings) sigue en app_data (Roaming).
fn videos_base(app: &tauri::AppHandle) -> Option<PathBuf> {
    app.path().video_dir().ok().map(|d| d.join("Flashback"))
}

fn legacy_clips_dir(app: &tauri::AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .map(|d| d.join("clips"))
        .unwrap_or_default()
}

fn default_clips_dir(app: &tauri::AppHandle) -> PathBuf {
    videos_base(app)
        .map(|d| d.join("Clips"))
        .unwrap_or_else(|| legacy_clips_dir(app))
}

// Carpeta de capturas de fotograma (Vídeos\Flashback\Screenshots). Se asegura de que exista.
pub fn screenshots_dir(app: &tauri::AppHandle) -> PathBuf {
    let dir = videos_base(app).map(|d| d.join("Screenshots")).unwrap_or_else(|| {
        app.path()
            .app_data_dir()
            .map(|d| d.join("screenshots"))
            .unwrap_or_default()
    });
    let _ = std::fs::create_dir_all(&dir);
    dir
}

// Carpeta fija para los clips editados (Vídeos\Flashback\Clips-Edit), separada de la principal.
// Se escanea en la biblioteca igual que las demás, así que los editados siguen siendo visibles.
pub fn clips_edit_dir(app: &tauri::AppHandle) -> PathBuf {
    let dir = videos_base(app).map(|d| d.join("Clips-Edit")).unwrap_or_else(|| {
        app.path()
            .app_data_dir()
            .map(|d| d.join("clips-edit"))
            .unwrap_or_default()
    });
    let _ = std::fs::create_dir_all(&dir);
    dir
}

// Carpeta ACTIVA donde se guardan los clips nuevos. Si el usuario no la ha cambiado, el valor
// por defecto es app_data/clips. Se asegura de que el directorio exista.
pub fn clips_dir(app: &tauri::AppHandle) -> PathBuf {
    let dir = read_setting(app, "clips_dir")
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| default_clips_dir(app));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

// Todas las carpetas que la biblioteca escanea: la por defecto, la activa y las usadas antes.
// Cambiar la carpeta solo cambia dónde se guardan los clips nuevos; los de carpetas anteriores
// se siguen listando sin moverlos. Dedupe sin distinguir mayúsculas (rutas Windows).
pub fn library_dirs(app: &tauri::AppHandle) -> Vec<PathBuf> {
    // Incluye la heredada app_data/clips para seguir mostrando los clips de usuarios que ya
    // tenían vídeos guardados ahí antes de mover el destino por defecto a Vídeos\Flashback.
    let mut dirs = vec![
        default_clips_dir(app),
        legacy_clips_dir(app),
        clips_edit_dir(app),
        clips_dir(app),
    ];
    for s in read_array(app, "library_dirs") {
        let s = s.trim();
        if !s.is_empty() {
            dirs.push(PathBuf::from(s));
        }
    }
    let mut seen = std::collections::HashSet::new();
    dirs.retain(|p| seen.insert(p.to_string_lossy().to_lowercase()));
    dirs
}

pub fn get_disabled_games(app: &tauri::AppHandle) -> Vec<String> {
    read_array(app, "disabled_games")
}

pub fn set_disabled_games(app: &tauri::AppHandle, games: Vec<String>) -> Result<(), String> {
    write_setting(app, "disabled_games", serde_json::json!(games))
}

pub fn get_seen_games(app: &tauri::AppHandle) -> Vec<SeenGame> {
    let Some(path) = settings_path(app) else { return Vec::new() };
    let Ok(s) = std::fs::read_to_string(path) else { return Vec::new() };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) else { return Vec::new() };
    let mut games: Vec<SeenGame> = v
        .get("seen_games")
        .and_then(|a| serde_json::from_value(a.clone()).ok())
        .unwrap_or_default();
    games.sort_by(|a, b| b.last_seen.cmp(&a.last_seen));
    games
}

// Registra un juego detectado. Solo escribe en disco si es nuevo o han pasado >60 s,
// para no generar I/O constante durante una sesión de juego.
pub fn record_seen_game(app: &tauri::AppHandle, name: &str, steam_appid: Option<u32>) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let Some(path) = settings_path(app) else { return };
    let Ok(s) = std::fs::read_to_string(&path) else {
        let _ = write_setting(app, "seen_games", serde_json::json!([{
            "name": name, "steam_appid": steam_appid, "last_seen": now
        }]));
        return;
    };
    let mut v: serde_json::Value = serde_json::from_str(&s).unwrap_or_else(|_| serde_json::json!({}));
    let arr = v.get_mut("seen_games").and_then(|a| a.as_array_mut());

    if let Some(arr) = arr {
        if let Some(entry) = arr.iter_mut().find(|e| e.get("name").and_then(|n| n.as_str()) == Some(name)) {
            let stale = entry.get("last_seen").and_then(|t| t.as_u64())
                .map(|t| now.saturating_sub(t) >= 60)
                .unwrap_or(true);
            if !stale { return; }
            *entry = serde_json::json!({ "name": name, "steam_appid": steam_appid, "last_seen": now });
        } else {
            arr.push(serde_json::json!({ "name": name, "steam_appid": steam_appid, "last_seen": now }));
        }
    } else {
        v["seen_games"] = serde_json::json!([{ "name": name, "steam_appid": steam_appid, "last_seen": now }]);
    }

    let _ = std::fs::write(&path, v.to_string());
}

pub fn get_encoder(app: &tauri::AppHandle) -> String {
    read_setting(app, "encoder").unwrap_or_else(|| "Auto".into())
}

pub fn set_encoder(app: &tauri::AppHandle, enc: &str) -> Result<(), String> {
    write_setting(app, "encoder", serde_json::json!(enc))
}

pub fn set_clips_dir(app: &tauri::AppHandle, dir: &str) -> Result<(), String> {
    let path = PathBuf::from(dir);
    std::fs::create_dir_all(&path).map_err(|e| format!("No se pudo crear la carpeta: {e}"))?;
    write_setting(app, "clips_dir", serde_json::json!(dir))?;
    // Recordar la carpeta para seguir mostrando sus clips aunque más tarde se cambie de nuevo.
    let mut history = read_array(app, "library_dirs");
    let lower = dir.to_lowercase();
    if !history.iter().any(|x| x.to_lowercase() == lower) {
        history.push(dir.to_string());
        write_setting(app, "library_dirs", serde_json::json!(history))?;
    }
    allow_asset_scopes(app);
    Ok(())
}

// El protocolo `asset` solo sirve archivos dentro de su scope; sin esto, el editor no podría
// reproducir clips ni leer las capturas (para copiarlas) guardadas fuera de app_data. Se amplía
// en runtime a todas las carpetas de la biblioteca y a la de capturas (al arrancar y al cambiar).
pub fn allow_asset_scopes(app: &tauri::AppHandle) {
    let scope = app.asset_protocol_scope();
    for dir in library_dirs(app) {
        let _ = scope.allow_directory(dir, false);
    }
    let _ = scope.allow_directory(screenshots_dir(app), false);
}

#[cfg(windows)]
pub fn pick_folder() -> Result<Option<String>, String> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Shell::{
        FileOpenDialog, IFileOpenDialog, IShellItem, FOS_PICKFOLDERS, SIGDN_FILESYSPATH,
    };

    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let result = (|| -> Result<Option<String>, String> {
            let dialog: IFileOpenDialog =
                CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)
                    .map_err(|e| e.to_string())?;
            let opts = dialog.GetOptions().map_err(|e| e.to_string())?;
            dialog
                .SetOptions(opts | FOS_PICKFOLDERS)
                .map_err(|e| e.to_string())?;
            // Show devuelve error si el usuario cancela: en ese caso no hay carpeta elegida.
            if dialog.Show(Some(HWND::default())).is_err() {
                return Ok(None);
            }
            let item: IShellItem = dialog.GetResult().map_err(|e| e.to_string())?;
            let pwstr = item
                .GetDisplayName(SIGDN_FILESYSPATH)
                .map_err(|e| e.to_string())?;
            let path = pwstr.to_string().map_err(|e| e.to_string());
            CoTaskMemFree(Some(pwstr.0 as *const _));
            Ok(Some(path?))
        })();
        CoUninitialize();
        result
    }
}

#[cfg(not(windows))]
pub fn pick_folder() -> Result<Option<String>, String> {
    Err("El selector de carpeta solo está disponible en Windows".into())
}
