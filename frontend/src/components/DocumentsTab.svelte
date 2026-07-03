<script lang="ts">
  import { onMount } from 'svelte'
  import { documents as docsApi, services as servicesApi, parts as partsApi, incidents as incidentsApi } from '../lib/api'
  import type { Document, ServiceRecordWithLinks, Part, IncidentWithDetails } from '../lib/types'
  import { formatDate } from '../lib/dates'

  let { vehicleId }: { vehicleId: number } = $props()

  let docs: Document[] = $state([])
  let loading = $state(true)
  let showUpload = $state(false)

  // Upload form
  let title = $state('')
  let docType = $state('other')
  let notes = $state('')
  let linkedEntityType = $state('')
  let linkedEntityId: number | null = $state(null)
  let fileInput: HTMLInputElement | undefined = $state(undefined)
  let uploading = $state(false)
  let error = $state('')

  // Linkable entities
  let serviceRecords: ServiceRecordWithLinks[] = $state([])
  let partsList: Part[] = $state([])
  let incidentsList: IncidentWithDetails[] = $state([])

  const docTypes = ['invoice', 'receipt', 'photo', 'title', 'warranty', 'manual', 'other']

  onMount(loadData)

  async function loadData() {
    try {
      ;[docs, serviceRecords, partsList, incidentsList] = await Promise.all([
        docsApi.list({ vehicle_id: vehicleId }),
        servicesApi.list(vehicleId),
        partsApi.list(vehicleId),
        incidentsApi.list(vehicleId),
      ])
    } catch (e) {
      console.error(e)
    } finally {
      loading = false
    }
  }

  async function upload() {
    const file = fileInput?.files?.[0]
    if (!file) { error = 'Select a file'; return }
    uploading = true
    error = ''
    try {
      const formData = new FormData()
      formData.append('file', file)
      formData.append('vehicle_id', String(vehicleId))
      formData.append('title', title || file.name)
      formData.append('doc_type', docType)
      if (notes) formData.append('notes', notes)
      if (linkedEntityType && linkedEntityId) {
        formData.append('linked_entity_type', linkedEntityType)
        formData.append('linked_entity_id', String(linkedEntityId))
      }
      await docsApi.upload(formData)
      showUpload = false
      title = ''; notes = ''; linkedEntityType = ''; linkedEntityId = null
      await loadData()
    } catch (e: any) {
      error = e.message
    } finally {
      uploading = false
    }
  }

  async function deleteDoc(id: number) {
    await docsApi.delete(id)
    await loadData()
  }

  function formatSize(bytes: number | null): string {
    if (bytes == null) return ''
    if (bytes < 1024) return `${bytes} B`
    if (bytes < 1048576) return `${(bytes / 1024).toFixed(1)} KB`
    return `${(bytes / 1048576).toFixed(1)} MB`
  }

  function isImage(mime: string | null): boolean {
    return !!mime && mime.startsWith('image/')
  }

  function linkedEntityLabel(doc: Document): string {
    if (!doc.linked_entity_type || !doc.linked_entity_id) return ''
    if (doc.linked_entity_type === 'service') {
      const svc = serviceRecords.find(s => s.id === doc.linked_entity_id)
      return svc ? `Service: ${svc.service_date}${svc.description ? ' — ' + svc.description : ''}` : `Service #${doc.linked_entity_id}`
    }
    if (doc.linked_entity_type === 'part') {
      const part = partsList.find(p => p.id === doc.linked_entity_id)
      return part ? `Part: ${part.name}` : `Part #${doc.linked_entity_id}`
    }
    if (doc.linked_entity_type === 'incident') {
      const inc = incidentsList.find(i => i.id === doc.linked_entity_id)
      return inc ? `Incident: ${inc.occurred_at.split('T')[0].split(' ')[0]} — ${inc.title.slice(0, 40)}` : `Incident #${doc.linked_entity_id}`
    }
    return `${doc.linked_entity_type} #${doc.linked_entity_id}`
  }
</script>

