import { invoke, convertFileSrc } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { Clip } from './clips';

type ClipAudio = {
  system: string | null;
  mic: string | null;
  sys_peaks: number[] | null;
  mic_peaks: number[] | null;
  mix_peaks: number[] | null;
};

export type MixerState = {
  sys_vol: number;
  sys_muted: boolean;
  mic_vol: number;
  mic_muted: boolean;
};

const defaultMixer: MixerState = {
  sys_vol: 1,
  sys_muted: false,
  mic_vol: 1,
  mic_muted: false,
};

export type Segment = {
  // Tramo de origen que se muestra actualmente (recortado).
  startMs: number;
  endMs: number;
  // Posición del borde izquierdo en la línea de tiempo del editor (permite huecos en negro).
  posMs: number;
  // Tamaño máximo de la sección: el rango que tenía al cortarla. El recorte solo puede encoger
  // dentro de [boundStartMs, boundEndMs]; nunca alargarla más allá (ese material es de otras).
  boundStartMs: number;
  boundEndMs: number;
  // Bloque desactivado: sigue visible en la timeline y se puede reactivar, pero la reproducción y
  // la exportación lo ignoran como si no existiera.
  disabled: boolean;
};

const MIN_SEG_MS = 50;

type SavedSegment = {
  start_ms: number;
  end_ms: number;
  pos_ms?: number | null;
  bound_start_ms?: number | null;
  bound_end_ms?: number | null;
  disabled?: boolean | null;
};

export const editorState = $state<{
  clip: Clip | null;
  videoSrc: string | null;
  system: string | null;
  mic: string | null;
  loading: boolean;
  error: string | null;
  segments: Segment[];
  activeSegment: number;
  durationMs: number;
  frameTimes: number[];
  fps: number;
  mixer: MixerState;
  exporting: boolean;
  exportProgress: number;
  sysPeaks: number[] | null;
  micPeaks: number[] | null;
  mixPeaks: number[] | null;
}>({
  clip: null,
  videoSrc: null,
  system: null,
  mic: null,
  loading: false,
  error: null,
  segments: [],
  activeSegment: 0,
  durationMs: 0,
  frameTimes: [],
  fps: 30,
  mixer: { ...defaultMixer },
  exporting: false,
  exportProgress: 0,
  sysPeaks: null,
  micPeaks: null,
  mixPeaks: null,
});

// Lista de clips en el orden en que se ven en la rejilla desde la que se abrió el editor. Cada
// página la publica aquí; permite navegar al clip anterior/siguiente sin salir del editor.
export const clipOrder = $state<{ list: Clip[] }>({ list: [] });

function resetEditorState() {
  editorState.clip = null;
  editorState.videoSrc = null;
  editorState.system = null;
  editorState.mic = null;
  editorState.error = null;
  editorState.loading = false;
  editorState.segments = [];
  editorState.activeSegment = 0;
  editorState.durationMs = 0;
  editorState.frameTimes = [];
  editorState.fps = 30;
  editorState.mixer = { ...defaultMixer };
  editorState.exporting = false;
  editorState.exportProgress = 0;
  editorState.sysPeaks = null;
  editorState.micPeaks = null;
  editorState.mixPeaks = null;
}

// Duración total de salida = suma de las duraciones de los segmentos (sin huecos).
export function keptMs(): number {
  return editorState.segments.reduce((a, s) => a + (s.endMs - s.startMs), 0);
}

