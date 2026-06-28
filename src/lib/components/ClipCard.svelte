<script lang="ts">
  import Icon from './Icon.svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { revealItemInDir } from '@tauri-apps/plugin-opener';
  import { menu } from '$lib/menu.svelte';
  import { formatDuration, formatRelative, type Clip } from '$lib/clips';
  import {
    isFavorite,
    toggleFavorite,
    requestThumb,
    refreshLibrary,
    renameFavorite,
    removeFavorite
  } from '$lib/library.svelte';
  import { openEditor } from '$lib/editor.svelte';

  let { clip }: { clip: Clip } = $props();

  const open = $derived(menu.openId === clip.id);
  const favorite = $derived(isFavorite(clip.id));
  const favLabel = $derived(
    favorite ? 'Marcado como favorito, click para desmarcar' : 'Marcar como favorito'
  );

  let cardEl = $state<HTMLElement | null>(null);
  let poster = $state<string | null>(null);
  let hovering = $state(false);
  // El vídeo solo se monta en hover (sin precarga); videoReady marca cuándo ya tiene su
  // primer frame para fundirlo sobre el negro al que se desvanece el póster.
  let videoReady = $state(false);
  // Burst al marcar favorito: un duplicado del icono que escala y se desvanece. burstKey
  // remonta el elemento para replayar la animación en marcados rápidos sucesivos.
  let bursting = $state(false);
  let burstKey = $state(0);

  // Carátula perezosa: la miniatura (un JPEG ligero cacheado por el backend) se pide solo
  // cuando la tarjeta se acerca al viewport. El <video> no se monta hasta el hover, así que
  // nunca hay decenas de decodificadores de vídeo activos a la vez.
  $effect(() => {
    const el = cardEl;
    if (!el || poster || !clip.path) return;
    const io = new IntersectionObserver(
      (entries) => {
        if (entries[0].isIntersecting) {
          io.disconnect();
          requestThumb(clip.path).then((u) => {
            if (u) poster = u;
          });
        }
      },
      { rootMargin: '300px' }
    );
    io.observe(el);
    return () => io.disconnect();
  });

  function toggleMenu(e: MouseEvent) {
    e.stopPropagation();
    menu.openId = open ? null : clip.id;
  }

  // Abrir el editor al pulsar la tarjeta, salvo cuando el click nace dentro del menú de
  // acciones: soltar el botón en una opción distinta de donde se pulsó sintetiza un click
  // sobre el contenedor del menú, y sin esta guarda subiría hasta aquí y abriría el editor.
  function openFromCard(e: MouseEvent) {
    if ((e.target as HTMLElement).closest('.actions')) return;
    openEditor(clip);
  }

  function favClick(e: MouseEvent) {
    e.stopPropagation();
    const wasFav = favorite;
    toggleFavorite(clip.id);
    const reduced = window.matchMedia?.('(prefers-reduced-motion: reduce)').matches;
    if (!wasFav && !reduced) {
      burstKey++;
      bursting = true;
    }
  }

  let renaming = $state(false);
  let renameValue = $state('');

  function startRename(e: MouseEvent) {
    e.stopPropagation();
    menu.openId = null;
    renameValue = clip.title;
    renaming = true;
  }

  async function commitRename() {
    if (!renaming) return;
    renaming = false;
    const name = renameValue.trim();
    if (!name || name === clip.title) return;
    try {
      const newPath = await invoke<string>('rename_clip', { path: clip.path, newName: name });
      const newId = newPath.split(/[\\/]/).pop() ?? clip.id;
      renameFavorite(clip.id, newId);
      await refreshLibrary();
    } catch (err) {
      console.error('rename_clip', err);
    }
  }

  function focusSelect(node: HTMLInputElement) {
    node.focus();
    node.select();
  }

  async function openLocation(e: MouseEvent) {
    e.stopPropagation();
    menu.openId = null;
    try {
      await revealItemInDir(clip.path);
    } catch (err) {
      console.error('revealItemInDir', err);
    }
  }

  async function deleteClip(e: MouseEvent) {
    e.stopPropagation();
    menu.openId = null;
    try {
      await invoke('delete_clip', { path: clip.path });
      removeFavorite(clip.id);
      await refreshLibrary();
    } catch (err) {
      console.error('delete_clip', err);
    }
  }
</script>

<svelte:window onclick={() => (menu.openId = null)} />

