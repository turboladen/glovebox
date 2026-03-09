<script lang="ts">
  import { research as researchApi, services as servicesApi, parts as partsApi } from '../lib/api'
  import type { RecallCheckResult, ResearchReport, ReportWithFindings, ResearchFinding, ServiceRecordWithLinks, Part } from '../lib/types'
  import { formatDate } from '../lib/dates'
  import AiProviderSelect from './AiProviderSelect.svelte'

  let { vehicleId }: { vehicleId: number } = $props()

  let recalls: RecallCheckResult | null = $state(null)
  let recallLoading = $state(false)
  let recallError = $state('')

  let reports: ResearchReport[] = $state([])
  let reportsLoading = $state(true)
  let expandedReport: ReportWithFindings | null = $state(null)

  let generating = $state(false)
  let generateError = $state('')
  let generateStep = $state('')

  let selectedProviderId: number | undefined = $state(undefined)

  // Link picker state
  let linkPickerFinding: ResearchFinding | null = $state(null)
  let linkableServices: ServiceRecordWithLinks[] = $state([])
  let linkableParts: Part[] = $state([])
  let linkLoading = $state(false)

  // Severity filter state — all enabled by default
  const severityLevels = ['critical', 'recommended', 'optional', 'informational'] as const
  let activeSeverities: Set<string> = $state(new Set(severityLevels))

  // Derived: findings grouped by category with active filters applied
  const categoryOrder = ['recall', 'suggested_maintenance', 'forum_report', 'upgrade_idea']

  let filteredGroupedFindings = $derived.by(() => {
    if (!expandedReport) return []
    const filtered = expandedReport.findings.filter(f => activeSeverities.has(f.severity ?? 'informational'))
    const groups: { category: string; findings: ResearchFinding[] }[] = []
    const byCategory = new Map<string, ResearchFinding[]>()
    for (const f of filtered) {
      const cat = f.category
      if (!byCategory.has(cat)) byCategory.set(cat, [])
      byCategory.get(cat)!.push(f)
    }
    // Sort categories in defined order, then any unknown categories at the end
    for (const cat of categoryOrder) {
      if (byCategory.has(cat)) {
        groups.push({ category: cat, findings: byCategory.get(cat)! })
        byCategory.delete(cat)
      }
    }
    for (const [cat, findings] of byCategory) {
      groups.push({ category: cat, findings })
    }
    return groups
  })

  function toggleSeverity(level: string) {
    const next = new Set(activeSeverities)
    if (next.has(level)) {
      // Don't allow deselecting all
      if (next.size > 1) next.delete(level)
    } else {
      next.add(level)
    }
    activeSeverities = next
  }

  async function loadReports() {
    try {
      reports = await researchApi.listReports(vehicleId)
    } catch (e: any) {
      console.error('Failed to load reports:', e)
    } finally {
      reportsLoading = false
    }
  }

  loadReports()

  async function checkRecalls() {
    recallLoading = true
    recallError = ''
    try {
      recalls = await researchApi.checkRecalls(vehicleId)
      // Recall results are now persisted as findings — reload reports and expand the latest
      await loadReports()
      if (reports.length > 0 && reports[0].report_type === 'recalls_only') {
        await viewReport(reports[0].id)
      }
    } catch (e: any) {
      recallError = e.message
    } finally {
      recallLoading = false
    }
  }

  const progressSteps = [
    'Fetching vehicle data...',
    'Checking NHTSA recalls...',
    'Compiling maintenance history...',
    'Generating AI analysis...',
  ]

  async function generateReport() {
    generating = true
    generateError = ''
    generateStep = progressSteps[0]

    // Advance through progress steps on a timer
    let stepIndex = 0
    const stepInterval = setInterval(() => {
      stepIndex++
      if (stepIndex < progressSteps.length) {
        generateStep = progressSteps[stepIndex]
      }
    }, 3000)

    try {
      const report = await researchApi.generateReport(vehicleId, 'full_check', selectedProviderId)
      expandedReport = report
      reports = [report, ...reports]
    } catch (e: any) {
      generateError = e.message
    } finally {
      clearInterval(stepInterval)
      generating = false
      generateStep = ''
    }
  }

  async function viewReport(id: number) {
    try {
      expandedReport = await researchApi.getReport(vehicleId, id)
    } catch (e: any) {
      console.error('Failed to load report:', e)
    }
  }

  async function updateFindingStatus(finding: ResearchFinding, newStatus: string) {
    if (!expandedReport) return
    try {
      const updated = await researchApi.updateFinding(vehicleId, finding.report_id, finding.id, { status: newStatus })
      expandedReport = {
        ...expandedReport,
        findings: expandedReport.findings.map(f => f.id === updated.id ? updated : f),
      }
    } catch (e: any) {
      console.error('Failed to update finding:', e)
    }
  }

  async function openLinkPicker(finding: ResearchFinding) {
    linkPickerFinding = finding
    linkLoading = true
    try {
      const [svc, pts] = await Promise.all([
        servicesApi.list(vehicleId),
        partsApi.list(vehicleId),
      ])
      linkableServices = svc
      linkableParts = pts
    } catch (e) {
      console.error('Failed to load linkable records:', e)
    } finally {
      linkLoading = false
    }
  }

  async function linkAndComplete(entityType: string | null, entityId: number | null) {
    if (!linkPickerFinding) return
    try {
      const updated = await researchApi.updateFinding(vehicleId, linkPickerFinding.report_id, linkPickerFinding.id, {
        status: 'completed',
        linked_entity_type: entityType,
        linked_entity_id: entityId,
      })
      if (expandedReport) {
        expandedReport = {
          ...expandedReport,
          findings: expandedReport.findings.map(f => f.id === updated.id ? updated : f),
        }
      }
    } catch (e: any) {
      console.error('Failed to link finding:', e)
    } finally {
      linkPickerFinding = null
    }
  }

  function linkedLabel(finding: ResearchFinding): string | null {
    if (!finding.linked_entity_type || !finding.linked_entity_id) return null
    switch (finding.linked_entity_type) {
      case 'service': return `Linked to service #${finding.linked_entity_id}`
      case 'part': return `Linked to part #${finding.linked_entity_id}`
      default: return `Linked to ${finding.linked_entity_type} #${finding.linked_entity_id}`
    }
  }

  function severityClass(severity: string | null): string {
    switch (severity) {
      case 'critical': return 'severity-critical'
      case 'recommended': return 'severity-recommended'
      case 'optional': return 'severity-optional'
      default: return 'severity-info'
    }
  }

  function statusLabel(status: string): string {
    switch (status) {
      case 'new': return 'New'
      case 'dismissed': return 'Dismissed'
      case 'planned': return 'Planned'
      case 'completed': return 'Completed'
      default: return status
    }
  }

  function reportTypeLabel(t: string | null): string {
    switch (t) {
      case 'full_check': return 'Full Check'
      case 'recalls_only': return 'Recalls Only'
      case 'community_wisdom': return 'Community Wisdom'
      default: return t ?? 'Full Check'
    }
  }

  function categoryLabel(category: string): string {
    switch (category) {
      case 'recall': return 'Recall'
      case 'forum_report': return 'Forum Report'
      case 'suggested_maintenance': return 'Suggested Maintenance'
      case 'upgrade_idea': return 'Upgrade Idea'
      default: return category
    }
  }

