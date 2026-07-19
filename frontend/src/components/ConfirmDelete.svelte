<script lang="ts">
  // Inline delete confirmation with the attached-documents choice. Owns the
  // Delete button and the confirm row: when the record has linked documents
  // the confirm becomes a 3-way (delete docs too / keep docs / cancel), where
  // "keep" unlinks the documents so they never dangle at a deleted record.
  // Errors from getDocCount/onDelete surface here — hosts must NOT swallow
  // them, or a failed delete closes the confirm as if it succeeded.
  interface Props {
    label?: string
    getDocCount: () => Promise<number> | number
    onDelete: (documents: 'keep' | 'delete') => Promise<void> | void
  }

  let { label = 'Delete this record?', getDocCount, onDelete }: Props = $props()

  let confirming = $state(false)
  let docCount = $state(0)
  let busy = $state(false)
  let error = $state('')

  async function open() {
    error = ''
    try {
      docCount = await getDocCount()
      confirming = true
    } catch (e: any) {
      error = e?.message ?? 'Failed to check attached documents'
    }
  }

  async function run(documents: 'keep' | 'delete') {
    busy = true
    error = ''
    try {
      await onDelete(documents)
      confirming = false
    } catch (e: any) {
      error = e?.message ?? 'Delete failed'
    } finally {
      busy = false
    }
  }
</script>

{#if confirming}
  <span class="confirm-text">
    {label}{#if docCount > 0}{' '}It has {docCount} attached document{docCount === 1 ? '' : 's'}.{/if}
  </span>
  {#if docCount > 0}
    <button class="btn btn-danger btn-sm" onclick={() => run('delete')} disabled={busy}>
      {busy ? 'Deleting...' : 'Delete + documents'}
    </button>
    <button class="btn btn-danger btn-sm" onclick={() => run('keep')} disabled={busy}>
      {busy ? 'Deleting...' : 'Delete, keep documents'}
    </button>
  {:else}
    <button class="btn btn-danger btn-sm" onclick={() => run('keep')} disabled={busy}>
      {busy ? 'Deleting...' : 'Yes, Delete'}
    </button>
  {/if}
  <button class="btn btn-secondary btn-sm" onclick={() => (confirming = false)} disabled={busy}>
    Cancel
  </button>
{:else}
  <button class="btn btn-danger-outline btn-sm" onclick={open}>Delete</button>
{/if}
{#if error}
  <span class="confirm-error" role="alert">{error}</span>
{/if}

<style>
  .confirm-text {
    font-size: 0.8rem;
    color: var(--danger);
    font-weight: 500;
  }

  .confirm-error {
    font-size: 0.8rem;
    color: var(--danger);
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