export function openEditor(clip: Clip) {
  resetEditorState();
  editorState.clip = clip;
  editorState.videoSrc = clip.previewSrc ?? null;
  editorState.loading = true;

  (async () => {
    if (!clip.path) { editorState.error = 'clip sin ruta'; editorState.loading = false; return; }

    try {
      editorState.frameTimes = await invoke<number[]>('frame_times', { path: clip.path });
    } catch {
      editorState.frameTimes = [];
    }

    try {
      const fps = await invoke<number>('clip_fps', { path: clip.path });
      if (fps > 0) editorState.fps = fps;
    } catch {
      /* fps por defecto */
    }

    try {
      const saved = await invoke<{ segments: SavedSegment[]; mixer: MixerState }>(
        'load_clip_edit',
        { path: clip.path },
      );
      if (saved?.segments?.length) {
        // Ediciones antiguas no traen posición/límites: se empaquetan en orden y se asume que su
        // rango actual era su tamaño máximo.
        let acc = 0;
        const segs = saved.segments.map((s) => {
          const startMs = s.start_ms;
          const endMs = s.end_ms;
          const posMs = s.pos_ms ?? acc;
          acc = posMs + (endMs - startMs);
          return {
            startMs,
            endMs,
            posMs,
            boundStartMs: s.bound_start_ms ?? startMs,
            boundEndMs: s.bound_end_ms ?? endMs,
            disabled: s.disabled ?? false,
          };
        });
        segs.sort((a, b) => a.posMs - b.posMs);
        editorState.segments = segs;
        editorState.activeSegment = 0;
      }
      if (saved?.mixer) editorState.mixer = { ...defaultMixer, ...saved.mixer };
    } catch {
      /* sin edición previa */
    }

    try {
      const res = await invoke<ClipAudio>('prepare_clip_audio', { path: clip.path });
      editorState.system = res.system ? convertFileSrc(res.system) : null;
      editorState.mic = res.mic ? convertFileSrc(res.mic) : null;
      editorState.sysPeaks = res.sys_peaks ?? null;
      editorState.micPeaks = res.mic_peaks ?? null;
      editorState.mixPeaks = res.mix_peaks ?? null;
    } catch (e) {
      editorState.error = String(e);
    } finally {
      editorState.loading = false;
    }
  })();
}

export async function exportClip() {
  if (!editorState.clip?.path || editorState.segments.length === 0) return;
  const segments = serializeSegments(true);
  if (segments.length === 0) throw new Error('No hay bloques activos para exportar');
  editorState.exporting = true;
  editorState.exportProgress = 0;
  // El backend emite el progreso (0..1) durante la recodificación; lo reflejamos en el popup.
  const unlisten = await listen<number>('export-progress', (e) => {
    editorState.exportProgress = e.payload;
  });
  try {
    const src = editorState.clip.path;
    // El backend decide el destino: los clips editados van a su carpeta dedicada (Clips-Edit).
    const dst = await invoke<string>('edit_dest', { src });

    await invoke('export_clip', {
      src,
      dst,
      edit: {
        segments,
        mixer: editorState.mixer,
      },
    });
    return dst;
  } catch (e) {
    throw e;
  } finally {
    unlisten();
    editorState.exporting = false;
    editorState.exportProgress = 0;
  }
}

// Captura el fotograma mostrado (al tiempo de origen dado) y lo guarda como JPG junto al clip;
// devuelve la ruta del archivo. El backend hace el seek + codificación (no toca el canvas).
export async function captureFrame(timeMs: number): Promise<string | undefined> {
  if (!editorState.clip?.path) return;
  return await invoke<string>('capture_frame', { path: editorState.clip.path, timeMs });
}

export function resetTrim() {
  if (editorState.durationMs > 0) {
    const end = editorState.durationMs;
    editorState.segments = [{ startMs: 0, endMs: end, posMs: 0, boundStartMs: 0, boundEndMs: end, disabled: false }];
    editorState.activeSegment = 0;
    markEdited();
  }
}

// Parte un segmento en su posición de origen `srcMs`. Las dos mitades quedan contiguas (sin
// hueco) y cada una fija como tamaño máximo su propio rango: ese material ya no pertenece a la
// otra mitad, así que ninguna podrá alargarse más allá de su corte.
export function cutSegmentAt(index: number, srcMs: number) {
  const seg = editorState.segments[index];
  if (!seg) return;
  if (srcMs - seg.startMs < MIN_SEG_MS || seg.endMs - srcMs < MIN_SEG_MS) return;
  const leftDur = srcMs - seg.startMs;
  const newSegs = [...editorState.segments];
  newSegs.splice(index, 1,
    { startMs: seg.startMs, endMs: srcMs, posMs: seg.posMs, boundStartMs: seg.startMs, boundEndMs: srcMs, disabled: seg.disabled },
    { startMs: srcMs, endMs: seg.endMs, posMs: seg.posMs + leftDur, boundStartMs: srcMs, boundEndMs: seg.endMs, disabled: seg.disabled },
  );
  editorState.segments = newSegs;
  editorState.activeSegment = index + 1;
  markEdited();
}

