<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { getVersion } from '@tauri-apps/api/app';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import Icon from '$lib/components/Icon.svelte';
  import Dropdown from '$lib/components/Dropdown.svelte';
  import { refreshLibrary } from '$lib/library.svelte';
  import {
    hotkeys,
    capture,
    setHotkey,
    labelFor,
    comboFromEvent,
    hasMainKey,
    eventHasUnsupportedKey,
    type HotkeyAction
  } from '$lib/hotkeys.svelte';

  import { replay, setReplayEnabled, setReplaySeconds, BUFFER_OPTIONS } from '$lib/replay.svelte';
  import {
    replaySound,
    setReplaySoundLevel,
    playReplaySound,
    SOUND_OPTIONS
  } from '$lib/replay-sound.svelte';
  import {
    captureConfig,
    setFps,
    setQuality,
    setResolution,
    FPS_OPTIONS,
    QUALITY_OPTIONS,
    RES_OPTIONS
  } from '$lib/capture-config.svelte';

  const ENCODER_OPTIONS = ['Auto', 'NVENC', 'AMF', 'Quick Sync', 'Software'] as const;
  type EncoderOption = typeof ENCODER_OPTIONS[number];

  const resOptions = RES_OPTIONS.map((o) => ({ label: o.label, value: o.height }));
  const fpsOptions = FPS_OPTIONS.map((o) => ({ label: `${o} fps`, value: o }));
  const qualityOptions = QUALITY_OPTIONS.map((o) => ({ label: o.label, value: o.key }));
  const encoderOptions = ENCODER_OPTIONS.map((o) => ({ label: o, value: o }));
  const bufferOptions = BUFFER_OPTIONS.map((o) => ({ label: o.label, value: o.seconds }));
  const soundOptions = SOUND_OPTIONS.map((o) => ({ label: o.label, value: o.key }));

  let encoder = $state<EncoderOption>('Auto');
  let autoDelete = $state(true);

  let folder = $state('');
  let changingFolder = $state(false);
  let appVersion = $state('');
  $effect(() => {
    invoke<string>('clips_dir').then((d) => (folder = d)).catch(() => {});
    invoke<string>('get_encoder').then((e) => {
      if (ENCODER_OPTIONS.includes(e as EncoderOption)) encoder = e as EncoderOption;
    }).catch(() => {});
    getVersion().then((v) => (appVersion = v)).catch(() => {});
  });

  function openLink(url: string) {
    openUrl(url).catch(() => {});
  }

  function setEncoder(opt: EncoderOption) {
    encoder = opt;
    invoke('set_encoder', { enc: opt }).catch(() => {});
  }

  async function changeFolder() {
    if (changingFolder) return;
    changingFolder = true;
    try {
      const picked = await invoke<string | null>('pick_folder');
      if (picked) {
        await invoke('set_clips_dir', { dir: picked });
        folder = picked;
        // La biblioteca lee de la carpeta activa: recargar para reflejar el cambio.
        await refreshLibrary();
      }
    } catch (e) {
      console.error('set_clips_dir', e);
    } finally {
      changingFolder = false;
    }
  }

  const shortcutRows: { key: HotkeyAction; label: string }[] = [
    { key: 'saveReplay', label: 'Guardar replay' },
    { key: 'record', label: 'Grabar / detener' },
    { key: 'open', label: 'Abrir Flashback' }
  ];

  let rebinding = $state<HotkeyAction | null>(null);
  let liveTokens = $state<string[]>([]);
  let badKey = $state(false);
  let canSave = $derived(liveTokens.length > 0 && hasMainKey(liveTokens));

  function onKeyDown(e: KeyboardEvent) {
    if (!rebinding) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.code === 'Escape') {
      endCapture(false);
      return;
    }
    const combo = comboFromEvent(e);
    if (combo.length && hasMainKey(combo)) {
      liveTokens = combo;
      badKey = false;
    } else if (eventHasUnsupportedKey(e)) {
      badKey = true;
    }
  }

  function startCapture(action: HotkeyAction) {
    rebinding = action;
    liveTokens = [];
    badKey = false;
    // Soltar los atajos globales mientras se escucha, o el SO se traga la combinación.
    capture.active = true;
    window.addEventListener('keydown', onKeyDown, true);
  }

  function endCapture(save: boolean) {
    window.removeEventListener('keydown', onKeyDown, true);
    if (save && rebinding && liveTokens.length && hasMainKey(liveTokens)) {
      setHotkey(rebinding, liveTokens.join('+'));
    }
    rebinding = null;
    liveTokens = [];
    badKey = false;
    capture.active = false;
  }

  // Tocar el atajo inicia la captura; se guarda con el botón ✓ y ESC cancela. Tocar
  // otra fila cancela la reasignación en curso sin guardar.
  function startRebind(action: HotkeyAction) {
    if (rebinding === action) return;
    if (rebinding) endCapture(false);
    startCapture(action);
  }

  $effect(() => {
    return () => endCapture(false);
  });
