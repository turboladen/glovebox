<script lang="ts">
  // Inline delete confirmation with the attached-documents choice. Owns the
  // Delete button and the confirm row: when the record has linked documents
  // the confirm becomes a 3-way (delete docs too / keep docs / cancel), where
  // "keep" unlinks the documents so they never dangle at a deleted record.
  interface Props {
    label?: string
    getDocCount: () => Promise<number> | number
    onDelete: (documents: 'keep' | 'delete') => Promise<void> | void
  }

  let { label = 'Delete this record?', getDocCount, onDelete }: Props = $props()

  let confirming = $state(false)
  let docCount = $state(0)
  let busy = $state(false)

  async function open() {
    docCount = await getDocCount()
    confirming = true
  }

  async function run(documents: 'keep' | 'delete') {
    busy = true
    try {
      await onDelete(documents)
      confirming = false
    } finally {
      busy = false
    }
  }
</script>

{#if confirming}
  {#if docCount > 0}
    <span class="confirm-text">
      {label} It has {docCount} attached document{docCount === 1 ? '' : 's'}.
    </span>
    <button class="btn btn-danger btn-sm" onclick={() => run('delete')} disabled={busy}>
      {busy ? 'Deleting...' : 'Delete + documents'}
    </button>
    <button class="btn btn-danger btn-sm" onclick={() => run('keep')} disabled={busy}>
      Delete, keep documents
    </button>
    <button class="btn btn-secondary btn-sm" onclick={() => (confirming = false)} disabled={busy}>
      Cancel
    </button>
  {:else}
    <span class="confirm-text">{label}</span>
    <button class="btn btn-danger btn-sm" onclick={() => run('keep')} disabled={busy}>
      {busy ? 'Deleting...' : 'Yes, Delete'}
    </button>
    <button class="btn btn-secondary btn-sm" onclick={() => (confirming = false)} disabled={busy}>
      Cancel
    </button>
  {/if}
{:else}
  <button class="btn btn-danger-outline btn-sm" onclick={open}>Delete</button>
{/if}

<style>
  .confirm-text {
    font-size: 0.8rem;
    color: var(--danger);
    font-weight: 500;
  }

  .btn-danger-outline {
    background: none;
    color: var(--danger);
    border: 1px solid var(--danger);
  }

  .btn-danger-outline:hover {
    background: var(--danger-bg);
  }
</style>
