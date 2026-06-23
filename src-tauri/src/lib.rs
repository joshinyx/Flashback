mod artwork;
#[cfg(target_os = "windows")]
mod audio;
mod capture;
mod detect;
mod editor;
mod library;
#[cfg(target_os = "windows")]
mod overlay;
mod thumbnail;

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
    bitrate: u32,
    mic: bool,
    mic_device: String,
) -> Result<(), String> {
    use tauri::Manager;
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("clips");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    capture::start(
        target,
        dir.to_string_lossy().into_owned(),
        fps,
        quality,
        resolution,
        bitrate,
        mic,
        mic_device,
    )
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
    bitrate: u32,
    mic: bool,
    mic_device: String,
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
        bitrate,
        mic,
        mic_device,
    )
}

#[tauri::command]
fn stop_replay() {
    capture::stop_replay();
}

#[tauri::command]
fn save_replay(source: String) -> Option<String> {
    capture::save_replay(&source)
}

#[tauri::command]
fn replay_active() -> bool {
    capture::replay_active()
}

#[tauri::command]
fn prepare_clip_audio(path: String) -> Result<editor::ClipAudio, String> {
    editor::prepare_clip_audio(path)
}

#[tauri::command]
fn load_clip_edit(path: String) -> Result<editor::ClipEdit, String> {
    editor::load_edit(path)
}

#[tauri::command]
fn save_clip_edit(path: String, edit: editor::ClipEdit) -> Result<(), String> {
    editor::save_edit(path, edit)
}

#[tauri::command]
fn keyframe_times(path: String) -> Result<Vec<f64>, String> {
    editor::keyframe_times(path)
}

#[tauri::command]
fn clip_fps(path: String) -> Result<u32, String> {
    editor::clip_fps(path)
}

#[tauri::command]
fn export_clip(src: String, dst: String, edit: editor::ClipEdit) -> Result<(), String> {
    editor::export_clip(src, dst, edit)
}

#[tauri::command]
fn clip_thumbnail(app: tauri::AppHandle, path: String) -> Result<String, String> {
    use std::hash::{Hash, Hasher};
    use tauri::Manager;
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("thumbs");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut h);
    let dst = dir.join(format!("{:016x}.jpg", h.finish()));
    let ready = dst.metadata().map(|m| m.len() > 0).unwrap_or(false);
    if !ready {
        // max_w = 0: miniatura a la resolución nativa del clip (la carátula se muestra a la
        // máxima calidad y empareja con el vídeo en el hover).
        thumbnail::generate(path, dst.to_string_lossy().into_owned(), 0)?;
    }
    Ok(dst.to_string_lossy().into_owned())
}

#[tauri::command]
fn rename_clip(path: String, new_name: String) -> Result<String, String> {
    library::rename_clip(&path, &new_name)
}

#[tauri::command]
fn delete_clip(path: String) -> Result<(), String> {
    library::delete_clip(&path)
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
            use tauri::Manager;
            #[cfg(desktop)]
            app.handle()
                .plugin(tauri_plugin_global_shortcut::Builder::new().build())?;
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.set_min_size(Some(tauri::LogicalSize { width: 1200.0, height: 675.0 }));
                let _ = w.set_size(tauri::LogicalSize { width: 1200.0, height: 675.0 });
            }
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
            prepare_clip_audio,
            load_clip_edit,
            save_clip_edit,
            keyframe_times,
            clip_fps,
            clip_thumbnail,
            export_clip,
            rename_clip,
            delete_clip,
            start_replay,
            stop_replay,
            save_replay,
            replay_active
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
