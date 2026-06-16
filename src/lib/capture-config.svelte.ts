const FPS_KEY = 'flashback.capture.fps';
const QUALITY_KEY = 'flashback.capture.quality';
const RESOLUTION_KEY = 'flashback.capture.resolution';

export const FPS_OPTIONS = [20, 30, 60, 120, 144];

export type QualityKey = 'low' | 'normal' | 'high' | 'ultra';

export const QUALITY_OPTIONS: { key: QualityKey; label: string }[] = [
  { key: 'low', label: 'Bajo' },
  { key: 'normal', label: 'Medio' },
  { key: 'high', label: 'Alto' },
  { key: 'ultra', label: 'Ultra' }
];

// Alto objetivo del clip. El backend captura a nativo y escala a este alto (manteniendo
// el aspecto), sin superar la resolución nativa. Se envía como número al backend.
export const RES_OPTIONS: { height: number; label: string }[] = [
  { height: 480, label: '480p' },
  { height: 720, label: '720p' },
  { height: 1080, label: '1080p' },
  { height: 1440, label: '1440p' },
  { height: 2160, label: '2160p' }
];

function loadFps(): number {
  if (typeof localStorage === 'undefined') return 60;
  const n = Number(localStorage.getItem(FPS_KEY));
  return FPS_OPTIONS.includes(n) ? n : 60;
}

function loadQuality(): QualityKey {
  if (typeof localStorage === 'undefined') return 'high';
  const q = localStorage.getItem(QUALITY_KEY);
  return QUALITY_OPTIONS.some((o) => o.key === q) ? (q as QualityKey) : 'high';
}

function loadResolution(): number {
  if (typeof localStorage === 'undefined') return 1080;
  const n = Number(localStorage.getItem(RESOLUTION_KEY));
  return RES_OPTIONS.some((o) => o.height === n) ? n : 1080;
}

// Config de captura compartida por la barra superior y los ajustes. Alimenta el backend
// al iniciar grabación/replay (fps + calidad + resolución).
export const captureConfig = $state<{ fps: number; quality: QualityKey; resolution: number }>({
  fps: loadFps(),
  quality: loadQuality(),
  resolution: loadResolution()
});

export function qualityLabel(key: QualityKey): string {
  return QUALITY_OPTIONS.find((o) => o.key === key)?.label ?? key;
}

export function resolutionLabel(height: number): string {
  return RES_OPTIONS.find((o) => o.height === height)?.label ?? `${height}p`;
}

export function setFps(fps: number) {
  captureConfig.fps = fps;
  persist(FPS_KEY, String(fps));
}

export function setQuality(quality: QualityKey) {
  captureConfig.quality = quality;
  persist(QUALITY_KEY, quality);
}

export function setResolution(height: number) {
  captureConfig.resolution = height;
  persist(RESOLUTION_KEY, String(height));
}

function persist(key: string, value: string) {
  if (typeof localStorage === 'undefined') return;
  try {
    localStorage.setItem(key, value);
  } catch {
    // sin persistencia disponible
  }
}
