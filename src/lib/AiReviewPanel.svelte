<script lang="ts">
  import type { AiReview } from "./api";

  type Props = {
    review: AiReview;
    onClose: () => void;
    onAddTest: (line: string) => void;
    onAddExtract: (varName: string, source: string) => void;
    onAddTests: (lines: string[]) => void;
    onAddExtracts: (entries: Array<{ var_name: string; source: string }>) => void;
  };

  let { review, onClose, onAddTest, onAddExtract, onAddTests, onAddExtracts }: Props = $props();

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
<div class="modal" role="dialog" aria-modal="true" aria-label="AI review">
  <div class="modal-head">
    <div class="title">
      <span class="badge">AI</span>
      <span class="suggested-name">{review.suggested_name}</span>
    </div>
    <button class="close" onclick={onClose} aria-label="close">×</button>
  </div>

  <div class="body">
    {#if review.summary}
      <div class="summary">{review.summary}</div>
    {/if}

    {#if review.security_observations.length > 0}
      <div class="section">
        <div class="section-head">
          <span class="dot warn">⚠</span>
          <span class="section-title">security observations</span>
        </div>
        <ul class="obs-list">
          {#each review.security_observations as obs}
            <li>{obs}</li>
          {/each}
        </ul>
      </div>
    {/if}

    {#if review.tests.length > 0}
      <div class="section">
        <div class="section-head">
          <span class="dot ok">✓</span>
          <span class="section-title">suggested tests</span>
          <button
            class="apply-all"
            onclick={() => onAddTests(review.tests)}
          >add all</button>
        </div>
        <div class="suggestions">
          {#each review.tests as test}
            <div class="suggestion">
              <code>{test}</code>
              <button class="add" onclick={() => onAddTest(test)}>add</button>
            </div>
          {/each}
        </div>
      </div>
    {/if}

    {#if review.extracts.length > 0}
      <div class="section">
        <div class="section-head">
          <span class="dot accent">→</span>
          <span class="section-title">suggested extracts</span>
          <button
            class="apply-all"
            onclick={() => onAddExtracts(review.extracts)}
          >add all</button>
        </div>
        <div class="suggestions">
          {#each review.extracts as ex}
            <div class="suggestion">
              <code><strong>{ex.var_name}</strong> = {ex.source}</code>
              <button
                class="add"
                onclick={() => onAddExtract(ex.var_name, ex.source)}
              >add</button>
            </div>
          {/each}
        </div>
      </div>
    {/if}
  </div>
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
    width: min(640px, 90vw);
    max-height: 80vh;
    background: var(--bg-elev);
    border: 1px solid var(--border);
    border-radius: 6px;
    z-index: 101;
    display: flex;
    flex-direction: column;
    box-shadow: 0 20px 60px rgba(0, 0, 0, 0.5);
    overflow: hidden;
  }

  .modal-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    border-bottom: 1px solid var(--border);
    padding: 8px 12px;
  }

  .title {
    display: flex;
    align-items: center;
    gap: 8px;
    font-family: var(--mono);
    font-size: 13px;
  }

  .badge {
    background: linear-gradient(90deg, var(--accent), var(--accent-dim));
    color: white;
    font-size: 10px;
    font-weight: 600;
    padding: 1px 6px;
    border-radius: 3px;
    letter-spacing: 0.04em;
  }

  .suggested-name {
    color: var(--text);
    font-weight: 500;
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

  .body {
    padding: 12px;
    overflow-y: auto;
    flex: 1;
  }

  .summary {
    color: var(--text);
    font-size: 13px;
    line-height: 1.5;
    margin-bottom: 16px;
    padding-bottom: 12px;
    border-bottom: 1px solid var(--border);
  }

  .section {
    margin-bottom: 16px;
  }

  .section:last-child {
    margin-bottom: 0;
  }

  .section-head {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-bottom: 6px;
  }

  .section-title {
    color: var(--text-dim);
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }

  .dot {
    font-size: 12px;
    font-weight: 700;
  }

  .dot.warn { color: #facc15; }
  .dot.ok   { color: var(--green); }
  .dot.accent { color: var(--accent); }

  .obs-list {
    margin: 0;
    padding-left: 20px;
    color: var(--text);
    font-size: 12px;
    line-height: 1.5;
  }

  .obs-list li {
    margin-bottom: 4px;
  }

  .suggestions {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .suggestion {
    display: grid;
    grid-template-columns: 1fr auto;
    gap: 8px;
    align-items: center;
    padding: 6px 10px;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 4px;
    font-family: var(--mono);
    font-size: 12px;
  }

  .suggestion code {
    color: var(--text);
    font-family: inherit;
  }

  .suggestion code strong {
    color: var(--accent);
  }

  .add {
    background: var(--bg-elev-2);
    border: 1px solid var(--border);
    color: var(--text-dim);
    padding: 2px 10px;
    border-radius: 3px;
    font-size: 11px;
    cursor: pointer;
  }

  .add:hover {
    color: var(--text);
    border-color: var(--accent-dim);
  }

  .apply-all {
    margin-left: auto;
    background: var(--accent);
    color: white;
    border: 1px solid var(--accent);
    padding: 2px 10px;
    border-radius: 3px;
    font-size: 11px;
    cursor: pointer;
  }

  .apply-all:hover {
    background: var(--accent-dim);
  }
</style>
