<script lang="ts" generics="T extends string | number">
  import Icon from '$lib/components/Icon.svelte';

  type Option = { label: string; value: T };

  let {
    value,
    options,
    onchange,
    minWidth = 140,
    ariaLabel = ''
  }: {
    value: T;
    options: readonly Option[];
    onchange: (value: T) => void;
    minWidth?: number;
    ariaLabel?: string;
  } = $props();

  let open = $state(false);
  let el = $state<HTMLElement | null>(null);

  const label = $derived(options.find((o) => o.value === value)?.label ?? String(value));

  function pick(v: T) {
    onchange(v);
    open = false;
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
  <button
    class="dd-trigger"
    style:min-width="{minWidth}px"
    aria-haspopup="listbox"
    aria-expanded={open}
    aria-label={ariaLabel}
    onclick={() => (open = !open)}
  >
    <span class="dd-value">{label}</span>
    <span class="dd-chev"><Icon name="chevron-down" size={13} sw={2} /></span>
  </button>
  {#if open}
    <div class="dd-list" role="listbox">
      {#each options as o (o.value)}
        <button
          class="dd-item"
          class:on={o.value === value}
          role="option"
          aria-selected={o.value === value}
          onclick={() => pick(o.value)}
        >
          {o.label}
          <span class="dd-check"><Icon name="check" size={13} sw={2.2} /></span>
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .dd {
    position: relative;
    flex-shrink: 0;
  }
  .dd-trigger {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    height: 34px;
    padding: 0 12px;
    font-size: 13px;
    color: var(--text-0);
    background: var(--bg-0);
    border: 1px solid var(--line);
    border-radius: var(--r-sm);
    cursor: pointer;
    text-align: left;
    transition: border-color 0.14s ease;
  }
  .dd-trigger:hover,
  .dd.open .dd-trigger {
    border-color: var(--line-strong);
  }
  .dd-value {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .dd-chev {
    display: inline-flex;
    color: var(--text-3);
    flex-shrink: 0;
    transition: transform 0.15s ease;
  }
  .dd.open .dd-chev {
    transform: rotate(180deg);
  }
  .dd-list {
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
  .dd-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 8px 10px;
    font-size: 12.5px;
    border-radius: 6px;
    color: var(--text-1);
    text-align: left;
    white-space: nowrap;
    transition: background 0.12s ease, color 0.12s ease;
  }
  .dd-item:hover {
    background: var(--bg-3);
    color: var(--text-0);
  }
  .dd-item.on {
    color: var(--bright);
  }
  .dd-check {
    opacity: 0;
    flex-shrink: 0;
    color: var(--bright);
  }
  .dd-item.on .dd-check {
    opacity: 1;
  }
</style>
