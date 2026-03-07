<script lang="ts">
  import { mileage as mileageApi } from '../lib/api'

  let { vehicleId, onComplete, onCancel }: {
    vehicleId: number
    onComplete: () => void
    onCancel: () => void
  } = $props()

  let odometer = $state(0)
  let notes = $state('')
  let saving = $state(false)
  let error = $state('')

  async function submit() {
    if (odometer <= 0) {
      error = 'Odometer must be greater than 0'
      return
    }
    saving = true
    error = ''
    try {
      await mileageApi.create(vehicleId, {
        mileage: odometer,
        notes: notes || undefined,
      })
      onComplete()
    } catch (e: any) {
      error = e.message
    } finally {
      saving = false
    }
  }
</script>

<div class="form-card">
  <h3>Update Mileage</h3>
  <form onsubmit={(e) => { e.preventDefault(); submit() }}>
    <div class="field">
      <label for="odometer">Current Odometer</label>
      <input id="odometer" type="number" bind:value={odometer} min="1" required />
    </div>
    <div class="field">
      <label for="notes">Notes (optional)</label>
      <input id="notes" type="text" bind:value={notes} />
    </div>
    {#if error}
      <p class="error">{error}</p>
    {/if}
    <div class="form-actions">
      <button type="button" class="btn btn-secondary" onclick={onCancel}>Cancel</button>
      <button type="submit" class="btn btn-primary" disabled={saving}>
        {saving ? 'Saving...' : 'Save'}
      </button>
    </div>
  </form>
</div>

<style>
  .form-card {
    padding: 1rem;
    border: 1px solid var(--border);
    border-radius: 8px;
    margin-bottom: 1rem;
    background: var(--surface);
  }

  .form-card h3 {
    margin: 0 0 0.75rem;
  }

  .field {
    margin-bottom: 0.75rem;
  }

  .field label {
    display: block;
    font-size: 0.85rem;
    margin-bottom: 0.25rem;
    color: var(--text-muted);
  }

  .field input {
    width: 100%;
    padding: 0.4rem 0.6rem;
    border: 1px solid var(--border);
    border-radius: 4px;
    font-size: 0.9rem;
    background: var(--bg);
    color: var(--text);
  }

  .form-actions {
    display: flex;
    gap: 0.5rem;
    justify-content: flex-end;
  }

  .error {
    color: var(--danger);
    font-size: 0.85rem;
  }
</style>
