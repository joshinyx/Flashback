import { invoke } from '@tauri-apps/api/core';

export type SeenGame = {
  name: string;
  steam_appid: number | null;
  last_seen: number;
};

let disabledGames = $state<string[]>([]);
let disabledLoaded = false;

export const gameSettings = {
  get disabled() { return disabledGames; },
  isDisabled(name: string): boolean {
    return disabledGames.includes(name);
  }
};

export async function loadDisabledGames() {
  if (disabledLoaded) return;
  try {
    disabledGames = await invoke<string[]>('get_disabled_games');
    disabledLoaded = true;
  } catch {}
}

export async function toggleGameDisabled(name: string) {
  const idx = disabledGames.indexOf(name);
  if (idx >= 0) {
    disabledGames.splice(idx, 1);
  } else {
    disabledGames.push(name);
  }
  try {
    await invoke('set_disabled_games', { games: [...disabledGames] });
  } catch {}
}

export async function fetchSeenGames(): Promise<SeenGame[]> {
  try {
    return await invoke<SeenGame[]>('get_seen_games');
  } catch {
    return [];
  }
}
