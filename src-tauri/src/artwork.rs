use serde::Deserialize;
use tauri::Manager;

const API_BASE: &str = "https://www.steamgriddb.com/api/v2";

#[derive(Deserialize)]
struct SearchResp {
    data: Vec<GameHit>,
}

#[derive(Deserialize)]
struct GameHit {
    id: u32,
    name: String,
    #[serde(default)]
    types: Vec<String>,
}

#[derive(Deserialize)]
struct HeroResp {
    data: Vec<HeroAsset>,
}

#[derive(Deserialize)]
struct HeroAsset {
    url: String,
}

#[derive(Deserialize)]
struct SteamGameResp {
    data: SteamGameData,
}

#[derive(Deserialize)]
struct SteamGameData {
    id: u32,
}

fn api_key(app: &tauri::AppHandle) -> Option<String> {
    if let Ok(k) = std::env::var("STEAMGRIDDB_API_KEY") {
        let k = k.trim().to_string();
        if !k.is_empty() {
            return Some(k);
        }
    }
    let path = app.path().app_config_dir().ok()?.join("steamgriddb.key");
    let k = std::fs::read_to_string(path).ok()?.trim().to_string();
    (!k.is_empty()).then_some(k)
}

fn slug(name: &str) -> String {
    let s: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    s.trim_matches('-').to_string()
}

fn mime_of(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        "image/jpeg"
    } else if bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
        "image/png"
    } else if bytes.len() > 11 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        "image/webp"
    } else {
        "image/jpeg"
    }
}

fn to_data_url(bytes: &[u8]) -> String {
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    format!("data:{};base64,{}", mime_of(bytes), b64)
}

pub async fn game_hero(
    app: &tauri::AppHandle,
    name: &str,
    steam_appid: Option<u32>,
) -> Option<String> {
    let dir = app.path().app_cache_dir().ok()?.join("artwork");
    let _ = std::fs::create_dir_all(&dir);
    // La clave de caché distingue por AppID cuando lo hay (remasters/secuelas con
    // el mismo nombre tienen AppID distinto), o por nombre cuando no es de Steam.
    let cache_key = match steam_appid {
        Some(id) => format!("steam-{id}"),
        None => slug(name),
    };
    let path = dir.join(&cache_key);

    if let Ok(bytes) = std::fs::read(&path) {
        if !bytes.is_empty() {
            return Some(to_data_url(&bytes));
        }
    }

    let client = reqwest::Client::new();
    let bytes = match steam_appid {
        Some(id) => steam_hero(&client, app, id).await?,
        None => name_hero(&client, app, name).await?,
    };
    if bytes.is_empty() {
        return None;
    }
    let _ = std::fs::write(&path, &bytes);
    Some(to_data_url(&bytes))
}

async fn steam_hero(client: &reqwest::Client, app: &tauri::AppHandle, appid: u32) -> Option<Vec<u8>> {
    // header.jpg primero: el capsule clásico tiene siempre el logo/nombre del juego
    // visible y es mucho más reconocible que el library_hero artístico.
    let candidates = [
        format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{appid}/header.jpg"),
        format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{appid}/library_hero.jpg"),
    ];
    for url in &candidates {
        if let Ok(resp) = client.get(url).send().await {
            if resp.status().is_success() {
                if let Ok(bytes) = resp.bytes().await {
                    if !bytes.is_empty() {
                        return Some(bytes.to_vec());
                    }
                }
            }
        }
    }
    // Fallback: SteamGridDB por AppID (juego exacto, sin ambigüedad de nombre).
    let key = api_key(app)?;
    let game_id = sgdb_by_steam(client, &key, appid).await?;
    let hero_url = first_hero(client, &key, game_id).await?;
    download(client, &hero_url).await
}

async fn name_hero(client: &reqwest::Client, app: &tauri::AppHandle, name: &str) -> Option<Vec<u8>> {
    let key = api_key(app)?;
    let game_id = search_game(client, &key, name).await?;
    let hero_url = first_hero(client, &key, game_id).await?;
    download(client, &hero_url).await
}

