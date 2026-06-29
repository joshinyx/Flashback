<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import Icon from '$lib/components/Icon.svelte';
  import { type SeenGame, gameSettings, loadDisabledGames, toggleGameDisabled, fetchSeenGames } from '$lib/games.svelte';

  type Detected = { name: string; steam_appid: number | null };

  let seenGames = $state<SeenGame[]>([]);
  let currentGame = $state<Detected | null>(null);
  let logos = $state<Record<string, string | null>>({});

  function initials(name: string): string {
    const parts = name.replace(/[^a-zA-Z0-9 ]/g, '').split(/\s+/).filter(Boolean);
    return parts.slice(0, 2).map((w) => w[0]).join('').toUpperCase() || '?';
  }

  function lastSeenLabel(ts: number): string {
    const diff = Math.floor(Date.now() / 1000 - ts);
    if (diff < 60) return 'Hace un momento';
    if (diff < 3600) return `Hace ${Math.floor(diff / 60)} min`;
    if (diff < 86400) return `Hace ${Math.floor(diff / 3600)} h`;
    const days = Math.floor(diff / 86400);
    if (days === 1) return 'Ayer';
    if (days < 7) return `Hace ${days} días`;
    if (days < 30) return `Hace ${Math.floor(days / 7)} semana${Math.floor(days / 7) > 1 ? 's' : ''}`;
    if (days < 365) return `Hace ${Math.floor(days / 30)} mes${Math.floor(days / 30) > 1 ? 'es' : ''}`;
    return `Hace ${Math.floor(days / 365)} año${Math.floor(days / 365) > 1 ? 's' : ''}`;
  }

  function logoKey(name: string, steam_appid: number | null): string {
    return steam_appid ? `steam:${steam_appid}` : `name:${name}`;
  }

  async function ensureLogo(name: string, steam_appid: number | null) {
    const key = logoKey(name, steam_appid);
    if (key in logos) return;
    logos[key] = null;
    try {
      const url = await invoke<string | null>('game_icon', { name, steamAppid: steam_appid });
      logos = { ...logos, [key]: url ?? null };
    } catch {}
  }

  $effect(() => {
    loadDisabledGames();
    (async () => {
      const [seen, detected] = await Promise.all([
        fetchSeenGames(),
        invoke<Detected | null>('detect_game').catch(() => null)
      ]);
      seenGames = seen;
      currentGame = detected;
      for (const g of seen) ensureLogo(g.name, g.steam_appid);
      if (detected) ensureLogo(detected.name, detected.steam_appid);
    })();
  });

  const otherGames = $derived(
    currentGame
      ? seenGames.filter((g) => g.name !== currentGame!.name)
      : seenGames
  );
</script>

