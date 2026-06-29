<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import Icon from '$lib/components/Icon.svelte';
  import { isScreenSource, sameFilter, type Clip, type LibraryFilter } from '$lib/clips';

  let { clips, selected = $bindable() }: { clips: Clip[]; selected: LibraryFilter[] } = $props();

  let open = $state(false);
  let el = $state<HTMLElement | null>(null);

  // Orígenes presentes en la biblioteca, separados en juegos y pantallas (orden alfabético).
  const games = $derived(
    [...new Set(clips.filter((c) => c.source && !isScreenSource(c.source)).map((c) => c.source))].sort()
  );
  const screens = $derived(
    [...new Set(clips.filter((c) => isScreenSource(c.source)).map((c) => c.source))].sort()
  );
  const hasEdited = $derived(clips.some((c) => c.edited));

  const activeLabel = $derived.by(() => {
    if (selected.length === 0) return 'Filtro';
    if (selected.length === 1) {
      const f = selected[0];
      return f.kind === 'edited' ? 'Editados' : f.value;
    }
    return `${selected.length} filtros`;
  });
  const isFiltered = $derived(selected.length > 0);

  function isOn(f: LibraryFilter): boolean {
    return selected.some((s) => sameFilter(s, f));
  }
  function toggle(f: LibraryFilter) {
    selected = isOn(f) ? selected.filter((s) => !sameFilter(s, f)) : [...selected, f];
  }

  // Logos de juego (data URL) cacheados; se piden de forma perezosa al abrir el menú. Una vez
  // en caché del backend, las siguientes peticiones devuelven al instante.
  let logos = $state<Record<string, string | null>>({});
  async function ensureLogo(name: string) {
    if (name in logos) return;
    logos[name] = null;
    try {
      const url = await invoke<string | null>('game_icon', { name, steamAppid: null });
      logos = { ...logos, [name]: url ?? null };
    } catch {}
  }
  $effect(() => {
    if (open) for (const g of games) ensureLogo(g);
  });

  function clearAll() {
    selected = [];
  }

  function initials(name: string): string {
    const parts = name.replace(/[^a-zA-Z0-9 ]/g, '').split(/\s+/).filter(Boolean);
    return parts.slice(0, 2).map((w) => w[0]).join('').toUpperCase() || '?';
  }

  $effect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (el && !el.contains(e.target as Node)) open = false;
    };
    window.addEventListener('mousedown', onDown, true);
    return () => window.removeEventListener('mousedown', onDown, true);
  });
</script>

