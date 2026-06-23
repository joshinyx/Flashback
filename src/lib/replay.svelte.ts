const ENABLED_KEY = 'flashback.replay.enabled';
const SECONDS_KEY = 'flashback.replay.seconds';

export const BUFFER_OPTIONS: { label: string; seconds: number }[] = [
  { label: '00:30', seconds: 30 },
  { label: '01:00', seconds: 60 },
  { label: '02:00', seconds: 120 },
  { label: '03:00', seconds: 180 },
  { label: '05:00', seconds: 300 },
  { label: '10:00', seconds: 600 },
  { label: '15:00', seconds: 900 }
];

function loadEnabled(): boolean {
  if (typeof localStorage === 'undefined') return false;
  return localStorage.getItem(ENABLED_KEY) === '1';
}

function loadSeconds(): number {
  if (typeof localStorage === 'undefined') return 60;
  const n = Number(localStorage.getItem(SECONDS_KEY));
  return BUFFER_OPTIONS.some((o) => o.seconds === n) ? n : 60;
}

export const replay = $state<{ enabled: boolean; seconds: number }>({
  enabled: loadEnabled(),
  seconds: loadSeconds()
});

export function setReplayEnabled(v: boolean) {
  replay.enabled = v;
  if (typeof localStorage !== 'undefined') {
    try {
      localStorage.setItem(ENABLED_KEY, v ? '1' : '0');
    } catch {
      // sin persistencia disponible
    }
  }
}

export function setReplaySeconds(s: number) {
  replay.seconds = s;
  if (typeof localStorage !== 'undefined') {
    try {
      localStorage.setItem(SECONDS_KEY, String(s));
    } catch {
      // sin persistencia disponible
    }
  }
}