<div class="documents">
  <div class="docs-header">
    <h3>Documents</h3>
    <button class="btn btn-primary" onclick={() => (showUpload = !showUpload)}>
      {showUpload ? 'Cancel' : '+ Upload'}
    </button>
  </div>

  {#if showUpload}
    <div class="form-card">
      <form onsubmit={(e) => { e.preventDefault(); upload() }}>
        <div class="field">
          <label for="doc-file">File</label>
          <input id="doc-file" type="file" bind:this={fileInput} />
        </div>
        <div class="form-row">
          <div class="field">
            <label for="doc-title">Title</label>
            <input id="doc-title" type="text" bind:value={title} placeholder="Optional — defaults to filename" />
          </div>
          <div class="field">
            <label for="doc-type">Type</label>
            <select id="doc-type" bind:value={docType}>
              {#each docTypes as dt}
                <option value={dt}>{dt.charAt(0).toUpperCase() + dt.slice(1)}</option>
              {/each}
            </select>
          </div>
        </div>
        <div class="form-row">
          <div class="field">
            <label for="doc-link-type">Link to</label>
            <select id="doc-link-type" bind:value={linkedEntityType} onchange={() => { linkedEntityId = null }}>
              <option value="">None</option>
              <option value="service">Service Record</option>
              <option value="part">Part</option>
              <option value="incident">Incident</option>
            </select>
          </div>
          {#if linkedEntityType === 'service'}
            <div class="field">
              <label for="doc-link-id">Service</label>
              <select id="doc-link-id" bind:value={linkedEntityId}>
                <option value={null}>-- Select --</option>
                {#each serviceRecords as svc (svc.id)}
                  <option value={svc.id}>{svc.service_date}{svc.description ? ` — ${svc.description}` : ''}</option>
                {/each}
              </select>
            </div>
          {:else if linkedEntityType === 'part'}
            <div class="field">
              <label for="doc-link-id">Part</label>
              <select id="doc-link-id" bind:value={linkedEntityId}>
                <option value={null}>-- Select --</option>
                {#each partsList as p (p.id)}
                  <option value={p.id}>{p.name}{p.manufacturer ? ` (${p.manufacturer})` : ''}</option>
                {/each}
              </select>
            </div>
          {:else if linkedEntityType === 'incident'}
            <div class="field">
              <label for="doc-link-id">Incident</label>
              <select id="doc-link-id" bind:value={linkedEntityId}>
                <option value={null}>-- Select --</option>
                {#each incidentsList as inc (inc.id)}
                  <option value={inc.id}>{inc.occurred_at.split('T')[0].split(' ')[0]} — {inc.title.slice(0, 50)}</option>
                {/each}
              </select>
            </div>
          {/if}
        </div>
        <div class="field">
          <label for="doc-notes">Notes</label>
          <input id="doc-notes" type="text" bind:value={notes} />
        </div>
        {#if error}
          <p class="error">{error}</p>
        {/if}
        <div class="form-actions">
          <button type="submit" class="btn btn-primary" disabled={uploading}>
            {uploading ? 'Uploading...' : 'Upload'}
          </button>
        </div>
      </form>
    </div>
  {/if}

  {#if loading}
    <p>Loading documents...</p>
  {:else if docs.length === 0}
    <p class="empty">No documents yet.</p>
  {:else}
    <div class="docs-list">
      {#each docs as doc (doc.id)}
        <div class="doc-card">
          {#if isImage(doc.mime_type)}
            <div class="doc-preview">
              <img src="/files/{doc.file_path}" alt={doc.title} />
            </div>
          {/if}
          <div class="doc-info">
            <div class="doc-title">{doc.title}</div>
            <div class="doc-meta">
              <span class="doc-type-badge">{doc.doc_type ?? 'other'}</span>
              <span>{doc.file_name}</span>
              {#if doc.file_size_bytes}
                <span>{formatSize(doc.file_size_bytes)}</span>
              {/if}
              <span>{formatDate(doc.created_at)}</span>
            </div>
            {#if doc.linked_entity_type}
              <div class="doc-link-badge">{linkedEntityLabel(doc)}</div>
            {/if}
            {#if doc.notes}
              <p class="doc-notes">{doc.notes}</p>
            {/if}
            {#if doc.extracted_text}
              <details class="doc-extracted-text">
                <summary>Extracted Text</summary>
                <pre>{doc.extracted_text}</pre>
              </details>
            {/if}
          </div>
          <div class="doc-actions">
            <a href="/files/{doc.file_path}" target="_blank" class="btn btn-secondary">View</a>
            <button class="btn btn-secondary" onclick={() => deleteDoc(doc.id)}>Delete</button>
          </div>
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .docs-header {
    display: flex; justify-content: space-between; align-items: center; margin-bottom: var(--sp-4);
  }
  .docs-header h3 { margin: 0; }

  .error { color: var(--danger); font-size: 0.85rem; }

  .docs-list { display: flex; flex-direction: column; gap: var(--sp-2); }

  .doc-card {
    padding: var(--sp-3) var(--sp-4); border: 1px solid var(--border-subtle); border-radius: var(--radius-md);
    background: var(--bg-raised);
    display: flex; align-items: center; gap: var(--sp-4);
    transition: border-color var(--duration-base) var(--ease-out);
  }

  .doc-card:hover {
    border-color: var(--border);
  }

  .doc-preview { width: 60px; height: 60px; flex-shrink: 0; }
  .doc-preview img { width: 100%; height: 100%; object-fit: cover; border-radius: var(--radius-sm); }

  .doc-info { flex: 1; }
  .doc-title { font-weight: 600; }
  .doc-meta { font-size: 0.8rem; color: var(--text-muted); display: flex; gap: var(--sp-2); flex-wrap: wrap; }
  .doc-type-badge {
    background: var(--surface); border: 1px solid var(--border-subtle); border-radius: var(--radius-sm);
    padding: 0 var(--sp-1); text-transform: uppercase; font-size: 0.7rem; font-weight: 500;
    font-family: var(--font-display);
  }
  .doc-link-badge {
    font-size: 0.75rem;
    color: var(--primary);
    margin-top: var(--sp-1);
  }
  .doc-notes { font-size: 0.85rem; color: var(--text-muted); margin: var(--sp-1) 0 0; }
  .doc-extracted-text { margin-top: var(--sp-2); font-size: 0.8rem; }
  .doc-extracted-text summary {
    cursor: pointer; color: var(--text-muted); font-weight: 500;
    user-select: none;
  }
  .doc-extracted-text pre {
    margin: var(--sp-2) 0 0; padding: var(--sp-3); background: var(--surface);
    border: 1px solid var(--border-subtle); border-radius: var(--radius-sm);
    white-space: pre-wrap; word-break: break-word; font-size: 0.8rem;
    max-height: 300px; overflow-y: auto;
  }

  .doc-actions { display: flex; gap: var(--sp-2); flex-shrink: 0; }

  .empty { color: var(--text-muted); text-align: center; padding: var(--sp-8) 0; }
</style>
