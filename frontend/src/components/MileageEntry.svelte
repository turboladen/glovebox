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
  .form-card h3 {
    margin: 0 0 var(--sp-3);
  }

  .error {
    color: var(--danger);
    font-size: 0.85rem;
  }
</style>
