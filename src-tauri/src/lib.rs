mod artwork;
mod capture;
mod detect;
mod library;

#[tauri::command]
async fn game_hero(
    app: tauri::AppHandle,
    name: String,
    steam_appid: Option<u32>,
) -> Option<String> {
    artwork::game_hero(&app, &name, steam_appid).await
}

#[tauri::command]
async fn detect_game(app: tauri::AppHandle) -> Option<detect::DetectedGame> {
    detect::detect_game(&app).await
}

#[tauri::command]
fn list_monitors() -> Vec<capture::MonitorInfo> {
    capture::list_monitors()
}

#[tauri::command]
fn list_audio_inputs() -> Vec<capture::AudioInput> {
    capture::list_audio_inputs()
}

#[tauri::command]
fn start_capture(
    app: tauri::AppHandle,
    target: String,
    fps: u32,
    quality: String,
    resolution: u32,
) -> Result<(), String> {
    use tauri::Manager;
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("clips");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    capture::start(target, dir.to_string_lossy().into_owned(), fps, quality, resolution)
}

#[tauri::command]
fn stop_capture() -> Option<String> {
    capture::stop()
}

#[tauri::command]
fn capture_status() -> capture::CaptureStatus {
    capture::status()
}

#[tauri::command]
fn start_replay(
    app: tauri::AppHandle,
    target: String,
    seconds: u32,
    fps: u32,
    quality: String,
    resolution: u32,
) -> Result<(), String> {
    use tauri::Manager;
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("clips");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    capture::start_replay(
        target,
        dir.to_string_lossy().into_owned(),
        seconds,
        fps,
        quality,
        resolution,
    )
}

#[tauri::command]
fn stop_replay() {
    capture::stop_replay();
}

#[tauri::command]
fn save_replay() -> Option<String> {
    capture::save_replay()
}

#[tauri::command]
fn replay_active() -> bool {
    capture::replay_active()
}

#[tauri::command]
fn list_clips(app: tauri::AppHandle) -> Vec<library::ClipInfo> {
    use tauri::Manager;
    let dir = app
        .path()
        .app_data_dir()
        .map(|d| d.join("clips"))
        .unwrap_or_default();
    library::list_clips(dir)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            #[cfg(desktop)]
            app.handle()
                .plugin(tauri_plugin_global_shortcut::Builder::new().build())?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            game_hero,
            detect_game,
            list_monitors,
            list_audio_inputs,
            start_capture,
            stop_capture,
            capture_status,
            list_clips,
            start_replay,
            stop_replay,
            save_replay,
            replay_active
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
