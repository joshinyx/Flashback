<script lang="ts">
  import Icon from '$lib/components/Icon.svelte';
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
    soundLabel,
    SOUND_OPTIONS,
    type SoundLevel
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

  let encoder = $state('Automático');
  let autoDelete = $state(true);

  const folder = 'C:\\Users\\joshiny\\Videos\\Flashback';

  const shortcutRows: { key: HotkeyAction; label: string }[] = [
    { key: 'saveReplay', label: 'Guardar replay' },
    { key: 'record', label: 'Grabar / detener' },
    { key: 'open', label: 'Abrir Flashback' }
  ];

  let rebinding = $state<HotkeyAction | null>(null);
  let liveTokens = $state<string[]>([]);
  let badKey = $state(false);
  let canSave = $derived(liveTokens.length > 0 && hasMainKey(liveTokens));

  let soundDDOpen = $state(false);
  let soundDDEl = $state<HTMLElement | null>(null);

  function pickSound(level: SoundLevel) {
    setReplaySoundLevel(level);
    soundDDOpen = false;
  }

  $effect(() => {
    if (!soundDDOpen) return;
    const onDown = (e: MouseEvent) => {
      if (soundDDEl && !soundDDEl.contains(e.target as Node)) soundDDOpen = false;
    };
    window.addEventListener('mousedown', onDown, true);
    return () => window.removeEventListener('mousedown', onDown, true);
  });

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
      <div class="seg">
        {#each RES_OPTIONS as o (o.height)}
          <button class:on={captureConfig.resolution === o.height} onclick={() => setResolution(o.height)}>{o.label}</button>
        {/each}
      </div>
    </div>

    <div class="setting">
      <div class="info"><h3>Frecuencia</h3><p>Fotogramas por segundo del clip.</p></div>
      <div class="seg">
        {#each FPS_OPTIONS as o (o)}
          <button class:on={captureConfig.fps === o} onclick={() => setFps(o)}>{o}</button>
        {/each}
      </div>
    </div>

    <div class="setting">
      <div class="info"><h3>Calidad</h3><p>Más calidad = archivos más pesados.</p></div>
      <div class="seg">
        {#each QUALITY_OPTIONS as o (o.key)}
          <button class:on={captureConfig.quality === o.key} onclick={() => setQuality(o.key)}>{o.label}</button>
        {/each}
      </div>
    </div>
  </section>

  <section class="panel">
    <span class="label panel-title">Codificación</span>
    <div class="setting">
      <div class="info">
        <h3>Encoder</h3>
        <p>Automático elige el mejor por hardware disponible. <span class="hw mono">NVENC detectado</span></p>
      </div>
      <div class="seg">
        {#each ['Automático', 'NVENC', 'AMF', 'Quick Sync', 'Software'] as o (o)}
          <button class:on={encoder === o} onclick={() => (encoder = o)}>{o}</button>
        {/each}
      </div>
    </div>
  </section>

  <section class="panel">
    <span class="label panel-title">Instant Replay</span>

    <div class="setting">
      <div class="info"><h3>Replay en segundo plano</h3><p>Mantén un buffer listo para guardar.</p></div>
      <button class="switch" class:on={replay.enabled} onclick={() => setReplayEnabled(!replay.enabled)} role="switch" aria-checked={replay.enabled} aria-label="Replay en segundo plano">
        <span class="knob"></span>
      </button>
    </div>

    <div class="setting" class:disabled={!replay.enabled}>
      <div class="info"><h3>Duración del buffer</h3><p>Cuántos segundos/minutos se guardan al pulsar el atajo.</p></div>
      <div class="seg">
        {#each BUFFER_OPTIONS as o (o.seconds)}
          <button class:on={replay.seconds === o.seconds} onclick={() => setReplaySeconds(o.seconds)}>{o.label}</button>
        {/each}
      </div>
    </div>

  </section>

  <section class="panel">
    <span class="label panel-title">Sonido</span>
    <div class="setting">
      <div class="info"><h3>Sonido al guardar</h3><p>Se reproduce un aviso al guardar un replay.</p></div>
      <div class="sound-row">
        <div class="dd" class:open={soundDDOpen} bind:this={soundDDEl}>
          <button
            class="dd-trigger"
            aria-haspopup="listbox"
            aria-expanded={soundDDOpen}
            aria-label="Volumen del sonido"
            onclick={() => (soundDDOpen = !soundDDOpen)}
          >
            <span class="dd-value">{soundLabel(replaySound.level)}</span>
            <span class="dd-chev"><Icon name="chevron-down" size={13} sw={2} /></span>
          </button>
          {#if soundDDOpen}
            <div class="dd-list" role="listbox">
              {#each SOUND_OPTIONS as o (o.key)}
                <button
                  class="dd-item"
                  class:on={replaySound.level === o.key}
                  role="option"
                  aria-selected={replaySound.level === o.key}
                  onclick={() => pickSound(o.key)}
                >
                  {o.label}
                  <span class="dd-check"><Icon name="check" size={13} sw={2.2} /></span>
                </button>
              {/each}
            </div>
          {/if}
        </div>
        <button class="play-btn" aria-label="Probar sonido" onclick={() => playReplaySound()}>
          <Icon name="play" size={18} />
        </button>
      </div>
    </div>
  </section>

  <section class="panel">
    <span class="label panel-title">Almacenamiento</span>
    <div class="setting">
      <div class="info"><h3>Carpeta de clips</h3><p class="mono path">{folder}</p></div>
      <button class="btn"><Icon name="folder" size={15} /> Cambiar</button>
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
  .hw {
    color: var(--accent);
    font-size: 11px;
    margin-left: 4px;
  }

  .seg {
    display: flex;
    flex-shrink: 0;
    padding: 3px;
    gap: 2px;
    background: var(--bg-0);
    border: 1px solid var(--line);
    border-radius: var(--r-sm);
  }
  .seg button {
    padding: 7px 13px;
    font-family: var(--font-mono);
    font-size: 12px;
    color: var(--text-2);
    border-radius: 5px;
    transition: color 0.14s ease, background 0.14s ease;
  }
  .seg button:hover {
    color: var(--text-0);
  }
  .seg button.on {
    color: var(--on-accent);
    background: var(--accent);
    font-weight: 600;
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
  .dd {
    position: relative;
    width: 132px;
  }
  .dd-trigger {
    display: flex;
    align-items: center;
    justify-content: space-between;
    width: 100%;
    height: 34px;
    padding: 0 12px;
    font-size: 13px;
    color: var(--text-1);
    background: var(--bg-2);
    border: 1px solid var(--line-strong);
    border-radius: var(--r-sm);
    transition: background 0.15s ease, border-color 0.15s ease, color 0.15s ease;
  }
  .dd-trigger:hover {
    background: var(--bg-3);
    color: var(--text-0);
  }
  .dd.open .dd-trigger {
    border-color: var(--accent);
  }
  .dd-chev {
    display: inline-flex;
    color: var(--text-2);
    transition: transform 0.18s ease;
  }
  .dd.open .dd-chev {
    transform: rotate(180deg);
  }
  .dd-list {
    position: absolute;
    top: calc(100% + 6px);
    left: 0;
    right: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
    padding: 5px;
    background: var(--bg-1);
    border: 1px solid var(--line-strong);
    border-radius: var(--r-sm);
    box-shadow: 0 18px 42px -14px rgba(0, 0, 0, 0.7);
    z-index: 70;
  }
  .dd-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    padding: 8px 10px;
    font-size: 13px;
    color: var(--text-1);
    border-radius: 6px;
    transition: background 0.13s ease, color 0.13s ease;
  }
  .dd-item:hover {
    background: var(--bg-3);
    color: var(--text-0);
  }
  .dd-item.on {
    color: var(--text-0);
  }
  .dd-check {
    opacity: 0;
    flex-shrink: 0;
    color: var(--accent);
  }
  .dd-item.on .dd-check {
    opacity: 1;
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
</style>
