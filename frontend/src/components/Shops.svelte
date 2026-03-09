<script lang="ts">
  import { onMount } from 'svelte'
  import { shops as shopsApi } from '../lib/api'
  import type { Shop } from '../lib/types'

  let loading = $state(true)
  let error = $state('')
  let success = $state('')
  let shopList: Shop[] = $state([])

  // Form state
  let showForm = $state(false)
  let editingId: number | null = $state(null)
  let saving = $state(false)

  let formName = $state('')
  let formAddress = $state('')
  let formPhone = $state('')
  let formWebsite = $state('')
  let formSpecialty = $state('')
  let formNotes = $state('')

  onMount(async () => {
    try {
      shopList = await shopsApi.list()
    } catch (e: any) {
      error = e.message
    } finally {
      loading = false
    }
  })

  function resetForm() {
    formName = ''
    formAddress = ''
    formPhone = ''
    formWebsite = ''
    formSpecialty = ''
    formNotes = ''
    editingId = null
    showForm = false
  }

  function startAdd() {
    resetForm()
    showForm = true
  }

  function startEdit(shop: Shop) {
    editingId = shop.id
    formName = shop.name
    formAddress = shop.address ?? ''
    formPhone = shop.phone ?? ''
    formWebsite = shop.website ?? ''
    formSpecialty = shop.specialty ?? ''
    formNotes = shop.notes ?? ''
    showForm = true
  }

  async function saveShop() {
    saving = true
    error = ''
    success = ''
    try {
      const data = {
        name: formName.trim(),
        address: formAddress || undefined,
        phone: formPhone || undefined,
        website: formWebsite || undefined,
        specialty: formSpecialty || undefined,
        notes: formNotes || undefined,
      }
      if (editingId) {
        await shopsApi.update(editingId, data)
        success = 'Shop updated.'
      } else {
        await shopsApi.create(data)
        success = 'Shop added.'
      }
      shopList = await shopsApi.list()
      resetForm()
    } catch (e: any) {
      error = e.message
    } finally {
      saving = false
    }
  }

  async function deleteShop(shop: Shop) {
    if (!confirm(`Delete "${shop.name}"? Services referencing this shop will keep their shop name but lose the link.`)) return
    error = ''
    success = ''
    try {
      await shopsApi.delete(shop.id)
      shopList = await shopsApi.list()
      success = 'Shop removed.'
      if (editingId === shop.id) resetForm()
    } catch (e: any) {
      error = e.message
    }
  }
</script>

