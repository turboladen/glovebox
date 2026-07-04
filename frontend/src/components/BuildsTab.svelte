<script lang="ts">
  // Builds tab (unit F): the first UI over the existing builds HTTP
  // surface — list + create/edit, lifecycle transitions, and a live
  // progress detail (linked services/parts/incidents + spend), with
  // linked records deep-linking into Timeline / Records.
  import { onMount } from 'svelte'
  import { push } from '@keenmate/svelte-spa-router'
  import { builds as buildsApi } from '../lib/api'
  import { formatCents } from '../lib/money'
  import type { Build, BuildProgress } from '../lib/types'
  import { formatDate } from '../lib/dates'
  import { refreshDashboard } from '../lib/stores'

  let { vehicleId }: { vehicleId: number } = $props()

  const STATUSES = ['planned', 'active', 'on_hold', 'completed', 'abandoned']

  let buildList: Build[] = $state([])
  let progressMap: Map<number, BuildProgress> = $state(new Map())
  let expandedId: number | null = $state(null)
  let loading = $state(true)
  let error = $state('')

  // Create/edit form
  let showForm = $state(false)
  let editingId: number | null = $state(null)
  let name = $state('')
  let description = $state('')
  let targetDate = $state('')
  let saving = $state(false)

  async function loadData() {
    try {
      buildList = await buildsApi.list(vehicleId)
    } catch (e: any) {
      error = e.message
    } finally {
      loading = false
    }
  }

  onMount(loadData)

  async function refresh() {
    progressMap = new Map()
    await loadData()
    if (expandedId != null) await loadProgress(expandedId)
    refreshDashboard().catch(() => {})
  }

  async function loadProgress(id: number) {
    try {
      const p = await buildsApi.get(vehicleId, id)
      progressMap.set(id, p)
      progressMap = new Map(progressMap)
    } catch (e: any) {
      error = e.message
    }
  }

  async function toggleExpand(id: number) {
    if (expandedId === id) {
      expandedId = null
      return
    }
    expandedId = id
    if (!progressMap.has(id)) await loadProgress(id)
  }

  function startAdd() {
    editingId = null
    name = ''
    description = ''
    targetDate = ''
    error = ''
    showForm = true
  }

  function startEdit(b: Build) {
    editingId = b.id
    name = b.name
    description = b.description ?? ''
    targetDate = b.target_date ?? ''
    error = ''
    showForm = true
  }

  async function submit() {
    if (!name.trim()) {
      error = 'Name is required'
      return
    }
    saving = true
    error = ''
    try {
      if (editingId) {
        // Edit clears send explicit null (double-option update).
        await buildsApi.update(vehicleId, editingId, {
          name: name.trim(),
          description: description || null,
          target_date: targetDate || null,
        })
      } else {
        await buildsApi.create(vehicleId, {
          name: name.trim(),
          description: description || undefined,
          target_date: targetDate || undefined,
        })
      }
      showForm = false
      await refresh()
    } catch (e: any) {
      error = e.message
    } finally {
      saving = false
    }
  }

  async function setStatus(b: Build, status: string) {
    if (status === b.status) return
    error = ''
    try {
      await buildsApi.update(vehicleId, b.id, { status })
      await refresh()
    } catch (e: any) {
      error = e.message
    }
  }

  async function deleteBuild(b: Build) {
    if (!confirm(`Delete build "${b.name}"? Linked records stay; their build link clears.`)) return
    error = ''
    try {
      await buildsApi.delete(vehicleId, b.id)
      expandedId = null
      await refresh()
    } catch (e: any) {
      error = e.message
    }
  }

  function statusLabel(s: string): string {
    return s.replace(/_/g, ' ')
  }

  const fmt = formatCents
</script>

