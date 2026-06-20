const FPS_KEY = 'flashback.capture.fps';
const QUALITY_KEY = 'flashback.capture.quality';
const RESOLUTION_KEY = 'flashback.capture.resolution';
const BITRATE_KEY = 'flashback.capture.bitrate';
const MIC_KEY = 'flashback.capture.mic';
const MIC_DEVICE_KEY = 'flashback.capture.micDevice';

export const FPS_OPTIONS = [20, 30, 60];

// Bitrate manual en Mbps. Solo se usa cuando la calidad es "Personalizado": sobreescribe el
// cálculo automático (p. ej. 100M en 1080p, por encima de Ultra). En el resto de calidades el
// bitrate lo calcula el backend según calidad/resolución/fps.
export const BITRATE_OPTIONS: { mbps: number; label: string }[] = [
  { mbps: 5, label: '5M' },
  { mbps: 10, label: '10M' },
  { mbps: 15, label: '15M' },
  { mbps: 20, label: '20M' },
  { mbps: 30, label: '30M' },
  { mbps: 50, label: '50M' },
  { mbps: 75, label: '75M' },
  { mbps: 100, label: '100M' }
];

export type QualityKey = 'low' | 'normal' | 'high' | 'ultra' | 'custom';

export const QUALITY_OPTIONS: { key: QualityKey; label: string }[] = [
  { key: 'low', label: 'Bajo' },
  { key: 'normal', label: 'Medio' },
  { key: 'high', label: 'Alto' },
  { key: 'ultra', label: 'Ultra' },
  { key: 'custom', label: 'Personalizado' }
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

function loadBitrate(): number {
  if (typeof localStorage === 'undefined') return 50;
  const n = Number(localStorage.getItem(BITRATE_KEY));
  return BITRATE_OPTIONS.some((o) => o.mbps === n) ? n : 50;
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
  bitrate: number;
  mic: boolean;
  micDevice: string;
}>({
  fps: loadFps(),
  quality: loadQuality(),
  resolution: loadResolution(),
  bitrate: loadBitrate(),
  mic: loadMic(),
  micDevice: loadMicDevice()
});

export function qualityLabel(key: QualityKey): string {
  return QUALITY_OPTIONS.find((o) => o.key === key)?.label ?? key;
}

export function resolutionLabel(height: number): string {
  return RES_OPTIONS.find((o) => o.height === height)?.label ?? `${height}p`;
}

export function bitrateLabel(mbps: number): string {
  return BITRATE_OPTIONS.find((o) => o.mbps === mbps)?.label ?? `${mbps}M`;
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

export function setBitrate(mbps: number) {
  captureConfig.bitrate = mbps;
  persist(BITRATE_KEY, String(mbps));
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