<div class="shops">
  <div class="shops-header">
    <button class="btn-back" onclick={() => history.back()} aria-label="Go back">
      <svg width="20" height="20" viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <path d="M12 4l-6 6 6 6"/>
      </svg>
    </button>
    <h1>Shops</h1>
  </div>

  {#if loading}
    <div class="form-card">
      <div class="skeleton skeleton-heading" style="width: 40%"></div>
      <div class="field">
        <div class="skeleton skeleton-text-short"></div>
        <div class="skeleton skeleton-text"></div>
      </div>
    </div>
  {:else}
    {#if error}
      <p class="error">{error}</p>
    {/if}

    {#if success}
      <div class="notice notice-success">{success}</div>
    {/if}

    <div class="form-card">
      <div class="section-header">
        <h4>Manage Shops</h4>
        {#if !showForm}
          <button class="btn btn-sm btn-primary" onclick={startAdd}>Add Shop</button>
        {/if}
      </div>

      {#if shopList.length === 0 && !showForm}
        <p class="empty-hint">No shops yet. Add a shop to quickly reference it when logging services.</p>
      {/if}

      {#if shopList.length > 0}
        <div class="shop-list">
          {#each shopList as shop (shop.id)}
            <div class="shop-row">
              <div class="shop-info">
                <div class="shop-name">{shop.name}</div>
                <div class="shop-meta">
                  {#if shop.specialty}
                    <span>{shop.specialty}</span>
                  {/if}
                  {#if shop.address}
                    <span>{shop.address}</span>
                  {/if}
                  {#if shop.phone}
                    <span>{shop.phone}</span>
                  {/if}
                </div>
              </div>
              <div class="shop-actions">
                <button class="btn btn-xs btn-ghost" onclick={() => startEdit(shop)}>Edit</button>
                <button class="btn btn-xs btn-ghost btn-danger" onclick={() => deleteShop(shop)}>Delete</button>
              </div>
            </div>
          {/each}
        </div>
      {/if}

      {#if showForm}
        <form class="shop-form" onsubmit={(e) => { e.preventDefault(); saveShop() }}>
          <h5>{editingId ? 'Edit Shop' : 'Add Shop'}</h5>

          <div class="form-row">
            <div class="field">
              <label for="shop-name">Name</label>
              <input id="shop-name" type="text" bind:value={formName} placeholder="e.g., Main Street Auto" required />
            </div>
            <div class="field">
              <label for="shop-specialty">Specialty</label>
              <input id="shop-specialty" type="text" bind:value={formSpecialty} placeholder="e.g., European, Transmission" />
            </div>
          </div>

          <div class="field">
            <label for="shop-address">Address</label>
            <input id="shop-address" type="text" bind:value={formAddress} placeholder="123 Main St, City, ST 12345" />
          </div>

          <div class="form-row">
            <div class="field">
              <label for="shop-phone">Phone</label>
              <input id="shop-phone" type="tel" bind:value={formPhone} placeholder="(555) 123-4567" />
            </div>
            <div class="field">
              <label for="shop-website">Website</label>
              <input id="shop-website" type="url" bind:value={formWebsite} placeholder="https://..." />
            </div>
          </div>

          <div class="field">
            <label for="shop-notes">Notes</label>
            <textarea id="shop-notes" bind:value={formNotes} rows="2" placeholder="Hours, contact person, etc."></textarea>
          </div>

          <div class="form-actions">
            <button type="submit" class="btn btn-primary" disabled={saving || !formName.trim()}>
              {saving ? 'Saving...' : editingId ? 'Update' : 'Add Shop'}
            </button>
            <button type="button" class="btn btn-secondary" onclick={resetForm}>Cancel</button>
          </div>
        </form>
      {/if}
    </div>
  {/if}
</div>

<style>
  .shops-header {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    margin-bottom: var(--sp-6);
  }

  .shops-header h1 {
    margin: 0;
  }

  .btn-back {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 36px;
    height: 36px;
    padding: 0;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    background: var(--surface);
    color: var(--text-secondary);
    cursor: pointer;
    transition: all var(--duration-fast) var(--ease-out);
  }

  .btn-back:hover {
    background: var(--bg-hover, var(--surface));
    border-color: var(--border);
    color: var(--text-primary);
  }

  .form-card {
    animation: fade-in-down var(--duration-base) var(--ease-out) both;
  }

  .section-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: var(--sp-4);
  }

  .section-header h4 {
    margin: 0;
  }

  .empty-hint {
    color: var(--text-muted);
    font-size: 0.85rem;
    text-align: center;
    padding: var(--sp-6) 0;
  }

  .shop-list {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    margin-bottom: var(--sp-4);
  }

  .shop-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--sp-3);
    padding: var(--sp-3) var(--sp-4);
    background: var(--surface);
    border-radius: var(--radius-md);
    border: 1px solid var(--border-subtle);
  }

  .shop-info {
    flex: 1;
    min-width: 0;
  }

  .shop-name {
    font-weight: 600;
    font-size: 0.9rem;
  }

  .shop-meta {
    font-size: 0.75rem;
    color: var(--text-muted);
    margin-top: 2px;
    display: flex;
    flex-wrap: wrap;
    gap: var(--sp-1);
  }

  .shop-meta span:not(:last-child)::after {
    content: '\00b7';
    margin-left: var(--sp-1);
  }

  .shop-actions {
    display: flex;
    gap: var(--sp-1);
    flex-shrink: 0;
  }

  .btn-xs {
    padding: 2px 8px;
    font-size: 0.7rem;
  }

  .btn-ghost {
    background: transparent;
    border: 1px solid var(--border-subtle);
    color: var(--text-secondary);
  }

  .btn-ghost:hover {
    background: var(--surface);
    border-color: var(--border);
  }

  .btn-ghost.btn-danger {
    color: var(--danger);
  }

  .btn-ghost.btn-danger:hover {
    background: var(--danger-bg);
    border-color: var(--danger);
  }

  .shop-form {
    margin-top: var(--sp-4);
    padding-top: var(--sp-4);
    border-top: 1px solid var(--border-subtle);
    animation: fade-in-down var(--duration-fast) var(--ease-out) both;
  }

  .shop-form h5 {
    margin: 0 0 var(--sp-3) 0;
    font-size: 0.9rem;
  }

  .form-actions {
    margin-top: var(--sp-4);
    padding-top: var(--sp-4);
    border-top: 1px solid var(--border-subtle);
    display: flex;
    gap: var(--sp-2);
  }

  .notice-success {
    padding: var(--sp-3) var(--sp-4);
    background: var(--success-bg);
    border: 1px solid var(--success-border);
    border-radius: var(--radius-md);
    color: var(--success);
    font-size: 0.85rem;
    margin-bottom: var(--sp-4);
    animation: fade-in-down var(--duration-fast) var(--ease-out) both;
  }

  .error {
    color: var(--danger);
    padding: var(--sp-3) var(--sp-4);
    background: var(--danger-bg);
    border: 1px solid var(--danger-border);
    border-radius: var(--radius-md);
    margin-bottom: var(--sp-4);
  }

  @media (max-width: 640px) {
    .shops-header {
      margin-bottom: var(--sp-4);
    }

    .shop-row {
      flex-direction: column;
      align-items: flex-start;
    }

    .shop-actions {
      flex-wrap: wrap;
    }
  }
</style>
