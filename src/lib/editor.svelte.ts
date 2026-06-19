import { invoke, convertFileSrc } from '@tauri-apps/api/core';
import type { Clip } from './clips';

type ClipAudio = { system: string | null; mic: string | null };

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
  startMs: number;
  endMs: number;
};

const MIN_SEG_MS = 50;

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
  keyframes: number[];
  fps: number;
  mixer: MixerState;
  exporting: boolean;
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
  keyframes: [],
  fps: 30,
  mixer: { ...defaultMixer },
  exporting: false,
});

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
  editorState.keyframes = [];
  editorState.fps = 30;
  editorState.mixer = { ...defaultMixer };
  editorState.exporting = false;
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
      editorState.keyframes = await invoke<number[]>('keyframe_times', { path: clip.path });
    } catch {
      editorState.keyframes = [];
    }

    try {
      const fps = await invoke<number>('clip_fps', { path: clip.path });
      if (fps > 0) editorState.fps = fps;
    } catch {
      /* fps por defecto */
    }

    try {
      const saved = await invoke<{ segments: { start_ms: number; end_ms: number }[]; mixer: MixerState }>(
        'load_clip_edit',
        { path: clip.path },
      );
      if (saved?.segments?.length) {
        editorState.segments = saved.segments.map((s) => ({ startMs: s.start_ms, endMs: s.end_ms }));
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
    } catch (e) {
      editorState.error = String(e);
    } finally {
      editorState.loading = false;
    }
  })();
}

export async function exportClip() {
  if (!editorState.clip?.path || editorState.segments.length === 0) return;
  editorState.exporting = true;
  try {
    const src = editorState.clip.path;
    const dot = src.lastIndexOf('.');
    const dst = dot > 0 ? `${src.slice(0, dot)}_edit.mp4` : `${src}_edit.mp4`;

    await invoke('export_clip', {
      src,
      dst,
      edit: {
        segments: editorState.segments.map(s => ({ start_ms: s.startMs, end_ms: s.endMs })),
        mixer: editorState.mixer,
      },
    });
    return dst;
  } catch (e) {
    throw e;
  } finally {
    editorState.exporting = false;
  }
}

export function resetTrim() {
  if (editorState.durationMs > 0) {
    editorState.segments = [{ startMs: 0, endMs: editorState.durationMs }];
    editorState.activeSegment = 0;
    markEdited();
  }
}

// Parte un segmento en su posición de origen `srcMs`. Ambas mitades se conservan, contiguas
// y en el mismo sitio del orden de salida (el corte solo añade un punto de separación).
export function cutSegmentAt(index: number, srcMs: number) {
  const seg = editorState.segments[index];
  if (!seg) return;
  if (srcMs - seg.startMs < MIN_SEG_MS || seg.endMs - srcMs < MIN_SEG_MS) return;
  const newSegs = [...editorState.segments];
  newSegs.splice(index, 1,
    { startMs: seg.startMs, endMs: srcMs },
    { startMs: srcMs, endMs: seg.endMs },
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

// Recorta un borde (in/out) de un segmento. Cada segmento conserva su propio tramo del vídeo
// original, así que el recorte solo se acota a [0, duración] y a una longitud mínima; los
// segmentos son independientes entre sí (pueden referirse incluso a tramos solapados).
export function trimSegment(index: number, edge: 'start' | 'end', newMs: number) {
  const segs = editorState.segments.map((s) => ({ ...s }));
  const seg = segs[index];
  if (!seg) return;

  if (edge === 'start') {
    seg.startMs = Math.max(0, Math.min(newMs, seg.endMs - MIN_SEG_MS));
  } else {
    seg.endMs = Math.min(editorState.durationMs, Math.max(newMs, seg.startMs + MIN_SEG_MS));
  }

  segs[index] = seg;
  editorState.segments = segs;
  markEdited();
}

// Cambia un segmento de posición en el orden de salida (reordenar bloques). Cada bloque
// conserva su contenido; el export y el preview reproducen en este orden.
export function reorderSegment(from: number, to: number) {
  if (from === to) return;
  const segs = [...editorState.segments];
  if (from < 0 || from >= segs.length || to < 0 || to >= segs.length) return;
  const [moved] = segs.splice(from, 1);
  segs.splice(to, 0, moved);
  editorState.segments = segs;
  editorState.activeSegment = to;
  markEdited();
}

let persistTimer: ReturnType<typeof setTimeout> | null = null;

export async function persistEdit() {
  if (!editorState.clip?.path || editorState.durationMs <= 0) return;
  try {
    await invoke('save_clip_edit', {
      path: editorState.clip.path,
      edit: {
        segments: editorState.segments.map((s) => ({ start_ms: s.startMs, end_ms: s.endMs })),
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

export function closeEditor() {
  if (persistTimer) {
    clearTimeout(persistTimer);
    persistTimer = null;
  }
  resetEditorState();
}
