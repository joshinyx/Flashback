use std::path::Path;
use std::sync::Mutex;

use serde_json::{Map, Value};

// Índice único del estado de edición no destructiva: un solo archivo en app-data mapea la
// ruta del clip → sus cortes y mezcla, en vez de un sidecar por vídeo en la carpeta de clips.
// El Mutex serializa el read-modify-write del archivo entre comandos concurrentes de Tauri.
static LOCK: Mutex<()> = Mutex::new(());

fn read_map(index: &Path) -> Map<String, Value> {
    std::fs::read_to_string(index)
        .ok()
        .and_then(|s| serde_json::from_str::<Map<String, Value>>(&s).ok())
        .unwrap_or_default()
}

fn write_map(index: &Path, map: &Map<String, Value>) -> std::io::Result<()> {
    if let Some(parent) = index.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string(map).unwrap_or_else(|_| "{}".into());
    std::fs::write(index, json)
}

pub fn load(index: &Path, key: &str) -> Option<Value> {
    let _g = LOCK.lock().unwrap();
    read_map(index).remove(key)
}

pub fn save(index: &Path, key: &str, val: Value) {
    let _g = LOCK.lock().unwrap();
    let mut map = read_map(index);
    map.insert(key.to_string(), val);
    let _ = write_map(index, &map);
}

pub fn rekey(index: &Path, old: &str, new: &str) {
    let _g = LOCK.lock().unwrap();
    let mut map = read_map(index);
    if let Some(v) = map.remove(old) {
        map.insert(new.to_string(), v);
        let _ = write_map(index, &map);
    }
}

pub fn remove(index: &Path, key: &str) {
    let _g = LOCK.lock().unwrap();
    let mut map = read_map(index);
    if map.remove(key).is_some() {
        let _ = write_map(index, &map);
    }
}
