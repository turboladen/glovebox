<script lang="ts">
  import { onMount } from 'svelte'
  import { documents as docsApi, ai, services as servicesApi, parts as partsApi, accidents as accidentsApi } from '../lib/api'
  import type { Document, ParsedInvoice, ServiceRecordWithLinks, Part, AccidentWithDetails } from '../lib/types'
  import { formatDate } from '../lib/dates'
  import AiProviderSelect from './AiProviderSelect.svelte'

  let { vehicleId, onAnalyzeWithAI }: { vehicleId: number; onAnalyzeWithAI?: (docId: number, docTitle: string) => void } = $props()
  let selectedProviderId: number | undefined = $state(undefined)

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
  let accidentsList: AccidentWithDetails[] = $state([])
  let parsing = $state<number | null>(null)
  let parsedInvoice: ParsedInvoice | null = $state(null)
  let parseError = $state('')

  const docTypes = ['invoice', 'receipt', 'photo', 'title', 'warranty', 'manual', 'other']

  onMount(loadData)

  async function loadData() {
    try {
      ;[docs, serviceRecords, partsList, accidentsList] = await Promise.all([
        docsApi.list({ vehicle_id: vehicleId }),
        servicesApi.list(vehicleId),
        partsApi.list(vehicleId),
        accidentsApi.list(vehicleId),
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

  function isPdf(mime: string | null): boolean {
    return !!mime && mime.includes('pdf')
  }

  async function parseInvoice(docId: number) {
    parsing = docId
    parseError = ''
    parsedInvoice = null
    try {
      parsedInvoice = await ai.parseInvoice(docId, selectedProviderId)
      // Refresh docs to pick up the persisted extracted_text
      docs = await docsApi.list({ vehicle_id: vehicleId })
    } catch (e: any) {
      parseError = e.message
    } finally {
      parsing = null
    }
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
    if (doc.linked_entity_type === 'accident') {
      const acc = accidentsList.find(a => a.id === doc.linked_entity_id)
      return acc ? `Accident: ${acc.occurred_at.split('T')[0].split(' ')[0]}${acc.description ? ' — ' + acc.description.slice(0, 40) : ''}` : `Accident #${doc.linked_entity_id}`
    }
    return `${doc.linked_entity_type} #${doc.linked_entity_id}`
  }

  let creatingService = $state(false)
  let serviceCreated = $state(false)

  async function createServiceFromInvoice() {
    if (!parsedInvoice) return
    creatingService = true
    try {
      await servicesApi.create(vehicleId, {
        service_date: parsedInvoice.service_date || new Date().toISOString().split('T')[0],
        mileage: parsedInvoice.mileage ?? undefined,
        description: parsedInvoice.description ?? undefined,
        parts_cost_cents: parsedInvoice.parts_cost_cents ?? undefined,
        labor_cost_cents: parsedInvoice.labor_cost_cents ?? undefined,
        total_cost_cents: parsedInvoice.total_cost_cents ?? undefined,
        shop_name: parsedInvoice.shop_name ?? undefined,
        notes: parsedInvoice.notes ?? undefined,
        line_items: parsedInvoice.line_items.length > 0
          ? parsedInvoice.line_items.map(li => ({
              description: li.description,
              category: li.category ?? undefined,
              quantity: li.quantity ?? undefined,
              unit_cost_cents: li.unit_cost_cents ?? undefined,
              cost_cents: li.cost_cents ?? undefined,
            }))
          : undefined,
      })
      serviceCreated = true
    } catch (e: any) {
      parseError = e.message
    } finally {
      creatingService = false
    }
  }

  function formatCents(cents: number | null): string {
    if (cents == null) return ''
    return `$${(cents / 100).toFixed(2)}`
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
              <option value="accident">Accident</option>
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
          {:else if linkedEntityType === 'accident'}
            <div class="field">
              <label for="doc-link-id">Accident</label>
              <select id="doc-link-id" bind:value={linkedEntityId}>
                <option value={null}>-- Select --</option>
                {#each accidentsList as acc (acc.id)}
                  <option value={acc.id}>{acc.occurred_at.split('T')[0].split(' ')[0]} — {acc.description.slice(0, 50)}</option>
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
    {#if docs.some(d => isPdf(d.mime_type))}
      <div class="ai-provider-row">
        <span class="ai-provider-label">AI Provider:</span>
        <AiProviderSelect bind:selectedProviderId />
      </div>
    {/if}
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
            {#if isPdf(doc.mime_type)}
              <button class="btn btn-secondary" onclick={() => parseInvoice(doc.id)} disabled={parsing === doc.id}>
                {parsing === doc.id ? 'Parsing...' : 'Parse with AI'}
              </button>
            {/if}
            {#if onAnalyzeWithAI}
              <button class="btn btn-secondary" onclick={() => onAnalyzeWithAI(doc.id, doc.title)}>
                Analyze in Chat
              </button>
            {/if}
            <button class="btn btn-secondary" onclick={() => deleteDoc(doc.id)}>Delete</button>
          </div>
        </div>
      {/each}
    </div>
  {/if}

  {#if parseError}
    <div class="parse-error">
      <p>Failed to parse invoice: {parseError}</p>
      <button class="btn btn-secondary" onclick={() => (parseError = '')}>Dismiss</button>
    </div>
  {/if}

  {#if parsedInvoice}
    <div class="parsed-result">
      <h4>Parsed Invoice Data</h4>
      <div class="parsed-fields">
        {#if parsedInvoice.service_date}
          <div class="parsed-field"><strong>Date:</strong> {formatDate(parsedInvoice.service_date)}</div>
        {/if}
        {#if parsedInvoice.shop_name}
          <div class="parsed-field"><strong>Shop:</strong> {parsedInvoice.shop_name}</div>
        {/if}
        {#if parsedInvoice.mileage}
          <div class="parsed-field"><strong>Mileage:</strong> {parsedInvoice.mileage.toLocaleString()}</div>
        {/if}
        {#if parsedInvoice.description}
          <div class="parsed-field"><strong>Description:</strong> {parsedInvoice.description}</div>
        {/if}
        {#if parsedInvoice.line_items.length > 0}
          <div class="parsed-field">
            <strong>Line Items:</strong>
            <ul>
              {#each parsedInvoice.line_items as item}
                <li>
                  {#if item.category}<span class="line-item-cat-tag">{item.category}</span>{/if}
                  {item.description}
                  {#if item.quantity != null} (x{item.quantity}){/if}
                  {#if item.unit_cost_cents != null} @ {formatCents(item.unit_cost_cents)} ea{/if}
                  {#if item.cost_cents != null} — {formatCents(item.cost_cents)}{/if}
                </li>
              {/each}
            </ul>
          </div>
        {/if}
        {#if parsedInvoice.total_cost_cents}
          <div class="parsed-field"><strong>Total:</strong> {formatCents(parsedInvoice.total_cost_cents)}</div>
        {/if}
        {#if parsedInvoice.notes}
          <div class="parsed-field"><strong>Notes:</strong> {parsedInvoice.notes}</div>
        {/if}
      </div>
      <div class="parsed-actions">
        {#if serviceCreated}
          <span class="service-created-notice">Service record created! View it in the History tab.</span>
        {:else}
          <button class="btn btn-primary" onclick={createServiceFromInvoice} disabled={creatingService}>
            {creatingService ? 'Creating...' : 'Create Service Record'}
          </button>
        {/if}
        <button class="btn btn-secondary" onclick={() => { parsedInvoice = null; serviceCreated = false }}>Dismiss</button>
      </div>
    </div>
  {/if}
</div>

<style>
  .docs-header {
    display: flex; justify-content: space-between; align-items: center; margin-bottom: var(--sp-4);
  }
  .docs-header h3 { margin: 0; }

  .error { color: var(--danger); font-size: 0.85rem; }

  .ai-provider-row {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    margin-bottom: var(--sp-3);
    font-size: 0.8rem;
    color: var(--text-muted);
  }

  .ai-provider-label {
    font-weight: 500;
  }

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

  .parse-error {
    margin-top: var(--sp-4); padding: var(--sp-3) var(--sp-4); border: 1px solid var(--danger-border);
    border-radius: var(--radius-md); background: var(--danger-bg); display: flex; align-items: center; gap: var(--sp-4);
  }
  .parse-error p { margin: 0; color: var(--danger); font-size: 0.9rem; flex: 1; }

  .parsed-result {
    margin-top: var(--sp-4); padding: var(--sp-4); border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md); background: var(--bg-raised);
  }
  .parsed-result h4 { margin: 0 0 var(--sp-3); }
  .parsed-fields { display: flex; flex-direction: column; gap: var(--sp-2); font-size: 0.9rem; }
  .parsed-field ul { margin: var(--sp-1) 0 0; padding-left: var(--sp-5); }
  .parsed-field li { font-size: 0.85rem; }
  .line-item-cat-tag {
    font-size: 0.65rem;
    font-weight: 500;
    text-transform: uppercase;
    color: var(--text-muted);
    background: var(--surface);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    padding: 0 3px;
    margin-right: 4px;
  }
  .parsed-actions { margin-top: var(--sp-3); display: flex; gap: var(--sp-2); justify-content: flex-end; align-items: center; }
  .service-created-notice { font-size: 0.85rem; color: var(--success); font-weight: 500; }
</style>
