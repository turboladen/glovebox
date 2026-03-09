<script lang="ts">
  import { onMount } from 'svelte'
  import { observations as obsApi, services as servicesApi } from '../lib/api'
  import type { Observation, ServiceRecordWithLinks } from '../lib/types'
  import { formatDate } from '../lib/dates'

  let { vehicleId }: { vehicleId: number } = $props()

  let items: Observation[] = $state([])
  let serviceRecords: ServiceRecordWithLinks[] = $state([])
  let loading = $state(true)
  let showForm = $state(false)
  let resolvingObsId: number | null = $state(null)

  // Form fields
  let category = $state('general')
  let title = $state('')
  let description = $state('')
  let observedAt = $state(new Date().toISOString().slice(0, 10))
  let odometer = $state<number | undefined>()
  let obdCodes = $state('')
  let saving = $state(false)
  let error = $state('')

  const categories = ['general', 'noise', 'warning_light', 'cosmetic', 'performance', 'obd_code']

  onMount(loadData)

  async function loadData() {
    try {
      const [obsList, svcList] = await Promise.all([
        obsApi.list(vehicleId),
        servicesApi.list(vehicleId),
      ])
      items = obsList
      serviceRecords = svcList
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
        observed_at: observedAt || undefined,
        odometer,
        obd_codes: obdCodes || undefined,
      })
      showForm = false
      title = ''; description = ''; odometer = undefined; obdCodes = ''
      observedAt = new Date().toISOString().slice(0, 10)
      await loadData()
    } catch (e: any) {
      error = e.message
    } finally {
      saving = false
    }
  }

  async function resolveWith(obs: Observation, serviceId: number | null) {
    await obsApi.update(vehicleId, obs.id, {
      resolved: true,
      resolved_service_id: serviceId,
    })
    resolvingObsId = null
    await loadData()
  }

  async function unresolve(obs: Observation) {
    await obsApi.update(vehicleId, obs.id, {
      resolved: false,
      resolved_service_id: null,
    })
    await loadData()
  }

  function categoryLabel(c: string): string {
    return c.replace(/_/g, ' ').replace(/\b\w/g, l => l.toUpperCase())
  }

  function parseObdCodes(raw: string): string[] {
    try {
      const parsed = JSON.parse(raw)
      return Array.isArray(parsed) ? parsed : [raw]
    } catch {
      return [raw]
    }
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
            <label for="obs-date">Date Observed</label>
            <input id="obs-date" type="date" bind:value={observedAt} />
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
          <label for="obs-desc">Details</label>
          <textarea id="obs-desc" bind:value={description} rows="3" placeholder="What did you notice? Include any follow-up thoughts or action items..."></textarea>
        </div>
        {#if category === 'obd_code'}
          <div class="field">
            <label for="obs-obd">OBD Codes (JSON array)</label>
            <input id="obs-obd" type="text" bind:value={obdCodes} placeholder='["P0301","P0302"]' />
          </div>
        {/if}
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
          {#if obs.description || obs.notes}
            <p class="obs-desc">{[obs.description, obs.notes].filter(Boolean).join(' — ')}</p>
          {/if}
          {#if obs.odometer}
            <span class="obs-meta">{obs.odometer.toLocaleString()} mi</span>
          {/if}
          {#if obs.obd_codes}
            <div class="obd-codes">
              {#each parseObdCodes(obs.obd_codes) as code}
                <span class="obd-chip">{code}</span>
              {/each}
            </div>
          {/if}
          <div class="obs-actions">
            {#if obs.resolved}
              <span class="resolved-info">
                Resolved
                {#if obs.resolved_service_id}
                  {@const svc = serviceRecords.find(s => s.id === obs.resolved_service_id)}
                  {#if svc}
                    — {svc.description || 'Service'} ({formatDate(svc.service_date)})
                  {/if}
                {/if}
              </span>
              <button class="btn btn-secondary" onclick={() => unresolve(obs)}>
                Unresolve
              </button>
            {:else if resolvingObsId === obs.id}
              <div class="resolve-picker">
                <select onchange={(e) => {
                  const val = (e.target as HTMLSelectElement).value
                  resolveWith(obs, val === '' ? null : parseInt(val))
                }}>
                  <option value="" disabled selected>Link to a service...</option>
                  <option value="">Resolve without service</option>
                  {#each serviceRecords as svc (svc.id)}
                    <option value={svc.id}>
                      {formatDate(svc.service_date)} — {svc.description || 'Service'}
                    </option>
                  {/each}
                </select>
                <button class="btn btn-secondary" onclick={() => (resolvingObsId = null)}>
                  Cancel
                </button>
              </div>
            {:else}
              <button class="btn btn-secondary" onclick={() => (resolvingObsId = obs.id)}>
                Mark Resolved
              </button>
            {/if}
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

  .obd-codes {
    display: flex; flex-wrap: wrap; gap: var(--sp-1); margin-top: var(--sp-1);
  }

  .obd-chip {
    font-family: var(--font-mono, monospace);
    font-size: 0.75rem;
    font-weight: 600;
    padding: 0.1rem 0.5rem;
    border-radius: var(--radius-sm);
    background: var(--warning-bg);
    color: var(--warning);
    letter-spacing: 0.03em;
  }
  .obs-actions { margin-top: var(--sp-2); display: flex; align-items: center; gap: var(--sp-2); }

  .resolved-info {
    font-size: 0.8rem;
    color: var(--success);
    font-weight: 500;
  }

  .resolve-picker {
    display: flex; align-items: center; gap: var(--sp-2); flex-wrap: wrap;
  }

  .resolve-picker select {
    font-size: 0.85rem;
    max-width: 300px;
  }

  .empty { color: var(--text-muted); text-align: center; padding: var(--sp-8) 0; }
</style>
