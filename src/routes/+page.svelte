<script lang="ts">
  import Icon from '$lib/components/Icon.svelte';
  import ClipCard from '$lib/components/ClipCard.svelte';
  import LibraryFilter from '$lib/components/LibraryFilter.svelte';
  import { groupClips, clipMatchesFilters, type LibraryFilter as Filter } from '$lib/clips';
  import { library, refreshLibrary } from '$lib/library.svelte';
  import { clipOrder } from '$lib/editor.svelte';

  let query = $state('');
  let filters = $state<Filter[]>([]);
  let sortAsc = $state(false);
  let sortOpen = $state(false);
  let sortEl = $state<HTMLElement | null>(null);

  $effect(() => {
    refreshLibrary();
  });

  const filtered = $derived(
    library.clips.filter((c) => {
      const q = query.trim().toLowerCase();
      const matchesQuery = !q || c.title.toLowerCase().includes(q) || c.source.toLowerCase().includes(q);
      return matchesQuery && clipMatchesFilters(c, filters);
    })
  );
  const groups = $derived(groupClips(filtered, sortAsc));

  $effect(() => {
    if (!sortOpen) return;
    const onDown = (e: MouseEvent) => {
      if (sortEl && !sortEl.contains(e.target as Node)) sortOpen = false;
    };
    window.addEventListener('mousedown', onDown, true);
    return () => window.removeEventListener('mousedown', onDown, true);
  });

  // El editor navega anterior/siguiente por este mismo orden (grupos aplanados).
  $effect(() => {
    clipOrder.list = groups.flatMap((g) => g.clips);
  });
</script>

<div class="clips">
  <header class="head">
    <div class="left">
      <h1>Todos los clips</h1>
    </div>

    <div class="right">
      <label class="search">
        <Icon name="search" size={15} />
        <input placeholder="Buscar clips" bind:value={query} />
      </label>
      <LibraryFilter clips={library.clips} bind:selected={filters} />
      <div class="sort-dd" class:open={sortOpen} bind:this={sortEl}>
        <button class="ctrl" onclick={() => (sortOpen = !sortOpen)}>
          {sortAsc ? 'Más antiguo' : 'Más reciente'}
          <Icon name="chevron-down" size={13} sw={2} />
        </button>
        {#if sortOpen}
          <div class="sort-menu">
            <button class="sort-item" class:on={!sortAsc} onclick={() => { sortAsc = false; sortOpen = false; }}>
              Más reciente
            </button>
            <button class="sort-item" class:on={sortAsc} onclick={() => { sortAsc = true; sortOpen = false; }}>
              Más antiguo
            </button>
          </div>
        {/if}
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
      <p>{query ? `Sin resultados para “${query}”.` : 'Sin resultados con este filtro.'}</p>
    </div>
  {:else}
    {#each groups as group (group.label)}
      <section class="group">
        <div class="group-head">
          <span class="label">{group.label}</span>
          <span class="dash"></span>
        </div>
        <div class="grid">
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
  .sort-dd {
    position: relative;
  }
  .sort-dd.open .ctrl {
    border-color: var(--line-strong);
    color: var(--text-0);
  }
  .sort-menu {
    position: absolute;
    top: calc(100% + 6px);
    right: 0;
    min-width: 150px;
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
  .sort-item {
    padding: 7px 10px;
    font-size: 13px;
    text-align: left;
    color: var(--text-1);
    border-radius: 6px;
    transition: background 0.13s ease, color 0.13s ease;
  }
  .sort-item:hover {
    background: var(--bg-3);
    color: var(--text-0);
  }
  .sort-item.on {
    color: var(--text-0);
    font-weight: 560;
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
  .dash {
    flex: 1;
    height: 1px;
    background: var(--line);
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 20px;
  }
  @media (min-width: 1280px) {
    .grid {
      grid-template-columns: repeat(3, minmax(0, 1fr));
    }
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
