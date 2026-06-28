<script lang="ts">
  import Icon from '$lib/components/Icon.svelte';
  import ClipCard from '$lib/components/ClipCard.svelte';
  import { groupClips } from '$lib/clips';
  import { library, refreshLibrary, isFavorite } from '$lib/library.svelte';
  import { clipOrder } from '$lib/editor.svelte';

  $effect(() => {
    refreshLibrary();
  });

  const favs = $derived(library.clips.filter((c) => isFavorite(c.id)));
  const groups = $derived(groupClips(favs));

  // El editor navega anterior/siguiente por este mismo orden (grupos aplanados).
  $effect(() => {
    clipOrder.list = groups.flatMap((g) => g.clips);
  });
</script>

<div class="favs">
  <header class="head">
    <h1>Tus Favoritos</h1>
  </header>

  {#if favs.length === 0}
    <div class="empty">
      <Icon name="bookmark" size={50} sw={1.3} />
      <p>Aún no tienes clips guardados como favoritos.</p>
      <span class="hint mono">Marca la estrella de un clip para guardarlo aquí.</span>
    </div>
  {:else}
    {#each groups as group (group.label)}
      <section class="group">
        <div class="group-head">
          <span class="label">{group.label}</span>
          <span class="dash"></span>
          {#if group.source}<span class="label src">{group.source}</span>{/if}
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
  .favs {
    padding: 22px 26px 40px;
  }

  .head {
    display: flex;
    align-items: center;
    gap: 13px;
    margin-bottom: 26px;
  }
  h1 {
    font-size: 22px;
    font-weight: 650;
    letter-spacing: -0.01em;
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
    .grid {
      grid-template-columns: repeat(3, 1fr);
    }
  }

  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 12px;
    padding: 90px 0;
    color: var(--text-3);
  }
  .empty p {
    font-size: 14px;
    color: var(--text-1);
  }
  .hint {
    font-size: 11.5px;
    color: var(--text-3);
  }
</style>