export function removeSegment(index: number) {
  if (editorState.segments.length <= 1) return;
  const newSegs = editorState.segments.filter((_, i) => i !== index);
  editorState.segments = newSegs;
  if (editorState.activeSegment >= newSegs.length) {
    editorState.activeSegment = newSegs.length - 1;
  }
  markEdited();
}

export function selectSegment(index: number) {
  if (index >= 0 && index < editorState.segments.length) {
    editorState.activeSegment = index;
  }
}

// Recorta un borde (in/out) de un segmento, acotado a su tamaño máximo [boundStartMs, boundEndMs]
// y a una longitud mínima. Al recortar el inicio, el borde izquierdo se mueve también en la línea
// de tiempo (posMs) para que el borde derecho quede fijo.
export function trimSegment(index: number, edge: 'start' | 'end', newMs: number) {
  const segs = editorState.segments.map((s) => ({ ...s }));
  const seg = segs[index];
  if (!seg) return;

  if (edge === 'start') {
    // El array está ordenado por posición: la sección previa marca hasta dónde puede crecer el
    // borde izquierdo sin solaparla.
    const prev = segs[index - 1];
    const leftLimitPos = prev ? prev.posMs + (prev.endMs - prev.startMs) : 0;
    const minStartByPos = seg.startMs + (leftLimitPos - seg.posMs);
    const newStart = Math.max(seg.boundStartMs, minStartByPos, Math.min(newMs, seg.endMs - MIN_SEG_MS));
    seg.posMs = Math.max(0, seg.posMs + (newStart - seg.startMs));
    seg.startMs = newStart;
  } else {
    seg.endMs = Math.min(seg.boundEndMs, Math.max(newMs, seg.startMs + MIN_SEG_MS));
  }

  segs[index] = seg;
  editorState.segments = segs;
  markEdited();
}

// Reordena el array según la posición en la línea de tiempo (izquierda→derecha). Como la salida
// se reproduce/exporta en orden de array, así coincide con lo que ve el usuario.
export function sortSegmentsByPos() {
  editorState.segments = [...editorState.segments].sort((a, b) => a.posMs - b.posMs);
  markEdited();
}

let persistTimer: ReturnType<typeof setTimeout> | null = null;

function serializeSegments(enabledOnly = false) {
  const list = enabledOnly ? editorState.segments.filter((s) => !s.disabled) : editorState.segments;
  return list.map((s) => ({
    start_ms: s.startMs,
    end_ms: s.endMs,
    pos_ms: s.posMs,
    bound_start_ms: s.boundStartMs,
    bound_end_ms: s.boundEndMs,
    disabled: s.disabled,
  }));
}

export async function persistEdit() {
  if (!editorState.clip?.path || editorState.durationMs <= 0) return;
  try {
    await invoke('save_clip_edit', {
      path: editorState.clip.path,
      edit: {
        segments: serializeSegments(),
        mixer: editorState.mixer,
      },
    });
  } catch (e) {
    console.error('save_clip_edit', e);
  }
}

export function markEdited() {
  if (editorState.clip) editorState.clip.edited = true;
  if (persistTimer) clearTimeout(persistTimer);
  persistTimer = setTimeout(() => {
    persistTimer = null;
    void persistEdit();
  }, 400);
}

// Abre el clip vecino (dir -1 = anterior, +1 = siguiente) dentro del orden visible, persistiendo
// antes la edición en curso (y cancelando el guardado diferido pendiente para que no se dispare
// ya con el clip nuevo cargado).
export async function navigateClip(dir: 1 | -1): Promise<void> {
  const id = editorState.clip?.id;
  if (!id) return;
  const i = clipOrder.list.findIndex((c) => c.id === id);
  if (i < 0) return;
  const next = clipOrder.list[i + dir];
  if (!next) return;
  if (persistTimer) {
    clearTimeout(persistTimer);
    persistTimer = null;
  }
  await persistEdit();
  openEditor(next);
}

export function closeEditor() {
  if (persistTimer) {
    clearTimeout(persistTimer);
    persistTimer = null;
  }
  resetEditorState();
}