</script>

<div class="research-tab">
  <div class="section">
    <div class="section-header">
      <h3>NHTSA Recall Check</h3>
      <button class="btn btn-primary" onclick={checkRecalls} disabled={recallLoading}>
        {recallLoading ? 'Checking...' : 'Check Recalls'}
      </button>
    </div>

    {#if recallError}
      <p class="error">{recallError}</p>
    {/if}

    {#if recalls}
      {#if recalls.recall_count === 0}
        <div class="recall-clear">
          No open recalls found for {recalls.model_year} {recalls.make} {recalls.model}.
        </div>
      {:else}
        <div class="recall-warning">
          {recalls.recall_count} recall(s) found for {recalls.model_year} {recalls.make} {recalls.model}
        </div>
        <p class="recall-hint">Recalls are saved as findings below. Use Plan/Complete/Dismiss to track status.</p>
      {/if}
    {/if}
  </div>

  <div class="section">
    <div class="section-header">
      <div>
        <h3>Research Reports</h3>
        <p class="section-desc">AI-generated analysis of common issues, maintenance tips, recalls, and popular upgrades for your vehicle.</p>
      </div>
      <div class="generate-controls">
        <AiProviderSelect bind:selectedProviderId />
        <button class="btn btn-primary" onclick={generateReport} disabled={generating}>
          {generating ? 'Generating...' : 'Run Full Check'}
        </button>
      </div>
    </div>

    {#if generating}
      <div class="progress-indicator">
        <span class="spinner"></span>
        <span class="progress-step">{generateStep}</span>
      </div>
    {/if}

    {#if generateError}
      <p class="error">{generateError}</p>
    {/if}

    {#if expandedReport}
      <div class="expanded-report">
        <div class="report-detail-header">
          <h4>Report: {reportTypeLabel(expandedReport.report_type)}</h4>
          <button class="btn btn-secondary btn-sm" onclick={() => (expandedReport = null)}>Close</button>
        </div>
        <p class="report-summary">{expandedReport.summary}</p>
        <p class="report-date">Generated: {formatDate(expandedReport.generated_at)}</p>

        {#if expandedReport.findings.length === 0}
          <p class="no-data">No findings in this report.</p>
        {:else}
          <div class="severity-filters">
            {#each severityLevels as level}
              <button
                class="filter-chip {severityClass(level)}"
                class:inactive={!activeSeverities.has(level)}
                onclick={() => toggleSeverity(level)}
              >
                {level}
              </button>
            {/each}
          </div>

          <div class="findings-list">
            {#each filteredGroupedFindings as group}
              <div class="category-group">
                <h5 class="category-header">{categoryLabel(group.category)}</h5>
                {#each group.findings as finding}
                  <div class="finding-card" class:dismissed={finding.status === 'dismissed'} class:completed={finding.status === 'completed'}>
                    <div class="finding-header">
                      <span class="badge {severityClass(finding.severity)}">{finding.severity ?? 'info'}</span>
                      <span class="badge status-badge status-{finding.status}">{statusLabel(finding.status)}</span>
                    </div>
                    <h5>{finding.title}</h5>
                    {#if finding.description}
                      <p class="finding-desc">{finding.description}</p>
                    {/if}
                    {#if finding.source_url}
                      <details class="sources-detail">
                        <summary class="sources-toggle">Sources</summary>
                        <div class="sources-content">
                          <a href={finding.source_url} target="_blank" rel="noopener" class="source-link">{finding.source_url}</a>
                        </div>
                      </details>
                    {/if}
                    {#if linkedLabel(finding)}
                      <p class="linked-label">{linkedLabel(finding)}</p>
                    {/if}
                    <div class="finding-actions">
                      {#if finding.status !== 'dismissed'}
                        <button class="btn btn-sm btn-secondary" onclick={() => updateFindingStatus(finding, 'dismissed')}>Dismiss</button>
                      {/if}
                      {#if finding.status !== 'planned'}
                        <button class="btn btn-sm btn-secondary" onclick={() => updateFindingStatus(finding, 'planned')}>Plan</button>
                      {/if}
                      {#if finding.status !== 'completed'}
                        <button class="btn btn-sm btn-primary" onclick={() => openLinkPicker(finding)}>Complete</button>
                      {/if}
                      {#if finding.status !== 'new'}
                        <button class="btn btn-sm btn-secondary" onclick={() => updateFindingStatus(finding, 'new')}>Reopen</button>
                      {/if}
                    </div>
                  </div>
                {/each}
              </div>
            {/each}
          </div>
        {/if}
      </div>
    {/if}

    {#if reportsLoading}
      <p class="loading">Loading reports...</p>
    {:else if reports.length === 0 && !expandedReport}
      <p class="no-data">No research reports yet. Click "Run Full Check" to generate one.</p>
    {:else}
      <div class="reports-list">
        {#each reports as report}
          <button class="report-row" onclick={() => viewReport(report.id)} class:active={expandedReport?.id === report.id}>
            <span class="report-type">{reportTypeLabel(report.report_type)}</span>
            <span class="report-date">{formatDate(report.generated_at)}</span>
            <span class="report-summary-preview">{report.summary ?? ''}</span>
          </button>
        {/each}
      </div>
    {/if}
  </div>
</div>

{#if linkPickerFinding}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="modal-backdrop" onclick={() => (linkPickerFinding = null)} onkeydown={(e) => e.key === 'Escape' && (linkPickerFinding = null)}>
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="modal-content" onclick={(e) => e.stopPropagation()}>
      <h4>Mark as Completed</h4>
      <p class="modal-desc">Optionally link this finding to a service record or part that addressed it.</p>

      {#if linkLoading}
        <p class="loading">Loading records...</p>
      {:else}
        <button class="btn btn-primary link-option" onclick={() => linkAndComplete(null, null)}>
          Complete without linking
        </button>

        {#if linkableServices.length > 0}
          <h5 class="link-section-header">Service Records</h5>
          {#each linkableServices as svc}
            <button class="link-option" onclick={() => linkAndComplete('service', svc.id)}>
              <span class="link-option-title">{svc.description ?? 'Service'}</span>
              <span class="link-option-meta">{formatDate(svc.service_date)}{svc.shop_name ? ` · ${svc.shop_name}` : ''}</span>
            </button>
          {/each}
        {/if}

        {#if linkableParts.length > 0}
          <h5 class="link-section-header">Parts</h5>
          {#each linkableParts as part}
            <button class="link-option" onclick={() => linkAndComplete('part', part.id)}>
              <span class="link-option-title">{part.name}</span>
              <span class="link-option-meta">{part.manufacturer ?? ''}{part.part_number ? ` · ${part.part_number}` : ''}</span>
            </button>
          {/each}
        {/if}
      {/if}

      <button class="btn btn-secondary btn-sm modal-close" onclick={() => (linkPickerFinding = null)}>Cancel</button>
    </div>
  </div>
{/if}

<style>
  .research-tab {
    display: flex;
    flex-direction: column;
    gap: var(--sp-6);
  }

  .section {
    padding: var(--sp-4);
    background: var(--bg-raised);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-lg);
  }

  .section-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: var(--sp-3);
  }

  .section-header h3 {
    margin: 0;
    font-family: var(--font-display);
    font-size: 1rem;
  }

  .section-desc {
    font-size: 0.8rem;
    color: var(--text-muted);
    margin: var(--sp-1) 0 0;
  }

  .progress-indicator {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    padding: var(--sp-3);
    background: var(--info-bg);
    border: 1px solid var(--info);
    border-radius: var(--radius-md);
    margin-bottom: var(--sp-3);
    font-size: 0.85rem;
    color: var(--info);
  }

  .spinner {
    display: inline-block;
    width: 14px;
    height: 14px;
    border: 2px solid currentColor;
    border-top-color: transparent;
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
    flex-shrink: 0;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  .progress-step {
    font-weight: 500;
  }

  .severity-filters {
    display: flex;
    gap: var(--sp-1);
    flex-wrap: wrap;
    margin-bottom: var(--sp-3);
  }

  .filter-chip {
    font-size: 0.75rem;
    padding: var(--sp-1) var(--sp-2);
    border-radius: var(--radius-full, 999px);
    cursor: pointer;
    font-weight: 500;
    transition: opacity var(--duration-fast) var(--ease-out);
  }

  .filter-chip.inactive {
    opacity: 0.35;
  }

  .category-group {
    margin-bottom: var(--sp-3);
  }

  .category-header {
    font-family: var(--font-display);
    font-size: 0.8rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--text-muted);
    margin: var(--sp-3) 0 var(--sp-1);
    padding-bottom: var(--sp-1);
    border-bottom: 1px solid var(--border-subtle);
  }

  .sources-detail {
    margin: var(--sp-1) 0;
    font-size: 0.8rem;
  }

  .sources-toggle {
    cursor: pointer;
    color: var(--primary);
    font-size: 0.8rem;
  }

  .sources-toggle:hover {
    text-decoration: underline;
  }

  .sources-content {
    padding: var(--sp-1) var(--sp-2);
    margin-top: var(--sp-1);
    background: var(--surface);
    border-radius: var(--radius-sm);
    word-break: break-all;
  }

  .recall-clear {
    color: var(--success);
    padding: var(--sp-3);
    background: var(--success-bg);
    border: 1px solid var(--success-border);
    border-radius: var(--radius-md);
    font-size: 0.9rem;
  }

  .recall-warning {
    color: var(--danger);
    padding: var(--sp-3);
    background: var(--danger-bg);
    border: 1px solid var(--danger-border);
    border-radius: var(--radius-md);
    font-weight: 600;
    margin-bottom: var(--sp-3);
    font-size: 0.9rem;
  }


  .recall-hint {
    font-size: 0.85rem;
    color: var(--text-muted);
    margin: var(--sp-2) 0 0;
    font-style: italic;
  }

  .expanded-report {
    padding: var(--sp-3);
    border: 1px solid var(--primary);
    border-radius: var(--radius-md);
    margin-bottom: var(--sp-4);
    background: var(--bg-raised);
  }

  .report-detail-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .report-detail-header h4 {
    margin: 0;
  }

  .report-summary {
    font-size: 0.9rem;
    color: var(--text-muted);
    margin: var(--sp-2) 0;
  }

  .report-date {
    font-size: 0.8rem;
    color: var(--text-muted);
  }

  .finding-card {
    padding: var(--sp-3);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    margin-top: var(--sp-2);
    transition: border-color var(--duration-base) var(--ease-out);
  }

  .finding-card:hover {
    border-color: var(--border);
  }

  .finding-card.dismissed {
    opacity: 0.5;
  }

  .finding-card.completed {
    border-color: var(--success);
    opacity: 0.7;
  }

  .finding-header {
    display: flex;
    gap: var(--sp-2);
    flex-wrap: wrap;
    margin-bottom: var(--sp-1);
  }

  .finding-card h5 {
    margin: var(--sp-1) 0;
    font-family: var(--font-display);
    font-size: 0.9rem;
  }

  .finding-desc {
    font-size: 0.85rem;
    color: var(--text-muted);
    margin: var(--sp-1) 0;
  }

  .source-link {
    font-size: 0.8rem;
    display: inline-block;
    margin: var(--sp-1) 0;
  }

  .finding-actions {
    display: flex;
    gap: var(--sp-1);
    margin-top: var(--sp-2);
  }

  .severity-critical { background: var(--danger-bg); color: var(--danger); border: 1px solid var(--danger-border); }
  .severity-recommended { background: var(--warning-bg); color: var(--warning); border: 1px solid var(--warning-border); }
  .severity-optional { background: var(--info-bg); color: var(--info); border: 1px solid transparent; }
  .severity-info { background: var(--surface); color: var(--text-muted); border: 1px solid var(--border-subtle); }

  .status-new { background: var(--info-bg); color: var(--info); border: 1px solid transparent; }
  .status-planned { background: rgba(168, 85, 247, 0.12); color: #a855f7; border: 1px solid rgba(168, 85, 247, 0.25); }
  .status-completed { background: var(--success-bg); color: var(--success); border: 1px solid var(--success-border); }
  .status-dismissed { background: var(--surface); color: var(--text-muted); border: 1px solid var(--border-subtle); }

  .reports-list {
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
  }

  .report-row {
    display: flex;
    gap: var(--sp-4);
    align-items: center;
    padding: var(--sp-2) var(--sp-3);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    background: none;
    cursor: pointer;
    text-align: left;
    font-size: 0.85rem;
    width: 100%;
    color: var(--text);
    transition: background var(--duration-fast) var(--ease-out), border-color var(--duration-fast) var(--ease-out);
  }

  .report-row:hover {
    background: var(--surface-hover);
  }

  .report-row.active {
    border-color: var(--primary);
  }

  .report-type {
    font-family: var(--font-display);
    font-weight: 600;
    min-width: 80px;
  }

  .report-summary-preview {
    flex: 1;
    color: var(--text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .no-data {
    color: var(--text-muted);
    font-size: 0.9rem;
    text-align: center;
    padding: var(--sp-4) 0;
  }

  .error {
    color: var(--danger);
    font-size: 0.85rem;
  }

  .loading {
    color: var(--text-muted);
    font-size: 0.9rem;
  }

  .generate-controls {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    flex-shrink: 0;
  }

  .linked-label {
    font-size: 0.75rem;
    color: var(--success);
    margin: var(--sp-1) 0 0;
    font-style: italic;
  }

  .modal-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.5);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 100;
  }

  .modal-content {
    background: var(--bg-raised);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    padding: var(--sp-4);
    max-width: 480px;
    width: 90vw;
    max-height: 70vh;
    overflow-y: auto;
  }

  .modal-content h4 {
    margin: 0 0 var(--sp-1);
  }

  .modal-desc {
    font-size: 0.85rem;
    color: var(--text-muted);
    margin: 0 0 var(--sp-3);
  }

  .link-section-header {
    font-size: 0.75rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--text-muted);
    margin: var(--sp-3) 0 var(--sp-1);
  }

  .link-option {
    display: flex;
    flex-direction: column;
    width: 100%;
    padding: var(--sp-2) var(--sp-3);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    background: none;
    cursor: pointer;
    text-align: left;
    color: var(--text);
    margin-bottom: var(--sp-1);
    transition: background var(--duration-fast) var(--ease-out);
  }

  .link-option:hover {
    background: var(--surface-hover);
  }

  .link-option-title {
    font-size: 0.85rem;
    font-weight: 500;
  }

  .link-option-meta {
    font-size: 0.75rem;
    color: var(--text-muted);
  }

  .modal-close {
    margin-top: var(--sp-3);
    width: 100%;
  }
</style>
