const FPS_KEY = 'flashback.capture.fps';
const QUALITY_KEY = 'flashback.capture.quality';
const RESOLUTION_KEY = 'flashback.capture.resolution';
const MIC_KEY = 'flashback.capture.mic';
const MIC_DEVICE_KEY = 'flashback.capture.micDevice';

export const FPS_OPTIONS = [20, 30, 60, 120, 240];

export type QualityKey = 'low' | 'normal' | 'high' | 'veryhigh' | 'ultra';

// Escalera de calidad alineada con SteelSeries Moments (1080p60): Bajo ≈ 19, Medio ≈ 34,
// Alto ≈ 50, Muy alta ≈ 90, Ultra ≈ 130 Mbps. El bitrate real lo calcula el backend
// (ancho·alto·fps·factor), así escala con la resolución y los fps.
export const QUALITY_OPTIONS: { key: QualityKey; label: string }[] = [
  { key: 'low', label: 'Bajo' },
  { key: 'normal', label: 'Medio' },
  { key: 'high', label: 'Alto' },
  { key: 'veryhigh', label: 'Muy alta' },
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

function loadMic(): boolean {
  if (typeof localStorage === 'undefined') return false;
  return localStorage.getItem(MIC_KEY) === '1';
}

function loadMicDevice(): string {
  if (typeof localStorage === 'undefined') return '';
  return localStorage.getItem(MIC_DEVICE_KEY) ?? '';
}

// Config de captura compartida por la barra superior y los ajustes. Alimenta el backend
// al iniciar grabación/replay (fps + calidad + resolución + micrófono).
export const captureConfig = $state<{
  fps: number;
  quality: QualityKey;
  resolution: number;
  mic: boolean;
  micDevice: string;
}>({
  fps: loadFps(),
  quality: loadQuality(),
  resolution: loadResolution(),
  mic: loadMic(),
  micDevice: loadMicDevice()
});

export function qualityLabel(key: QualityKey): string {
  return QUALITY_OPTIONS.find((o) => o.key === key)?.label ?? key;
}

export function resolutionLabel(height: number): string {
  return RES_OPTIONS.find((o) => o.height === height)?.label ?? `${height}p`;
}

// Bits por píxel y frame de cada calidad. Debe coincidir con bitrate_factor() del backend.
export function qualityFactor(quality: QualityKey): number {
  switch (quality) {
    case 'low':
      return 0.15;
    case 'normal':
      return 0.27;
    case 'veryhigh':
      return 0.72;
    case 'ultra':
      return 1.05;
    default:
      return 0.4; // high (Alto)
  }
}

// Estima el bitrate de vídeo (bps) replicando la fórmula del backend
// (ancho·alto·fps·factor, piso 1 Mbps). Asume 16:9 sobre el alto objetivo: suficiente
// para una estimación de tamaño en la barra sin conocer el aspecto real de la pantalla.
function videoBitrate(quality: QualityKey, height: number, fps: number): number {
  const width = Math.round((height * 16) / 9 / 2) * 2;
  return Math.max(width * height * fps * qualityFactor(quality), 1_000_000);
}

// Tamaño aproximado de un clip de `seconds` con los ajustes dados (vídeo + una pista de audio).
export function estimatedClipSize(
  seconds: number,
  quality: QualityKey,
  height: number,
  fps: number
): string {
  const bps = videoBitrate(quality, height, fps) + 128_000;
  const mb = ((bps / 8) * seconds) / 1_000_000;
  if (mb >= 1000) return `${(mb / 1000).toFixed(mb >= 10_000 ? 0 : 1)} GB`;
  return `${Math.round(mb)} MB`;
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

export function setMic(enabled: boolean) {
  captureConfig.mic = enabled;
  persist(MIC_KEY, enabled ? '1' : '0');
}

export function setMicDevice(id: string) {
  captureConfig.micDevice = id;
  persist(MIC_DEVICE_KEY, id);
}

function persist(key: string, value: string) {
  if (typeof localStorage === 'undefined') return;
  try {
    localStorage.setItem(key, value);
  } catch {
    // sin persistencia disponible
  }
}
