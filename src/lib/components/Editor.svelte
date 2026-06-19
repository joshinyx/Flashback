<script lang="ts">
  import Icon from './Icon.svelte';
  import {
    editorState,
    closeEditor,
    resetTrim,
    exportClip,
    markEdited,
    cutSegmentAt,
    removeSegment,
    selectSegment,
    trimSegment,
    reorderSegment,
    persistEdit,
  } from '$lib/editor.svelte';
  import { formatSize } from '$lib/clips';
  import { refreshLibrary } from '$lib/library.svelte';

  let video = $state<HTMLVideoElement | null>(null);
  let sysAudio = $state<HTMLAudioElement | null>(null);
  let micAudio = $state<HTMLAudioElement | null>(null);

  let playing = $state(false);
  let outPos = $state(0); // posición en el tiempo de salida (ms)
  let playIndex = 0; // segmento que se está reproduciendo
  let raf = 0;

  let notice = $state<string | null>(null);

  let laneEl = $state<HTMLDivElement | null>(null);
  let bodyEl = $state<HTMLDivElement | null>(null);
  let laneW = $state(0);
  type Drag =
    | { kind: 'seek' }
    | { kind: 'trim'; index: number; edge: 'start' | 'end'; downX: number; origStart: number; origEnd: number }
    | { kind: 'move'; index: number; downX: number; moved: boolean };
  let drag: Drag | null = null;
  let scrubbing = $state(false);

  let sysCanvas = $state<HTMLCanvasElement | null>(null);
  let micCanvas = $state<HTMLCanvasElement | null>(null);
  let sysPeaks = $state<number[] | null>(null);
  let micPeaks = $state<number[] | null>(null);
  let analyzing = $state(false);

  const SYS_HUE = '#6f93f9';
  const SYS_DIM = 'rgba(111, 147, 249, 0.16)';
  const MIC_HUE = '#f4c95d';
  const MIC_DIM = 'rgba(244, 201, 93, 0.16)';

  let audioCtx: AudioContext | null = null;

  const FRAME_MS = $derived(1000 / (editorState.fps || 30));
  const hasSeparate = $derived(!!(editorState.system || editorState.mic));

  // Duración total de salida = suma de las duraciones de cada bloque (sin huecos).
  const kept = $derived(editorState.segments.reduce((a, s) => a + (s.endMs - s.startMs), 0));
  const frac = $derived(kept > 0 ? outPos / kept : 0);

  const segView = $derived.by(() => {
    if (kept <= 0) return [] as { left: number; width: number; startMs: number; endMs: number }[];
    let acc = 0;
    const out = [];
    for (const s of editorState.segments) {
      const d = s.endMs - s.startMs;
      out.push({ left: (acc / kept) * 100, width: (d / kept) * 100, startMs: s.startMs, endMs: s.endMs });
      acc += d;
    }
    return out;
  });

  const activeInfo = $derived.by(() => {
    const s = editorState.segments[editorState.activeSegment];
    if (!s) return null;
    return {
      n: editorState.activeSegment + 1,
      total: editorState.segments.length,
      start: s.startMs,
      end: s.endMs,
      dur: s.endMs - s.startMs,
    };
  });

  const ticks = $derived.by(() => {
    if (kept <= 0) return [] as { frac: number; label: string }[];
    const count = 6;
    return Array.from({ length: count + 1 }, (_, i) => ({
      frac: i / count,
      label: fmtClock((kept * i) / count / 1000),
    }));
  });

  const two = (n: number) => String(Math.floor(n)).padStart(2, '0');
  function fmtTime(sec: number): string {
    if (!isFinite(sec) || sec < 0) sec = 0;
    return `${two(sec / 60)}:${two(sec % 60)}.${two((sec * 100) % 100)}`;
  }
  function fmtClock(sec: number): string {
    if (!isFinite(sec) || sec < 0) sec = 0;
    return `${Math.floor(sec / 60)}:${two(sec % 60)}`;
  }

  // ---- mapeo salida <-> origen ----
  function cumStartOf(i: number): number {
    const segs = editorState.segments;
    let a = 0;
    for (let k = 0; k < i && k < segs.length; k++) a += segs[k].endMs - segs[k].startMs;
    return a;
  }
  function outToSeg(T: number): { index: number; srcMs: number } {
    const segs = editorState.segments;
    let a = 0;
    for (let i = 0; i < segs.length; i++) {
      const d = segs[i].endMs - segs[i].startMs;
      if (T < a + d || i === segs.length - 1) {
        return { index: i, srcMs: segs[i].startMs + Math.max(0, Math.min(T - a, d)) };
      }
      a += d;
    }
    return { index: 0, srcMs: segs[0]?.startMs ?? 0 };
  }
  function outFromClientX(clientX: number): number {
    if (!laneEl || kept <= 0) return 0;
    const r = laneEl.getBoundingClientRect();
    const f = Math.max(0, Math.min(1, (clientX - r.left) / r.width));
    return f * kept;
  }

  // ---- audio sync ----
  function applyAudio() {
    if (video) video.muted = hasSeparate;
    if (sysAudio) sysAudio.volume = editorState.mixer.sys_muted ? 0 : editorState.mixer.sys_vol;
    if (micAudio) micAudio.volume = editorState.mixer.mic_muted ? 0 : editorState.mixer.mic_vol;
  }

  $effect(() => {
    void [
      editorState.mixer.sys_vol,
      editorState.mixer.mic_vol,
      editorState.mixer.sys_muted,
      editorState.mixer.mic_muted,
      editorState.system,
      editorState.mic,
      video,
      sysAudio,
      micAudio,
    ];
    applyAudio();
  });

  $effect(() => () => cancelAnimationFrame(raf));

  function seekSource(srcMs: number) {
    if (!video) return;
    const t = srcMs / 1000;
    video.currentTime = t;
    for (const a of [sysAudio, micAudio]) {
      if (a && Math.abs(a.currentTime - t) > 0.05) a.currentTime = t;
    }
  }

  function seekOutput(T: number) {
    T = Math.max(0, Math.min(T, kept));
    const { index, srcMs } = outToSeg(T);
    playIndex = index;
    outPos = T;
    seekSource(srcMs);
  }

  function tick() {
    if (!video || !playing) return;
    const segs = editorState.segments;
    const seg = segs[playIndex];
    if (!seg) {
      pause();
      return;
    }
    const srcMs = video.currentTime * 1000;
    if (srcMs >= seg.endMs - 1) {
      if (playIndex < segs.length - 1) {
        playIndex++;
        seekSource(segs[playIndex].startMs);
        outPos = cumStartOf(playIndex);
      } else {
        pause();
        outPos = kept;
        return;
      }
    } else {
      outPos = cumStartOf(playIndex) + Math.max(0, srcMs - seg.startMs);
      for (const a of [sysAudio, micAudio]) {
        if (a && Math.abs(a.currentTime - video.currentTime) > 0.12) a.currentTime = video.currentTime;
      }
    }
    raf = requestAnimationFrame(tick);
  }

  async function play() {
    if (!video || kept <= 0) return;
    seekOutput(outPos >= kept ? 0 : outPos);
    try {
      await video.play();
    } catch (e) {
      console.error('editor play', e);
      return;
    }
    sysAudio?.play().catch(() => {});
    micAudio?.play().catch(() => {});
    playing = true;
    cancelAnimationFrame(raf);
    raf = requestAnimationFrame(tick);
  }

  function pause() {
    video?.pause();
    sysAudio?.pause();
    micAudio?.pause();
    playing = false;
    cancelAnimationFrame(raf);
  }

  function toggle() {
    if (playing) pause();
    else play();
  }

  function onLoaded() {
    const dMs = (video?.duration || 0) * 1000;
    editorState.durationMs = dMs;
    if (editorState.segments.length === 0) {
      editorState.segments = [{ startMs: 0, endMs: dMs }];
      editorState.activeSegment = 0;
    }
    playIndex = 0;
    outPos = 0;
    seekSource(editorState.segments[0]?.startMs ?? 0);
    applyAudio();
  }

  function stepFrame(dir: number) {
    seekOutput(outPos + dir * FRAME_MS);
  }

  function cut() {
    const { index, srcMs } = outToSeg(outPos);
    cutSegmentAt(index, srcMs);
  }

  function removeActive() {
    removeSegment(editorState.activeSegment);
    seekOutput(Math.min(outPos, kept));
  }

  // ---- timeline interacción ----
  function onLaneDown(e: MouseEvent) {
    if (e.button !== 0) return;
    drag = { kind: 'seek' };
    seekOutput(outFromClientX(e.clientX));
  }

  function onBlockDown(e: MouseEvent, i: number) {
    if (e.button !== 0) return;
    e.stopPropagation();
    selectSegment(i);
    drag = { kind: 'move', index: i, downX: e.clientX, moved: false };
  }

  function onGripDown(e: MouseEvent, i: number, edge: 'start' | 'end') {
    if (e.button !== 0) return;
    e.stopPropagation();
    e.preventDefault();
    selectSegment(i);
    const s = editorState.segments[i];
    drag = { kind: 'trim', index: i, edge, downX: e.clientX, origStart: s.startMs, origEnd: s.endMs };
  }

  function onKnobDown(e: MouseEvent) {
    if (e.button !== 0) return;
    e.stopPropagation();
    e.preventDefault();
    scrubbing = true;
    drag = { kind: 'seek' };
  }

  function onWinMove(e: MouseEvent) {
    if (!drag) return;
    if (drag.kind === 'seek') {
      seekOutput(outFromClientX(e.clientX));
    } else if (drag.kind === 'move') {
      if (Math.abs(e.clientX - drag.downX) > 3) drag.moved = true;
      const tgt = outToSeg(outFromClientX(e.clientX)).index;
      if (tgt !== drag.index) {
        reorderSegment(drag.index, tgt);
        drag.index = tgt;
      }
    } else if (drag.kind === 'trim') {
      const r = laneEl?.getBoundingClientRect();
      if (!r) return;
      const dms = ((e.clientX - drag.downX) * kept) / r.width;
      const val = drag.edge === 'start' ? drag.origStart + dms : drag.origEnd + dms;
      trimSegment(drag.index, drag.edge, val);
      const seg = editorState.segments[drag.index];
      if (seg) {
        if (drag.edge === 'start') {
          seekSource(seg.startMs);
          outPos = cumStartOf(drag.index);
        } else {
          seekSource(seg.endMs);
          outPos = cumStartOf(drag.index) + (seg.endMs - seg.startMs);
        }
      }
    }
  }

  function onWinUp() {
    if (drag && drag.kind === 'move' && !drag.moved) seekOutput(outFromClientX(drag.downX));
    drag = null;
    scrubbing = false;
  }

  function onKey(e: KeyboardEvent) {
    if ((e.target as HTMLElement)?.tagName === 'INPUT') return;
    if (e.key === 'Escape') close();
    else if (e.key === 'c' || e.key === 'C') {
      e.preventDefault();
      cut();
    } else if (e.key === ' ' || e.key === 'k') {
      e.preventDefault();
      toggle();
    } else if (e.key === 'Delete' || e.key === 'Backspace') {
      e.preventDefault();
      removeActive();
    } else if (e.key === 'ArrowLeft') {
      e.preventDefault();
      stepFrame(-1);
    } else if (e.key === 'ArrowRight') {
      e.preventDefault();
      stepFrame(1);
    }
  }

  async function handleExport() {
    try {
      const dst = await exportClip();
      if (dst) {
        await refreshLibrary();
        setNotice(`Exportado: ${dst.split(/[/\\]/).pop()}`, 4000);
      }
    } catch (e) {
      setNotice(`Error al exportar: ${e}`, 6000);
      console.error('export', e);
    }
  }

  let noticeTimer: ReturnType<typeof setTimeout> | null = null;
  function setNotice(msg: string, ms: number) {
    notice = msg;
    if (noticeTimer) clearTimeout(noticeTimer);
    noticeTimer = setTimeout(() => (notice = null), ms);
  }

  async function close() {
    pause();
    await persistEdit();
    closeEditor();
  }

  // ---- forma de onda (en orden de salida) ----
  async function computePeaks(url: string, buckets: number): Promise<number[] | null> {
    try {
      const resp = await fetch(url);
      const arr = await resp.arrayBuffer();
      audioCtx ??= new AudioContext();
      const buf = await audioCtx.decodeAudioData(arr);
      const chs: Float32Array[] = [];
      for (let c = 0; c < buf.numberOfChannels; c++) chs.push(buf.getChannelData(c));
      const n = buf.length;
      const size = Math.max(1, Math.floor(n / buckets));
      const peaks = new Array(buckets).fill(0);
      for (let b = 0; b < buckets; b++) {
        const start = b * size;
        const end = Math.min(n, start + size);
        let peak = 0;
        for (let i = start; i < end; i++) {
          for (const ch of chs) {
            const v = Math.abs(ch[i]);
            if (v > peak) peak = v;
          }
        }
        peaks[b] = peak;
      }
      return peaks;
    } catch (e) {
      console.error('waveform', e);
      return null;
    }
  }

  $effect(() => {
    const sys = editorState.system;
    const mic = editorState.mic;
    sysPeaks = null;
    micPeaks = null;
    if (!sys && !mic) return;
    analyzing = true;
    (async () => {
      if (sys) sysPeaks = await computePeaks(sys, 1600);
      if (mic) micPeaks = await computePeaks(mic, 1600);
      analyzing = false;
    })();
  });

  $effect(() => {
    const el = bodyEl;
    if (!el) return;
    const ro = new ResizeObserver((entries) => {
      laneW = entries[0].contentRect.width;
    });
    ro.observe(el);
    return () => ro.disconnect();
  });

  function renderWave(
    canvas: HTMLCanvasElement | null,
    peaks: number[] | null,
    hue: string,
    dim: string,
    muted: boolean
  ) {
    if (!canvas) return;
    const w = canvas.clientWidth;
    const h = canvas.clientHeight;
    if (w === 0 || h === 0) return;
    const dpr = window.devicePixelRatio || 1;
    canvas.width = Math.round(w * dpr);
    canvas.height = Math.round(h * dpr);
    const ctx = canvas.getContext('2d');
    if (!ctx) return;
    ctx.scale(dpr, dpr);
    ctx.clearRect(0, 0, w, h);
    const dur = editorState.durationMs;
    if (!peaks || kept <= 0 || dur <= 0) return;

    const mid = h / 2;
    const amp = h * 0.44;
    ctx.fillStyle = muted ? dim : hue;
    ctx.beginPath();
    let acc = 0;
    for (const s of editorState.segments) {
      const segdur = s.endMs - s.startMs;
      const x0 = (acc / kept) * w;
      const x1 = ((acc + segdur) / kept) * w;
      for (let x = Math.floor(x0); x < x1; x++) {
        const f = (x - x0) / Math.max(1e-6, x1 - x0);
        const srcMs = s.startMs + f * segdur;
        const b = Math.min(peaks.length - 1, Math.max(0, Math.floor((srcMs / dur) * peaks.length)));
        const y = Math.max(0.5, peaks[b] * amp);
        ctx.rect(x, mid - y, 1, y * 2);
      }
      acc += segdur;
    }
    ctx.fill();

    // separadores entre bloques
    if (editorState.segments.length > 1) {
      ctx.fillStyle = 'rgba(0, 0, 0, 0.6)';
      let a = 0;
      for (let i = 0; i < editorState.segments.length - 1; i++) {
        a += editorState.segments[i].endMs - editorState.segments[i].startMs;
        ctx.fillRect((a / kept) * w - 0.5, 0, 1, h);
      }
    }
  }

  $effect(() => {
    void [
      editorState.segments,
      editorState.durationMs,
      sysPeaks,
      micPeaks,
      laneW,
      editorState.mixer.sys_muted,
      editorState.mixer.mic_muted,
    ];
    renderWave(sysCanvas, sysPeaks, SYS_HUE, SYS_DIM, editorState.mixer.sys_muted);
    renderWave(micCanvas, micPeaks, MIC_HUE, MIC_DIM, editorState.mixer.mic_muted);
  });

  // ---- sub-bar helpers ----
  function fmtDate(d?: Date): string {
    if (!d) return '';
    try {
      return d.toLocaleDateString('es-ES', { day: 'numeric', month: 'short', year: 'numeric' });
    } catch {
      return '';
    }
  }
  function shortPath(p: string): string {
    if (!p) return '';
    const parts = p.split(/[/\\]/);
    if (parts.length <= 3) return p;
    return `${parts[0]}\\...\\${parts[parts.length - 2]}\\${parts[parts.length - 1]}`;
  }
