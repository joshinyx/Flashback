const LEVEL_KEY = 'flashback.replay.soundLevel';

export type SoundLevel = 'low' | 'normal' | 'high';

export const SOUND_OPTIONS: { key: SoundLevel; label: string; gain: number }[] = [
  { key: 'low', label: 'Bajo', gain: 0.25 },
  { key: 'normal', label: 'Normal', gain: 0.55 },
  { key: 'high', label: 'Alto', gain: 1.0 }
];

function loadLevel(): SoundLevel {
  if (typeof localStorage === 'undefined') return 'normal';
  const v = localStorage.getItem(LEVEL_KEY);
  return SOUND_OPTIONS.some((o) => o.key === v) ? (v as SoundLevel) : 'normal';
}

export const replaySound = $state<{ level: SoundLevel }>({ level: loadLevel() });

export function setReplaySoundLevel(level: SoundLevel) {
  replaySound.level = level;
  if (typeof localStorage !== 'undefined') {
    try {
      localStorage.setItem(LEVEL_KEY, level);
    } catch {
      // sin persistencia disponible
    }
  }
}

export function gainFor(level: SoundLevel): number {
  return SOUND_OPTIONS.find((o) => o.key === level)?.gain ?? 0.55;
}

export function soundLabel(level: SoundLevel): string {
  return SOUND_OPTIONS.find((o) => o.key === level)?.label ?? 'Normal';
}

// Un único elemento reutilizado: evita crear un Audio por cada reproducción. Suena aunque
// la ventana esté oculta (el proceso del webview sigue vivo durante el juego).
let audio: HTMLAudioElement | null = null;

export function playReplaySound(level: SoundLevel = replaySound.level) {
  if (typeof Audio === 'undefined') return;
  if (!audio) {
    audio = new Audio('/sounds/replay-saved.wav');
    audio.preload = 'auto';
  }
  audio.volume = gainFor(level);
  audio.currentTime = 0;
  audio.play().catch(() => {
    // el navegador puede bloquear la reproducción sin interacción previa
  });
}