<div class="games">
  <header class="head">
    <h1>Juegos detectados</h1>
  </header>

  {#if currentGame}
    <section class="game-group">
      <div class="game-group-head">
        <span class="label">Ahora mismo</span>
        <span class="dash"></span>
      </div>
      <div class="game-list">
        <div class="game-row running" class:cap-off={gameSettings.isDisabled(currentGame.name)}>
          <span class="game-ico mono">
            {#if logos[logoKey(currentGame.name, currentGame.steam_appid)]}
              <img src={logos[logoKey(currentGame.name, currentGame.steam_appid)]} alt={currentGame.name} />
            {:else}
              {initials(currentGame.name)}
            {/if}
          </span>
          <div class="game-info">
            <span class="game-name">{currentGame.name}</span>
            <span class="game-path">{gameSettings.isDisabled(currentGame.name) ? 'Captura deshabilitada' : 'Capturando clips'}</span>
          </div>
          <div class="cap-toggle">
            <span class="cap-toggle-label">Capturar</span>
            <button
              class="switch"
              class:on={!gameSettings.isDisabled(currentGame.name)}
              onclick={() => toggleGameDisabled(currentGame!.name)}
              role="switch"
              aria-checked={!gameSettings.isDisabled(currentGame.name)}
              aria-label={`Capturar ${currentGame.name}`}
            >
              <span class="knob"></span>
            </button>
          </div>
        </div>
      </div>
    </section>
  {/if}

  {#if otherGames.length > 0}
    <section class="game-group">
      <div class="game-group-head">
        <span class="label">Juegos Recientes</span>
        <span class="dash"></span>
      </div>
      <div class="game-list">
        {#each otherGames as g (g.name)}
          {@const key = logoKey(g.name, g.steam_appid)}
          {@const logo = logos[key] ?? null}
          {@const disabled = gameSettings.isDisabled(g.name)}
          <div class="game-row" class:cap-off={disabled}>
            <span class="game-ico mono">
              {#if logo}<img src={logo} alt={g.name} />{:else}{initials(g.name)}{/if}
            </span>
            <div class="game-info">
              <span class="game-name">{g.name}</span>
              <span class="game-sub">
                <span class="game-path sub-primary">{lastSeenLabel(g.last_seen)}</span>
                <span class="game-path mono sub-secondary">{disabled ? 'Captura deshabilitada' : 'Captura activa'}</span>
              </span>
            </div>
            <div class="cap-toggle">
              <span class="cap-toggle-label">Capturar</span>
              <button
                class="switch"
                class:on={!disabled}
                onclick={() => toggleGameDisabled(g.name)}
                role="switch"
                aria-checked={!disabled}
                aria-label={`Capturar ${g.name}`}
              >
                <span class="knob"></span>
              </button>
            </div>
          </div>
        {/each}
      </div>
    </section>
  {/if}

  {#if !currentGame && otherGames.length === 0}
    <div class="empty">
      <Icon name="gamepad" size={46} sw={1.3} />
      <p>Aún no has abierto ningún juego con Flashback activo.</p>
      <span class="hint mono">Los juegos aparecen aquí la primera vez que los detectamos.</span>
    </div>
  {/if}
</div>

<style>
  .games {
    padding: 22px 26px 40px;
  }

  .head {
    margin-bottom: 26px;
  }
  h1 {
    font-size: 22px;
    font-weight: 650;
    letter-spacing: -0.01em;
  }

  .game-group {
    margin-bottom: 28px;
  }
  .game-group-head {
    display: flex;
    align-items: center;
    gap: 11px;
    margin-bottom: 13px;
  }
  .dash {
    flex: 1;
    height: 1px;
    background: var(--line);
  }

  .game-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .game-row {
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 13px 16px;
    background: var(--bg-1);
    border: 1px solid var(--line);
    border-radius: var(--r-md);
    transition: border-color 0.15s ease, opacity 0.15s ease;
  }
  .game-row:hover {
    border-color: var(--line-strong);
  }
  .game-row.running {
    background: var(--bg-2);
  }
  .game-row.cap-off {
    opacity: 0.55;
  }

  .game-ico {
    display: grid;
    place-items: center;
    width: 46px;
    height: 46px;
    flex-shrink: 0;
    font-size: 14px;
    font-weight: 600;
    letter-spacing: 0.02em;
    color: var(--text-2);
    border-radius: 10px;
    overflow: hidden;
  }
  .game-ico img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    border-radius: 10px;
  }
  .game-info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .game-name {
    font-size: 14.5px;
    font-weight: 560;
    color: var(--text-0);
    line-height: 1.1;
  }
  .game-path {
    font-size: 11.5px;
    color: var(--text-3);
    line-height: 1.2;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .game-sub {
    position: relative;
    display: block;
    height: 1.2em;
    overflow: hidden;
  }
  .game-sub .game-path {
    position: absolute;
    left: 0;
    top: 0;
    width: 100%;
    transition: transform 0.3s cubic-bezier(0.4, 0, 0.2, 1), opacity 0.25s ease;
  }
  .sub-primary { transform: translateY(0); opacity: 1; }
  .sub-secondary { transform: translateY(105%); opacity: 0; color: var(--text-2); }
  .game-row:hover .sub-primary { transform: translateY(-105%); opacity: 0; }
  .game-row:hover .sub-secondary { transform: translateY(0); opacity: 1; }
  @media (prefers-reduced-motion: reduce) {
    .game-sub .game-path { transition: none; }
  }

  .cap-toggle {
    display: flex;
    align-items: center;
    gap: 9px;
    flex-shrink: 0;
    padding-left: 16px;
    margin-left: 2px;
    border-left: 1px solid var(--line);
  }
  .cap-toggle-label {
    font-size: 11px;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--text-2);
  }

  .switch {
    flex-shrink: 0;
    width: 44px;
    height: 25px;
    border-radius: 999px;
    background: var(--bg-3);
    border: 1px solid var(--line);
    padding: 2px;
    transition: background 0.18s ease, border-color 0.18s ease;
  }
  .switch .knob {
    display: block;
    width: 19px;
    height: 19px;
    border-radius: 999px;
    background: var(--text-2);
    transition: transform 0.18s ease, background 0.18s ease;
  }
  .switch.on {
    background: var(--bright);
    border-color: transparent;
  }
  .switch.on .knob {
    transform: translateX(19px);
    background: var(--bg-1);
  }

  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 14px;
    padding: 90px 0;
    color: var(--text-3);
  }
  .empty p {
    font-size: 14px;
    color: var(--text-2);
    text-align: center;
  }
  .empty .hint {
    font-size: 11.5px;
    color: var(--text-3);
  }
</style>