</script>

<svelte:window onkeydown={onKey} onmousemove={onWinMove} onmouseup={onWinUp} />

<div class="overlay">
  <div class="sub-bar mono">
    <div class="sub-left">
      <div class="sub-chevrons">
        <button class="sub-btn" aria-label="Clip anterior">
          <svg width="256" height="256" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m15 18-6-6 6-6"/></svg>
        </button>
        <button class="sub-btn" aria-label="Clip siguiente">
          <svg width="256" height="256" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m9 18 6-6-6-6"/></svg>
        </button>
      </div>
      <span class="sub-sep">|</span>
      <span class="sub-label">Creado:</span>
      <span class="sub-info">{fmtDate(editorState.clip?.createdAt)}</span>
      <span class="sub-sep">|</span>
      <span class="sub-label">Tamaño:</span>
      <span class="sub-info">{formatSize(editorState.clip?.sizeBytes ?? 0)}</span>
      <span class="sub-sep">|</span>
      <span class="sub-label">Ruta:</span>
      <span class="sub-path">{shortPath(editorState.clip?.path ?? '')}</span>
    </div>
    <button class="sub-close" aria-label="Cerrar editor" onclick={close}>✕</button>
  </div>

  <div class="stage">
    {#if editorState.videoSrc}
      <video
        bind:this={video}
        src={editorState.videoSrc}
        playsinline
        onloadedmetadata={onLoaded}
        onended={pause}
        onclick={toggle}
      ><track kind="captions" /></video>
    {/if}
    {#if editorState.system}
      <audio bind:this={sysAudio} src={editorState.system} preload="auto"></audio>
    {/if}
    {#if editorState.mic}
      <audio bind:this={micAudio} src={editorState.mic} preload="auto"></audio>
    {/if}

    {#if editorState.loading}
      <div class="prep mono">Preparando pistas de audio…</div>
    {:else if editorState.error}
      <div class="prep mono err">No se pudieron separar las pistas: {editorState.error}</div>
    {/if}
  </div>

  <div class="dock">
    <div class="transport">
      <div class="tp-left">
        <button class="tp-btn" aria-label="Ir al inicio" onclick={() => seekOutput(0)}>
          <Icon name="skip-back" size={16} />
        </button>
        <button class="tp-btn" aria-label="Fotograma anterior" onclick={() => stepFrame(-1)}>
          <Icon name="step-back" size={16} />
        </button>
        <button class="tp-play" aria-label={playing ? 'Pausar' : 'Reproducir'} onclick={toggle}>
          <Icon name={playing ? 'stop' : 'play'} size={19} />
        </button>
        <button class="tp-btn" aria-label="Fotograma siguiente" onclick={() => stepFrame(1)}>
          <Icon name="step-fwd" size={16} />
        </button>
        <button class="tp-btn" aria-label="Ir al final" onclick={() => seekOutput(kept)}>
          <Icon name="skip-fwd" size={16} />
        </button>
        <span class="tp-time mono">
          <span class="t-cur">{fmtTime(outPos / 1000)}</span>
          <span class="t-sep">/</span>
          <span class="t-dur">{fmtTime(kept / 1000)}</span>
        </span>
      </div>

      <div class="tp-mid">
        {#if activeInfo}
          <span class="seg-read mono">
            <span class="seg-tag">Bloque {activeInfo.n}/{activeInfo.total}</span>
            <span class="seg-range">orig {fmtClock(activeInfo.start / 1000)}–{fmtClock(activeInfo.end / 1000)}</span>
            <span class="seg-dur">({fmtTime(activeInfo.dur / 1000)})</span>
          </span>
        {/if}
      </div>

      <div class="tp-right">
        <button class="act" onclick={cut}>
          <Icon name="scissors" size={14} /> Cortar <kbd>C</kbd>
        </button>
        <button class="act" disabled={editorState.segments.length <= 1} onclick={removeActive}>
          <Icon name="trash" size={14} /> Quitar
        </button>
        <button class="act" onclick={resetTrim}>Reestablecer</button>
        <button class="act export" onclick={handleExport} disabled={editorState.exporting}>
          {editorState.exporting ? 'Exportando…' : 'Exportar'}
        </button>
      </div>
    </div>

    {#if kept > 0}
      <div class="tl" style="--gutter: 176px;">
        <div class="tl-ruler">
          {#each ticks as t, i}
            <span
              class="tick mono"
              style="left: calc((100% - var(--gutter)) * {t.frac} + var(--gutter)); transform: translateX({i === 0 ? '0' : i === ticks.length - 1 ? '-100%' : '-50%'});"
            >{t.label}</span>
          {/each}
        </div>

        <div class="tl-body" bind:this={bodyEl}>
          <div class="row">
            <div class="gutter">
              <span class="g-title">Vídeo</span>
              <span class="g-meta mono">
                {editorState.segments.length} bloque{editorState.segments.length > 1 ? 's' : ''} · {fmtClock(kept / 1000)}
              </span>
            </div>
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div class="lane vlane" bind:this={laneEl} onmousedown={onLaneDown}>
              {#each segView as s, i (i)}
                <!-- svelte-ignore a11y_no_static_element_interactions -->
                <div
                  class="block"
                  class:active={i === editorState.activeSegment}
                  style="left: {s.left}%; width: {s.width}%;"
                  onmousedown={(e) => onBlockDown(e, i)}
                >
                  <span class="block-n mono">{i + 1}</span>
                  {#if i === editorState.activeSegment}
                    <!-- svelte-ignore a11y_no_static_element_interactions -->
                    <span class="grip start" onmousedown={(e) => onGripDown(e, i, 'start')}></span>
                    <!-- svelte-ignore a11y_no_static_element_interactions -->
                    <span class="grip end" onmousedown={(e) => onGripDown(e, i, 'end')}></span>
                  {/if}
                </div>
              {/each}
            </div>
          </div>

          {#if editorState.system}
            <div class="row">
              <div class="gutter audio">
                <div class="g-head">
                  <span class="dot" style="background: {SYS_HUE};"></span>
                  <span class="g-title">Sistema</span>
                </div>
                <div class="vol">
                  <button
                    class="mute"
                    class:on={editorState.mixer.sys_muted}
                    aria-label="Silenciar sistema"
                    onclick={() => { editorState.mixer.sys_muted = !editorState.mixer.sys_muted; markEdited(); }}
                  ><Icon name="speaker" size={14} /></button>
                  <input
                    class="slider"
                    type="range" min="0" max="1" step="0.01"
                    value={editorState.mixer.sys_vol}
                    oninput={(e) => { editorState.mixer.sys_vol = +(e.target as HTMLInputElement).value; markEdited(); }}
                  />
                </div>
              </div>
              <div class="lane alane" class:muted={editorState.mixer.sys_muted}>
                <canvas bind:this={sysCanvas}></canvas>
                {#if analyzing && !sysPeaks}<span class="wf-note mono">Analizando…</span>{/if}
              </div>
            </div>
          {/if}

          {#if editorState.mic}
            <div class="row">
              <div class="gutter audio">
                <div class="g-head">
                  <span class="dot" style="background: {MIC_HUE};"></span>
                  <span class="g-title">Micrófono</span>
                </div>
                <div class="vol">
                  <button
                    class="mute"
                    class:on={editorState.mixer.mic_muted}
                    aria-label="Silenciar micrófono"
                    onclick={() => { editorState.mixer.mic_muted = !editorState.mixer.mic_muted; markEdited(); }}
                  ><Icon name="speaker" size={14} /></button>
                  <input
                    class="slider"
                    type="range" min="0" max="1" step="0.01"
                    value={editorState.mixer.mic_vol}
                    oninput={(e) => { editorState.mixer.mic_vol = +(e.target as HTMLInputElement).value; markEdited(); }}
                  />
                </div>
              </div>
              <div class="lane alane" class:muted={editorState.mixer.mic_muted}>
                <canvas bind:this={micCanvas}></canvas>
                {#if analyzing && !micPeaks}<span class="wf-note mono">Analizando…</span>{/if}
              </div>
            </div>
          {/if}

          {#if !hasSeparate && !editorState.loading && !editorState.error}
            <div class="row">
              <div class="gutter audio"><span class="g-title">Audio</span></div>
              <div class="lane single mono">Este clip tiene una sola pista de audio.</div>
            </div>
          {/if}

          <div
            class="playhead"
            class:scrub={scrubbing}
            style="left: calc((100% - var(--gutter)) * {frac} + var(--gutter));"
          >
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <span class="ph-knob" onmousedown={onKnobDown}></span>
          </div>
        </div>
      </div>
    {/if}
  </div>

  {#if notice}
    <div class="notice mono">{notice}</div>
  {/if}
</div>

<style>
  .overlay {
    position: fixed;
    left: 0;
    right: 0;
    top: var(--topbar-h, 60px);
    bottom: 0;
    z-index: 100;
    display: flex;
    flex-direction: column;
    background: var(--bg-0);
  }

  /* ===== sub-bar (sin tocar) ===== */
  .sub-bar {
    height: 35px;
    flex-shrink: 0;
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 14px;
    border-bottom: 1px solid var(--line);
    background: var(--bg-1);
    font-size: 12px;
    color: var(--text-3);
  }
  .sub-left { display: flex; align-items: center; gap: 10px; min-width: 0; }
  .sub-chevrons { display: flex; align-items: center; gap: 5px; }
  .sub-btn {
    width: 26px; height: 26px;
    display: grid; place-items: center;
    color: var(--text-1); border-radius: 4px;
    transition: color 0.14s ease, background 0.14s ease;
  }
  .sub-btn svg { width: 18px; height: 18px; }
  .sub-btn:hover { color: var(--text-0); background: var(--bg-2); }
  .sub-sep { color: var(--line); font-size: 13px; }
  .sub-label { color: var(--text-3); white-space: nowrap; }
  .sub-info { color: var(--text-1); white-space: nowrap; }
  .sub-path {
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
    max-width: 260px; color: var(--text-2);
  }
  .sub-close {
    width: 28px; height: 28px;
    display: grid; place-items: center;
    font-size: 15px; color: var(--text-2); border-radius: 4px; flex-shrink: 0;
    transition: color 0.14s ease, background 0.14s ease;
  }
  .sub-close:hover { color: var(--text-0); background: var(--bg-2); }

  /* ===== stage ===== */
  .stage {
    position: relative;
    flex: 1;
    min-height: 0;
    display: grid;
    place-items: center;
    background: radial-gradient(120% 90% at 50% 0%, #121214 0%, #0a0a0b 70%, #060607 100%);
    overflow: hidden;
    padding: 22px 40px;
  }
  .stage video {
    max-width: 100%;
    max-height: 100%;
    display: block;
    cursor: pointer;
    border-radius: var(--r-md);
    box-shadow: 0 24px 60px -28px rgba(0, 0, 0, 0.9);
    object-fit: contain;
  }
  .prep {
    position: absolute;
    bottom: 16px;
    left: 50%;
    transform: translateX(-50%);
    padding: 7px 12px;
    font-size: 12px;
    color: var(--text-1);
    background: var(--bg-2);
    border: 1px solid var(--line);
    border-radius: 8px;
  }
  .prep.err {
    color: var(--rec);
    border-color: color-mix(in srgb, var(--rec) 50%, var(--line));
  }

  /* ===== dock ===== */
  .dock {
    flex-shrink: 0;
    border-top: 1px solid var(--line);
    background: var(--bg-1);
    padding: 10px 16px 16px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .transport {
    display: grid;
    grid-template-columns: 1fr auto 1fr;
    align-items: center;
    gap: 12px;
    min-height: 34px;
  }
  .tp-left { display: flex; align-items: center; gap: 4px; }
  .tp-mid { display: flex; justify-content: center; min-width: 0; }
  .tp-right { display: flex; align-items: center; justify-content: flex-end; gap: 6px; }

  .tp-btn {
    width: 30px; height: 30px;
    display: grid; place-items: center;
    color: var(--text-2); border-radius: 7px;
    transition: color 0.14s ease, background 0.14s ease;
  }
  .tp-btn:hover { color: var(--text-0); background: var(--bg-3); }
  .tp-play {
    width: 36px; height: 36px;
    display: grid; place-items: center;
    color: var(--bg-0); background: var(--bright);
    border-radius: 999px; margin: 0 4px; flex-shrink: 0;
    transition: transform 0.12s ease, opacity 0.12s ease;
  }
  .tp-play:hover { opacity: 0.88; }
  .tp-play:active { transform: scale(0.94); }

  .tp-time { margin-left: 10px; font-size: 12.5px; letter-spacing: 0.02em; white-space: nowrap; }
  .tp-time .t-cur { color: var(--text-0); }
  .tp-time .t-sep { color: var(--text-3); margin: 0 4px; }
  .tp-time .t-dur { color: var(--text-3); }

  .seg-read {
    display: inline-flex;
    align-items: center;
    gap: 9px;
    padding: 5px 11px;
    font-size: 11.5px;
    border-radius: 999px;
    background: var(--bg-2);
    border: 1px solid var(--line);
    white-space: nowrap;
    overflow: hidden;
    max-width: 100%;
  }
  .seg-tag { color: var(--accent-soft); font-weight: 600; }
  .seg-range { color: var(--text-1); }
  .seg-dur { color: var(--text-3); }

  .act {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 6px 11px;
    font-size: 12px;
    color: var(--text-1);
    background: var(--bg-2);
    border: 1px solid var(--line);
    border-radius: var(--r-sm);
    white-space: nowrap;
    transition: background 0.14s ease, color 0.14s ease, border-color 0.14s ease;
  }
  .act:hover { background: var(--bg-hover); color: var(--text-0); }
  .act:disabled { opacity: 0.4; pointer-events: none; }
  .act kbd {
    font-family: var(--font-mono);
    font-size: 9.5px;
    padding: 1px 4px;
    color: var(--text-3);
    background: var(--bg-0);
    border: 1px solid var(--line);
    border-radius: 4px;
  }
  .act.export {
    color: var(--bg-0);
    background: var(--bright);
    border-color: transparent;
    font-weight: 600;
  }
  .act.export:hover { opacity: 0.9; background: var(--bright); color: var(--bg-0); }

  /* ===== timeline ===== */
  .tl { display: flex; flex-direction: column; gap: 6px; }
  .tl-ruler { position: relative; height: 14px; }
  .tick { position: absolute; top: 0; font-size: 10px; color: var(--text-3); white-space: nowrap; }

  .tl-body { position: relative; display: flex; flex-direction: column; gap: 7px; }
  .row { display: grid; grid-template-columns: var(--gutter) 1fr; align-items: stretch; }

  .gutter {
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: 5px;
    padding: 0 14px 0 2px;
    min-width: 0;
  }
  .g-title { font-size: 12px; font-weight: 600; color: var(--text-1); white-space: nowrap; }
  .g-meta { font-size: 10px; color: var(--text-3); white-space: nowrap; }
  .g-head { display: flex; align-items: center; gap: 7px; }
  .dot { width: 8px; height: 8px; border-radius: 999px; flex-shrink: 0; }

  .vol { display: flex; align-items: center; gap: 8px; }
  .mute {
    width: 24px; height: 24px;
    display: grid; place-items: center;
    color: var(--text-1); border-radius: 5px; flex-shrink: 0;
    transition: color 0.14s ease, background 0.14s ease;
  }
  .mute:hover { color: var(--text-0); background: var(--bg-hover); }
  .mute.on { color: var(--rec); }
  .slider {
    -webkit-appearance: none;
    appearance: none;
    width: 100%;
    height: 4px;
    border-radius: 3px;
    background: var(--bg-3);
    cursor: pointer;
  }
  .slider::-webkit-slider-thumb {
    -webkit-appearance: none;
    appearance: none;
    width: 13px;
    height: 13px;
    border-radius: 999px;
    background: var(--bright);
    border: none;
  }

  .lane {
    position: relative;
    border-radius: var(--r-sm);
    overflow: hidden;
    border: 1px solid var(--line);
    background: var(--bg-0);
  }
  .vlane { height: 46px; cursor: pointer; }
  .alane { height: 50px; transition: opacity 0.16s ease; }
  .alane.muted { opacity: 0.5; }
  .alane canvas { display: block; width: 100%; height: 100%; }
  .wf-note {
    position: absolute;
    top: 50%;
    left: 12px;
    transform: translateY(-50%);
    font-size: 10.5px;
    color: var(--text-3);
  }

  .block {
    position: absolute;
    top: 3px;
    bottom: 3px;
    border-radius: 5px;
    background: linear-gradient(
      180deg,
      color-mix(in srgb, var(--accent) 28%, transparent),
      color-mix(in srgb, var(--accent) 17%, transparent)
    );
    border: 1px solid color-mix(in srgb, var(--accent) 40%, var(--line));
    cursor: grab;
    overflow: hidden;
    transition: border-color 0.12s ease, background 0.12s ease;
  }
  .block:hover { border-color: color-mix(in srgb, var(--accent) 62%, var(--line)); }
  .block.active {
    background: linear-gradient(
      180deg,
      color-mix(in srgb, var(--accent) 44%, transparent),
      color-mix(in srgb, var(--accent) 27%, transparent)
    );
    border-color: var(--accent-soft);
    box-shadow: 0 0 0 1px var(--accent-soft), 0 4px 16px -6px var(--accent-glow);
  }
  .block-n {
    position: absolute;
    top: 3px;
    left: 6px;
    font-size: 10px;
    font-weight: 600;
    color: rgba(255, 255, 255, 0.7);
    pointer-events: none;
  }
  .grip {
    position: absolute;
    top: -1px;
    bottom: -1px;
    width: 9px;
    background: var(--accent-soft);
    cursor: ew-resize;
    z-index: 3;
  }
  .grip::after {
    content: '';
    position: absolute;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    width: 2px;
    height: 14px;
    border-radius: 2px;
    background: rgba(255, 255, 255, 0.85);
  }
  .grip.start { left: -1px; border-radius: 5px 0 0 5px; }
  .grip.end { right: -1px; border-radius: 0 5px 5px 0; }

  .single {
    display: flex;
    align-items: center;
    padding: 0 14px;
    height: 50px;
    font-size: 12px;
    color: var(--text-3);
    cursor: default;
  }

  .playhead {
    position: absolute;
    top: -7px;
    bottom: 0;
    width: 2px;
    background: var(--bright);
    z-index: 6;
    pointer-events: none;
    transform: translateX(-1px);
  }
  .ph-knob {
    position: absolute;
    top: -5px;
    left: 50%;
    transform: translateX(-50%);
    width: 11px;
    height: 11px;
    border-radius: 999px;
    background: var(--bright);
    box-shadow: 0 0 8px rgba(240, 242, 247, 0.45);
    pointer-events: auto;
    cursor: grab;
    transition: transform 0.12s ease, box-shadow 0.12s ease;
  }
  .ph-knob::before {
    content: '';
    position: absolute;
    inset: -8px;
    border-radius: 999px;
  }
  .ph-knob:hover {
    transform: translateX(-50%) scale(1.65);
    box-shadow: 0 0 12px rgba(240, 242, 247, 0.7);
  }
  .playhead.scrub .ph-knob {
    transform: translateX(-50%) scale(1.65);
    cursor: grabbing;
    box-shadow: 0 0 14px rgba(240, 242, 247, 0.85);
  }

  .notice {
    position: absolute;
    bottom: 96px;
    left: 50%;
    transform: translateX(-50%);
    padding: 8px 14px;
    font-size: 11.5px;
    color: var(--text-1);
    background: var(--bg-2);
    border: 1px solid var(--line-strong);
    border-radius: 8px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.35);
    z-index: 110;
    max-width: 70%;
    text-align: center;
    pointer-events: none;
  }
</style>
