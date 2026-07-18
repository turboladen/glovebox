<script lang="ts">
  import { onMount } from 'svelte'
  import { querystring, replace } from '@keenmate/svelte-spa-router'
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
  // The file to upload, set by drop OR by the input's change — makes upload()
  // source-agnostic.
  let pendingFile: File | null = $state(null)
  let dragActive = $state(false)
  let uploading = $state(false)
  let error = $state('')

  // Attach-mode (glovebox-alki): MCP's record_service hands the user a deep
  // link `?attach=service:<id>` so their browser can upload a file the
  // sandboxed client can't carry. In this mode the entity is pre-selected and
  // the pickers are hidden — just drop the file.
  let attachMode = $state(false)
  let attachServiceId: number | null = $state(null)
  let attachService: ServiceRecordWithLinks | null = $state(null)

  // Linkable entities
  let serviceRecords: ServiceRecordWithLinks[] = $state([])
  let partsList: Part[] = $state([])
  let incidentsList: IncidentWithDetails[] = $state([])

  const docTypes = ['invoice', 'receipt', 'photo', 'title', 'warranty', 'manual', 'other']

  onMount(loadData)

  // Consume `?attach=service:<id>` (split on ':' like the ?hl= grammar). Guard
  // on attachServiceId so re-runs (querystring is reactive) are no-ops.
  $effect(() => {
    const q = new URLSearchParams(querystring() ?? '')
    const raw = q.get('attach')
    const recordId = raw?.startsWith('service:') ? parseInt(raw.slice('service:'.length), 10) : null
    if (recordId != null && !Number.isNaN(recordId)) {
      if (attachServiceId !== recordId) {
        attachServiceId = recordId
        attachMode = true
        showUpload = true
        linkedEntityType = 'service'
        linkedEntityId = recordId
        servicesApi
          .get(vehicleId, recordId)
          .then((s) => { attachService = s })
          .catch(() => { attachService = null })
      }
    } else if (attachMode) {
      // Param removed (e.g. consumed after a successful upload).
      attachMode = false
      attachServiceId = null
      attachService = null
    }
  })

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

  function onDrop(e: DragEvent) {
    e.preventDefault()
    dragActive = false
    const f = e.dataTransfer?.files?.[0]
    if (f) {
      pendingFile = f
      // In attach-mode the entity is already chosen — drop = upload.
      if (attachMode) upload()
    }
  }

  async function upload() {
    const file = pendingFile ?? fileInput?.files?.[0]
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
      title = ''; notes = ''; pendingFile = null
      if (attachMode) {
        // Consume the ?attach= param (mirrors TimelineTab consuming ?action=).
        attachMode = false
        attachServiceId = null
        attachService = null
        linkedEntityType = ''; linkedEntityId = null
        replace(`/vehicles/${vehicleId}/records/documents`)
      } else {
        linkedEntityType = ''; linkedEntityId = null
      }
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

  async function unlinkDoc(id: number) {
    await docsApi.unlink(id)
    await loadData()
  }

  // A linked document whose target no longer exists (pre-feature deletes left
  // these dangling). Exactly as reliable as the lists this tab already renders
  // labels from.
  function isOrphaned(doc: Document): boolean {
    if (!doc.linked_entity_type || !doc.linked_entity_id) return false
    if (doc.linked_entity_type === 'service') return !serviceRecords.some(s => s.id === doc.linked_entity_id)
    if (doc.linked_entity_type === 'part') return !partsList.some(p => p.id === doc.linked_entity_id)
    if (doc.linked_entity_type === 'incident') return !incidentsList.some(i => i.id === doc.linked_entity_id)
    return false
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
    {#if !attachMode}
      <button class="btn btn-primary" onclick={() => (showUpload = !showUpload)}>
        {showUpload ? 'Cancel' : '+ Upload'}
      </button>
    {/if}
  </div>

  {#if attachMode}
    <div class="attach-banner" data-testid="attach-banner">
      <span class="attach-label">Attaching to</span>
      {#if attachService}
        <span class="attach-target">Service #{attachService.id} — {formatDate(attachService.service_date)}{attachService.description ? ` · ${attachService.description}` : ''}</span>
      {:else}
        <span class="attach-target">Service #{attachServiceId}</span>
      {/if}
    </div>
  {/if}

  {#if showUpload}
    <div class="form-card">
      <form onsubmit={(e) => { e.preventDefault(); upload() }}>
        <div class="field">
          <span class="field-label" id="doc-file-label">File</span>
          <!-- Dropzone (net-new): drag a file onto it, or click to browse. The
               <input> stays as a click fallback (and for setInputFiles). -->
          <label
            class="dropzone"
            class:attach={attachMode}
            class:active={dragActive}
            aria-labelledby="doc-file-label"
            data-testid="dropzone"
            ondragover={(e) => { e.preventDefault(); dragActive = true }}
            ondragenter={(e) => { e.preventDefault(); dragActive = true }}
            ondragleave={() => { dragActive = false }}
            ondrop={onDrop}
          >
            <input
              id="doc-file"
              class="dropzone-input"
              type="file"
              bind:this={fileInput}
              onchange={() => { pendingFile = fileInput?.files?.[0] ?? null }}
            />
            <span class="dropzone-hint">
              {#if pendingFile}
                Selected: <strong>{pendingFile.name}</strong>
              {:else}
                Drag a file here, or click to browse
              {/if}
            </span>
          </label>
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
        {#if !attachMode}
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
        {/if}
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
              <div class="doc-link-badge">
                {linkedEntityLabel(doc)}
                {#if isOrphaned(doc)}
                  <span class="orphan-badge" title="The linked record no longer exists">orphaned</span>
                {/if}
              </div>
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
            {#if doc.linked_entity_type}
              <button class="btn btn-secondary" onclick={() => unlinkDoc(doc.id)}>Unlink</button>
            {/if}
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

  /* Attach-mode banner: the record the dropped file will link to. */
  .attach-banner {
    display: flex; align-items: baseline; gap: var(--sp-2); flex-wrap: wrap;
    padding: var(--sp-2) var(--sp-3); margin-bottom: var(--sp-3);
    border: 1px solid var(--info-border); border-radius: var(--radius-md);
    background: var(--info-bg);
  }
  .attach-label {
    font-family: var(--font-display);
    font-size: 0.7rem; text-transform: uppercase; letter-spacing: 0.08em; font-weight: 600;
    color: var(--info);
  }
  .attach-target { font-weight: 600; font-size: 0.9rem; }

  .field-label {
    font-size: 0.75rem; font-weight: 600; color: var(--text-muted);
    text-transform: uppercase; letter-spacing: 0.03em;
  }

  /* Dropzone — a clear drop target; the file input is visually hidden but
     still the click/keyboard/setInputFiles surface (label wraps it). */
  .dropzone {
    display: flex; align-items: center; justify-content: center;
    padding: var(--sp-4);
    border: 1px dashed var(--border);
    border-radius: var(--radius-md);
    background: var(--bg);
    color: var(--text-muted);
    text-align: center;
    cursor: pointer;
    transition: border-color var(--duration-fast) var(--ease-out),
                background var(--duration-fast) var(--ease-out);
  }
  .dropzone:hover { border-color: var(--text-muted); }
  .dropzone.active {
    border-color: var(--primary);
    background: var(--surface);
    color: var(--text);
  }
  /* Prominent in attach-mode: this IS the task. */
  .dropzone.attach {
    padding: var(--sp-8) var(--sp-4);
    border-width: 2px;
    border-color: var(--info-border);
  }
  .dropzone.attach.active { border-color: var(--primary); }

  @media (prefers-reduced-motion: reduce) {
    .dropzone { transition: none; }
  }

  .dropzone-input {
    position: absolute; width: 1px; height: 1px;
    padding: 0; margin: -1px; overflow: hidden;
    clip: rect(0, 0, 0, 0); white-space: nowrap; border: 0;
  }
  .dropzone-hint { font-size: 0.9rem; }

  .docs-list { display: flex; flex-direction: column; gap: var(--sp-2); }

  .doc-card {
    padding: var(--sp-3) var(--sp-4); border: 1px solid var(--border-subtle); border-radius: var(--radius-lg);
    background: var(--bg-raised);
    box-shadow: inset 0 1px 0 var(--edge-highlight);
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
    background: var(--surface); border: 1px solid var(--border-subtle); border-radius: 999px;
    padding: 0 var(--sp-2); text-transform: uppercase; font-size: 0.7rem; font-weight: 600;
    letter-spacing: 0.07em;
    font-family: var(--font-display);
  }
  .doc-link-badge {
    font-size: 0.75rem;
    color: var(--primary);
    margin-top: var(--sp-1);
  }
  .orphan-badge {
    margin-left: var(--sp-1);
    padding: 0 var(--sp-1);
    font-size: 0.7rem;
    font-weight: 600;
    text-transform: uppercase;
    color: var(--danger);
    border: 1px solid var(--danger);
    border-radius: var(--radius-sm);
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