async fn download(client: &reqwest::Client, url: &str) -> Option<Vec<u8>> {
    let bytes = client.get(url).send().await.ok()?.bytes().await.ok()?.to_vec();
    (!bytes.is_empty()).then_some(bytes)
}

async fn sgdb_by_steam(client: &reqwest::Client, key: &str, appid: u32) -> Option<u32> {
    let url = format!("{API_BASE}/games/steam/{appid}");
    let resp = client.get(url).bearer_auth(key).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let parsed: SteamGameResp = resp.json().await.ok()?;
    Some(parsed.data.id)
}

async fn search_game(client: &reqwest::Client, key: &str, name: &str) -> Option<u32> {
    let url = format!("{API_BASE}/search/autocomplete/{}", urlencoding::encode(name));
    let resp = client.get(url).bearer_auth(key).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let parsed: SearchResp = resp.json().await.ok()?;
    pick_game(&parsed.data, name)
}

fn pick_game(data: &[GameHit], name: &str) -> Option<u32> {
    let lower = name.to_lowercase();
    // Entre entradas con el mismo nombre (p. ej. God of War 2005 vs 2018) preferir la
    // versión de PC: la que tiene plataforma (Steam u otra), no la de consola.
    let best_exact = data
        .iter()
        .filter(|g| g.name.to_lowercase() == lower)
        .max_by_key(|g| {
            let steam = g.types.iter().any(|t| t == "steam");
            (steam, !g.types.is_empty())
        });
    best_exact.or_else(|| data.first()).map(|g| g.id)
}

async fn first_hero(client: &reqwest::Client, key: &str, game_id: u32) -> Option<String> {
    let url = format!("{API_BASE}/heroes/game/{game_id}");
    let resp = client.get(url).bearer_auth(key).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let parsed: HeroResp = resp.json().await.ok()?;
    parsed.data.into_iter().next().map(|a| a.url)
}

async fn first_icon(client: &reqwest::Client, key: &str, game_id: u32) -> Option<String> {
    let url = format!("{API_BASE}/icons/game/{game_id}");
    let resp = client.get(url).bearer_auth(key).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let parsed: HeroResp = resp.json().await.ok()?;
    parsed.data.into_iter().next().map(|a| a.url)
}

pub async fn game_icon(
    app: &tauri::AppHandle,
    name: &str,
    steam_appid: Option<u32>,
) -> Option<String> {
    let dir = app.path().app_cache_dir().ok()?.join("artwork");
    let _ = std::fs::create_dir_all(&dir);
    let cache_key = match steam_appid {
        Some(id) => format!("icon-steam-{id}"),
        None => format!("icon-{}", slug(name)),
    };
    let path = dir.join(&cache_key);

    if let Ok(bytes) = std::fs::read(&path) {
        if !bytes.is_empty() {
            return Some(to_data_url(&bytes));
        }
    }

    let client = reqwest::Client::new();
    let bytes = match steam_appid {
        Some(id) => steam_icon(&client, app, id).await?,
        None => name_icon(&client, app, name).await?,
    };
    if bytes.is_empty() {
        return None;
    }
    let _ = std::fs::write(&path, &bytes);
    Some(to_data_url(&bytes))
}

async fn steam_icon(client: &reqwest::Client, app: &tauri::AppHandle, appid: u32) -> Option<Vec<u8>> {
    // SteamGridDB primero si hay API key: iconos cuadrados reales.
    if let Some(key) = api_key(app) {
        if let Some(game_id) = sgdb_by_steam(client, &key, appid).await {
            if let Some(icon_url) = first_icon(client, &key, game_id).await {
                if let Some(bytes) = download(client, &icon_url).await {
                    return Some(bytes);
                }
            }
        }
    }
    // Fallback sin API key: portada vertical del CDN de Steam (600×900), se recorta a cuadrado en CSS.
    let cdn_url = format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{appid}/library_600x900.jpg");
    download(client, &cdn_url).await
}

async fn name_icon(client: &reqwest::Client, app: &tauri::AppHandle, name: &str) -> Option<Vec<u8>> {
    let key = api_key(app)?;
    let game_id = search_game(client, &key, name).await?;
    let icon_url = first_icon(client, &key, game_id).await?;
    download(client, &icon_url).await
}
