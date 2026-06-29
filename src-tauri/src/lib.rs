mod artwork;
#[cfg(target_os = "windows")]
mod audio;
mod capture;
mod config;
mod detect;
mod editor;
mod edits;
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
async fn game_icon(
    app: tauri::AppHandle,
    name: String,
    steam_appid: Option<u32>,
) -> Option<String> {
    artwork::game_icon(&app, &name, steam_appid).await
}

#[tauri::command]
async fn detect_game(app: tauri::AppHandle) -> Option<detect::DetectedGame> {
    detect::detect_game(&app).await
}

#[tauri::command]
fn get_seen_games(app: tauri::AppHandle) -> Vec<config::SeenGame> {
    config::get_seen_games(&app)
}

#[tauri::command]
fn get_disabled_games(app: tauri::AppHandle) -> Vec<String> {
    config::get_disabled_games(&app)
}

#[tauri::command]
fn set_disabled_games(app: tauri::AppHandle, games: Vec<String>) -> Result<(), String> {
    config::set_disabled_games(&app, games)
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
fn get_encoder(app: tauri::AppHandle) -> String {
    config::get_encoder(&app)
}

#[tauri::command]
fn set_encoder(app: tauri::AppHandle, enc: String) -> Result<(), String> {
    config::set_encoder(&app, &enc)
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
    let dir = config::clips_dir(&app).to_string_lossy().into_owned();
    let encoder_pref = config::get_encoder(&app);
    capture::start(
        target,
        dir,
        fps,
        quality,
        resolution,
        bitrate,
        mic,
        mic_device,
        encoder_pref,
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
    let dir = config::clips_dir(&app).to_string_lossy().into_owned();
    let encoder_pref = config::get_encoder(&app);
    capture::start_replay(
        target,
        dir,
        seconds,
        fps,
        quality,
        resolution,
        bitrate,
        mic,
        mic_device,
        encoder_pref,
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
async fn prepare_clip_audio(app: tauri::AppHandle, path: String) -> Result<editor::ClipAudio, String> {
    use tauri::Manager;
    let audio_dir = app.path().app_data_dir()
        .map_err(|e| e.to_string())?
        .join("audio");
    std::fs::create_dir_all(&audio_dir).map_err(|e| e.to_string())?;
    let audio_dir = audio_dir.to_string_lossy().into_owned();
    tokio::task::spawn_blocking(move || editor::prepare_clip_audio(path, audio_dir))
        .await
        .map_err(|e| format!("Error interno: {e}"))?
}

fn edit_index(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    use tauri::Manager;
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("edits.json"))
}

#[tauri::command]
fn load_clip_edit(app: tauri::AppHandle, path: String) -> Result<editor::ClipEdit, String> {
    editor::load_edit(edit_index(&app)?.to_string_lossy().into_owned(), path)
}

#[tauri::command]
fn save_clip_edit(app: tauri::AppHandle, path: String, edit: editor::ClipEdit) -> Result<(), String> {
    editor::save_edit(edit_index(&app)?.to_string_lossy().into_owned(), path, edit)
}

#[tauri::command]
async fn keyframe_times(path: String) -> Result<Vec<f64>, String> {
    tokio::task::spawn_blocking(move || editor::keyframe_times(path))
        .await
        .map_err(|e| format!("Error interno: {e}"))?
}

#[tauri::command]
async fn frame_times(path: String) -> Result<Vec<f64>, String> {
    tokio::task::spawn_blocking(move || editor::frame_times(path))
        .await
        .map_err(|e| format!("Error interno: {e}"))?
}

#[tauri::command]
async fn clip_fps(path: String) -> Result<u32, String> {
    tokio::task::spawn_blocking(move || editor::clip_fps(path))
        .await
        .map_err(|e| format!("Error interno: {e}"))?
}

#[tauri::command]
async fn export_clip(
    app: tauri::AppHandle,
    src: String,
    dst: String,
    edit: editor::ClipEdit,
) -> Result<(), String> {
    use tauri::Emitter;
    tokio::task::spawn_blocking(move || {
        editor::export_clip(src, dst, edit, move |p: f32| {
            let _ = app.emit("export-progress", p);
        })
    })
    .await
    .map_err(|e| format!("Error interno: {e}"))?
}

#[tauri::command]
async fn clip_thumbnail(app: tauri::AppHandle, path: String) -> Result<String, String> {
    use tauri::Manager;
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?.join("thumbs");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let thumb_path = tokio::task::spawn_blocking(move || -> Result<String, String> {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        path.hash(&mut h);
        let dst = dir.join(format!("{:016x}.jpg", h.finish()));
        let ready = dst.metadata().map(|m| m.len() > 0).unwrap_or(false);
        if !ready {
            thumbnail::generate(path, dst.to_string_lossy().into_owned(), 0)?;
        }
        Ok(dst.to_string_lossy().into_owned())
    })
    .await
    .map_err(|e| format!("Error interno: {e}"))?;
    thumb_path
}

#[tauri::command]
async fn capture_frame(app: tauri::AppHandle, path: String, time_ms: f64) -> Result<String, String> {
    let dir = config::screenshots_dir(&app);
    tokio::task::spawn_blocking(move || -> Result<String, String> {
        let stem = std::path::Path::new(&path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("clip");
        let dst = dir
            .join(format!("{stem}_{}.png", time_ms.max(0.0).round() as i64))
            .to_string_lossy()
            .into_owned();
        thumbnail::capture(path.clone(), dst.clone(), time_ms)?;
        Ok(dst)
    })
    .await
    .map_err(|e| format!("Error interno: {e}"))?
}

#[tauri::command]
fn rename_clip(app: tauri::AppHandle, path: String, new_name: String) -> Result<String, String> {
    library::rename_clip(&path, &new_name, &edit_index(&app)?)
}

#[tauri::command]
fn delete_clip(app: tauri::AppHandle, path: String) -> Result<(), String> {
    library::delete_clip(&path, &edit_index(&app)?)
}

#[tauri::command]
fn list_clips(app: tauri::AppHandle) -> Vec<library::ClipInfo> {
    library::list_clips(config::library_dirs(&app))
}

#[tauri::command]
fn clips_dir(app: tauri::AppHandle) -> String {
    config::clips_dir(&app).to_string_lossy().into_owned()
}

#[tauri::command]
fn set_clips_dir(app: tauri::AppHandle, dir: String) -> Result<(), String> {
    config::set_clips_dir(&app, &dir)
}

#[tauri::command]
async fn pick_folder() -> Result<Option<String>, String> {
    tokio::task::spawn_blocking(config::pick_folder)
        .await
        .map_err(|e| format!("Error interno: {e}"))?
}

// Destino de exportación: los clips editados van a su carpeta dedicada, no junto al original.
#[tauri::command]
fn edit_dest(app: tauri::AppHandle, src: String) -> String {
    let stem = std::path::Path::new(&src)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("clip");
    config::clips_edit_dir(&app)
        .join(format!("{stem}_edit.mp4"))
        .to_string_lossy()
        .into_owned()
}

#[derive(Clone, serde::Serialize)]
struct ToastPayload {
    text: String,
    kind: String,
}

// Muestra el toast en la ventana overlay (transparente, siempre encima, click-through). La
// ventana permanece oculta entre avisos (una ventana transparente vacía se compone gris en
// Windows) y se muestra solo durante el toast. Es no-activable (set_focusable(false) en el
// setup), así que show() no le roba el foco al juego. Se reposiciona arriba-derecha del
// monitor primario por si cambió la resolución/escala. El overlay la oculta al terminar.
#[tauri::command]
fn toast(app: tauri::AppHandle, text: String, kind: String) -> Result<(), String> {
    use tauri::{Emitter, Manager};
    let w = app
        .get_webview_window("overlay")
        .ok_or("overlay window missing")?;
    if let Ok(Some(mon)) = w.primary_monitor() {
        let mpos = mon.position();
        let msize = mon.size();
        let scale = mon.scale_factor();
        let margin = (16.0 * scale) as i32;
        if let Ok(size) = w.outer_size() {
            // Lengüeta anclada al borde derecho: pegada a la derecha (x sin margen), con un
            // pequeño margen arriba.
            let x = mpos.x + msize.width as i32 - size.width as i32;
            let y = mpos.y + margin;
            let _ = w.set_position(tauri::PhysicalPosition { x, y });
        }
    }
    // Contenido primero (el webview oculto sigue ejecutando JS), luego mostrar: así nunca se
    // ve la ventana transparente vacía (gris) antes de pintar el toast.
    app.emit_to("overlay", "show-toast", ToastPayload { text, kind })
        .map_err(|e| e.to_string())?;
    let _ = w.show();
    // Reafirmar topmost en cada toast: así se coloca por encima de juegos en ventana o
    // borderless. No activa la ventana (es no-focusable), así que no le roba el foco.
    let _ = w.set_always_on_top(true);
    Ok(())
}

#[tauri::command]
fn dismiss_toast(app: tauri::AppHandle) {
    use tauri::Manager;
    if let Some(w) = app.get_webview_window("overlay") {
        let _ = w.hide();
    }
}

// Trae la ventana principal al frente (desde la bandeja o el atajo de abrir).
fn show_main(app: &tauri::AppHandle) {
    use tauri::Manager;
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default();
    // Instancia única: si se intenta abrir una segunda, se enfoca la existente y la nueva
    // sale. Debe ir como primer plugin para cortar antes de crear ventanas.
    #[cfg(desktop)]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main(app);
        }));
    }
    builder
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            use tauri::Manager;
            #[cfg(desktop)]
            app.handle()
                .plugin(tauri_plugin_global_shortcut::Builder::new().build())?;
            // Permitir al protocolo asset leer las carpetas de clips y capturas (pueden estar fuera
            // de app_data), o el editor no podría reproducir/leer los archivos guardados ahí.
            config::allow_asset_scopes(app.handle());
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.set_min_size(Some(tauri::LogicalSize { width: 1200.0, height: 675.0 }));
                let _ = w.set_size(tauri::LogicalSize { width: 1200.0, height: 675.0 });
            }
            if let Some(o) = app.get_webview_window("overlay") {
                let _ = o.set_ignore_cursor_events(true);
                // No-activable: al mostrarse durante un toast no debe robar el foco al juego.
                let _ = o.set_focusable(false);
            }

            // Bandeja del sistema. Doble clic izquierdo abre la app; clic derecho abre el
            // menú con "Abrir Flashback" y "Cerrar". El replay sigue corriendo aunque la
            // ventana esté oculta (vive en hilos de Rust, no en la UI).
            use tauri::menu::{Menu, MenuItem};
            use tauri::tray::{MouseButton, TrayIconBuilder, TrayIconEvent};
            let open_i = MenuItem::with_id(app, "open", "Abrir Flashback", true, None::<&str>)?;
            let quit_i = MenuItem::with_id(app, "quit", "Cerrar", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open_i, &quit_i])?;
            let mut tray = TrayIconBuilder::with_id("flashback")
                .tooltip("Flashback")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "open" => show_main(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::DoubleClick { button: MouseButton::Left, .. } = event {
                        show_main(tray.app_handle());
                    }
                });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;

            // Cerrar la ventana (botón X) no termina la app: la oculta a la bandeja. Salir de
            // verdad es solo desde el menú de la bandeja ("Cerrar" → app.exit).
            if let Some(main) = app.get_webview_window("main") {
                let main_c = main.clone();
                main.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = main_c.hide();
                    }
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            game_hero,
            game_icon,
            detect_game,
            get_seen_games,
            get_disabled_games,
            set_disabled_games,
            get_encoder,
            set_encoder,
            list_monitors,
            list_audio_inputs,
            start_capture,
            stop_capture,
            capture_status,
            list_clips,
            clips_dir,
            set_clips_dir,
            pick_folder,
            edit_dest,
            prepare_clip_audio,
            load_clip_edit,
            save_clip_edit,
            keyframe_times,
            frame_times,
            clip_fps,
            clip_thumbnail,
            capture_frame,
            export_clip,
            rename_clip,
            delete_clip,
            start_replay,
            stop_replay,
            save_replay,
            replay_active,
            toast,
            dismiss_toast
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
