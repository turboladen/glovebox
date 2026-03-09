<script lang="ts">
  import { onMount } from 'svelte'
  import { observations as obsApi } from '../lib/api'
  import type { Observation } from '../lib/types'
  import { formatDate } from '../lib/dates'

  let { vehicleId }: { vehicleId: number } = $props()

  let items: Observation[] = $state([])
  let loading = $state(true)
  let showForm = $state(false)

  // Form fields
  let category = $state('general')
  let title = $state('')
  let description = $state('')
  let odometer = $state<number | undefined>()
  let obdCodes = $state('')
  let notes = $state('')
  let saving = $state(false)
  let error = $state('')

  const categories = ['general', 'noise', 'warning_light', 'cosmetic', 'performance', 'obd_code']

  onMount(loadData)

  async function loadData() {
    try {
      items = await obsApi.list(vehicleId)
    } catch (e) {
      console.error(e)
    } finally {
      loading = false
    }
  }

  async function submit() {
    if (!title.trim()) { error = 'Title is required'; return }
    saving = true
    error = ''
    try {
      await obsApi.create(vehicleId, {
        category,
        title: title.trim(),
        description: description || undefined,
        odometer,
        obd_codes: obdCodes || undefined,
        notes: notes || undefined,
      })
      showForm = false
      title = ''; description = ''; odometer = undefined; obdCodes = ''; notes = ''
      await loadData()
    } catch (e: any) {
      error = e.message
    } finally {
      saving = false
    }
  }

  async function toggleResolved(obs: Observation) {
    await obsApi.update(vehicleId, obs.id, { resolved: !obs.resolved })
    await loadData()
  }

  function categoryLabel(c: string): string {
    return c.replace(/_/g, ' ').replace(/\b\w/g, l => l.toUpperCase())
  }
</script>

<div class="observations">
  <div class="obs-header">
    <h3>Observations</h3>
    <button class="btn btn-primary" onclick={() => (showForm = !showForm)}>
      {showForm ? 'Cancel' : '+ Add Observation'}
    </button>
  </div>

  {#if showForm}
    <div class="form-card">
      <form onsubmit={(e) => { e.preventDefault(); submit() }}>
        <div class="form-row">
          <div class="field">
            <label for="obs-cat">Category</label>
            <select id="obs-cat" bind:value={category}>
              {#each categories as c}
                <option value={c}>{categoryLabel(c)}</option>
              {/each}
            </select>
          </div>
          <div class="field">
            <label for="obs-odometer">Odometer</label>
            <input id="obs-odometer" type="number" bind:value={odometer} min="0" />
          </div>
        </div>
        <div class="field">
          <label for="obs-title">Title</label>
          <input id="obs-title" type="text" bind:value={title} required placeholder="e.g., Rattle on cold start" />
        </div>
        <div class="field">
          <label for="obs-desc">Description</label>
          <textarea id="obs-desc" bind:value={description} rows="2"></textarea>
        </div>
        {#if category === 'obd_code'}
          <div class="field">
            <label for="obs-obd">OBD Codes (JSON array)</label>
            <input id="obs-obd" type="text" bind:value={obdCodes} placeholder='["P0301","P0302"]' />
          </div>
        {/if}
        <div class="field">
          <label for="obs-notes">Notes</label>
          <input id="obs-notes" type="text" bind:value={notes} />
        </div>
        {#if error}
          <p class="error">{error}</p>
        {/if}
        <div class="form-actions">
          <button type="submit" class="btn btn-primary" disabled={saving}>
            {saving ? 'Saving...' : 'Save'}
          </button>
        </div>
      </form>
    </div>
  {/if}

  {#if loading}
    <p>Loading observations...</p>
  {:else if items.length === 0}
    <p class="empty">No observations yet.</p>
  {:else}
    <div class="obs-list">
      {#each items as obs (obs.id)}
        <div class="obs-card" class:resolved={obs.resolved}>
          <div class="obs-card-header">
            <span class="obs-category">{categoryLabel(obs.category)}</span>
            <span class="obs-date">{formatDate(obs.observed_at)}</span>
          </div>
          <div class="obs-title">{obs.title}</div>
          {#if obs.description}
            <p class="obs-desc">{obs.description}</p>
          {/if}
          {#if obs.odometer}
            <span class="obs-meta">{obs.odometer.toLocaleString()} mi</span>
          {/if}
          {#if obs.obd_codes}
            <span class="obs-meta obd">{obs.obd_codes}</span>
          {/if}
          <div class="obs-actions">
            <button class="btn btn-secondary" onclick={() => toggleResolved(obs)}>
              {obs.resolved ? 'Unresolve' : 'Mark Resolved'}
            </button>
          </div>
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .obs-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: var(--sp-4);
  }

  .obs-header h3 { margin: 0; }

  .error { color: var(--danger); font-size: 0.85rem; }

  .obs-list { display: flex; flex-direction: column; gap: var(--sp-2); }

  .obs-card {
    padding: var(--sp-3) var(--sp-4);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    background: var(--bg-raised);
    transition: border-color var(--duration-base) var(--ease-out);
  }

  .obs-card:hover {
    border-color: var(--border);
  }

  .obs-card.resolved { opacity: 0.6; }

  .obs-card-header {
    display: flex; justify-content: space-between; align-items: center;
    margin-bottom: var(--sp-1);
  }

  .obs-category {
    font-family: var(--font-display);
    font-size: 0.75rem; text-transform: uppercase; letter-spacing: 0.05em;
    color: var(--primary); font-weight: 500;
  }

  .obs-date { font-size: 0.85rem; color: var(--text-muted); }
  .obs-title { font-weight: 600; }
  .obs-desc { font-size: 0.85rem; color: var(--text-muted); margin: var(--sp-1) 0; }
  .obs-meta { font-size: 0.8rem; color: var(--text-muted); margin-right: var(--sp-2); }
  .obs-meta.obd { font-family: var(--font-mono); }
  .obs-actions { margin-top: var(--sp-2); }
  .empty { color: var(--text-muted); text-align: center; padding: var(--sp-8) 0; }
</style>
