<script lang="ts">
  import type { Dataset } from "./dataset";
  import type { RequestSpec } from "./types";
  import { generate, LANGUAGES, type Language } from "./generate";

  type Props = {
    spec: RequestSpec;
    dataset?: Dataset | null;
    onClose: () => void;
  };

  let { spec, dataset = null, onClose }: Props = $props();
  let activeLang = $state<Language>("curl");
  let copied = $state(false);

  let code = $derived(generate(activeLang, spec, dataset));

  async function copy() {
    try {
      await navigator.clipboard.writeText(code);
      copied = true;
      setTimeout(() => (copied = false), 1200);
    } catch {
      // ignore
    }
  }

  function backdropKey(e: KeyboardEvent) {
    if (e.key === "Escape") onClose();
  }
</script>

<svelte:window onkeydown={backdropKey} />

<div
  class="backdrop"
  role="presentation"
  onclick={onClose}
  onkeydown={(e) => {
    if (e.key === "Escape") onClose();
  }}
></div>
<div class="modal" role="dialog" aria-modal="true" aria-label="Generate code">
  <div class="modal-head">
    <div class="tabs">
      {#each LANGUAGES as lang (lang.id)}
        <button
          class="tab"
          class:active={activeLang === lang.id}
          onclick={() => (activeLang = lang.id)}
        >{lang.label}</button>
      {/each}
    </div>
    <div class="head-right">
      {#if dataset && dataset.rows.length > 0}
        <span class="data-badge" title="{dataset.rows.length} data rows baked into the script">
          × {dataset.rows.length}
        </span>
      {/if}
      <button class="copy" onclick={copy}>
        {copied ? "✓ copied" : "copy"}
      </button>
      <button class="close" onclick={onClose} aria-label="close">×</button>
    </div>
  </div>
  <pre class="code">{code}</pre>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.5);
    z-index: 100;
  }

  .modal {
    position: fixed;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    width: min(720px, 90vw);
    max-height: 80vh;
    background: var(--bg-elev);
    border: 1px solid var(--border);
    border-radius: 6px;
    z-index: 101;
    display: flex;
    flex-direction: column;
    box-shadow: 0 20px 60px rgba(0, 0, 0, 0.5);
  }

  .modal-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    border-bottom: 1px solid var(--border);
    padding: 4px;
  }

  .tabs {
    display: flex;
    gap: 2px;
  }

  .tab {
    background: transparent;
    border: none;
    color: var(--text-dim);
    padding: 6px 12px;
    border-radius: 4px;
    font-size: 12px;
    cursor: pointer;
  }

  .tab:hover {
    color: var(--text);
    background: var(--bg-elev-2);
  }

  .tab.active {
    background: var(--bg-elev-2);
    color: var(--accent);
    font-weight: 500;
  }

  .head-right {
    display: flex;
    gap: 4px;
    align-items: center;
    padding-right: 4px;
  }

  .data-badge {
    background: rgba(74, 222, 128, 0.15);
    color: var(--green);
    border: 1px solid rgba(74, 222, 128, 0.3);
    padding: 2px 8px;
    border-radius: 3px;
    font-size: 11px;
    font-family: var(--mono);
    margin-right: 4px;
  }

  .copy {
    background: var(--accent);
    color: white;
    border: 1px solid var(--accent);
    border-radius: 4px;
    padding: 4px 12px;
    font-size: 12px;
    cursor: pointer;
  }

  .copy:hover {
    background: var(--accent-dim);
  }

  .close {
    background: transparent;
    border: none;
    color: var(--text-dim);
    font-size: 18px;
    cursor: pointer;
    padding: 0 8px;
  }

  .close:hover {
    color: var(--text);
  }

  .code {
    margin: 0;
    padding: 16px;
    font-family: var(--mono);
    font-size: 12px;
    background: var(--bg);
    color: var(--text);
    overflow: auto;
    flex: 1;
    white-space: pre;
    border-radius: 0 0 6px 6px;
  }
</style>
