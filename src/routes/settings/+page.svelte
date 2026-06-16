<script lang="ts">
  import Icon from '$lib/components/Icon.svelte';
  import {
    hotkeys,
    capture,
    setHotkey,
    labelFor,
    comboFromEvent,
    hasMainKey,
    type HotkeyAction
  } from '$lib/hotkeys.svelte';
  import { ui, setIcon, iconSrc, type AppIcon } from '$lib/theme.svelte';
  import { replay, setReplayEnabled, setReplaySeconds, BUFFER_OPTIONS } from '$lib/replay.svelte';
  import {
    captureConfig,
    setFps,
    setQuality,
    setResolution,
    FPS_OPTIONS,
    QUALITY_OPTIONS,
    RES_OPTIONS
  } from '$lib/capture-config.svelte';

  const icons: { key: AppIcon; label: string }[] = [
    { key: 'color', label: 'Color' },
    { key: 'mono', label: 'Monocromo' }
  ];

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

  function onKeyDown(e: KeyboardEvent) {
    if (!rebinding) return;
    e.preventDefault();
    e.stopPropagation();
    const combo = comboFromEvent(e);
    if (combo.length) liveTokens = combo;
  }

  function startCapture(action: HotkeyAction) {
    rebinding = action;
    liveTokens = [];
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
    capture.active = false;
  }

  // El mismo botón inicia la captura y, pulsado de nuevo, la guarda. Tocar otra fila
  // cancela la reasignación en curso sin guardar.
  function toggleRebind(action: HotkeyAction) {
    if (rebinding === action) endCapture(true);
    else {
      if (rebinding) endCapture(false);
      startCapture(action);
    }
  }

  $effect(() => {
    return () => endCapture(false);
  });
</script>

<div class="settings">
  <header><h1>Ajustes</h1></header>

  <section class="panel">
    <span class="label panel-title">Apariencia</span>
    <div class="setting">
      <div class="info"><h3>Icono de la app</h3><p>El logo de la barra lateral.</p></div>
      <div class="icon-pick">
        {#each icons as ic (ic.key)}
          <button class="icon-opt" class:on={ui.icon === ic.key} onclick={() => setIcon(ic.key)}>
            <img src={iconSrc(ic.key)} alt={ic.label} />
            <span>{ic.label}</span>
          </button>
        {/each}
      </div>
    </div>
  </section>

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
    <p class="hk-hint">Pulsa <strong>Cambiar</strong>, haz la combinación (1 o 2 teclas) y pulsa <strong>Guardar</strong>.</p>
    {#each shortcutRows as row (row.key)}
      <div class="setting">
        <div class="info"><h3>{row.label}</h3></div>
        <div class="hk-edit">
          <span class="combo mono" class:rec={rebinding === row.key}>
            {#if rebinding === row.key}
              {liveTokens.length ? labelFor(liveTokens.join('+')) : 'Pulsa 1–2 teclas…'}
            {:else}
              {labelFor(hotkeys[row.key])}
            {/if}
          </span>
          <button
            class="rebind"
            class:on={rebinding === row.key}
            onclick={() => toggleRebind(row.key)}
          >
            {rebinding === row.key ? 'Guardar' : 'Cambiar'}
          </button>
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

  .icon-pick {
    display: flex;
    flex-shrink: 0;
    gap: 8px;
  }
  .icon-opt {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 7px;
    width: 88px;
    padding: 12px 10px 9px;
    background: var(--bg-0);
    border: 1px solid var(--line);
    border-radius: var(--r-sm);
    color: var(--text-2);
    transition: border-color 0.14s ease, color 0.14s ease;
  }
  .icon-opt img {
    width: 30px;
    height: 30px;
  }
  .icon-opt span {
    font-size: 11.5px;
  }
  .icon-opt:hover {
    color: var(--text-0);
    border-color: var(--line-strong);
  }
  .icon-opt.on {
    border-color: var(--accent);
    color: var(--text-0);
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
    background: var(--accent);
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
    display: flex;
    align-items: center;
    gap: 10px;
    flex-shrink: 0;
  }
  .combo {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 124px;
    height: 34px;
    padding: 0 14px;
    font-size: 12px;
    letter-spacing: 0.04em;
    color: var(--text-1);
    background: var(--bg-0);
    border: 1px solid var(--line);
    border-radius: var(--r-sm);
  }
  .combo.rec {
    color: var(--accent);
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 12%, var(--bg-0));
  }
  .rebind {
    height: 34px;
    padding: 0 16px;
    font-size: 12.5px;
    color: var(--text-1);
    background: var(--bg-2);
    border: 1px solid var(--line-strong);
    border-radius: var(--r-sm);
    transition: background 0.15s ease, color 0.15s ease, border-color 0.15s ease;
  }
  .rebind:hover {
    background: var(--bg-3);
    color: var(--text-0);
  }
  .rebind.on {
    color: var(--on-accent);
    background: var(--accent);
    border-color: transparent;
    font-weight: 600;
  }
</style>
