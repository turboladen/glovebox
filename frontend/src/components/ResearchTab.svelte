<script lang="ts">
  import { research as researchApi } from '../lib/api'
  import type { RecallCheckResult, ResearchReport, ReportWithFindings, ResearchFinding } from '../lib/types'

  let { vehicleId }: { vehicleId: number } = $props()

  let recalls: RecallCheckResult | null = $state(null)
  let recallLoading = $state(false)
  let recallError = $state('')

  let reports: ResearchReport[] = $state([])
  let reportsLoading = $state(true)
  let expandedReport: ReportWithFindings | null = $state(null)

  let generating = $state(false)
  let generateError = $state('')

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
    } catch (e: any) {
      recallError = e.message
    } finally {
      recallLoading = false
    }
  }

  async function generateReport() {
    generating = true
    generateError = ''
    try {
      const report = await researchApi.generateReport(vehicleId, 'full_check')
      expandedReport = report
      reports = [report, ...reports]
    } catch (e: any) {
      generateError = e.message
    } finally {
      generating = false
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

  function formatDate(dateStr: string): string {
    try {
      return new Date(dateStr).toLocaleDateString()
    } catch {
      return dateStr
    }
  }
</script>

<div class="research-tab">
  <div class="section">
    <div class="section-header">
      <h3>NHTSA Recall Check</h3>
      <button class="btn btn-secondary" onclick={checkRecalls} disabled={recallLoading}>
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
        <div class="recall-list">
          {#each recalls.recalls as recall}
            <div class="recall-card">
              <div class="recall-header">
                <span class="campaign-number">{recall.campaign_number}</span>
                <span class="recall-component">{recall.component ?? ''}</span>
              </div>
              <h4>{recall.subject}</h4>
              {#if recall.summary}
                <p class="recall-summary">{recall.summary}</p>
              {/if}
              {#if recall.consequence}
                <p class="recall-consequence"><strong>Consequence:</strong> {recall.consequence}</p>
              {/if}
              {#if recall.remedy}
                <p class="recall-remedy"><strong>Remedy:</strong> {recall.remedy}</p>
              {/if}
              {#if recall.report_date}
                <p class="recall-date">Reported: {recall.report_date}</p>
              {/if}
            </div>
          {/each}
        </div>
      {/if}
    {/if}
  </div>

  <div class="section">
    <div class="section-header">
      <h3>Research Reports</h3>
      <button class="btn btn-primary" onclick={generateReport} disabled={generating}>
        {generating ? 'Generating...' : 'Run Full Check'}
      </button>
    </div>

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
          <div class="findings-list">
            {#each expandedReport.findings as finding}
              <div class="finding-card" class:dismissed={finding.status === 'dismissed'} class:completed={finding.status === 'completed'}>
                <div class="finding-header">
                  <span class="badge category-badge">{categoryLabel(finding.category)}</span>
                  <span class="badge {severityClass(finding.severity)}">{finding.severity ?? 'info'}</span>
                  <span class="badge status-badge status-{finding.status}">{statusLabel(finding.status)}</span>
                </div>
                <h5>{finding.title}</h5>
                {#if finding.description}
                  <p class="finding-desc">{finding.description}</p>
                {/if}
                {#if finding.source_url}
                  <a href={finding.source_url} target="_blank" rel="noopener" class="source-link">View source</a>
                {/if}
                <div class="finding-actions">
                  {#if finding.status !== 'dismissed'}
                    <button class="btn btn-sm btn-secondary" onclick={() => updateFindingStatus(finding, 'dismissed')}>Dismiss</button>
                  {/if}
                  {#if finding.status !== 'planned'}
                    <button class="btn btn-sm btn-secondary" onclick={() => updateFindingStatus(finding, 'planned')}>Plan</button>
                  {/if}
                  {#if finding.status !== 'completed'}
                    <button class="btn btn-sm btn-primary" onclick={() => updateFindingStatus(finding, 'completed')}>Complete</button>
                  {/if}
                  {#if finding.status !== 'new'}
                    <button class="btn btn-sm btn-secondary" onclick={() => updateFindingStatus(finding, 'new')}>Reopen</button>
                  {/if}
                </div>
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

<style>
  .research-tab {
    display: flex;
    flex-direction: column;
    gap: 1.5rem;
  }

  .section {
    padding: 1rem;
    background: var(--surface);
    border-radius: 8px;
  }

  .section-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 0.75rem;
  }

  .section-header h3 {
    margin: 0;
    font-size: 1rem;
  }

  .recall-clear {
    color: var(--success, #22c55e);
    padding: 0.75rem;
    background: rgba(34, 197, 94, 0.1);
    border-radius: 6px;
    font-size: 0.9rem;
  }

  .recall-warning {
    color: var(--danger, #ef4444);
    padding: 0.75rem;
    background: rgba(239, 68, 68, 0.1);
    border-radius: 6px;
    font-weight: 600;
    margin-bottom: 0.75rem;
    font-size: 0.9rem;
  }

  .recall-card {
    padding: 0.75rem;
    border: 1px solid var(--border);
    border-radius: 6px;
    margin-bottom: 0.5rem;
  }

  .recall-header {
    display: flex;
    gap: 0.5rem;
    font-size: 0.8rem;
    color: var(--text-muted);
    margin-bottom: 0.25rem;
  }

  .recall-card h4 {
    margin: 0.25rem 0 0.5rem;
    font-size: 0.95rem;
  }

  .recall-summary, .recall-consequence, .recall-remedy, .recall-date {
    font-size: 0.85rem;
    margin: 0.25rem 0;
    color: var(--text-muted);
  }

  .expanded-report {
    padding: 0.75rem;
    border: 1px solid var(--primary);
    border-radius: 6px;
    margin-bottom: 1rem;
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
    margin: 0.5rem 0;
  }

  .report-date {
    font-size: 0.8rem;
    color: var(--text-muted);
  }

  .finding-card {
    padding: 0.75rem;
    border: 1px solid var(--border);
    border-radius: 6px;
    margin-top: 0.5rem;
  }

  .finding-card.dismissed {
    opacity: 0.5;
  }

  .finding-card.completed {
    border-color: var(--success, #22c55e);
    opacity: 0.7;
  }

  .finding-header {
    display: flex;
    gap: 0.4rem;
    flex-wrap: wrap;
    margin-bottom: 0.25rem;
  }

  .finding-card h5 {
    margin: 0.25rem 0;
    font-size: 0.9rem;
  }

  .finding-desc {
    font-size: 0.85rem;
    color: var(--text-muted);
    margin: 0.25rem 0;
  }

  .source-link {
    font-size: 0.8rem;
    display: inline-block;
    margin: 0.25rem 0;
  }

  .finding-actions {
    display: flex;
    gap: 0.3rem;
    margin-top: 0.5rem;
  }

  .badge {
    font-size: 0.7rem;
    padding: 0.15rem 0.4rem;
    border-radius: 4px;
    text-transform: uppercase;
    font-weight: 600;
  }

  .severity-critical { background: rgba(239, 68, 68, 0.15); color: var(--danger, #ef4444); }
  .severity-recommended { background: rgba(234, 179, 8, 0.15); color: #ca8a04; }
  .severity-optional { background: rgba(59, 130, 246, 0.15); color: #3b82f6; }
  .severity-info { background: rgba(107, 114, 128, 0.15); color: var(--text-muted); }

  .status-new { background: rgba(59, 130, 246, 0.15); color: #3b82f6; }
  .status-planned { background: rgba(168, 85, 247, 0.15); color: #a855f7; }
  .status-completed { background: rgba(34, 197, 94, 0.15); color: #22c55e; }
  .status-dismissed { background: rgba(107, 114, 128, 0.15); color: var(--text-muted); }

  .reports-list {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }

  .report-row {
    display: flex;
    gap: 1rem;
    align-items: center;
    padding: 0.5rem 0.75rem;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: none;
    cursor: pointer;
    text-align: left;
    font-size: 0.85rem;
    width: 100%;
    color: var(--text);
  }

  .report-row:hover {
    background: var(--surface-hover, rgba(0, 0, 0, 0.05));
  }

  .report-row.active {
    border-color: var(--primary);
  }

  .report-type {
    font-weight: 600;
    min-width: 80px;
  }

  .report-date {
    font-size: 0.8rem;
    color: var(--text-muted);
    min-width: 80px;
  }

  .report-summary-preview {
    flex: 1;
    color: var(--text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .btn-sm {
    font-size: 0.75rem;
    padding: 0.2rem 0.5rem;
  }

  .no-data {
    color: var(--text-muted);
    font-size: 0.9rem;
    text-align: center;
    padding: 1rem 0;
  }

  .error {
    color: var(--danger);
    font-size: 0.85rem;
  }

  .loading {
    color: var(--text-muted);
    font-size: 0.9rem;
  }
</style>
