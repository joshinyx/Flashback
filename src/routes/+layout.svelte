<script lang="ts">
  import '@fontsource-variable/geist';
  import '../app.css';
  import { page } from '$app/state';
  import { goto } from '$app/navigation';
  import Icon from '$lib/components/Icon.svelte';
  import WindowControls from '$lib/components/WindowControls.svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { register, unregisterAll } from '@tauri-apps/plugin-global-shortcut';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { hotkeys, capture, labelFor } from '$lib/hotkeys.svelte';
  import { ui, iconSrc } from '$lib/theme.svelte';
  import { refreshLibrary } from '$lib/library.svelte';
  import { replay, setReplaySeconds, BUFFER_OPTIONS } from '$lib/replay.svelte';
  import {
    captureConfig,
    setFps,
    setQuality,
    setResolution,
    qualityLabel,
    resolutionLabel,
    FPS_OPTIONS,
    QUALITY_OPTIONS,
    RES_OPTIONS,
    type QualityKey
  } from '$lib/capture-config.svelte';

  let { children } = $props();

  const nav = [
    { href: '/', icon: 'clips-fill', label: 'Clips' },
    { href: '/favoritos', icon: 'bookmark-fill', label: 'Favoritos' }
  ];

  const isActive = (href: string) =>
    href === '/' ? page.url.pathname === '/' : page.url.pathname.startsWith(href);

  type Monitor = {
    id: string;
    label: string;
    width: number;
    height: number;
    primary: boolean;
    thumb: string | null;
  };
  type AudioInput = { id: string; name: string };
  type SegOpt = { val: string; label?: string; raw: number | string };
  type Seg = { key: string; value: string; options: SegOpt[] };

  let monitors = $state<Monitor[]>([]);
  let selectedMonitor = $state<string | null>(null);
  let pickerOpen = $state(false);
  let recording = $state(false);
  let micOn = $state(false);
  let audioInputs = $state<AudioInput[]>([]);
  let micInput = $state('');
  let micDDOpen = $state(false);
  let openSeg = $state<string | null>(null);

  // Ajustes rápidos de la barra, enlazados a los stores persistidos. Tiempo → buffer del
  // replay; calidad, FPS y resolución → config de captura (los consume el backend).
  const secondsLabel = (s: number) => BUFFER_OPTIONS.find((o) => o.seconds === s)?.label ?? `${s}s`;

  const segs = $derived<Seg[]>([
    {
      key: 'tiempo',
      value: secondsLabel(replay.seconds),
      options: BUFFER_OPTIONS.map((o) => ({ val: o.label, raw: o.seconds }))
    },
    {
      key: 'calidad',
      value: qualityLabel(captureConfig.quality),
      options: QUALITY_OPTIONS.map((q) => ({ val: q.label, raw: q.key }))
    },
    {
      key: 'resolucion',
      value: resolutionLabel(captureConfig.resolution),
      options: RES_OPTIONS.map((r) => ({ val: r.label, raw: r.height }))
    },
    {
      key: 'fps',
      value: `${captureConfig.fps} FPS`,
      options: FPS_OPTIONS.map((f) => ({ val: `${f} FPS`, raw: f }))
    }
  ]);

  let notice = $state<string | null>(null);
  let noticeTimer: ReturnType<typeof setTimeout> | null = null;
  function setNotice(msg: string | null) {
    notice = msg;
    if (noticeTimer) clearTimeout(noticeTimer);
    if (msg) noticeTimer = setTimeout(() => (notice = null), 6000);
  }

  const activeMonitor = $derived(monitors.find((m) => m.id === selectedMonitor) ?? null);
  const micName = $derived(audioInputs.find((d) => d.id === micInput)?.name ?? 'Sin micrófonos');

  async function loadMonitors() {
    try {
      monitors = await invoke<Monitor[]>('list_monitors');
    } catch {
      // fuera de Tauri (preview en navegador)
    }
  }

  async function loadAudioInputs() {
    try {
      audioInputs = await invoke<AudioInput[]>('list_audio_inputs');
      if (!audioInputs.some((d) => d.id === micInput)) {
        micInput = audioInputs[0]?.id ?? '';
      }
    } catch {
      // fuera de Tauri (preview en navegador)
    }
  }

  function togglePicker(e: MouseEvent) {
    e.stopPropagation();
    pickerOpen = !pickerOpen;
    if (pickerOpen) {
      loadMonitors();
      loadAudioInputs();
    }
  }

  function closeAll() {
    pickerOpen = false;
    micDDOpen = false;
    openSeg = null;
  }

  function toggleMicDD(e: MouseEvent) {
    e.stopPropagation();
    micDDOpen = !micDDOpen;
  }
  function pickMic(e: MouseEvent, id: string) {
    e.stopPropagation();
    micInput = id;
    micDDOpen = false;
  }

  function toggleSeg(e: MouseEvent, key: string) {
    e.stopPropagation();
    openSeg = openSeg === key ? null : key;
  }
  function pickSeg(e: MouseEvent, key: string, opt: SegOpt) {
    e.stopPropagation();
    if (key === 'tiempo') setReplaySeconds(opt.raw as number);
    else if (key === 'calidad') setQuality(opt.raw as QualityKey);
    else if (key === 'fps') setFps(opt.raw as number);
    else if (key === 'resolucion') setResolution(opt.raw as number);
    openSeg = null;
  }

  // Elegir pantalla solo fija el objetivo; grabar es aparte (atajo / botón).
  async function selectMonitor(id: string) {
    pickerOpen = false;
    if (id === selectedMonitor) return;
    if (recording) await stopRecording();
    selectedMonitor = id;
  }

  async function backToApp() {
    pickerOpen = false;
    if (recording) await stopRecording();
    selectedMonitor = null;
  }

  async function startRecording() {
    if (recording) return;
    const target = captureTarget;
    if (!target) {
      setNotice('Selecciona una pantalla para grabar, o abre un juego para el modo Aplicación.');
      return;
    }
    setNotice('Iniciando grabación…');
    try {
      await invoke('start_capture', {
        target,
        fps: captureConfig.fps,
        quality: captureConfig.quality,
        resolution: captureConfig.resolution
      });
      setNotice(null);
      recording = true;
    } catch (e) {
      setNotice(`No se pudo iniciar la grabación: ${e}`);
      console.error('start_capture', e);
    }
  }

  async function stopRecording() {
    if (!recording) return;
    recording = false;
    try {
      const path = await invoke<string | null>('stop_capture');
      setNotice(path ? `Clip guardado: ${path}` : 'Grabación detenida (no se guardó archivo).');
      if (path) await refreshLibrary();
    } catch (e) {
      setNotice(`Error al detener la grabación: ${e}`);
      console.error('stop_capture', e);
    }
  }

  function toggleRecording() {
    if (recording) stopRecording();
    else startRecording();
  }

  function editHotkey() {
    goto('/settings#atajos');
  }

  async function saveReplay() {
    if (!replay.enabled) {
      setNotice('Activa “Replay en segundo plano” en Ajustes para guardar.');
      return;
    }
    if (!captureTarget) {
      setNotice('Sin objetivo: selecciona una pantalla o abre un juego para que el replay grabe.');
      return;
    }
    setNotice('Guardando replay…');
    try {
      const path = await invoke<string | null>('save_replay');
      if (path) {
        setNotice(`Replay guardado: ${path}`);
        await refreshLibrary();
      } else {
        setNotice('No se pudo guardar el replay (el buffer aún no tiene un keyframe).');
      }
    } catch (e) {
      setNotice(`Error al guardar el replay: ${e}`);
      console.error('save_replay', e);
    }
  }

  async function openFlashback() {
    try {
      const w = getCurrentWindow();
      await w.show();
      await w.unminimize();
      await w.setFocus();
    } catch (e) {
      console.error('open window', e);
    }
  }

  type Detected = { name: string; steam_appid: number | null };

  let game = $state('');
  let frame = $state('');
  let frameKey = '';

  // Objetivo de captura: una pantalla concreta, o la ventana del juego detectado en
  // modo Aplicación. Si es modo Aplicación y NO hay juego, no hay objetivo (null): el
  // usuario debe elegir una pantalla; no se cae al fallback de grabar la principal.
  const captureTarget = $derived(selectedMonitor ? selectedMonitor : game ? 'window' : null);

  async function refresh() {
    try {
      const detected = (await invoke<Detected | null>('detect_game')) ?? null;
      game = detected?.name ?? '';
      if (!detected) {
        frame = '';
        frameKey = '';
        return;
      }
      const key = detected.steam_appid ? `steam:${detected.steam_appid}` : `name:${detected.name}`;
      if (key !== frameKey || !frame) {
        const url = await invoke<string | null>('game_hero', {
          name: detected.name,
          steamAppid: detected.steam_appid
        });
        if (url) {
          frame = url;
          frameKey = key;
        }
      }
    } catch {
      // fuera de Tauri (preview en navegador)
    }
  }

  $effect(() => {
    refresh();
    loadMonitors();
    loadAudioInputs();
    const id = setInterval(refresh, 5000);
    return () => {
      clearInterval(id);
    };
  });

  // Registro de atajos globales: depende de las combinaciones del store y de si se
  // está reasignando alguna (en ese caso se sueltan todos para que el SO no intercepte
  // las teclas). `unregisterAll` al entrar evita callbacks colgados de instancias
  // previas de `tauri dev`; el flag `cancelled` evita que un re-run viejo pise al nuevo.
  $effect(() => {
    const { saveReplay: sr, record: rec, open: op } = hotkeys;
    const paused = capture.active;
    let cancelled = false;
    (async () => {
      try {
        await unregisterAll();
        if (cancelled || paused) return;
        await register(sr, (e) => {
          if (e.state === 'Pressed') saveReplay();
        });
        if (cancelled) return;
        await register(rec, (e) => {
          if (e.state === 'Pressed') toggleRecording();
        });
        if (cancelled) return;
        await register(op, (e) => {
          if (e.state === 'Pressed') openFlashback();
        });
      } catch (e) {
        if (!cancelled) {
          console.error('register hotkeys', e);
          setNotice(`No se pudieron registrar los atajos: ${e}`);
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  });

  // Instant replay: se codifica en segundo plano mientras el toggle esté activo. El
  // efecto reacciona al toggle, a la duración, a la calidad/FPS y a la pantalla objetivo;
  // `lastReplayKey` evita reinicios redundantes cuando nada relevante cambia.
  let lastReplayKey = '';
  $effect(() => {
    const enabled = replay.enabled;
    const seconds = replay.seconds;
    const fps = captureConfig.fps;
    const quality = captureConfig.quality;
    const resolution = captureConfig.resolution;
    const target = captureTarget;
    const key = enabled && target ? `${target}|${seconds}|${fps}|${quality}|${resolution}` : 'off';
    if (key === lastReplayKey) return;
    lastReplayKey = key;
    (async () => {
      try {
        await invoke('stop_replay');
        if (key !== 'off') {
          await invoke('start_replay', { target, seconds, fps, quality, resolution });
        }
      } catch (e) {
        setNotice(`No se pudo iniciar el replay: ${e}`);
        console.error('replay', e);
      }
    })();
  });
</script>

<svelte:window onclick={closeAll} />

<div class="app">
  <aside class="sidebar" data-tauri-drag-region>
    <div class="logo" data-tauri-drag-region>
      <img src={iconSrc(ui.icon)} alt="Flashback" />
    </div>

    <nav>
      {#each nav as item (item.href)}
        <a
          class="nav-item"
          class:active={isActive(item.href)}
          href={item.href}
          aria-label={item.label}
        >
          <Icon name={item.icon} size={24} />
        </a>
      {/each}
    </nav>

    <a
      class="nav-item games-tab"
      class:active={isActive('/juegos')}
      href="/juegos"
      aria-label="Juegos"
    >
      <Icon name="gamepad" size={24} />
    </a>
    <a
      class="nav-item settings-tab"
      class:active={isActive('/settings')}
      href="/settings"
      aria-label="Ajustes"
    >
      <Icon name="settings-fill" size={24} />
    </a>
  </aside>

  <div class="main">
    <header class="topbar" data-tauri-drag-region>
      <div class="capture-target">
        <button
          class="capturing"
          class:idle={!selectedMonitor && !game}
          class:screen={!!selectedMonitor}
          class:rec={recording}
          class:open={pickerOpen}
          onclick={togglePicker}
          aria-haspopup="menu"
          aria-expanded={pickerOpen}
        >
          {#if selectedMonitor}
            <span class="cap-icon">
              {#if recording}<span class="rec-dot"></span>{:else}<Icon name="monitor" size={20} />{/if}
            </span>
          {:else}
            <span class="cap-frame" style:background-image={frame ? `url(${frame})` : 'none'}></span>
          {/if}
          <span class="cap-text">
            <span class="cap-label">
              {#if selectedMonitor}
                {recording ? 'Grabando pantalla' : 'Pantalla lista'}
              {:else}
                {game ? 'Capturando clips' : 'En espera'}
              {/if}
            </span>
            <span class="cap-proc">
              {selectedMonitor ? (activeMonitor?.label ?? 'Pantalla') : game || 'Sin juego'}
            </span>
          </span>
        </button>

        {#if pickerOpen}
          <div class="cap-menu" role="menu">
            <button class="cap-opt" class:on={!selectedMonitor} role="menuitem" onclick={backToApp}>
              <span class="opt-ico"><Icon name="gamepad" size={21} /></span>
              <span class="opt-text">
                <span class="opt-title">Aplicación</span>
                <span class="opt-sub">{game || 'Sin juego'}</span>
              </span>
              {#if !selectedMonitor}<span class="opt-check"><Icon name="check" size={15} sw={2.2} /></span>{/if}
            </button>

            <button
              class="cap-opt mic-opt"
              class:on={micOn}
              role="menuitemcheckbox"
              aria-checked={micOn}
              onclick={(e) => {
                e.stopPropagation();
                micOn = !micOn;
              }}
            >
              <span class="opt-ico"><Icon name="mic" size={21} /></span>
              <span class="mic-label">
                Capturar audio del micrófono
                <span class="help" aria-label="Qué hace esta opción">
                  ?
                  <span class="help-tip" role="tooltip">
                    Si está activo, graba también tu voz desde el micrófono elegido, mezclada con
                    el audio del clip.
                  </span>
                </span>
              </span>
              <span class="mic-switch"><span class="mic-knob"></span></span>
            </button>

            <div class="mic-input">
              <div class="mic-dd" class:open={micDDOpen}>
                <button
                  class="mic-trigger"
                  aria-haspopup="listbox"
                  aria-expanded={micDDOpen}
                  aria-label="Entrada de micrófono"
                  onclick={toggleMicDD}
                >
                  <span class="mic-value">{micName}</span>
                  <span class="mic-chev"><Icon name="chevron-down" size={13} sw={2} /></span>
                </button>
                {#if micDDOpen}
                  <div class="mic-list" role="listbox">
                    {#each audioInputs as inp (inp.id)}
                      <button
                        class="mic-item"
                        class:on={micInput === inp.id}
                        role="option"
                        aria-selected={micInput === inp.id}
                        onclick={(e) => pickMic(e, inp.id)}
                      >
                        {inp.name}
                        <span class="mic-check"><Icon name="check" size={13} sw={2.2} /></span>
                      </button>
                    {/each}
                    {#if audioInputs.length === 0}
                      <button class="mic-item" disabled>Sin micrófonos detectados</button>
                    {/if}
                  </div>
                {/if}
              </div>
            </div>

            <div class="cap-sep"></div>
            <span class="cap-group">Pantallas</span>

            <div class="screen-grid">
              {#each monitors as m (m.id)}
                <button
                  class="screen-card"
                  class:on={selectedMonitor === m.id}
                  role="menuitem"
                  onclick={() => selectMonitor(m.id)}
                >
                  <span class="screen-thumb" style:background-image={m.thumb ? `url(${m.thumb})` : 'none'}>
                    {#if !m.thumb}<Icon name="monitor" size={22} />{/if}
                    {#if selectedMonitor === m.id}
                      <span class="screen-check"><Icon name="check" size={14} sw={2.4} /></span>
                    {/if}
                  </span>
                </button>
              {/each}
            </div>
          </div>
        {/if}
      </div>

      <span class="pill combo mono">
        {#each segs as seg (seg.key)}
          {#if seg.key !== 'tiempo'}<span class="sep">|</span>{/if}
          <span class="segwrap" class:open={openSeg === seg.key}>
            <button
              class="seg"
              aria-haspopup="true"
              aria-expanded={openSeg === seg.key}
              onclick={(e) => toggleSeg(e, seg.key)}
            >
              <span class="seg-val">{seg.value}</span>
              <span class="chev"><Icon name="chevron-down" size={11} sw={2} /></span>
            </button>
            {#if openSeg === seg.key}
              <div class="seg-menu" role="menu">
                {#each seg.options as opt (opt.val)}
                  <button
                    class="seg-opt"
                    class:on={seg.value === opt.val}
                    onclick={(e) => pickSeg(e, seg.key, opt)}
                  >
                    {opt.label ?? opt.val}
                    <span class="seg-check"><Icon name="check" size={13} sw={2.2} /></span>
                  </button>
                {/each}
              </div>
            {/if}
          </span>
        {/each}
      </span>

      <div class="quick">
        <span class="pill combo recpill mono" class:on={recording}>
          <button class="seg rec-seg" onclick={toggleRecording}>
            <span class="rec-ico"><Icon name={recording ? 'stop' : 'play'} size={18} /></span>
            <span class="rec-label">{recording ? 'Detener grabación' : 'Iniciar grabación'}</span>
          </button>
          <span class="sep">|</span>
          <button class="seg rec-hotkey" title="Editar atajo de grabación" onclick={editHotkey}>
            {labelFor(hotkeys.record)}
          </button>
        </span>
      </div>

      <div class="winctl"><WindowControls /></div>
    </header>

    <div class="content">
      {@render children()}
    </div>

    {#if notice}
      <button class="notice mono" onclick={() => setNotice(null)}>{notice}</button>
    {/if}
  </div>
</div>

<style>
  .app {
    position: relative;
    z-index: 1;
    display: flex;
    height: 100vh;
  }

  .sidebar {
    width: var(--sidebar-w);
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    padding: 0 0 14px;
    background: #171717;
  }
  .logo {
    display: grid;
    place-items: center;
    width: 100%;
    height: var(--topbar-h);
    margin-bottom: 8px;
    border-bottom: 1px solid var(--line);
  }
  .logo img {
    display: block;
    width: 30px;
    height: 30px;
    pointer-events: none;
  }

  nav {
    display: flex;
    flex-direction: column;
    gap: 4px;
    width: 100%;
    align-items: center;
  }
  .nav-item {
    width: 52px;
    height: 52px;
    display: grid;
    place-items: center;
    border-radius: var(--r-md);
    color: var(--text-2);
    position: relative;
    transition: color 0.16s ease, background 0.16s ease;
  }
  .nav-item:hover {
    color: var(--text-1);
    background: var(--bg-2);
  }
  .nav-item.active {
    color: var(--text-0);
    background: var(--bg-2);
  }
  .nav-item.active::before {
    content: '';
    position: absolute;
    left: -12px;
    top: 50%;
    transform: translateY(-50%);
    width: 3px;
    height: 22px;
    border-radius: 0 3px 3px 0;
    background: rgba(255, 255, 255, 0.7);
    box-shadow: 0 0 12px rgba(255, 255, 255, 0.25);
  }
  .games-tab {
    margin-top: auto;
  }
  .settings-tab {
    margin-top: 6px;
  }

  .main {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }

  .topbar {
    height: var(--topbar-h);
    flex-shrink: 0;
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 0 18px;
    background: #171717;
    border-bottom: 1px solid var(--line);
  }

  .capture-target {
    position: relative;
    align-self: stretch;
    display: flex;
  }
  .capturing {
    position: relative;
    align-self: stretch;
    display: flex;
    align-items: center;
    min-width: 240px;
    padding: 0 16px;
    overflow: hidden;
    text-align: left;
    border-radius: var(--r-sm);
  }
  .cap-icon {
    position: relative;
    z-index: 2;
    display: grid;
    place-items: center;
    width: 34px;
    height: 34px;
    margin-right: 11px;
    border-radius: var(--r-sm);
    color: var(--accent);
    background: var(--bg-3);
    flex-shrink: 0;
  }
  .capturing.rec .cap-icon {
    background: color-mix(in srgb, var(--rec) 16%, var(--bg-3));
  }
  .rec-dot {
    width: 11px;
    height: 11px;
    border-radius: 999px;
    background: var(--rec);
    box-shadow: 0 0 0 0 var(--rec-glow);
    animation: rec-pulse 1.4s ease-out infinite;
  }
  @keyframes rec-pulse {
    0% {
      box-shadow: 0 0 0 0 var(--rec-glow);
    }
    70% {
      box-shadow: 0 0 0 7px transparent;
    }
    100% {
      box-shadow: 0 0 0 0 transparent;
    }
  }
  .cap-frame {
    position: absolute;
    inset: 0;
    z-index: 0;
    background-size: cover;
    background-position: center;
    filter: blur(1px);
    -webkit-mask-image: linear-gradient(to right, transparent 0%, #000 14%, #000 72%, transparent 100%);
    mask-image: linear-gradient(to right, transparent 0%, #000 14%, #000 72%, transparent 100%);
  }
  .capturing::after {
    content: '';
    position: absolute;
    inset: 0;
    z-index: 1;
    background: linear-gradient(90deg, #171717 0%, rgba(23, 23, 23, 0.55) 42%, rgba(23, 23, 23, 0) 72%);
  }
  .cap-text {
    position: relative;
    z-index: 2;
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
  }
  .cap-label {
    font-size: 12px;
    line-height: 1;
    color: var(--text-2);
  }
  .cap-proc {
    display: flex;
    align-items: center;
    gap: 5px;
    font-size: 16px;
    font-weight: 600;
    line-height: 1.2;
    color: var(--text-0);
  }
  .capturing.idle .cap-proc {
    color: var(--text-2);
    font-weight: 500;
  }

  .cap-menu {
    position: absolute;
    top: calc(100% + 4px);
    left: 0;
    right: auto;
    width: 420px;
    display: flex;
    flex-direction: column;
    gap: 1px;
    padding: 8px;
    background: var(--bg-1);
    border: 1px solid var(--line-strong);
    border-radius: var(--r-md);
    box-shadow: 0 18px 42px -14px rgba(0, 0, 0, 0.7);
    z-index: 50;
  }
  .cap-opt {
    display: flex;
    align-items: center;
    gap: 11px;
    padding: 9px 10px;
    border-radius: 7px;
    color: var(--text-1);
    text-align: left;
    transition: background 0.12s ease, color 0.12s ease;
  }
  .cap-opt:hover {
    background: var(--bg-3);
    color: var(--text-0);
  }
  .cap-opt.on {
    color: var(--text-0);
  }
  .opt-ico {
    display: grid;
    place-items: center;
    width: 24px;
    color: var(--text-3);
    flex-shrink: 0;
  }
  .cap-opt.on .opt-ico {
    color: var(--bright);
  }
  .screen-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 6px;
    padding: 2px;
  }
  .screen-card {
    display: flex;
    padding: 3px;
    border-radius: 8px;
  }
  .screen-thumb {
    position: relative;
    display: grid;
    place-items: center;
    width: 100%;
    aspect-ratio: 16 / 9;
    border-radius: 6px;
    background-color: var(--bg-3);
    background-size: cover;
    background-position: center;
    color: var(--text-3);
    border: 2px solid var(--line);
    overflow: hidden;
    transition: border-color 0.12s ease;
  }
  .screen-card:hover .screen-thumb,
  .screen-card.on .screen-thumb {
    border-color: var(--bright);
  }
  .screen-check {
    position: absolute;
    top: 6px;
    right: 6px;
    display: grid;
    place-items: center;
    width: 23px;
    height: 23px;
    border-radius: 999px;
    color: var(--bg-1);
    background: var(--bright);
    box-shadow: 0 2px 7px rgba(0, 0, 0, 0.35);
  }
  .opt-text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .opt-title {
    font-size: 13px;
    line-height: 1.1;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .opt-sub {
    font-size: 11px;
    line-height: 1;
    color: var(--text-3);
  }
  .opt-check {
    display: grid;
    place-items: center;
    color: var(--bright);
    flex-shrink: 0;
  }
  .cap-sep {
    height: 1px;
    margin: 5px 6px;
    background: var(--line);
  }
  .cap-group {
    padding: 2px 10px 4px;
    font-family: var(--font-mono);
    font-size: 11px;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    text-align: center;
    color: var(--text-1);
  }

  .mic-label {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    font-size: 13px;
    line-height: 1.15;
  }
  .help {
    position: relative;
    display: inline-grid;
    place-items: center;
    width: 15px;
    height: 15px;
    margin-left: 7px;
    border-radius: 999px;
    font-size: 10px;
    font-weight: 700;
    color: var(--text-2);
    background: var(--bg-3);
    cursor: help;
    flex-shrink: 0;
  }
  .help:hover {
    color: var(--text-0);
    background: var(--accent-deep);
  }
  .help-tip {
    position: absolute;
    top: calc(100% + 7px);
    left: 50%;
    transform: translateX(-50%);
    width: 214px;
    padding: 8px 10px;
    font-size: 11.5px;
    font-weight: 400;
    line-height: 1.35;
    color: var(--text-1);
    background: var(--bg-0);
    border: 1px solid var(--line-strong);
    border-radius: 8px;
    box-shadow: 0 12px 30px -10px rgba(0, 0, 0, 0.7);
    opacity: 0;
    visibility: hidden;
    pointer-events: none;
    z-index: 60;
  }
  .help:hover .help-tip {
    opacity: 1;
    visibility: visible;
  }

  .mic-input {
    margin-top: 2px;
    padding: 0 8px 2px;
  }
  .mic-dd {
    position: relative;
  }
  .mic-trigger {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    width: 100%;
    height: 30px;
    padding: 6px;
    font-size: 11px;
    color: var(--text-0);
    background: var(--bg-0);
    border: 1px solid var(--line);
    border-radius: 4px;
    cursor: pointer;
    text-align: left;
    transition: border-color 0.14s ease;
  }
  .mic-trigger:hover,
  .mic-dd.open .mic-trigger {
    border-color: var(--line-strong);
  }
  .mic-value {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .mic-chev {
    display: inline-flex;
    color: var(--text-3);
    flex-shrink: 0;
    transition: transform 0.15s ease;
  }
  .mic-dd.open .mic-chev {
    transform: rotate(180deg);
  }
  .mic-list {
    position: absolute;
    top: calc(100% + 4px);
    left: 0;
    right: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
    padding: 5px;
    background: var(--bg-1);
    border: 1px solid var(--line-strong);
    border-radius: 8px;
    box-shadow: 0 18px 42px -14px rgba(0, 0, 0, 0.7);
    z-index: 70;
  }
  .mic-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    padding: 8px 9px;
    font-size: 11px;
    border-radius: 6px;
    color: var(--text-1);
    text-align: left;
    white-space: nowrap;
    transition: background 0.12s ease, color 0.12s ease;
  }
  .mic-item:hover {
    background: var(--bg-3);
    color: var(--text-0);
  }
  .mic-item.on {
    color: var(--bright);
  }
  .mic-item .mic-check {
    opacity: 0;
    flex-shrink: 0;
    color: var(--bright);
  }
  .mic-item.on .mic-check {
    opacity: 1;
  }

  .mic-switch {
    flex-shrink: 0;
    width: 36px;
    height: 20px;
    border-radius: 999px;
    background: var(--bg-3);
    border: 1px solid var(--line);
    padding: 2px;
    transition: background 0.18s ease, border-color 0.18s ease;
  }
  .mic-knob {
    display: block;
    width: 14px;
    height: 14px;
    border-radius: 999px;
    background: var(--text-2);
    transition: transform 0.18s ease, background 0.18s ease;
  }
  .mic-opt.on .mic-switch {
    background: var(--bright);
    border-color: transparent;
  }
  .mic-opt.on .mic-knob {
    transform: translateX(16px);
    background: var(--bg-1);
  }

  .quick {
    margin-left: auto;
    display: flex;
    align-items: center;
    gap: 7px;
    flex-shrink: 0;
  }
  .pill {
    flex-shrink: 0;
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 6px 10px;
    font-size: 11.5px;
    color: var(--text-1);
    background: var(--bg-2);
    border: 1px solid var(--line);
    border-radius: var(--r-sm);
    white-space: nowrap;
  }
  .pill.combo {
    gap: 0;
    padding: 0;
    font-size: 13.5px;
  }
  .pill.combo .seg {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 9px 14px;
    background: none;
    border: 0;
    color: var(--text-1);
    font: inherit;
    white-space: nowrap;
  }
  .pill.combo button.seg {
    cursor: pointer;
    transition: color 0.15s ease;
  }
  .pill.combo button.seg:hover,
  .segwrap.open button.seg {
    color: var(--text-0);
  }
  .pill.combo .sep {
    color: var(--line-strong);
    padding: 0;
    user-select: none;
  }
  .pill.combo .seg .chev {
    display: inline-flex;
    transition: transform 0.15s ease;
  }
  .segwrap.open .seg .chev {
    transform: rotate(180deg);
  }
  .segwrap {
    position: relative;
    display: inline-flex;
  }
  .seg-menu {
    position: absolute;
    top: calc(100% + 6px);
    left: 50%;
    transform: translateX(-50%);
    min-width: 132px;
    display: flex;
    flex-direction: column;
    gap: 1px;
    padding: 6px;
    background: var(--bg-1);
    border: 1px solid var(--line-strong);
    border-radius: var(--r-md);
    box-shadow: 0 18px 42px -14px rgba(0, 0, 0, 0.7);
    z-index: 60;
  }
  .seg-opt {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 14px;
    padding: 7px 10px;
    border-radius: 7px;
    color: var(--text-1);
    text-align: left;
    white-space: nowrap;
    transition: background 0.12s ease, color 0.12s ease;
  }
  .seg-opt:hover {
    background: var(--bg-3);
    color: var(--text-0);
  }
  .seg-opt.on {
    color: var(--bright);
  }
  .seg-opt .seg-check {
    opacity: 0;
  }
  .seg-opt.on .seg-check {
    opacity: 1;
  }

  .recpill {
    background: var(--bg-0);
    transition: background 0.16s ease, border-color 0.16s ease;
  }
  .recpill .rec-seg {
    color: var(--text-0);
  }
  .recpill .rec-ico {
    display: inline-flex;
    align-items: center;
  }
  .recpill.on {
    background: rgba(229, 72, 77, 0.16);
    border-color: rgba(229, 72, 77, 0.55);
  }
  .recpill.on .rec-seg {
    color: #ff6166;
  }
  .recpill.on .sep {
    color: rgba(229, 72, 77, 0.45);
  }
  .recpill .rec-hotkey {
    font-size: 11px;
  }

  .winctl {
    align-self: stretch;
    display: flex;
    margin-right: -18px;
  }

  .content {
    flex: 1;
    overflow-y: auto;
    overflow-x: hidden;
    border-left: 1px solid var(--line);
  }

  .notice {
    position: fixed;
    bottom: 18px;
    left: 50%;
    transform: translateX(-50%);
    max-width: 70%;
    padding: 9px 14px;
    border: 1px solid color-mix(in srgb, var(--accent) 55%, var(--line));
    border-radius: 9px;
    background: var(--bg-2);
    color: var(--text-1);
    font-size: 12px;
    line-height: 1.35;
    text-align: left;
    box-shadow: 0 8px 24px rgb(0 0 0 / 0.35);
    cursor: pointer;
    z-index: 50;
    white-space: pre-wrap;
    word-break: break-word;
  }
  .notice:hover {
    border-color: var(--accent);
  }
</style>
