<script lang="ts">
  import Icon from '$lib/components/Icon.svelte';
  import ClipCard from '$lib/components/ClipCard.svelte';
  import { groupClips } from '$lib/clips';
  import { library, refreshLibrary } from '$lib/library.svelte';
  import { clipOrder } from '$lib/editor.svelte';

  let query = $state('');
  let view = $state<'grid' | 'list'>('grid');

  $effect(() => {
    refreshLibrary();
  });

  const filtered = $derived(
    library.clips.filter((c) => {
      const q = query.trim().toLowerCase();
      return !q || c.title.toLowerCase().includes(q) || c.source.toLowerCase().includes(q);
    })
  );
  const groups = $derived(groupClips(filtered));

  // El editor navega anterior/siguiente por este mismo orden (grupos aplanados).
  $effect(() => {
    clipOrder.list = groups.flatMap((g) => g.clips);
  });
</script>

<div class="clips">
  <header class="head">
    <div class="left">
      <h1>Todos los clips</h1>
      <button class="montage">
        <Icon name="plus" size={15} sw={2} />
        Crear montaje
      </button>
    </div>

    <div class="right">
      <label class="search">
        <Icon name="search" size={15} />
        <input placeholder="Buscar clips" bind:value={query} />
      </label>
      <button class="ctrl">
        <Icon name="filter" size={14} />
        Filtro
      </button>
      <button class="ctrl">
        Más reciente
        <Icon name="chevron-down" size={13} sw={2} />
      </button>
      <div class="viewtoggle">
        <button class:on={view === 'grid'} aria-label="Cuadrícula" onclick={() => (view = 'grid')}>
          <Icon name="clips" size={16} />
        </button>
        <button class:on={view === 'list'} aria-label="Lista" onclick={() => (view = 'list')}>
          <Icon name="list" size={16} />
        </button>
      </div>
    </div>
  </header>

  {#if library.clips.length === 0}
    <div class="empty">
      <Icon name="clips" size={50} sw={1.3} />
      <p>Aún no tienes clips.</p>
      <span class="hint mono">Graba con el botón o tu atajo y aparecerán aquí.</span>
    </div>
  {:else if filtered.length === 0}
    <div class="empty">
      <Icon name="chevrons" size={56} sw={1.2} />
      <p>Sin resultados para “{query}”.</p>
    </div>
  {:else}
    {#each groups as group (group.label)}
      <section class="group">
        <div class="group-head">
          <span class="label">{group.label}</span>
          <span class="dash"></span>
          {#if group.source}<span class="label src">{group.source}</span>{/if}
        </div>
        <div class="grid" class:list={view === 'list'}>
          {#each group.clips as clip (clip.id)}
            <ClipCard {clip} />
          {/each}
        </div>
      </section>
    {/each}
  {/if}
</div>

<style>
  .clips {
    padding: 22px 26px 40px;
  }

  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 18px;
    margin-bottom: 26px;
    flex-wrap: wrap;
  }
  .left {
    display: flex;
    align-items: center;
    gap: 14px;
  }
  h1 {
    font-size: 22px;
    font-weight: 650;
    letter-spacing: -0.01em;
  }
  .montage {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    margin-left: 4px;
    padding: 8px 14px;
    font-size: 13px;
    font-weight: 560;
    color: var(--bright);
    background: rgba(240, 242, 247, 0.1);
    border: 1px solid rgba(240, 242, 247, 0.3);
    border-radius: var(--r-sm);
    transition: background 0.15s ease, box-shadow 0.15s ease;
  }
  .montage:hover {
    background: rgba(240, 242, 247, 0.16);
    box-shadow: 0 0 0 3px rgba(240, 242, 247, 0.08);
  }

  .right {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .search {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 12px;
    height: 36px;
    width: 220px;
    color: var(--text-2);
    background: var(--bg-1);
    border: 1px solid var(--line);
    border-radius: var(--r-sm);
    transition: border-color 0.15s ease;
  }
  .search:focus-within {
    border-color: var(--line-strong);
  }
  .search input {
    flex: 1;
    min-width: 0;
    background: none;
    border: none;
    outline: none;
    font-size: 13px;
    color: var(--text-0);
  }
  .search input::placeholder {
    color: var(--text-3);
  }
  .ctrl {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    height: 36px;
    padding: 0 12px;
    font-size: 13px;
    color: var(--text-1);
    background: var(--bg-1);
    border: 1px solid var(--line);
    border-radius: var(--r-sm);
    transition: color 0.15s ease, border-color 0.15s ease;
  }
  .ctrl:hover {
    color: var(--text-0);
    border-color: var(--line-strong);
  }
  .viewtoggle {
    display: flex;
    padding: 3px;
    gap: 2px;
    background: var(--bg-1);
    border: 1px solid var(--line);
    border-radius: var(--r-sm);
  }
  .viewtoggle button {
    width: 30px;
    height: 28px;
    display: grid;
    place-items: center;
    border-radius: 5px;
    color: var(--text-2);
    transition: color 0.14s ease, background 0.14s ease;
  }
  .viewtoggle button:hover {
    color: var(--bright);
  }
  .viewtoggle button.on {
    color: var(--bright);
    background: var(--bg-3);
  }

  .group {
    margin-bottom: 30px;
  }
  .group-head {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 14px;
  }
  .src {
    color: var(--text-3);
  }
  .dash {
    flex: 1;
    height: 1px;
    background: var(--line);
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(2, 1fr);
    gap: 20px;
  }
  @media (min-width: 1280px) {
    .grid:not(.list) {
      grid-template-columns: repeat(3, 1fr);
    }
  }
  .grid.list {
    grid-template-columns: minmax(0, 760px);
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
  }
  .empty .hint {
    font-size: 11.5px;
    color: var(--text-3);
  }
</style>