</script>

<div class="settings">
  <header><h1>Ajustes</h1></header>

  <section class="panel">
    <span class="label panel-title">Captura</span>

    <div class="setting">
      <div class="info">
        <h3>Resolución</h3>
        <p>Alto de salida. Se escala desde la captura nativa, sin superarla.</p>
      </div>
      <Dropdown value={captureConfig.resolution} options={resOptions} onchange={setResolution} ariaLabel="Resolución" />
    </div>

    <div class="setting">
      <div class="info"><h3>Fotogramas por segundo</h3><p>Cantidad de fotogramas grabados por segundo.</p></div>
      <Dropdown value={captureConfig.fps} options={fpsOptions} onchange={setFps} ariaLabel="Fotogramas por segundo" />
    </div>

    <div class="setting">
      <div class="info"><h3>Calidad</h3><p>Más calidad produce archivos más pesados.</p></div>
      <Dropdown value={captureConfig.quality} options={qualityOptions} onchange={setQuality} ariaLabel="Calidad" />
    </div>

    <div class="setting">
      <div class="info"><h3>Replay en segundo plano</h3><p>Mantén un buffer listo para guardar.</p></div>
      <button class="switch" class:on={replay.enabled} onclick={() => setReplayEnabled(!replay.enabled)} role="switch" aria-checked={replay.enabled} aria-label="Replay en segundo plano">
        <span class="knob"></span>
      </button>
    </div>

    <div class="setting" class:disabled={!replay.enabled}>
      <div class="info"><h3>Duración del buffer</h3><p>Cuántos segundos/minutos se guardan al pulsar el atajo.</p></div>
      <Dropdown value={replay.seconds} options={bufferOptions} onchange={setReplaySeconds} ariaLabel="Duración del buffer" />
    </div>
  </section>

  <section class="panel">
    <span class="label panel-title">Codificación</span>
    <div class="setting">
      <div class="info">
        <h3>Encoder</h3>
        <p>Auto elige el mejor encoder por hardware disponible. Software usa CPU.</p>
      </div>
      <Dropdown value={encoder} options={encoderOptions} onchange={setEncoder} ariaLabel="Encoder" />
    </div>
  </section>

  <section class="panel">
    <span class="label panel-title">Sonido</span>
    <div class="setting">
      <div class="info"><h3>Sonido al guardar</h3><p>Se reproduce un aviso al guardar un replay.</p></div>
      <div class="sound-row">
        <Dropdown value={replaySound.level} options={soundOptions} onchange={setReplaySoundLevel} ariaLabel="Volumen del sonido" />
        <button class="play-btn" aria-label="Probar sonido" onclick={() => playReplaySound()}>
          <Icon name="play" size={18} />
        </button>
      </div>
    </div>
  </section>

  <section class="panel">
    <span class="label panel-title">Almacenamiento</span>
    <div class="setting">
      <div class="info"><h3>Carpeta de clips</h3><p class="mono path">{folder}</p><p>Cambiarla solo afecta a los clips nuevos; los anteriores se siguen mostrando.</p></div>
      <button class="btn" onclick={changeFolder} disabled={changingFolder}><Icon name="folder" size={15} /> Cambiar</button>
    </div>
    <div class="setting">
      <div class="info"><h3>Borrado automático</h3><p>Elimina los clips no marcados como favoritos al llenarse.</p></div>
      <button class="switch" class:on={autoDelete} onclick={() => (autoDelete = !autoDelete)} role="switch" aria-checked={autoDelete} aria-label="Borrado automático">
        <span class="knob"></span>
      </button>
    </div>
  </section>

  <section class="panel" id="atajos">
    <span class="label panel-title">Atajos</span>
    <p class="hk-hint">Toca un atajo, pulsa la nueva combinación y guárdala con <strong>✓</strong>. <strong>ESC</strong> cancela.</p>
    {#each shortcutRows as row (row.key)}
      <div class="setting">
        <div class="info"><h3>{row.label}</h3></div>
        <div class="hk-edit">
          <button
            type="button"
            class="combo mono"
            class:rec={rebinding === row.key}
            class:bad={rebinding === row.key && badKey && liveTokens.length === 0}
            onclick={() => startRebind(row.key)}
            aria-label={`Cambiar atajo de ${row.label}`}
          >
            {#if rebinding === row.key}
              {#if liveTokens.length}
                {labelFor(liveTokens.join('+'))}
              {:else if badKey}
                Tecla no compatible
              {:else}
                Pulsa una tecla…
              {/if}
            {:else}
              {labelFor(hotkeys[row.key])}
            {/if}
          </button>
          {#if rebinding === row.key}
            <button
              type="button"
              class="combo-save"
              disabled={!canSave}
              onclick={() => endCapture(true)}
              aria-label="Guardar atajo"
            >
              <Icon name="check" size={12} sw={3} />
            </button>
          {/if}
        </div>
      </div>
    {/each}
  </section>

  <section class="panel">
    <span class="label panel-title">Acerca de</span>
    <div class="about-head">
      <img class="about-logo" src="/flashback-mono.svg" alt="" />
      <div class="about-id">
        <span class="about-name">Flashback{#if appVersion}<span class="about-ver mono">v{appVersion}</span>{/if}</span>
        <span class="about-desc">Captura y edición ligera de clips de juego para Windows.</span>
      </div>
    </div>
    <div class="setting">
      <div class="info"><h3>Desarrollado por</h3></div>
      <button class="about-link" onclick={() => openLink('https://joshiny.dev')}>Josh Bernal</button>
    </div>
    <div class="setting">
      <div class="info"><h3>Licencia</h3></div>
      <span class="about-val mono">GPL-3.0-only</span>
    </div>
    <div class="setting">
      <div class="info"><h3>Código fuente</h3></div>
      <button class="about-link" onclick={() => openLink('https://github.com/kuzpire/Flashback')}>kuzpire/Flashback</button>
    </div>
  </section>
</div>

<style>
  .settings {
    padding: 22px 26px 48px;
    max-width: 860px;
  }
  header {
    margin-bottom: 22px;
  }
  h1 {
    font-size: 22px;
    font-weight: 650;
    letter-spacing: -0.01em;
  }

  .panel {
    background: var(--bg-1);
    border: 1px solid var(--line);
    border-radius: var(--r-lg);
    padding: 8px 20px;
    margin-bottom: 18px;
  }
  .panel-title {
    display: block;
    padding: 14px 0 6px;
    color: var(--accent-soft);
  }

  .setting {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 24px;
    padding: 16px 0;
    border-top: 1px solid var(--line);
  }
  .panel-title + .setting {
    border-top: none;
  }
  .setting.disabled {
    opacity: 0.45;
    pointer-events: none;
  }
  .info h3 {
    font-size: 14.5px;
    font-weight: 560;
    margin-bottom: 3px;
  }
  .info p {
    font-size: 12.5px;
    color: var(--text-2);
  }
  .path {
    font-size: 11.5px;
    color: var(--text-1);
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
    background: var(--accent-deep);
    border-color: transparent;
  }
  .switch.on .knob {
    transform: translateX(19px);
    background: var(--on-accent);
  }

  .btn {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    flex-shrink: 0;
    padding: 9px 15px;
    font-size: 13px;
    color: var(--text-1);
    background: var(--bg-2);
    border: 1px solid var(--line-strong);
    border-radius: var(--r-sm);
    transition: background 0.15s ease, color 0.15s ease;
  }
  .btn:hover {
    background: var(--bg-3);
    color: var(--text-0);
  }

  .hk-hint {
    font-size: 12px;
    color: var(--text-2);
    padding: 2px 0 10px;
  }
  .hk-hint strong {
    color: var(--text-1);
    font-weight: 600;
  }

  .hk-edit {
    position: relative;
    display: flex;
    align-items: center;
    flex-shrink: 0;
  }
  .combo {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 132px;
    height: 34px;
    padding: 0 14px;
    font-size: 12px;
    letter-spacing: 0.04em;
    color: var(--text-1);
    background: var(--bg-0);
    border: 1px solid var(--line);
    border-radius: var(--r-sm);
    transition: background 0.15s ease, color 0.15s ease, border-color 0.15s ease;
  }
  .combo:hover {
    color: var(--text-0);
    border-color: var(--line-strong);
  }
  .combo.rec {
    color: var(--accent);
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 12%, var(--bg-0));
  }
  .combo.bad {
    color: var(--rec);
    border-color: var(--rec);
    background: color-mix(in srgb, var(--rec) 12%, var(--bg-0));
  }
  .combo-save {
    position: absolute;
    top: -8px;
    right: -8px;
    z-index: 5;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    color: #000;
    background: #fff;
    border-radius: 999px;
    box-shadow: 0 3px 10px -2px rgba(0, 0, 0, 0.6);
    transition: transform 0.12s ease, opacity 0.12s ease;
  }
  .combo-save:hover:not(:disabled) {
    transform: scale(1.08);
  }
  .combo-save:active:not(:disabled) {
    transform: scale(0.94);
  }
  .combo-save:disabled {
    cursor: default;
  }

  .sound-row {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-shrink: 0;
  }
  .play-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    width: 44px;
    height: 34px;
    color: #000;
    background: var(--accent);
    border: 1px solid transparent;
    border-radius: var(--r-sm);
    transition: background 0.15s ease, transform 0.1s ease;
  }
  .play-btn:hover {
    background: var(--accent-deep);
  }
  .play-btn:active {
    transform: scale(0.96);
  }

  .about-head {
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 18px 0;
  }
  .about-logo {
    width: 38px;
    height: 38px;
    flex-shrink: 0;
  }
  .about-id {
    display: flex;
    flex-direction: column;
    gap: 3px;
    min-width: 0;
  }
  .about-name {
    font-size: 15px;
    font-weight: 600;
    color: var(--text-0);
  }
  .about-ver {
    margin-left: 7px;
    font-size: 12px;
    font-weight: 500;
    color: var(--text-2);
  }
  .about-desc {
    font-size: 12.5px;
    color: var(--text-2);
  }
  .about-link {
    flex-shrink: 0;
    font-size: 13px;
    color: var(--accent-soft);
    transition: color 0.15s ease;
  }
  .about-link:hover {
    color: var(--accent);
    text-decoration: underline;
  }
  .about-val {
    flex-shrink: 0;
    font-size: 12.5px;
    color: var(--text-1);
  }
</style>
