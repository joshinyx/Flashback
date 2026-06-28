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
    sortSegmentsByPos,
    persistEdit,
    captureFrame,
    navigateClip,
    clipOrder,
    type Segment,
  } from '$lib/editor.svelte';
  import { formatSize } from '$lib/clips';
  import { refreshLibrary } from '$lib/library.svelte';
  import { revealItemInDir } from '@tauri-apps/plugin-opener';
  import { convertFileSrc } from '@tauri-apps/api/core';

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
    | { kind: 'vol'; chan: 'sys' | 'mic'; rect: DOMRect };
  let drag: Drag | null = null;
  let scrubbing = $state(false);
  // Activo mientras se recorta un borde: desactiva la transición de `left` de los bloques para que,
  // al arrastrar el inicio, el borde izquierdo siga al cursor al instante (la animación lo hacía
  // flotar). El final solo cambia `width`, que no tiene transición, por eso ya iba fino.
  let trimming = $state(false);
  // Canal cuyo fader se arrastra (para mostrar su burbuja de % mientras dura el gesto). Alto del
  // pulgar del fader: el cursor controla su centro, así que se descuenta del recorrido útil.
  let volActive = $state<'sys' | 'mic' | null>(null);
  const VOL_THUMB = 20;

  // Burbuja de % flotante (position: fixed a nivel del overlay) para que el overflow de la timeline
  // no la recorte. Se posiciona sobre el pulgar del fader activo (al pasar el ratón o arrastrar).
  let bubble = $state<{ x: number; y: number; pct: number } | null>(null);

  // Arrastrar-para-reordenar: tras mantener pulsado un bloque LIFT_MS, se "levanta" (escala) y
  // sigue al cursor; al acercar su borde a menos de SNAP_MS de un hueco entre bloques, se cuela
  // ahí (imán). Si el puntero se mueve antes de levantarse, el gesto se trata como scrub.
  const LIFT_MS = 300;
  const LIFT_SCALE = 1.05;
  const SNAP_MS = 200;
  const LIFT_CANCEL_PX = 6;
  let liftTimer: ReturnType<typeof setTimeout> | null = null;
  let liftPending: { downX: number } | null = null;
  // index = bloque arrastrado; grabMs = desfase (ms de la base) entre su borde izquierdo y donde
  // se agarró. Mientras se arrastra se mueve su `posMs` directamente.
  let lift = $state<{ index: number; grabMs: number } | null>(null);

  // Zoom de la timeline (Ctrl + rueda). 1 = el clip completo cabe; al ampliar aparece scroll
  // horizontal para precisión fina. No es infinito.
  const ZOOM_MIN = 0.5;
  const ZOOM_MAX = 40;
  let zoom = $state(1);

  // Menú contextual (clic derecho) sobre un bloque: desactivar/activar o eliminar.
  let blockMenu = $state<{ x: number; y: number; index: number } | null>(null);

  let overlayEl = $state<HTMLDivElement | null>(null);
  let dockEl = $state<HTMLDivElement | null>(null);
  // Alto del dock en px cuando el usuario lo ajusta con el tirador; null = alto natural.
  // El stage (vídeo) ocupa el resto del alto, así que encoger el dock agranda el vídeo.
  let dockH = $state<number | null>(null);
  let dockDrag: { startY: number; startH: number } | null = null;

  let sysCanvas = $state<HTMLCanvasElement | null>(null);
  let micCanvas = $state<HTMLCanvasElement | null>(null);
  let mixCanvas = $state<HTMLCanvasElement | null>(null);

  const SYS_HUE = '#6f93f9';
  const SYS_DIM = 'rgba(111, 147, 249, 0.16)';
  const MIC_HUE = '#f4c95d';
  const MIC_DIM = 'rgba(244, 201, 93, 0.16)';
  const MIX_HUE = '#8b93a7';
  const MIX_DIM = 'rgba(139, 147, 167, 0.16)';

  const FRAME_MS = $derived(1000 / (editorState.fps || 30));
  const hasSeparate = $derived(!!(editorState.system || editorState.mic));

  // Navegación entre clips (anterior/siguiente) por el orden visible de la rejilla.
  const navIdx = $derived(clipOrder.list.findIndex((c) => c.id === editorState.clip?.id));
  const hasPrev = $derived(navIdx > 0);
  const hasNext = $derived(navIdx >= 0 && navIdx < clipOrder.list.length - 1);
  // Alto de la card del mezclador: fijo (el de micrófono + sistema) aunque solo haya sistema, para
  // que no encoja. 53 = lane de vídeo (46 + gap 7); 107 = dos lanes de audio (50 + 50 + gap 7);
  // +40 = 20px extra por arriba y 20 por abajo (la elevación sube otros 20 en el CSS).
  const mixerH = 53 + 40 + 107;

  // La base de la timeline es estática: su ancho (a escala fija) representa la duración original
  // del clip. Cada sección se dibuja en su posición libre `posMs`; el espacio sin sección queda
  // en negro. La exportación une las secciones sin huecos.
  const total = $derived(editorState.durationMs);
  // Duración de salida = suma de las duraciones conservadas (lo que dura el clip exportado).
  const kept = $derived(editorState.segments.reduce((a, s) => a + (s.disabled ? 0 : s.endMs - s.startMs), 0));
  // Posición del cursor sobre la base = posición en el editor (posMs) del tramo que se muestra.
  const frac = $derived.by(() => {
    if (total <= 0) return 0;
    const { index, srcMs } = outToSeg(outPos);
    const seg = editorState.segments[index];
    if (!seg) return 0;
    return (seg.posMs + (srcMs - seg.startMs)) / total;
  });

  const segView = $derived.by(() => {
    if (total <= 0) return [] as { seg: Segment; index: number; left: number; width: number }[];
    const z = Math.min(1, zoom);
    return editorState.segments.map((seg, i) => ({
      seg,
      index: i,
      left: (seg.posMs / total) * z * 100,
      width: ((seg.endMs - seg.startMs) / total) * z * 100,
    }));
  });

  const two = (n: number) => String(Math.floor(n)).padStart(2, '0');
  function fmtTime(sec: number): string {
    if (!isFinite(sec) || sec < 0) sec = 0;
    return `${two(sec / 60)}:${two(sec % 60)}.${two((sec * 100) % 100)}`;
  }
  // ---- mapeo salida <-> origen ----
  function cumStartOf(i: number): number {
    const segs = editorState.segments;
    let a = 0;
    for (let k = 0; k < i && k < segs.length; k++) {
      if (!segs[k].disabled) a += segs[k].endMs - segs[k].startMs;
    }
    return a;
  }
  // Mapea tiempo de salida -> (índice de bloque, ms de origen) recorriendo solo bloques activos;
  // los desactivados se saltan como si no existieran.
  function outToSeg(T: number): { index: number; srcMs: number } {
    const segs = editorState.segments;
    let a = 0;
    let last = -1;
    for (let i = 0; i < segs.length; i++) {
      if (segs[i].disabled) continue;
      last = i;
      const d = segs[i].endMs - segs[i].startMs;
      if (T < a + d) {
        return { index: i, srcMs: segs[i].startMs + Math.max(0, Math.min(T - a, d)) };
      }
      a += d;
    }
    if (last >= 0) return { index: last, srcMs: segs[last].endMs };
    return { index: 0, srcMs: segs[0]?.startMs ?? 0 };
  }
  // Traduce una posición horizontal sobre la base (posición de editor, posMs) a tiempo de salida.
  // Si cae en un hueco negro, engancha al borde de sección más cercano.
  function outFromClientX(clientX: number): number {
    const segs = editorState.segments;
    if (!laneEl || total <= 0 || segs.length === 0) return 0;
    const r = laneEl.getBoundingClientRect();
    const f = Math.max(0, Math.min(1, (clientX - r.left) / r.width));
    const editorMs = f * total / Math.min(1, zoom || 1);
    let cum = 0;
    let bestT = 0;
    let bestDist = Infinity;
    for (const s of segs) {
      if (s.disabled) continue;
      const d = s.endMs - s.startMs;
      if (editorMs >= s.posMs && editorMs <= s.posMs + d) return cum + (editorMs - s.posMs);
      const dStart = Math.abs(editorMs - s.posMs);
      if (dStart < bestDist) { bestDist = dStart; bestT = cum; }
      const dEnd = Math.abs(editorMs - (s.posMs + d));
      if (dEnd < bestDist) { bestDist = dEnd; bestT = cum + d; }
      cum += d;
    }
    return bestT;
  }

  // ---- audio sync ----
  function applyAudio() {
    if (video) {
      if (hasSeparate) {
        // Las pistas separadas suenan por los <audio>; el vídeo va en silencio.
        video.muted = true;
      } else {
        // Pista única embebida: el fader de headphones controla el audio del propio vídeo.
        video.muted = editorState.mixer.sys_muted;
        video.volume = editorState.mixer.sys_vol;
      }
    }
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

  $effect(() => () => {
    cancelAnimationFrame(raf);
    clearLiftTimer();
  });

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

  // Margen para considerar dos bloques "pegados" en el origen. Un corte que no quita nada deja
  // next.startMs == seg.endMs: en ese caso NO se hace seek, porque fijar video.currentTime vacía
  // el decoder y provoca el microcorte. Solo se salta cuando hay un hueco real entre bloques.
  const SEAM_MS = 12;

  function tick() {
    if (!video || !playing) return;
    const segs = editorState.segments;
    const seg = segs[playIndex];
    if (!seg) {
      pause();
      return;
    }
    const srcMs = video.currentTime * 1000;
    // drift correction for audio elements
    for (const a of [sysAudio, micAudio]) {
      if (a && Math.abs(a.currentTime - video.currentTime) > 0.12) a.currentTime = video.currentTime;
    }
    if (srcMs >= seg.endMs - 1) {
      let ni = playIndex + 1;
      while (ni < segs.length && segs[ni].disabled) ni++;
      if (ni < segs.length) {
        const next = segs[ni];
        playIndex = ni;
        if (Math.abs(next.startMs - seg.endMs) > SEAM_MS) {
          seekSource(next.startMs);
        }
        outPos = cumStartOf(playIndex);
      } else {
        pause();
        outPos = kept;
        return;
      }
    } else {
      outPos = cumStartOf(playIndex) + Math.max(0, srcMs - seg.startMs);
    }
    raf = requestAnimationFrame(tick);
  }

  async function play() {
    if (!video || kept <= 0) return;
    seekOutput(outPos >= kept ? 0 : outPos);
    try {
      await video.play();
      sysAudio?.play().catch(() => {});
      micAudio?.play().catch(() => {});
    } catch (e) {
      console.error('editor play', e);
      return;
    }

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
      editorState.segments = [{ startMs: 0, endMs: dMs, posMs: 0, boundStartMs: 0, boundEndMs: dMs, disabled: false }];
      editorState.activeSegment = 0;
    }
    playIndex = 0;
    outPos = 0;
    seekSource(editorState.segments[0]?.startMs ?? 0);
    applyAudio();
  }

  // Índice del fotograma mostrado en `srcMs`: el último cuyo PTS <= srcMs (búsqueda binaria).
  function frameIndexAt(ft: number[], srcMs: number): number {
    let lo = 0, hi = ft.length - 1, ans = -1;
    while (lo <= hi) {
      const mid = (lo + hi) >> 1;
      if (ft[mid] <= srcMs + 1e-3) { ans = mid; lo = mid + 1; }
      else hi = mid - 1;
    }
    return ans;
  }

  // Avanza/retrocede exactamente un fotograma. El clip es de framerate variable (captura WGC), así
  // que se usan los timestamps reales (editorState.frameTimes): se salta al frame contiguo de la
  // tabla y se cae un poco DENTRO de él (no en su borde) para que el decoder muestre justo ese y el
  // paso sea constante 1·1·1. Sin tabla, fallback al paso aproximado por FRAME_MS.
  function stepFrame(dir: number) {
    const ft = editorState.frameTimes;
    const { index, srcMs } = outToSeg(outPos);
    const seg = editorState.segments[index];
    if (!seg) return;
    if (ft.length < 2) {
      seekOutput(outPos + dir * FRAME_MS);
      return;
    }
    const k = Math.max(0, frameIndexAt(ft, srcMs));
    const tk = Math.max(0, Math.min(ft.length - 1, k + dir));
    const next = tk + 1 < ft.length ? ft[tk + 1] : ft[tk] + FRAME_MS;
    const target = ft[tk] + Math.min(1.5, (next - ft[tk]) * 0.25);
    if (target >= seg.startMs && target < seg.endMs) {
      seekOutput(cumStartOf(index) + (target - seg.startMs));
    } else {
      // Cruza el borde del segmento: el mapeo global lleva al frame contiguo del montaje.
      seekOutput(outPos + dir * FRAME_MS);
    }
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
    drag = { kind: 'seek' };
    seekOutput(outFromClientX(e.clientX));

    // Mantener pulsado sin mover LIFT_MS levanta el bloque para reordenar. Solo tiene sentido
    // si hay más de un bloque; con uno solo, el gesto se queda en simple scrub.
    clearLiftTimer();
    lift = null;
    if (editorState.segments.length > 1) {
      const downX = e.clientX;
      liftPending = { downX };
      liftTimer = setTimeout(() => {
        liftTimer = null;
        const seg = editorState.segments[i];
        const r = laneEl?.getBoundingClientRect();
        if (!seg || !r || total <= 0) return;
        const pointerMs = ((downX - r.left) / r.width) * total / Math.min(1, zoom || 1);
        lift = { index: i, grabMs: pointerMs - seg.posMs };
        drag = null;
      }, LIFT_MS);
    }
  }

  function clearLiftTimer() {
    if (liftTimer) {
      clearTimeout(liftTimer);
      liftTimer = null;
    }
  }

  function onBlockContext(e: MouseEvent, i: number) {
    e.preventDefault();
    e.stopPropagation();
    clearLiftTimer();
    liftPending = null;
    lift = null;
    drag = null;
    selectSegment(i);
    const mw = 170;
    const mh = 84;
    const x = Math.min(e.clientX, window.innerWidth - mw - 8);
    const y = Math.min(e.clientY, window.innerHeight - mh - 8);
    blockMenu = { x, y, index: i };
  }

  function closeBlockMenu() {
    blockMenu = null;
  }

  function toggleDisableBlock() {
    const i = blockMenu?.index;
    closeBlockMenu();
    if (i == null) return;
    const seg = editorState.segments[i];
    if (!seg) return;
    // Desactivar el bloque que se reproduce dejaría el playhead dentro de un tramo ya ignorado;
    // se pausa y se reengancha al tiempo de salida válido más cercano.
    pause();
    seg.disabled = !seg.disabled;
    markEdited();
    seekOutput(Math.min(outPos, kept));
  }

  function deleteBlock() {
    const i = blockMenu?.index;
    closeBlockMenu();
    if (i == null) return;
    removeSegment(i);
    seekOutput(Math.min(outPos, kept));
  }

  // Mueve la sección levantada siguiendo al cursor dentro del espacio negro libre (sin solaparse
  // con otras). Si un borde queda a menos de SNAP_MS de un hueco vecino (o del inicio/fin), se
  // pega ahí (imán). Lo que quede sin sección es negro; al exportar desaparece.
  function updateLift(clientX: number) {
    if (!lift || !laneEl || total <= 0) return;
    const segs = editorState.segments;
    const cur = segs[lift.index];
    if (!cur) return;
    const dur = cur.endMs - cur.startMs;
    const r = laneEl.getBoundingClientRect();
    const pointerMs = Math.max(0, ((clientX - r.left) / r.width) * total / Math.min(1, zoom || 1));
    const desired = pointerMs - lift.grabMs;

    // Huecos libres entre las demás secciones, dentro de [0, total].
    const occ = segs
      .filter((_, idx) => idx !== lift!.index)
      .map((s) => ({ a: s.posMs, b: s.posMs + (s.endMs - s.startMs) }))
      .sort((x, y) => x.a - y.a);
    const gaps: [number, number][] = [];
    let cursor = 0;
    for (const o of occ) {
      if (o.a - cursor > 0.5) gaps.push([cursor, o.a]);
      cursor = Math.max(cursor, o.b);
    }
    // espacio a la derecha para soltar bloques más allá del video
    const rightExtent = total / Math.min(1, zoom || 1);
    gaps.push([cursor, rightExtent]);

    // Hueco que admite la sección y queda más cerca del punto deseado.
    let best: { pos: number; lo: number; hi: number } | null = null;
    let bestDist = Infinity;
    for (const [gs, ge] of gaps) {
      if (ge - gs < dur - 0.5) continue;
      const lo = gs;
      const hi = ge - dur;
      const pos = Math.max(lo, Math.min(desired, hi));
      const dist = Math.abs(pos - desired);
      if (dist < bestDist) { bestDist = dist; best = { pos, lo, hi }; }
    }
    if (!best) return;

    let pos = best.pos;
    if (Math.abs(pos - best.lo) <= SNAP_MS) pos = best.lo;
    else if (Math.abs(pos - best.hi) <= SNAP_MS) pos = best.hi;
    cur.posMs = pos;
  }

  function onGripDown(e: MouseEvent, i: number, edge: 'start' | 'end') {
    if (e.button !== 0) return;
    e.stopPropagation();
    e.preventDefault();
    selectSegment(i);
    const s = editorState.segments[i];
    drag = { kind: 'trim', index: i, edge, downX: e.clientX, origStart: s.startMs, origEnd: s.endMs };
    trimming = true;
  }

  function onKnobDown(e: MouseEvent) {
    if (e.button !== 0) return;
    e.stopPropagation();
    e.preventDefault();
    scrubbing = true;
    drag = { kind: 'seek' };
  }

  // ---- faders de volumen ----
  // Sitúa la burbuja (en coordenadas de viewport, para position: fixed) centrada sobre el pulgar.
  function placeBubble(rect: DOMRect, v: number) {
    const thumbCenterY = rect.bottom - (v * (rect.height - VOL_THUMB) + VOL_THUMB / 2);
    bubble = {
      x: rect.left + rect.width / 2,
      y: thumbCenterY - VOL_THUMB / 2 - 6,
      pct: Math.round(v * 100),
    };
  }

  function volAt(rect: DOMRect, clientY: number): number {
    const usable = Math.max(1, rect.height - VOL_THUMB);
    const fromBottom = rect.height - (clientY - rect.top);
    return Math.max(0, Math.min(1, (fromBottom - VOL_THUMB / 2) / usable));
  }

  function setVol(chan: 'sys' | 'mic', clientY: number, rect: DOMRect) {
    const v = volAt(rect, clientY);
    if (chan === 'sys') editorState.mixer.sys_vol = v;
    else editorState.mixer.mic_vol = v;
    markEdited();
    placeBubble(rect, v);
  }

  function onFaderDown(e: MouseEvent, chan: 'sys' | 'mic') {
    if (e.button !== 0) return;
    e.preventDefault();
    e.stopPropagation();
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    drag = { kind: 'vol', chan, rect };
    volActive = chan;
    setVol(chan, e.clientY, rect);
  }

  // Al pasar el ratón sin arrastrar, muestra la burbuja en la posición actual del fader.
  function onFaderHover(e: MouseEvent, chan: 'sys' | 'mic') {
    if (volActive) return;
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    placeBubble(rect, chan === 'sys' ? editorState.mixer.sys_vol : editorState.mixer.mic_vol);
  }

  function onFaderLeave() {
    if (!volActive) bubble = null;
  }

  function onFaderKey(e: KeyboardEvent, chan: 'sys' | 'mic') {
    const cur = chan === 'sys' ? editorState.mixer.sys_vol : editorState.mixer.mic_vol;
    let v = cur;
    if (e.key === 'ArrowUp' || e.key === 'ArrowRight') v = Math.min(1, cur + 0.05);
    else if (e.key === 'ArrowDown' || e.key === 'ArrowLeft') v = Math.max(0, cur - 0.05);
    else if (e.key === 'Home') v = 0;
    else if (e.key === 'End') v = 1;
    else return;
    e.preventDefault();
    e.stopPropagation();
    if (chan === 'sys') editorState.mixer.sys_vol = v;
    else editorState.mixer.mic_vol = v;
    markEdited();
  }

  function toggleMute(chan: 'sys' | 'mic') {
    if (chan === 'sys') editorState.mixer.sys_muted = !editorState.mixer.sys_muted;
    else editorState.mixer.mic_muted = !editorState.mixer.mic_muted;
    markEdited();
  }

  function onDockGripDown(e: MouseEvent) {
    if (e.button !== 0) return;
    e.preventDefault();
    dockDrag = { startY: e.clientY, startH: dockEl?.getBoundingClientRect().height ?? 0 };
  }

  // Ctrl + rueda amplía/reduce la timeline para edición fina al milisegundo; con scroll horizontal
  // cuando el montaje no cabe. Sin Ctrl, la rueda hace su scroll normal.
  function onTimelineWheel(e: WheelEvent) {
    if (!e.ctrlKey) return;
    e.preventDefault();
    const factor = e.deltaY < 0 ? 1.15 : 1 / 1.15;
    zoom = Math.max(ZOOM_MIN, Math.min(ZOOM_MAX, zoom * factor));
  }

  function onWinMove(e: MouseEvent) {
    if (dockDrag) {
      // Arrastrar hacia arriba agranda el dock (vídeo más pequeño) y al revés. Se deja un
      // mínimo para el dock y un mínimo de stage para que el vídeo nunca desaparezca.
      const avail = overlayEl?.clientHeight ?? window.innerHeight;
      const max = Math.max(160, avail - 35 - 200);
      const next = dockDrag.startH + (dockDrag.startY - e.clientY);
      dockH = Math.round(Math.max(150, Math.min(next, max)));
      return;
    }
    if (lift) {
      updateLift(e.clientX);
      return;
    }
    // Si el puntero se mueve antes de que el bloque se levante, el gesto es un scrub normal.
    if (liftPending && Math.abs(e.clientX - liftPending.downX) > LIFT_CANCEL_PX) {
      clearLiftTimer();
      liftPending = null;
    }
    if (!drag) return;
    if (drag.kind === 'seek') {
      seekOutput(outFromClientX(e.clientX));
    } else if (drag.kind === 'vol') {
      setVol(drag.chan, e.clientY, drag.rect);
    } else if (drag.kind === 'trim') {
      const r = laneEl?.getBoundingClientRect();
      if (!r) return;
      const dms = ((e.clientX - drag.downX) * total) / r.width;
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
    if (dockDrag) {
      dockDrag = null;
      return;
    }
    clearLiftTimer();
    liftPending = null;
    if (lift) {
      // Al soltar, reordena el array por posición (izq→der = orden de salida) y reselecciona la
      // sección movida por su posMs (único al no solaparse); refresca el fotograma.
      const droppedPos = editorState.segments[lift.index]?.posMs ?? 0;
      lift = null;
      sortSegmentsByPos();
      const idx = editorState.segments.findIndex((s) => s.posMs === droppedPos);
      if (idx >= 0) selectSegment(idx);
      seekOutput(Math.min(outPos, kept));
    }
    drag = null;
    scrubbing = false;
    volActive = null;
    trimming = false;
    bubble = null;
  }

  function toggleFullscreen() {
    if (!video) return;
    if (document.fullscreenElement) {
      document.exitFullscreen().catch(() => {});
    } else {
      // Pantalla completa sobre el propio <video>: usa el modo de medios del navegador, que
      // escala según la proporción real del clip y llena el monitor sin depender del tamaño
      // de la ventana (poner el contenedor en fullscreen dejaba franjas al estar maximizada).
      video.requestFullscreen().catch(() => {});
    }
  }

  function onKey(e: KeyboardEvent) {
    if (blockMenu) {
      if (e.key === 'Escape') e.preventDefault();
      closeBlockMenu();
      return;
    }
    if ((e.target as HTMLElement)?.tagName === 'INPUT') return;
    if (e.key === 'Escape') {
      if (document.fullscreenElement) return;
      close();
    } else if (e.key === 'f' || e.key === 'F') {
      e.preventDefault();
      toggleFullscreen();
    } else if (e.key === 'c' || e.key === 'C') {
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

  async function screenshot() {
    if (!video) return;
    try {
      const dst = await captureFrame(video.currentTime * 1000);
      if (!dst) return;
      const name = dst.split(/[/\\]/).pop();
      // Copia automática al portapapeles leyendo el PNG ya guardado (sin canvas, que se "ensucia"
      // con el protocolo asset). Si falla, la captura igual queda guardada en disco.
      let copied = false;
      try {
        const blob = await (await fetch(convertFileSrc(dst))).blob();
        await navigator.clipboard.write([new ClipboardItem({ 'image/png': blob })]);
        copied = true;
      } catch (err) {
        console.error('clipboard', err);
      }
      setNotice(copied ? `Captura copiada y guardada: ${name}` : `Captura guardada: ${name}`, 4000);
    } catch (e) {
      setNotice(`Error al capturar: ${e}`, 5000);
      console.error('capture', e);
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
  // Los picos llegan ya calculados desde el backend (editorState.*Peaks): aquí solo se dibujan.

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
    if (!peaks || dur <= 0) return;

    const mid = h / 2;
    const amp = h * 0.44;
    const z = Math.min(1, zoom);

    const paint = (segs: Segment[], color: string) => {
      ctx.fillStyle = color;
      ctx.beginPath();
      for (const s of segs) {
        const segdur = s.endMs - s.startMs;
        const x0 = (s.posMs / dur) * z * w;
        const x1 = ((s.posMs + segdur) / dur) * z * w;
        for (let x = Math.floor(x0); x < x1; x++) {
          const f = (x - x0) / Math.max(1e-6, x1 - x0);
          const srcMs = s.startMs + f * segdur;
          const b = Math.min(peaks.length - 1, Math.max(0, Math.floor((srcMs / dur) * peaks.length)));
          const y = Math.max(0.5, Math.min(1, peaks[b] * 1.8) * amp);
          ctx.rect(x, mid - y, 1, y * 2);
        }
      }
      ctx.fill();
    };
    // Los bloques desactivados se pintan siempre atenuados (color dim), como en la timeline.
    paint(editorState.segments.filter((s) => !s.disabled), muted ? dim : hue);
    paint(editorState.segments.filter((s) => s.disabled), dim);
  }

  $effect(() => {
    void [
      editorState.segments,
      editorState.durationMs,
      editorState.sysPeaks,
      editorState.micPeaks,
      editorState.mixPeaks,
      sysCanvas,
      micCanvas,
      mixCanvas,
      laneW,
      editorState.mixer.sys_muted,
      editorState.mixer.mic_muted,
    ];
    renderWave(sysCanvas, editorState.sysPeaks, SYS_HUE, SYS_DIM, editorState.mixer.sys_muted);
    renderWave(micCanvas, editorState.micPeaks, MIC_HUE, MIC_DIM, editorState.mixer.mic_muted);
    renderWave(mixCanvas, editorState.mixPeaks, MIX_HUE, MIX_DIM, false);
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
  async function openLocation() {
    const p = editorState.clip?.path;
    if (!p) return;
    try {
      await revealItemInDir(p);
    } catch (err) {
      console.error('revealItemInDir', err);
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

<div class="overlay" bind:this={overlayEl}>
  <div class="sub-bar mono">
    <div class="sub-left">
      <div class="sub-chevrons">
        <button class="sub-btn" aria-label="Clip anterior" disabled={!hasPrev || editorState.exporting} onclick={() => navigateClip(-1)}>
          <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024" fill="currentColor"><path d="M 896 511.62C 896 531.17 882.83 547.9 864.24 552.88Q 859.03 554.27 847.09 554.27Q 331.65 554.25 274.29 554.25A 0.28 0.27 67.3 0 0 274.1 554.72Q 291.29 571.91 373.18 653.82C 375.44 656.08 378.29 660.47 379.72 663.6Q 388.28 682.4 379.81 700.58C 369.12 723.55 340.54 731.79 318.84 718.64Q 314.43 715.97 304.94 706.35Q 296.52 697.84 140.32 541.69C 132.48 533.85 128 522.66 128 511.61C 128 500.57 132.48 489.38 140.32 481.54Q 296.52 325.39 304.94 316.88Q 314.43 307.26 318.84 304.59C 340.54 291.44 369.12 299.68 379.81 322.65Q 388.28 340.83 379.72 359.63C 378.29 362.76 375.44 367.15 373.18 369.41Q 291.29 451.32 274.1 468.51A 0.28 0.27 -67.3 0 0 274.29 468.98Q 331.65 468.98 847.09 468.96Q 859.03 468.96 864.24 470.35C 882.83 475.33 896 492.06 896 511.62Z"/></svg>
        </button>
        <button class="sub-btn" aria-label="Clip siguiente" disabled={!hasNext || editorState.exporting} onclick={() => navigateClip(1)}>
          <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024" fill="currentColor"><path d="M 896 511.62C 896 520.05 893.43 528.39 888.82 535.4Q 886.22 539.36 879.43 545.94Q 878.49 546.85 719.95 705.45Q 708.63 716.76 704.04 719.33Q 694.05 724.92 682.46 724.88C 649.56 724.77 629.55 689.32 645.73 660.83Q 648.57 655.83 656.42 648.13Q 665.07 639.64 749.89 554.71A 0.26 0.26 22.15 0 0 749.7 554.27Q 188.43 554.23 177.34 554.3Q 165.13 554.38 159.82 552.91C 141.08 547.71 128 531.28 128 511.61C 128 491.95 141.08 475.52 159.82 470.32Q 165.13 468.85 177.34 468.93Q 188.43 469 749.7 468.96A 0.26 0.26 -22.15 0 0 749.89 468.52Q 665.07 383.59 656.42 375.1Q 648.57 367.4 645.73 362.4C 629.55 333.91 649.56 298.46 682.46 298.35Q 694.05 298.31 704.04 303.9Q 708.63 306.47 719.95 317.78Q 878.49 476.38 879.43 477.29Q 886.22 483.87 888.82 487.83C 893.43 494.84 896 503.18 896 511.62Z"/></svg>
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
      <button
        class="sub-path"
        onclick={openLocation}
        title={editorState.clip?.path ?? ''}
        disabled={!editorState.clip?.path}
      >{shortPath(editorState.clip?.path ?? '')}</button>
    </div>
    <div class="sub-right">
      <button class="sub-reset" onclick={resetTrim}>Reestablecer</button>
      <button class="sub-close" aria-label="Cerrar editor" onclick={close}>✕</button>
    </div>
  </div>

  <div class="stage">
    {#if editorState.videoSrc}
      <div class="video-fit">
        <video
          bind:this={video}
          src={editorState.videoSrc}
          playsinline
          onloadedmetadata={onLoaded}
          onended={pause}
          onclick={toggle}
        ><track kind="captions" /></video>
      </div>
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

  <div class="dock" bind:this={dockEl} style:height={dockH !== null ? `${dockH}px` : null}>
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="dock-grip" onmousedown={onDockGripDown} title="Arrastrar para redimensionar"></div>
    <div class="transport">
      <div class="tp-left">
        <span class="tp-time mono">
          <span class="t-cur">{fmtTime(outPos / 1000)}</span>
          <span class="t-sep">/</span>
          <span class="t-dur">{fmtTime(kept / 1000)}</span>
        </span>
      </div>

      <div class="tp-center">
        <button class="tp-btn" aria-label="Capturar fotograma" onclick={screenshot}>
          <Icon name="camera" size={18} />
        </button>
        <button class="tp-btn" aria-label="Ir al inicio" onclick={() => seekOutput(0)}>
          <Icon name="skip-back" size={20} />
        </button>
        <button class="tp-btn" aria-label="Fotograma anterior" onclick={() => stepFrame(-1)}>
          <Icon name="step-back" size={20} />
        </button>
        <button class="tp-play" aria-label={playing ? 'Pausar' : 'Reproducir'} onclick={toggle}>
          <Icon name={playing ? 'stop' : 'play'} size={19} />
        </button>
        <button class="tp-btn" aria-label="Fotograma siguiente" onclick={() => stepFrame(1)}>
          <Icon name="step-fwd" size={20} />
        </button>
        <button class="tp-btn" aria-label="Ir al final" onclick={() => seekOutput(kept)}>
          <Icon name="skip-fwd" size={20} />
        </button>
        <button class="tp-btn" aria-label="Pantalla completa (F)" onclick={toggleFullscreen}>
          <Icon name="maximize" size={18} />
        </button>
      </div>

      <div class="tp-right">
        <button class="act" disabled={editorState.segments.length <= 1} onclick={removeActive}>
          <Icon name="trash" size={14} /> Quitar
        </button>
        <button class="act export" onclick={handleExport} disabled={editorState.exporting}>
          {editorState.exporting ? 'Exportando…' : 'Exportar'}
          <Icon name="export" size={16} />
        </button>
      </div>
    </div>

    {#if kept > 0 || editorState.loading}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="tl" style="--gutter: 150px; --zoom: {zoom};" onwheel={onTimelineWheel}>
    <div class="tl-ruler"></div>

    <div class="tl-body" bind:this={bodyEl}>
      <div class="row">
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div class="lane vlane" class:reordering={!!lift} class:trimming bind:this={laneEl} onmousedown={onLaneDown}>
          <div class="zw">
          {#each segView as s (s.index)}
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div
            class="block"
            class:active={s.index === editorState.activeSegment}
            class:lifted={!!lift && s.index === lift.index}
            class:disabled={s.seg.disabled}
            style="left: {s.left}%; width: {s.width}%; --scale: {LIFT_SCALE};"
            onmousedown={(e) => onBlockDown(e, s.index)}
            oncontextmenu={(e) => onBlockContext(e, s.index)}
          >
            <span class="block-n mono">{s.index + 1}</span>
            {#if s.index === editorState.activeSegment}
              <!-- svelte-ignore a11y_no_static_element_interactions -->
              <span class="grip start" onmousedown={(e) => onGripDown(e, s.index, 'start')}></span>
              <!-- svelte-ignore a11y_no_static_element_interactions -->
              <span class="grip end" onmousedown={(e) => onGripDown(e, s.index, 'end')}></span>
            {/if}
          </div>
          {/each}
          </div>
        </div>
      </div>

      <div class="arow">
        {#snippet fader(chan: 'sys' | 'mic', vol: number, muted: boolean, label: string, icon: string)}
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div class="fader" class:muted style="--v: {vol};">
            <div
              class="fader-rail"
              role="slider"
              tabindex="0"
              aria-label={label}
              aria-valuemin="0"
              aria-valuemax="100"
              aria-valuenow={Math.round(vol * 100)}
              onmousedown={(e) => onFaderDown(e, chan)}
              onkeydown={(e) => onFaderKey(e, chan)}
              onmouseenter={(e) => onFaderHover(e, chan)}
              onmousemove={(e) => onFaderHover(e, chan)}
              onmouseleave={onFaderLeave}
            >
              <div class="fader-bar"><div class="fader-fill"></div></div>
              <div class="fader-thumb"><span class="thumb-line"></span></div>
            </div>
            <button
              class="fader-ico"
              class:on={muted}
              aria-label={muted ? `Activar ${label}` : `Silenciar ${label}`}
              aria-pressed={muted}
              onclick={() => toggleMute(chan)}
            ><Icon name={icon} size={22} /></button>
          </div>
        {/snippet}

        <div class="mixer-card" style="height: {mixerH}px;">
          <div class="faders">
            {#if !editorState.loading}
              {#if editorState.mic}
                {@render fader('mic', editorState.mixer.mic_vol, editorState.mixer.mic_muted, 'Audio del micrófono', 'mic')}
              {/if}
              {@render fader('sys', editorState.mixer.sys_vol, editorState.mixer.sys_muted, editorState.system ? 'Audio del sistema' : 'Audio', 'headphones')}
            {/if}
          </div>
        </div>
        <div class="alanes">
          {#if !editorState.loading}
            <div class="lane alane" class:muted={editorState.mixer.sys_muted}>
              <div class="zw">
                {#if editorState.system}
                  <canvas bind:this={sysCanvas}></canvas>
                {:else}
                  <canvas bind:this={mixCanvas}></canvas>
                  {#if !editorState.mixPeaks}<span class="wf-note mono">Sin audio</span>{/if}
                {/if}
              </div>
            </div>
            {#if editorState.mic}
              <div class="lane alane" class:muted={editorState.mixer.mic_muted}>
                <div class="zw">
                  <canvas bind:this={micCanvas}></canvas>
                </div>
              </div>
            {/if}
          {/if}
        </div>
      </div>

          <div
            class="playhead"
            class:scrub={scrubbing}
            style="left: calc((100% - var(--gutter)) * min(1, var(--zoom, 1)) * {frac} + var(--gutter));"
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

  {#if bubble}
    <div class="fader-tip mono" style="left: {bubble.x}px; top: {bubble.y}px;">{bubble.pct}%</div>
  {/if}

  {#if blockMenu}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="ctx-backdrop"
      onmousedown={closeBlockMenu}
      oncontextmenu={(e) => { e.preventDefault(); closeBlockMenu(); }}
    ></div>
    <div class="ctx-menu" style="left: {blockMenu.x}px; top: {blockMenu.y}px;">
      <button class="ctx-item" onclick={toggleDisableBlock}>
        {editorState.segments[blockMenu.index]?.disabled ? 'Activar' : 'Desactivar'}
      </button>
      <button class="ctx-item danger" onclick={deleteBlock} disabled={editorState.segments.length <= 1}>Eliminar</button>
    </div>
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
  .sub-btn:disabled { opacity: 0.3; pointer-events: none; }
  .sub-sep { color: var(--line); font-size: 13px; }
  .sub-label { color: var(--text-3); white-space: nowrap; }
  .sub-info { color: var(--text-1); white-space: nowrap; }
  .sub-path {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 260px;
    font: inherit;
    color: #5b9dff;
    background: none;
    border: none;
    padding: 0;
    cursor: pointer;
    transition: color 0.14s ease;
  }
  .sub-path:hover { color: #8ab6ff; text-decoration: underline; }
  .sub-path:disabled { color: var(--text-3); cursor: default; text-decoration: none; }

  .sub-right { display: flex; align-items: center; gap: 8px; flex-shrink: 0; }
  .sub-reset {
    padding: 5px 11px;
    font-size: 11.5px;
    color: var(--text-2);
    background: var(--bg-2);
    border: 1px solid var(--line);
    border-radius: 5px;
    white-space: nowrap;
    transition: background 0.14s ease, color 0.14s ease, border-color 0.14s ease;
  }
  .sub-reset:hover { background: var(--bg-hover); color: var(--text-0); }

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
    background: radial-gradient(120% 90% at 50% 0%, #121214 0%, #0a0a0b 70%, #060607 100%);
    overflow: hidden;
  }
  /* inset: 0 fija una altura definida (vía offsets) para que el vídeo, con max-height:100%,
     se reescale al cambiar el alto del dock en vez de quedarse a tamaño fijo. */
  .video-fit {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 22px 40px;
  }
  .stage video:fullscreen {
    border-radius: 0;
    box-shadow: none;
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
    position: relative;
    flex-shrink: 0;
    border-top: 1px solid var(--line);
    background: #080808;
    padding: 10px 16px 16px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .dock-grip {
    position: absolute;
    top: -5px;
    left: 0;
    right: 0;
    height: 12px;
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: ns-resize;
    z-index: 8;
  }
  .dock-grip::before {
    content: '';
    width: 34px;
    height: 3px;
    border-radius: 999px;
    background: var(--line);
    transition: background 0.14s ease, width 0.14s ease;
  }
  .dock-grip:hover::before {
    background: var(--line-strong);
    width: 46px;
  }

  .transport {
    display: grid;
    grid-template-columns: 1fr auto 1fr;
    align-items: center;
    gap: 12px;
    min-height: 34px;
  }
  .tp-left { display: flex; align-items: center; justify-self: start; }
  .tp-center { display: flex; align-items: center; gap: 4px; justify-self: center; }
  .tp-right { display: flex; align-items: center; gap: 6px; justify-self: end; }

  .tp-btn {
    width: 32px; height: 32px;
    display: grid; place-items: center;
    color: var(--text-1); border-radius: 7px;
    transition: color 0.14s ease, background 0.14s ease;
  }
  .tp-btn:hover { color: var(--text-0); background: var(--bg-3); }
  .tp-play {
    width: 36px; height: 36px;
    display: grid; place-items: center;
    color: var(--text-0);
    border-radius: 7px; margin: 0 4px; flex-shrink: 0;
    transition: color 0.14s ease, background 0.14s ease, transform 0.12s ease;
  }
  .tp-play:hover { color: var(--text-0); background: var(--bg-3); }
  .tp-play:active { transform: scale(0.94); }

  .tp-time { font-size: 12.5px; letter-spacing: 0.02em; white-space: nowrap; }
  .tp-time .t-cur { color: var(--text-0); }
  .tp-time .t-sep { color: var(--text-3); margin: 0 4px; }
  .tp-time .t-dur { color: var(--text-3); }

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
  .act.export {
    padding: 9px 22px;
    gap: 8px;
    font-size: 13px;
    color: var(--bg-0);
    background: var(--bright);
    border-color: transparent;
    font-weight: 600;
  }
  .act.export:hover { opacity: 0.9; background: var(--bright); color: var(--bg-0); }

  /* ===== timeline ===== */
  /* Lanes y ruler al 100% siempre. El contenido dentro de .zw escala con --zoom. */
  /* overflow: scroll (no auto) reserva siempre el hueco de ambas barras: como el ancho de
     reglas/lanes y el playhead se miden contra el content-box de .tl, dejar ese hueco fijo evita
     que todo se reescale/salte cuando la barra aparece. El track es transparente (estilos globales),
     así que el espacio reservado no se nota: solo asoma el thumb cuando hay desbordamiento real. */
  .tl {
    display: flex;
    flex-direction: column;
    gap: 6px;
    flex: 1;
    min-height: 0;
    overflow: scroll;
    scrollbar-gutter: stable;
  }
  .tl::-webkit-scrollbar-corner { background: transparent; }
  .tl-ruler {
    position: relative;
    height: 14px;
    width: max(100%, calc(var(--zoom, 1) * 100%));
    flex-shrink: 0;
  }
  .zw { position: relative; height: 100%; width: 100%; }

  .tl-body {
    position: relative;
    display: flex;
    flex-direction: column;
    gap: 7px;
    width: max(100%, calc(var(--zoom, 1) * 100%));
    flex-shrink: 0;
    overflow-x: clip;
  }
  .row { display: grid; grid-template-columns: var(--gutter) 1fr; align-items: stretch; }

  .lane {
    position: relative;
    border-radius: var(--r-sm);
    overflow: hidden;
    border: 1px solid var(--line);
    background: var(--bg-0);
  }
  /* Fondo negro: lo que no cubre ninguna sección es el "bloque negro" del montaje (no se exporta).
     grid-column: 2 mantiene el lane alineado con los de audio tras quitar el gutter de la fila. */
  .vlane { grid-column: 2; height: 46px; cursor: pointer; background: #000; }
  .alane { height: 50px; transition: opacity 0.16s ease; }
  .alane.muted { opacity: 0.5; }
  .alane canvas { display: block; width: 100%; height: 100%; }  .wf-note {
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
    cursor: pointer;
    overflow: hidden;
    transition: left 0.16s cubic-bezier(0.22, 1, 0.36, 1), transform 0.12s ease,
      box-shadow 0.12s ease, border-color 0.12s ease, background 0.12s ease, opacity 0.12s ease;
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
  /* Bloque levantado: sigue al cursor (sin transición de `left`), escalado y elevado. */
  .block.lifted {
    transition: transform 0.12s ease, box-shadow 0.12s ease;
    transform: scale(var(--scale, 1.05));
    z-index: 20;
    cursor: grabbing;
    border-color: var(--accent-soft);
    box-shadow: 0 10px 26px -8px rgba(0, 0, 0, 0.75), 0 0 0 1px var(--accent-soft);
  }
  /* Imán: mientras se reordena, los demás bloques se atenúan y se deslizan para hacer hueco. */
  .vlane.reordering { overflow: visible; }
  .vlane.reordering .block:not(.lifted) { opacity: 0.72; }
  /* Al recortar, el borde debe seguir al cursor al instante (sin la animación de `left`). */
  .vlane.trimming .block { transition: none; }
  /* Bloque desactivado: se ignora en reproducción/exportación pero sigue visible para reactivarlo.
     Atenuado, desaturado y con tramado diagonal; sin el resplandor de selección. */
  .block.disabled {
    opacity: 0.5;
    filter: grayscale(1);
    background:
      repeating-linear-gradient(
        -45deg,
        rgba(255, 255, 255, 0.06) 0,
        rgba(255, 255, 255, 0.06) 6px,
        rgba(255, 255, 255, 0.015) 6px,
        rgba(255, 255, 255, 0.015) 12px
      ),
      var(--bg-2);
    border-color: var(--line-strong);
    box-shadow: none;
  }
  .block.disabled .block-n { color: rgba(255, 255, 255, 0.5); }
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
    top: -7px;
    left: 50%;
    transform: translateX(-50%);
    width: 12px;
    height: 15px;
    border-radius: 3px;
    background: var(--bright);
    pointer-events: auto;
    cursor: grab;
    transition: transform 0.12s ease, opacity 0.12s ease;
  }
  .ph-knob::before {
    content: '';
    position: absolute;
    inset: -8px;
  }
  .playhead.scrub .ph-knob {
    cursor: grabbing;
    opacity: 0.85;
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

  /* Card del mezclador: panel flotante en el gutter, alineado a su borde derecho (junto a las
     ondas) y elevado (translateY -53) para que su borde superior cuadre con la lane de vídeo
     (46 + gap 7). Dos faders verticales (mic / sistema) con icono-mute debajo. */
  .arow { display: grid; grid-template-columns: var(--gutter) 1fr; align-items: start; }
  .mixer-card {
    --lift: 73px;
    position: sticky;
    left: 0;
    z-index: 25;
    justify-self: start;
    width: max-content;
    display: flex;
    margin-left: 2px;
    margin-right: 14px;
    padding: 14px 18px 12px;
    background: #0a0a0a;
    border: 1px solid var(--line-strong);
    border-radius: 18px;
    box-shadow: 0 18px 42px -14px rgba(0, 0, 0, 0.7);
    /* Sube la card para alinearla con la lane de vídeo; el margin-bottom negativo cancela el hueco
       muerto que esa subida dejaría en el flujo, para que la sección inferior no crezca de más. */
    transform: translateY(calc(-1 * var(--lift)));
    margin-bottom: calc(-1 * var(--lift));
  }
  .faders { display: flex; gap: 20px; }
  .fader { display: flex; flex-direction: column; align-items: center; gap: 10px; }
  /* El cursor controla el centro del pulgar, por eso el relleno y el pulgar descuentan su alto
     (--th) del recorrido. --v = volumen [0..1] (lo fija el componente por canal). */
  .fader-rail {
    --th: 20px;
    position: relative;
    flex: 1;
    width: 34px;
    cursor: pointer;
    outline: none;
  }
  .fader-rail:focus-visible { border-radius: 8px; box-shadow: 0 0 0 2px var(--accent-soft); }
  .fader-bar {
    position: absolute;
    left: 50%;
    top: 0;
    bottom: 0;
    width: 10px;
    transform: translateX(-50%);
    border-radius: 999px;
    overflow: hidden;
    background: linear-gradient(180deg, #3a3a3d, #242427);
  }
  .fader-fill {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    height: calc(var(--v) * (100% - var(--th)) + var(--th) / 2);
    background: #f2f2f2;
  }
  .fader-thumb {
    position: absolute;
    left: 50%;
    bottom: calc(var(--v) * (100% - var(--th)));
    transform: translateX(-50%);
    display: grid;
    place-items: center;
    width: 16px;
    height: var(--th);
    border-radius: 5px;
    background: #fff;
    box-shadow: 0 2px 6px rgba(0, 0, 0, 0.55);
    pointer-events: none;
  }
  .thumb-line { width: 2px; height: 10px; border-radius: 2px; background: #080808; }
  /* Burbuja flotante (fixed) posicionada por JS sobre el pulgar; vive a nivel del overlay para que
     el overflow de la timeline no la recorte. translate(-50%,-100%) deja su base sobre el pulgar. */
  .fader-tip {
    position: fixed;
    transform: translate(-50%, -100%);
    padding: 4px 9px;
    font-size: 13px;
    font-weight: 600;
    color: var(--text-1);
    background: #0a0a0a;
    border: 1px solid var(--line-strong);
    border-radius: 9px;
    white-space: nowrap;
    pointer-events: none;
    z-index: 300;
  }
  .fader-ico {
    width: 36px;
    height: 36px;
    display: grid;
    place-items: center;
    color: var(--text-0);
    border-radius: 8px;
    transition: color 0.12s ease, background 0.12s ease, opacity 0.16s ease;
  }
  .fader-ico:hover { background: var(--bg-3); }
  .fader-ico.on { color: var(--rec); }
  .fader.muted .fader-rail { opacity: 0.4; }
  .alanes { display: flex; flex-direction: column; gap: 7px; min-width: 0; }

  /* Menú contextual del bloque (clic derecho). El backdrop captura el clic fuera para cerrarlo. */
  .ctx-backdrop { position: fixed; inset: 0; z-index: 200; }
  .ctx-menu {
    position: fixed;
    z-index: 201;
    min-width: 150px;
    padding: 5px;
    background: var(--bg-2);
    border: 1px solid var(--line-strong);
    border-radius: 9px;
    box-shadow: 0 14px 36px -10px rgba(0, 0, 0, 0.7);
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .ctx-item {
    display: flex;
    align-items: center;
    padding: 8px 12px;
    font-size: 12.5px;
    color: var(--text-1);
    border-radius: 6px;
    white-space: nowrap;
    transition: background 0.12s ease, color 0.12s ease;
  }
  .ctx-item:hover { background: var(--bg-hover); color: var(--text-0); }
  .ctx-item:disabled { opacity: 0.4; pointer-events: none; }
  .ctx-item.danger { color: var(--rec); }
  .ctx-item.danger:hover { background: color-mix(in srgb, var(--rec) 16%, transparent); color: var(--rec); }
</style>
