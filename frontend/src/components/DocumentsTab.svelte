<script lang="ts">
  import { onMount } from 'svelte'
  import { documents as docsApi } from '../lib/api'
  import type { Document } from '../lib/types'

  let { vehicleId }: { vehicleId: number } = $props()

  let docs: Document[] = $state([])
  let loading = $state(true)
  let showUpload = $state(false)

  // Upload form
  let title = $state('')
  let docType = $state('other')
  let notes = $state('')
  let fileInput: HTMLInputElement
  let uploading = $state(false)
  let error = $state('')

  const docTypes = ['invoice', 'receipt', 'photo', 'title', 'warranty', 'manual', 'other']

  onMount(loadData)

  async function loadData() {
    try {
      docs = await docsApi.list({ vehicle_id: vehicleId })
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
      await docsApi.upload(formData)
      showUpload = false
      title = ''; notes = ''
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
              <span>{doc.created_at.split(' ')[0]}</span>
            </div>
            {#if doc.notes}
              <p class="doc-notes">{doc.notes}</p>
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
    display: flex; justify-content: space-between; align-items: center; margin-bottom: 1rem;
  }
  .docs-header h3 { margin: 0; }

  .form-card {
    padding: 1rem; border: 1px solid var(--border); border-radius: 8px;
    margin-bottom: 1rem; background: var(--surface);
  }
  .form-row { display: grid; grid-template-columns: 1fr 1fr; gap: 0.75rem; }
  .field { margin-bottom: 0.75rem; }
  .field label { display: block; font-size: 0.85rem; margin-bottom: 0.25rem; color: var(--text-muted); }
  .field input, .field select {
    width: 100%; padding: 0.4rem 0.6rem; border: 1px solid var(--border);
    border-radius: 4px; font-size: 0.9rem; background: var(--bg); color: var(--text);
  }
  .form-actions { display: flex; gap: 0.5rem; justify-content: flex-end; }
  .error { color: var(--danger); font-size: 0.85rem; }

  .docs-list { display: flex; flex-direction: column; gap: 0.5rem; }

  .doc-card {
    padding: 0.75rem 1rem; border: 1px solid var(--border); border-radius: 4px;
    display: flex; align-items: center; gap: 1rem;
  }

  .doc-preview { width: 60px; height: 60px; flex-shrink: 0; }
  .doc-preview img { width: 100%; height: 100%; object-fit: cover; border-radius: 4px; }

  .doc-info { flex: 1; }
  .doc-title { font-weight: 600; }
  .doc-meta { font-size: 0.8rem; color: var(--text-muted); display: flex; gap: 0.5rem; flex-wrap: wrap; }
  .doc-type-badge {
    background: var(--surface); border: 1px solid var(--border); border-radius: 3px;
    padding: 0 0.3rem; text-transform: uppercase; font-size: 0.7rem; font-weight: 500;
  }
  .doc-notes { font-size: 0.85rem; color: var(--text-muted); margin: 0.25rem 0 0; }

  .doc-actions { display: flex; gap: 0.5rem; flex-shrink: 0; }

  .empty { color: var(--text-muted); text-align: center; padding: 2rem 0; }
</style>
