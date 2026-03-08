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

  .recall-card {
    padding: var(--sp-3);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    margin-bottom: var(--sp-2);
    background: var(--bg-raised);
  }

  .recall-header {
    display: flex;
    gap: var(--sp-2);
    font-size: 0.8rem;
    color: var(--text-muted);
    margin-bottom: var(--sp-1);
  }

  .recall-card h4 {
    margin: var(--sp-1) 0 var(--sp-2);
    font-size: 0.95rem;
  }

  .recall-summary, .recall-consequence, .recall-remedy, .recall-date {
    font-size: 0.85rem;
    margin: var(--sp-1) 0;
    color: var(--text-muted);
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
</style>