<div class="builds-tab">
  <div class="tab-header">
    <h3>Builds</h3>
    <button class="btn btn-primary" onclick={() => (showForm ? (showForm = false) : startAdd())}>
      {showForm ? 'Cancel' : '+ New build'}
    </button>
  </div>

  {#if error}
    <p class="error">{error}</p>
  {/if}

  {#if showForm}
    <div class="form-card">
      <form onsubmit={(e) => { e.preventDefault(); submit() }}>
        <div class="form-row">
          <div class="field grow">
            <label for="b-name">Name</label>
            <input id="b-name" type="text" bind:value={name} required placeholder="e.g., Turbo swap" />
          </div>
          <div class="field">
            <label for="b-target">Target date</label>
            <input id="b-target" type="date" bind:value={targetDate} />
          </div>
        </div>
        <div class="field">
          <label for="b-desc">Description</label>
          <textarea id="b-desc" bind:value={description} rows="2" placeholder="What's the goal?"></textarea>
        </div>
        <div class="form-actions">
          <button type="button" class="btn btn-secondary" onclick={() => (showForm = false)} disabled={saving}>Cancel</button>
          <button type="submit" class="btn btn-primary" disabled={saving}>
            {saving ? 'Saving…' : editingId ? 'Update' : 'Create build'}
          </button>
        </div>
      </form>
    </div>
  {/if}

  {#if loading}
    <p>Loading builds…</p>
  {:else if buildList.length === 0}
    <p class="empty">No builds yet. A build is a one-shot project — a turbo upgrade, an engine swap, a restoration — that links services, parts, and incidents.</p>
  {:else}
    <div class="build-list" data-testid="build-list">
      {#each buildList as b (b.id)}
        {@const progress = progressMap.get(b.id)}
        <div class="build-card" class:expanded={expandedId === b.id}>
          <div
            class="build-header"
            role="button"
            tabindex="0"
            onclick={() => toggleExpand(b.id)}
            onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); toggleExpand(b.id) } }}
          >
            <span class="status-badge status-{b.status}">{statusLabel(b.status)}</span>
            <strong class="build-name">{b.name}</strong>
            {#if b.target_date}
              <span class="build-target">target {formatDate(b.target_date)}</span>
            {/if}
            <span class="expand-icon">{expandedId === b.id ? '▲' : '▼'}</span>
          </div>
          {#if b.description}
            <p class="build-desc">{b.description}</p>
          {/if}

          {#if expandedId === b.id}
            <div class="build-detail">
              {#if !progress}
                <p>Loading progress…</p>
              {:else}
                <div class="progress-grid">
                  <div class="progress-stat">
                    <span class="stat-label">Parts installed</span>
                    <span class="stat-value">{progress.parts_installed}/{progress.parts_total}</span>
                  </div>
                  <div class="progress-stat">
                    <span class="stat-label">Services</span>
                    <span class="stat-value">{progress.services_count}</span>
                  </div>
                  <div class="progress-stat">
                    <span class="stat-label">Incidents</span>
                    <span class="stat-value">{progress.incidents_count}</span>
                  </div>
                  <div class="progress-stat">
                    <span class="stat-label">Total spend</span>
                    <span class="stat-value">{fmt(progress.total_cost_cents)}</span>
                  </div>
                  <div class="progress-stat">
                    <span class="stat-label">Out of pocket</span>
                    <span class="stat-value">{fmt(progress.out_of_pocket_cents)}</span>
                  </div>
                </div>

                <div class="linked-links">
                  {#if progress.linked.service_record_ids.length > 0}
                    <button class="action-link" onclick={() => push(`/vehicles/${vehicleId}/timeline`)}>
                      {progress.linked.service_record_ids.length} linked service{progress.linked.service_record_ids.length !== 1 ? 's' : ''} →
                    </button>
                  {/if}
                  {#if progress.linked.part_ids.length > 0}
                    <button class="action-link" onclick={() => push(`/vehicles/${vehicleId}/records/parts`)}>
                      {progress.linked.part_ids.length} linked part{progress.linked.part_ids.length !== 1 ? 's' : ''} →
                    </button>
                  {/if}
                  {#if progress.linked.incident_ids.length > 0}
                    <button class="action-link" onclick={() => push(`/vehicles/${vehicleId}/timeline`)}>
                      {progress.linked.incident_ids.length} linked incident{progress.linked.incident_ids.length !== 1 ? 's' : ''} →
                    </button>
                  {/if}
                </div>
              {/if}

              <div class="build-actions">
                <label class="status-select-label">
                  Status
                  <select value={b.status} onchange={(e) => setStatus(b, (e.target as HTMLSelectElement).value)}>
                    {#each STATUSES as s}
                      <option value={s}>{statusLabel(s)}</option>
                    {/each}
                  </select>
                </label>
                <span class="spacer"></span>
                <button class="action-link" onclick={() => startEdit(b)}>Edit</button>
                <button class="action-link delete" onclick={() => deleteBuild(b)}>Delete</button>
              </div>
              {#if b.completed_at}
                <p class="completed-line">Completed {formatDate(b.completed_at)}</p>
              {/if}
            </div>
          {/if}
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .tab-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: var(--sp-4);
  }

  .tab-header h3 {
    margin: 0;
  }

  .build-list {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
  }

  .build-card {
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-lg);
    background: var(--bg-raised);
    padding: var(--sp-3) var(--sp-4);
    box-shadow: inset 0 1px 0 var(--edge-highlight);
    transition: border-color var(--duration-base) var(--ease-out);
  }

  .build-card:hover {
    border-color: var(--border);
  }

  .build-card.expanded {
    border-color: var(--primary);
  }

  .build-header {
    display: flex;
    align-items: baseline;
    gap: var(--sp-2);
    cursor: pointer;
    flex-wrap: wrap;
  }

  .build-name {
    flex: 1;
    font-family: var(--font-display);
  }

  .build-target {
    font-size: 0.8rem;
    color: var(--text-muted);
  }

  .build-desc {
    font-size: 0.85rem;
    color: var(--text-muted);
    margin: var(--sp-1) 0 0;
  }

  .expand-icon {
    font-size: 0.7rem;
    color: var(--text-muted);
  }

  .status-badge {
    font-family: var(--font-display);
    font-size: 0.68rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    padding: 0.1rem 0.5rem;
    border-radius: 999px;
  }

  .status-planned { background: var(--planned-bg); color: var(--planned); border: 1px solid var(--planned-border); }
  .status-active { background: var(--success-bg); color: var(--success); border: 1px solid var(--success-border); }
  .status-on_hold { background: var(--warning-bg); color: var(--warning); border: 1px solid var(--warning-border); }
  .status-completed { background: var(--surface); color: var(--text-secondary); border: 1px solid var(--border-subtle); }
  .status-abandoned { background: var(--surface); color: var(--text-muted); border: 1px solid var(--border-subtle); }

  .build-detail {
    margin-top: var(--sp-3);
    padding-top: var(--sp-3);
    border-top: 1px solid var(--border-subtle);
  }

  .progress-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(120px, 1fr));
    gap: var(--sp-3);
    margin-bottom: var(--sp-3);
  }

  .progress-stat {
    display: flex;
    flex-direction: column;
  }

  .stat-label {
    font-family: var(--font-display);
    font-size: 0.72rem;
    font-weight: 600;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.1em;
  }

  .stat-value {
    font-family: var(--font-numeral);
    font-variant-numeric: tabular-nums;
    font-size: 1.1rem;
    font-weight: 700;
  }

  .linked-links {
    display: flex;
    gap: var(--sp-4);
    flex-wrap: wrap;
    margin-bottom: var(--sp-3);
  }

  .build-actions {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    border-top: 1px solid var(--border-subtle);
    padding-top: var(--sp-3);
  }

  .status-select-label {
    display: inline-flex;
    align-items: center;
    gap: var(--sp-2);
    font-size: 0.8rem;
    color: var(--text-muted);
    margin: 0;
  }

  .status-select-label select {
    width: auto;
    font-size: 0.85rem;
    padding: var(--sp-1) var(--sp-2);
  }

  .spacer {
    flex: 1;
  }

  .completed-line {
    font-size: 0.8rem;
    color: var(--success);
    margin: var(--sp-2) 0 0;
  }

  .action-link {
    padding: 0;
    border: none;
    background: none;
    font-size: 0.8rem;
    color: var(--primary);
    cursor: pointer;
    font-weight: 500;
  }

  .action-link:hover {
    text-decoration: underline;
  }

  .action-link.delete {
    color: var(--danger);
  }

  .form-actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--sp-2);
  }

  .field.grow {
    flex: 2;
  }

  .error {
    color: var(--danger);
    font-size: 0.85rem;
  }

  .empty {
    color: var(--text-muted);
    text-align: center;
    padding: var(--sp-8) 0;
  }
</style>