<article class="card" class:open bind:this={cardEl} onmouseenter={() => (hovering = true)} onmouseleave={() => { hovering = false; videoReady = false; }} onclick={openFromCard} onkeydown={() => openEditor(clip)} role="presentation">
  <div class="thumb">
    {#if poster}
      <img class="preview poster" class:hide={hovering} src={poster} alt="" draggable="false" />
    {:else}
      <div class="watermark"><Icon name="chevrons" size={150} sw={1.1} /></div>
    {/if}
    {#if hovering && clip.previewSrc}
      <video
        class="preview vid"
        class:show={videoReady}
        src={clip.previewSrc}
        muted
        loop
        playsinline
        autoplay
        preload="none"
        onloadeddata={() => (videoReady = true)}
      ></video>
    {/if}
    <div class="scrim"></div>

    <div class="badge mono">
      {#if clip.trimmed}<Icon name="scissors" size={12} sw={1.9} />{/if}
      {#if clip.edited}<Icon name="edit" size={12} sw={1.9} />{/if}
      {formatDuration(clip.durationSec)}
    </div>

  </div>

  <button class="fav" class:on={favorite} aria-label={favLabel} onclick={favClick}>
    <Icon name={favorite ? 'bookmark-fill' : 'bookmark'} size={16} sw={1.8} />
    {#if bursting}
      {#key burstKey}
        <span class="fav-burst" onanimationend={() => (bursting = false)}>
          <Icon name="bookmark" size={16} sw={1.8} />
        </span>
      {/key}
    {/if}
    <span class="fav-tip" role="tooltip">{favLabel}</span>
  </button>

  <div class="meta">
    {#if clip.source}<span class="src label">{clip.source}</span>{/if}

    <div class="row">
      {#if renaming}
        <input
          class="title-edit"
          bind:value={renameValue}
          use:focusSelect
          onclick={(e) => e.stopPropagation()}
          onkeydown={(e) => {
            e.stopPropagation();
            if (e.key === 'Enter') commitRename();
            else if (e.key === 'Escape') renaming = false;
          }}
          onblur={commitRename}
        />
      {:else}
        <h3 class="title">{clip.title}</h3>
      {/if}
      <div class="actions">
        <button class="act" aria-label="Compartir" onclick={(e) => e.stopPropagation()}><Icon name="share" size={19} sw={2} /></button>
        <button
          class="act"
          aria-label="Más opciones"
          aria-haspopup="menu"
          aria-expanded={open}
          onclick={toggleMenu}
        >
          <Icon name="more" size={21} sw={2} />
        </button>

        {#if open}
          <div class="menu" role="menu">
            <button role="menuitem" onclick={(e) => { e.stopPropagation(); openEditor(clip); }}><Icon name="scissors" size={15} sw={1.9} /> Abrir en editor</button>
            <button role="menuitem" onclick={startRename}><Icon name="rename" size={15} sw={1.9} /> Renombrar</button>
            <button role="menuitem" onclick={openLocation}><Icon name="folder-open" size={15} sw={1.9} /> Abrir ubicación</button>
            <div class="sep"></div>
            <button role="menuitem" class="danger" onclick={deleteClip}><Icon name="trash" size={15} sw={1.9} /> Borrar</button>
          </div>
        {/if}
      </div>
    </div>

    <span class="when mono"><Icon name="clock" size={13} sw={2} />{formatRelative(clip.createdAt)}</span>
  </div>
</article>

<style>
  .card {
    position: relative;
    background: #121212;
    border-radius: 4px;
  }
  .card:hover {
    outline: 4px solid rgba(160, 167, 182, 0.3);
  }
  .card.open {
    z-index: 30;
    outline: 4px solid rgba(160, 167, 182, 0.3);
  }

  .thumb {
    position: relative;
    aspect-ratio: 16 / 9;
    overflow: hidden;
    border-radius: 4px 4px 0 0;
    background: #000;
  }
  .watermark {
    position: absolute;
    right: -26px;
    bottom: -34px;
    color: #ffffff;
    opacity: 0.07;
    transform: rotate(-8deg);
  }
  .scrim {
    position: absolute;
    inset: 0;
    background: linear-gradient(to bottom, rgba(0, 0, 0, 0.28), transparent 30%, transparent 62%, rgba(0, 0, 0, 0.34));
  }

  .preview {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }
  .poster {
    transition: opacity 0.25s ease;
  }
  .poster.hide {
    opacity: 0;
  }
  .vid {
    opacity: 0;
    transition: opacity 0.25s ease;
  }
  .vid.show {
    opacity: 1;
  }

  .badge {
    position: absolute;
    top: 10px;
    right: 10px;
    display: flex;
    align-items: center;
    gap: 5px;
    padding: 4px 9px;
    font-size: 12px;
    color: var(--text-0);
    background: rgba(27, 30, 38, 0.6);
    backdrop-filter: blur(6px);
    border: 1px solid var(--line);
    border-radius: 999px;
  }
  .badge :global(svg) {
    color: var(--accent);
  }

  .fav {
    position: absolute;
    top: 10px;
    left: 10px;
    width: 32px;
    height: 32px;
    display: grid;
    place-items: center;
    border-radius: 999px;
    color: var(--text-0);
    background: rgba(4, 8, 14, 0.55);
    backdrop-filter: blur(6px);
    border: 1px solid var(--line);
    opacity: 0;
    transition: opacity 0.16s ease, color 0.16s ease;
  }
  .card:hover .fav {
    opacity: 1;
  }
  .fav.on {
    opacity: 1;
    color: var(--gold);
  }
  .fav-burst {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    color: var(--gold);
    pointer-events: none;
    animation: fav-burst 0.45s ease-out forwards;
  }
  @keyframes fav-burst {
    from {
      transform: scale(1);
      opacity: 0.9;
    }
    to {
      transform: scale(1.5);
      opacity: 0;
    }
  }
  .fav-tip {
    position: absolute;
    top: calc(100% + 7px);
    /* Centrado sobre el botón, pero con clamp: el borde izquierdo nunca pasa del borde
       de la card (el botón va pegado a la izquierda). 84px = mitad del ancho del tooltip. */
    left: max(-8px, calc(50% - 84px));
    width: 168px;
    padding: 7px 10px;
    font-size: 11.5px;
    line-height: 1.3;
    text-align: center;
    color: var(--text-1);
    background: var(--bg-0);
    border: 1px solid var(--line-strong);
    border-radius: 8px;
    box-shadow: 0 12px 30px -10px rgba(0, 0, 0, 0.7);
    opacity: 0;
    visibility: hidden;
    pointer-events: none;
    transition: opacity 0.14s ease;
  }
  .fav:hover .fav-tip {
    opacity: 1;
    visibility: visible;
  }

  .meta {
    padding: 11px 14px 13px;
  }
  .src {
    display: block;
    margin-bottom: 3px;
    line-height: 1;
    font-size: 12px;
    color: var(--text-2);
  }

  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
  }
  .title {
    font-size: 16px;
    font-weight: 560;
    line-height: 1.2;
    color: var(--text-0);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .title-edit {
    flex: 1;
    min-width: 0;
    font-size: 16px;
    font-weight: 560;
    line-height: 1.2;
    color: var(--text-0);
    background: var(--bg-0);
    border: 1px solid var(--accent);
    border-radius: 5px;
    padding: 3px 7px;
    outline: none;
  }

  .actions {
    position: relative;
    display: flex;
    gap: 2px;
    flex-shrink: 0;
  }
  .act {
    width: 35px;
    height: 35px;
    display: grid;
    place-items: center;
    border-radius: var(--r-sm);
    color: var(--text-2);
    transition: background 0.14s ease, color 0.14s ease;
  }
  .act:hover {
    background: var(--bg-hover);
    color: var(--text-0);
  }

  .menu {
    position: absolute;
    top: calc(100% + 8px);
    right: 0;
    width: 196px;
    display: flex;
    flex-direction: column;
    gap: 1px;
    padding: 5px;
    background: var(--bg-1);
    border: 1px solid var(--line-strong);
    border-radius: var(--r-md);
    box-shadow: 0 18px 42px -14px rgba(0, 0, 0, 0.7);
    z-index: 40;
  }
  .menu button {
    display: flex;
    align-items: center;
    gap: 11px;
    width: 100%;
    padding: 9px 10px;
    font-size: 13px;
    color: var(--text-1);
    text-align: left;
    border-radius: 6px;
    transition: background 0.12s ease, color 0.12s ease;
  }
  .menu button:hover {
    background: var(--bg-3);
    color: var(--text-0);
  }
  .menu .danger {
    color: var(--rec);
  }
  .menu .danger:hover {
    background: rgba(255, 91, 91, 0.12);
    color: var(--rec);
  }
  .sep {
    height: 1px;
    margin: 4px 6px;
    background: var(--line);
  }

  .when {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-top: 4px;
    font-size: 11px;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    line-height: 1;
    color: var(--text-2);
  }
  .when :global(svg) {
    flex-shrink: 0;
  }
</style>