<div class="dd" class:open bind:this={el}>
  <button class="ctrl" class:active={isFiltered} onclick={() => (open = !open)} aria-haspopup="menu" aria-expanded={open}>
    <Icon name="filter" size={14} />
    <span class="lbl">{activeLabel}</span>
    {#if isFiltered}
      <span
        class="clear"
        role="button"
        tabindex="0"
        aria-label="Quitar filtro"
        onclick={(e) => { e.stopPropagation(); clearAll(); }}
        onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); e.stopPropagation(); clearAll(); } }}
      >×</span>
    {:else}
      <Icon name="chevron-down" size={12} sw={2} />
    {/if}
  </button>

  {#if open}
    <div class="menu" role="menu">
      <button class="item" class:on={selected.length === 0} onclick={clearAll} role="menuitemradio" aria-checked={selected.length === 0}>
        <span class="lead"><Icon name="clips" size={15} /></span>
        <span class="txt">Todos</span>
        <span class="chk"><Icon name="check" size={13} sw={2.2} /></span>
      </button>

      {#if hasEdited}
        <button class="item" class:on={isOn({ kind: 'edited' })} onclick={() => toggle({ kind: 'edited' })} role="menuitemcheckbox" aria-checked={isOn({ kind: 'edited' })}>
          <span class="lead"><Icon name="edit" size={15} /></span>
          <span class="txt">Editados</span>
          <span class="chk"><Icon name="check" size={13} sw={2.2} /></span>
        </button>
      {/if}

      {#if games.length}
        <div class="divider"></div>
        <div class="sep">Juegos</div>
        {#each games as g (g)}
          <button class="item" class:on={isOn({ kind: 'source', value: g })} onclick={() => toggle({ kind: 'source', value: g })} role="menuitemcheckbox" aria-checked={isOn({ kind: 'source', value: g })}>
            <span class="lead logo">
              {#if logos[g]}<img src={logos[g]} alt="" />{:else}<span class="ini mono">{initials(g)}</span>{/if}
            </span>
            <span class="txt">{g}</span>
            <span class="chk"><Icon name="check" size={13} sw={2.2} /></span>
          </button>
        {/each}
      {/if}

      {#if screens.length}
        <div class="divider"></div>
        <div class="sep">Pantallas</div>
        {#each screens as s (s)}
          <button class="item" class:on={isOn({ kind: 'source', value: s })} onclick={() => toggle({ kind: 'source', value: s })} role="menuitemcheckbox" aria-checked={isOn({ kind: 'source', value: s })}>
            <span class="lead"><Icon name="monitor" size={15} /></span>
            <span class="txt">{s}</span>
            <span class="chk"><Icon name="check" size={13} sw={2.2} /></span>
          </button>
        {/each}
      {/if}
    </div>
  {/if}
</div>

<style>
  .dd {
    position: relative;
  }
  .ctrl {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    height: 36px;
    padding: 0 10px 0 12px;
    font-size: 13px;
    color: var(--text-1);
    background: var(--bg-1);
    border: 1px solid var(--line);
    border-radius: var(--r-sm);
    transition: color 0.15s ease, border-color 0.15s ease, background 0.15s ease;
  }
  .ctrl:hover {
    color: var(--text-0);
    border-color: var(--line-strong);
  }
  .ctrl.active {
    color: var(--on-accent);
    background: var(--accent);
    border-color: transparent;
  }
  .dd.open .ctrl:not(.active) {
    border-color: var(--line-strong);
  }
  .lbl {
    max-width: 160px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .clear {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 18px;
    height: 18px;
    margin-left: 1px;
    font-size: 15px;
    line-height: 1;
    border-radius: 5px;
    opacity: 0.8;
    transition: background 0.13s ease, opacity 0.13s ease;
  }
  .clear:hover {
    opacity: 1;
    background: rgba(0, 0, 0, 0.18);
  }

  .menu {
    position: absolute;
    top: calc(100% + 6px);
    right: 0;
    min-width: 220px;
    max-height: 60vh;
    overflow-y: auto;
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
  .item {
    display: flex;
    align-items: center;
    gap: 5px;
    padding: 7px 8px;
    font-size: 13px;
    text-align: left;
    color: var(--text-1);
    border-radius: 6px;
    transition: background 0.13s ease, color 0.13s ease;
  }
  .item:hover {
    background: var(--bg-3);
    color: var(--text-0);
  }
  .item.on {
    color: var(--text-0);
  }
  .lead {
    display: inline-flex;
    align-items: center;
    flex-shrink: 0;
    color: var(--text-2);
  }
  .logo {
    width: 18px;
    height: 18px;
    border-radius: 5px;
    overflow: hidden;
    background: var(--bg-3);
    border: 1px solid var(--line);
  }
  .logo img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
  .ini {
    font-size: 10px;
    font-weight: 600;
    color: var(--text-1);
  }
  .txt {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .chk {
    flex-shrink: 0;
    opacity: 0;
    color: var(--accent);
  }
  .item.on .chk {
    opacity: 1;
  }
  .sep {
    padding: 6px 8px 4px;
    font-size: 10.5px;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-3);
  }
  .divider {
    height: 1px;
    margin: 4px 4px;
    background: var(--line);
  }
</style>
