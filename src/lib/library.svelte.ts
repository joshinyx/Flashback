import { invoke, convertFileSrc } from '@tauri-apps/api/core';
import type { Clip } from './clips';

type RawClip = {
  id: string;
  name: string;
  path: string;
  size_bytes: number;
  modified_ms: number;
  duration_sec: number;
  source: string;
};

const FAV_KEY = 'flashback.favorites';

function loadFavs(): string[] {
  if (typeof localStorage === 'undefined') return [];
  try {
    const raw = localStorage.getItem(FAV_KEY);
    if (raw) return JSON.parse(raw);
  } catch {
    // localStorage corrupto o bloqueado
  }
  return [];
}

export const library = $state<{ clips: Clip[]; loaded: boolean }>({ clips: [], loaded: false });

// Los favoritos no viven en el archivo: se guardan por id de clip (= nombre del MP4,
// estable) en localStorage. Cuando exista metadato real por clip se moverán al backend.
export const favorites = $state<{ ids: string[] }>({ ids: loadFavs() });

export function isFavorite(id: string): boolean {
  return favorites.ids.includes(id);
}

function persistFavs() {
  if (typeof localStorage !== 'undefined') {
    try {
      localStorage.setItem(FAV_KEY, JSON.stringify(favorites.ids));
    } catch {
      // sin persistencia disponible
    }
  }
}

export function toggleFavorite(id: string) {
  favorites.ids = isFavorite(id) ? favorites.ids.filter((x) => x !== id) : [...favorites.ids, id];
  persistFavs();
}

// Al renombrar/borrar un clip su id (= nombre del archivo) cambia o desaparece; se actualiza
// la lista de favoritos para que el estado siga al clip.
export function renameFavorite(oldId: string, newId: string) {
  if (!isFavorite(oldId)) return;
  favorites.ids = [...favorites.ids.filter((x) => x !== oldId), newId];
  persistFavs();
}

export function removeFavorite(id: string) {
  if (!isFavorite(id)) return;
  favorites.ids = favorites.ids.filter((x) => x !== id);
  persistFavs();
}

function toClip(r: RawClip): Clip {
  return {
    id: r.id,
    title: r.name,
    source: r.source,
    durationSec: r.duration_sec,
    sizeBytes: r.size_bytes,
    createdAt: new Date(r.modified_ms),
    path: r.path,
    // Los clips exportados desde el editor se nombran `<nombre>_edit.mp4`.
    edited: r.id.endsWith('_edit.mp4'),
    previewSrc: convertFileSrc(r.path)
  };
}

// Miniaturas: el backend extrae un fotograma JPEG cacheado por clip. Se piden de forma
// perezosa (solo las tarjetas visibles) y con un límite de concurrencia para no saturar la
// generación al abrir una biblioteca grande. Una vez en disco, las siguientes peticiones
// devuelven al instante.
const thumbCache = new Map<string, string>();
const thumbQueue: (() => void)[] = [];
let thumbActive = 0;
const THUMB_CONCURRENCY = 4;

function pumpThumbs() {
  while (thumbActive < THUMB_CONCURRENCY && thumbQueue.length > 0) {
    const job = thumbQueue.shift();
    if (job) {
      thumbActive++;
      job();
    }
  }
}

export function requestThumb(path: string): Promise<string | null> {
  const cached = thumbCache.get(path);
  if (cached) return Promise.resolve(cached);
  return new Promise((resolve) => {
    thumbQueue.push(async () => {
      try {
        const p = await invoke<string>('clip_thumbnail', { path });
        const url = convertFileSrc(p);
        thumbCache.set(path, url);
        resolve(url);
      } catch {
        resolve(null);
      } finally {
        thumbActive--;
        pumpThumbs();
      }
    });
    pumpThumbs();
  });
}

export async function refreshLibrary() {
  try {
    const raw = await invoke<RawClip[]>('list_clips');
    library.clips = raw.map(toClip);
  } catch {
    // fuera de Tauri (preview en navegador): biblioteca vacía
    library.clips = [];
  }
  library.loaded = true;
}
