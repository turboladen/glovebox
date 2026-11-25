//! Research findings filed by hand: facts worth acting on that are not
//! recalls.
//!
//! Recalls share the table and belong to [`recall`](super::recall), so every
//! function here treats a recall's id as an id that names nothing.
//!
//! A finding reads `dismissed` or `done` when it is marked so. Otherwise it
//! reads `planned` while a planned job addresses it, `done` when a job from
//! it is done with none planned, and `open` when no job addresses it.

use std::collections::{HashMap, hash_map::Entry};

use sea_orm::*;
use serde::Serialize;

use crate::{
    entities::{
        research_finding::{self, FindingCategory, FindingSeverity, FindingStatus},
        research_report::{self, ReportType},
        service_record,
        work_item::{self, JobStatus},
    },
    error::{DomainError, DomainResult},
    inputs::finding::{FindingFilter, NewFinding, UpdateFinding},
    services::incident::ServiceRef,
    vocab::Vocabulary,
};

crate::vocab::vocabulary! {
    /// The statuses a caller can set on a finding: `open` (still worth acting
    /// on), `done` (acted on) and `dismissed` (decided against).
    // A finding also reads `planned`, which comes from a planned job and is
    // never set. No column stores these texts, so a filter on a finding's
    // stored status takes `stored()`, never the variant.
    pub enum FiledFindingStatus {
        Open => "open",
        Done => "done",
        Dismissed => "dismissed",
    }
}

impl FiledFindingStatus {
    /// The stored status that makes a finding with no job read as this one.
    fn stored(self) -> FindingStatus {
        match self {
            Self::Open => FindingStatus::New,
            Self::Done => FindingStatus::Completed,
            Self::Dismissed => FindingStatus::Dismissed,
        }
    }

    /// The status a finding with no job reads from a stored status.
    fn from_stored(status: FindingStatus) -> Self {
        match status {
            FindingStatus::New => Self::Open,
            FindingStatus::Completed => Self::Done,
            FindingStatus::Dismissed => Self::Dismissed,
        }
    }
}

crate::vocab::vocabulary! {
    /// The status a finding reads.
    // `planned` comes from a planned job while the finding is neither marked
    // done nor dismissed, and no caller can set it. No column stores these
    // texts.
    pub enum ShownFindingStatus {
        Open => "open",
        Planned => "planned",
        Done => "done",
        Dismissed => "dismissed",
    }
}

impl From<FiledFindingStatus> for ShownFindingStatus {
    fn from(status: FiledFindingStatus) -> Self {
        match status {
            FiledFindingStatus::Open => Self::Open,
            FiledFindingStatus::Done => Self::Done,
            FiledFindingStatus::Dismissed => Self::Dismissed,
        }
    }
}

/// A job filed from a finding.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FindingJob {
    pub id: i32,
    pub title: String,
    pub status: JobStatus,
    /// The appointment the job is on, if any.
    pub appointment_id: Option<i32>,
}

impl From<work_item::Model> for FindingJob {
    fn from(job: work_item::Model) -> Self {
        Self {
            id: job.id,
            title: job.title,
            status: job.status,
            appointment_id: job.appointment_id,
        }
    }
}

/// A finding with what was done about it.
#[derive(Debug, Serialize)]
pub struct FindingView {
    pub id: i32,
    pub category: FindingCategory,
    pub title: String,
    pub description: Option<String>,
    pub source_url: Option<String>,
    pub severity: Option<FindingSeverity>,
    pub status: ShownFindingStatus,
    /// The job that addresses or addressed the finding: a planned one on an
    /// appointment when there is one, else the oldest planned one, else the
    /// latest one that is done. A dropped job is never shown.
    pub work_item: Option<FindingJob>,
    /// The service that completed `work_item`, when a service did.
    pub done_service: Option<ServiceRef>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    // The status the finding reads before a planned job turns `open` into
    // `planned`, which is what a caller's request is compared with.
    #[serde(skip)]
    settled: FiledFindingStatus,
}

impl FindingView {
    fn has_planned_work(&self) -> bool {
        self.work_item
            .as_ref()
            .is_some_and(|job| job.status == JobStatus::Planned)
    }

    /// Whether the finding still waits for the owner: it reads `open`, or it
    /// reads `planned` and its job is on no appointment.
    fn is_waiting(&self) -> bool {
        match self.status {
            ShownFindingStatus::Open => true,
            ShownFindingStatus::Planned => self
                .work_item
                .as_ref()
                .is_none_or(|job| job.appointment_id.is_none()),
            ShownFindingStatus::Done | ShownFindingStatus::Dismissed => false,
        }
    }
}

/// The findings still waiting on one vehicle, as [`open`] returns them.
#[derive(Debug)]
pub struct OpenFindings {
    /// The most pressing findings, at most [`OPEN_FINDINGS_SHOWN`].
    pub findings: Vec<FindingView>,
    /// How many findings are waiting, the ones left out of `findings`
    /// included.
    pub total: usize,
}

/// The most findings [`open`] returns.
pub const OPEN_FINDINGS_SHOWN: usize = 10;

/// What [`delete`] removed and what it changed besides.
#[derive(Debug, Serialize)]
pub struct DeletedFinding {
    pub id: i32,
    pub title: String,
    /// The jobs that named the finding, oldest first. Each is kept as it is
    /// and names no finding.
    pub unlinked_work_items: Vec<FindingJob>,
}

/// The error for an id that names no finding of the vehicle. A missing id,
/// another vehicle's finding and a recall all produce this same text.
pub(crate) fn not_found(finding_id: i32) -> DomainError {
    DomainError::NotFound(format!("Research finding {finding_id} not found"))
}

fn refuse_recall(category: FindingCategory, message: &str) -> DomainResult<()> {
    if category == FindingCategory::Recall {
        return Err(DomainError::invalid("category", message));
    }
    Ok(())
}

// The vehicle's findings that are not recalls.
fn filed(vehicle_id: i32) -> Select<research_finding::Entity> {
    research_finding::Entity::find()
        .inner_join(research_report::Entity)
        .filter(research_report::Column::VehicleId.eq(vehicle_id))
        .filter(research_finding::Column::Category.ne(FindingCategory::Recall))
}

async fn require(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    finding_id: i32,
) -> DomainResult<research_finding::Model> {
    filed(vehicle_id)
        .filter(research_finding::Column::Id.eq(finding_id))
        .one(db)
        .await?
        .ok_or_else(|| not_found(finding_id))
}

// How strongly a job claims to be the one a finding shows.
fn job_rank(job: &work_item::Model) -> u8 {
    match (job.status, job.appointment_id) {
        (JobStatus::Planned, Some(_)) => 2,
        (JobStatus::Planned, None) => 1,
        _ => 0,
    }
}

/// The job each finding shows, keyed by finding id, in one query.
async fn shown_jobs(
    db: &impl ConnectionTrait,
    finding_ids: Vec<i32>,
) -> DomainResult<HashMap<i32, work_item::Model>> {
    let mut shown: HashMap<i32, work_item::Model> = HashMap::new();
    for job in work_item::Entity::find()
        .filter(work_item::Column::ResearchFindingId.is_in(finding_ids))
        .filter(work_item::Column::Status.is_in([JobStatus::Planned, JobStatus::Done]))
        .order_by_asc(work_item::Column::Id)
        .all(db)
        .await?
    {
        let Some(finding_id) = job.research_finding_id else {
            continue;
        };
        match shown.entry(finding_id) {
            Entry::Vacant(slot) => {
                slot.insert(job);
            }
            Entry::Occupied(mut slot) => {
                // A planned job keeps its place against a later one of the
                // same rank, and the latest done job replaces an earlier one.
                let (new, old) = (job_rank(&job), job_rank(slot.get()));
                if new > old || (new == 0 && old == 0) {
                    slot.insert(job);
                }
            }
        }
    }
    Ok(shown)
}

/// Attaches to each finding its status, its job and that job's service, with
/// a fixed number of queries.
async fn with_views(
    db: &impl ConnectionTrait,
    findings: Vec<research_finding::Model>,
) -> DomainResult<Vec<FindingView>> {
    if findings.is_empty() {
        return Ok(Vec::new());
    }
    let mut jobs = shown_jobs(db, findings.iter().map(|f| f.id).collect()).await?;
    let services: HashMap<i32, service_record::Model> = service_record::Entity::find()
        .filter(
            service_record::Column::Id.is_in(jobs.values().filter_map(|j| j.completed_service_id)),
        )
        .all(db)
        .await?
        .into_iter()
        .map(|s| (s.id, s))
        .collect();

    Ok(findings
        .into_iter()
        .map(|f| {
            let job = jobs.remove(&f.id);
            let done_service = job
                .as_ref()
                .and_then(|j| j.completed_service_id)
                .and_then(|id| services.get(&id).cloned())
                .map(ServiceRef::from);
            // A planned job outranks a done one, so a done job shown means
            // none is planned, and it settles a finding nobody marked.
            let stored = FiledFindingStatus::from_stored(f.status);
            let job_status = job.as_ref().map(|j| j.status);
            let settled =
                if stored == FiledFindingStatus::Open && job_status == Some(JobStatus::Done) {
                    FiledFindingStatus::Done
                } else {
                    stored
                };
            let status =
                if settled == FiledFindingStatus::Open && job_status == Some(JobStatus::Planned) {
                    ShownFindingStatus::Planned
                } else {
                    settled.into()
                };
            FindingView {
                id: f.id,
                category: f.category,
                title: f.title,
                description: f.description,
                source_url: f.source_url,
                severity: f.severity,
                status,
                work_item: job.map(FindingJob::from),
                done_service,
                created_at: f.created_at,
                updated_at: f.updated_at,
                settled,
            }
        })
        .collect())
}

async fn view(
    db: &impl ConnectionTrait,
    finding: research_finding::Model,
) -> DomainResult<FindingView> {
    Ok(with_views(db, vec![finding]).await?.remove(0))
}

fn blank_to_none(text: Option<&str>) -> Option<String> {
    text.and_then(super::trim_to_none)
}

/// Files a finding on the vehicle and returns it as it reads, `open`.
///
/// # Errors
/// `Invalid` for the category `recall`, which only a recall check stores, and
/// for a blank title. `NotFound` for a vehicle that does not exist.
pub async fn file<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    input: NewFinding,
) -> DomainResult<FindingView> {
    refuse_recall(
        input.category,
        "a recall comes only from an NHTSA recall check and cannot be filed by hand",
    )?;
    super::validate_not_blank("title", &input.title)?;
    // The anchor report and the finding are written under one write lock, so
    // two first filings at once create one anchor.
    let txn = super::begin_write(db).await?;
    super::vehicle::require(&txn, vehicle_id).await?;

    // No unique index stops two anchors for one vehicle, so the oldest is
    // always the one picked.
    let anchor = research_report::Entity::find()
        .filter(research_report::Column::VehicleId.eq(vehicle_id))
        .filter(research_report::Column::ReportType.eq(ReportType::ExternalResearch))
        .order_by_asc(research_report::Column::Id)
        .one(&txn)
        .await?;
    let report = if let Some(report) = anchor {
        // The anchor dates from its newest filing.
        let mut active: research_report::ActiveModel = report.into();
        active.generated_at = Set(super::now());
        active.update(&txn).await?
    } else {
        research_report::ActiveModel {
            vehicle_id: Set(vehicle_id),
            report_type: Set(Some(ReportType::ExternalResearch)),
            ..Default::default()
        }
        .insert(&txn)
        .await?
    };

    let finding = research_finding::ActiveModel {
        report_id: Set(report.id),
        category: Set(input.category),
        title: Set(input.title),
        description: Set(blank_to_none(input.description.as_deref())),
        source_url: Set(blank_to_none(input.source_url.as_deref())),
        severity: Set(input.severity),
        status: Set(FindingStatus::New),
        ..Default::default()
    }
    .insert(&txn)
    .await?;
    let filed = view(&txn, finding).await?;
    txn.commit().await?;
    Ok(filed)
}

/// The vehicle's findings that pass `filter`, newest first, each with what
/// was done about it. No recall is listed.
///
/// # Errors
/// `Invalid` for a limit of zero and for the category `recall`. `NotFound`
/// for a vehicle that does not exist.
pub async fn list(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    filter: FindingFilter,
) -> DomainResult<Vec<FindingView>> {
    super::validate_limit(filter.limit)?;
    let mut query = filed(vehicle_id);
    if let Some(category) = filter.category {
        refuse_recall(
            category,
            "recalls are listed with the vehicle's recalls, not with its findings",
        )?;
        query = query.filter(research_finding::Column::Category.eq(category));
    }
    super::vehicle::require(db, vehicle_id).await?;
    let findings = query
        .order_by_desc(research_finding::Column::CreatedAt)
        .order_by_desc(research_finding::Column::Id)
        .all(db)
        .await?;

    // The status and the job are read from other rows, so both filters and
    // the limit are applied after them.
    let mut views = with_views(db, findings).await?;
    views.retain(|v| {
        filter.status.is_none_or(|status| v.status == status)
            && filter
                .has_planned_work
                .is_none_or(|wanted| v.has_planned_work() == wanted)
    });
    if let Some(limit) = filter.limit {
        views.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
    }
    Ok(views)
}

/// One finding of the vehicle with what was done about it.
///
/// # Errors
/// `NotFound` for an id that names no finding of the vehicle, a recall's id
/// included.
pub async fn get(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    finding_id: i32,
) -> DomainResult<FindingView> {
    let finding = require(db, vehicle_id, finding_id).await?;
    view(db, finding).await
}

/// Applies `input` to a finding and returns it as it reads afterwards.
///
/// # Errors
/// `NotFound` for an id that names no finding of the vehicle, a recall's id
/// included. `Invalid` for the category `recall` and for a blank title. A
/// finding whose job is done with none planned reads `done` until that job is
/// planned again or dropped, so asking it to read `open` is a `BadRequest`.
/// Nothing is written on any error.
pub async fn update<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    finding_id: i32,
    input: UpdateFinding,
) -> DomainResult<FindingView> {
    if let Some(category) = input.category {
        refuse_recall(
            category,
            "a finding cannot become a recall, which comes only from an NHTSA recall check",
        )?;
    }
    if let Some(title) = &input.title {
        super::validate_not_blank("title", title)?;
    }
    let txn = super::begin_write(db).await?;
    let finding = require(&txn, vehicle_id, finding_id).await?;

    if let Some(status) = input.status {
        // The requested status goes through the derivation every read uses,
        // so the refusal and the reads cannot disagree.
        let mut proposed = finding.clone();
        proposed.status = status.stored();
        if view(&txn, proposed).await?.settled != status {
            return Err(DomainError::BadRequest(format!(
                "Research finding {finding_id} has a job marked done, so it stays done until that \
                 job is planned again or dropped"
            )));
        }
    }

    let mut active: research_finding::ActiveModel = finding.into();
    if let Some(title) = input.title {
        active.title = Set(title);
    }
    if let Some(description) = input.description {
        active.description = Set(blank_to_none(description.as_deref()));
    }
    if let Some(category) = input.category {
        active.category = Set(category);
    }
    if let Some(severity) = input.severity {
        active.severity = Set(severity);
    }
    if let Some(source_url) = input.source_url {
        active.source_url = Set(blank_to_none(source_url.as_deref()));
    }
    if let Some(status) = input.status {
        active.status = Set(status.stored());
    }
    active.updated_at = Set(super::now());
    let updated = active.update(&txn).await?;
    let read = view(&txn, updated).await?;
    txn.commit().await?;
    Ok(read)
}

/// Deletes a finding. Every job that names it is kept and stops naming it,
/// and the reply lists those jobs.
///
/// # Errors
/// `NotFound` for an id that names no finding of the vehicle, a recall's id
/// included.
pub async fn delete<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    finding_id: i32,
) -> DomainResult<DeletedFinding> {
    let txn = super::begin_write(db).await?;
    let finding = require(&txn, vehicle_id, finding_id).await?;

    let jobs = work_item::Entity::find()
        .filter(work_item::Column::ResearchFindingId.eq(finding_id))
        .order_by_asc(work_item::Column::Id)
        .all(&txn)
        .await?;
    if !jobs.is_empty() {
        // The foreign key would clear the link by itself, without the stamp.
        work_item::Entity::update_many()
            .set(work_item::ActiveModel {
                research_finding_id: Set(None),
                updated_at: Set(super::now()),
                ..Default::default()
            })
            .filter(work_item::Column::ResearchFindingId.eq(finding_id))
            .exec(&txn)
            .await?;
    }
    research_finding::Entity::delete_by_id(finding_id)
        .exec(&txn)
        .await?;

    // A report exists to hold findings, so one with none left goes too.
    let left = research_finding::Entity::find()
        .filter(research_finding::Column::ReportId.eq(finding.report_id))
        .count(&txn)
        .await?;
    if left == 0 {
        research_report::Entity::delete_by_id(finding.report_id)
            .exec(&txn)
            .await?;
    }
    txn.commit().await?;
    Ok(DeletedFinding {
        id: finding.id,
        title: finding.title,
        unlinked_work_items: jobs.into_iter().map(FindingJob::from).collect(),
    })
}

// Where a severity sorts: in the order of its list, and none after them all.
fn severity_rank(severity: Option<FindingSeverity>) -> usize {
    severity
        .and_then(|s| FindingSeverity::ALL.iter().position(|listed| *listed == s))
        .unwrap_or(FindingSeverity::ALL.len())
}

/// The vehicle's findings that still wait for the owner: each reads `open`,
/// or reads `planned` with its job on no appointment. They are ordered by
/// severity, a finding with none last, and then newest first. No recall is
/// among them.
pub async fn open(db: &impl ConnectionTrait, vehicle_id: i32) -> DomainResult<OpenFindings> {
    let findings = filed(vehicle_id).all(db).await?;
    let mut waiting: Vec<FindingView> = with_views(db, findings)
        .await?
        .into_iter()
        .filter(FindingView::is_waiting)
        .collect();
    waiting.sort_by_key(|v| {
        (
            severity_rank(v.severity),
            std::cmp::Reverse((v.created_at, v.id)),
        )
    });
    let total = waiting.len();
    waiting.truncate(OPEN_FINDINGS_SHOWN);
    Ok(OpenFindings {
        findings: waiting,
        total,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        inputs::{
            appointment::NewAppointment, service_record::NewServiceRecord, work_item::NewWorkItem,
        },
        services::{
            appointment, recall, service_record as service_svc, work_item as work_item_svc,
        },
        test_support::{
            assert_empty, date, row_count, seed_recall, seed_vehicle, set_job_status, test_db,
        },
    };

    fn new_finding(category: FindingCategory, title: &str) -> NewFinding {
        NewFinding {
            category,
            title: title.into(),
            description: None,
            source_url: None,
            severity: None,
        }
    }

    async fn seed(db: &DatabaseConnection, vehicle_id: i32, title: &str) -> i32 {
        file(
            db,
            vehicle_id,
            new_finding(FindingCategory::KnownIssue, title),
        )
        .await
        .unwrap()
        .id
    }

    async fn plan_job(
        db: &DatabaseConnection,
        vehicle_id: i32,
        finding_id: i32,
        appointment_id: Option<i32>,
    ) -> i32 {
        work_item_svc::create(
            db,
            vehicle_id,
            NewWorkItem {
                title: "Replace the water pump".into(),
                research_finding_id: Some(finding_id),
                appointment_id,
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .id
    }

    async fn report_of(db: &DatabaseConnection, finding_id: i32) -> research_report::Model {
        let finding = research_finding::Entity::find_by_id(finding_id)
            .one(db)
            .await
            .unwrap()
            .unwrap();
        research_report::Entity::find_by_id(finding.report_id)
            .one(db)
            .await
            .unwrap()
            .unwrap()
    }

    fn titles(views: &[FindingView]) -> Vec<&str> {
        views.iter().map(|v| v.title.as_str()).collect()
    }

    fn mark(status: FiledFindingStatus) -> UpdateFinding {
        UpdateFinding {
            status: Some(status),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn a_filed_finding_reads_open_and_shares_the_vehicles_one_report() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let first = file(
            &db,
            vid,
            NewFinding {
                category: FindingCategory::SuggestedMaintenance,
                title: "DSG service interval is 40k, not 60k".into(),
                description: Some("Per community consensus for this gearbox".into()),
                source_url: Some(" ".into()),
                severity: Some(FindingSeverity::Recommended),
            },
        )
        .await
        .unwrap();
        assert_eq!(first.status, ShownFindingStatus::Open);
        assert_eq!(first.category, FindingCategory::SuggestedMaintenance);
        assert_eq!(first.severity, Some(FindingSeverity::Recommended));
        // A blank optional text is stored as none.
        assert_eq!(first.source_url, None);
        assert_eq!(
            (first.work_item, first.done_service.is_some()),
            (None, false)
        );

        let second = seed(&db, vid, "Second finding").await;
        let report = report_of(&db, first.id).await;
        assert_eq!(report_of(&db, second).await.id, report.id);
        assert_eq!(report.report_type, Some(ReportType::ExternalResearch));

        // Another vehicle gets its own report, which a lookup missing its
        // vehicle filter would not give it.
        let other = seed_vehicle(&db).await;
        let third = seed(&db, other, "Other car's finding").await;
        assert_ne!(report_of(&db, third).await.id, report.id);

        let err = file(&db, 999, new_finding(FindingCategory::Other, "x"))
            .await
            .unwrap_err();
        assert_eq!(err.to_string(), "Vehicle 999 not found");
    }

    #[tokio::test]
    async fn filing_refuses_a_recall_and_a_blank_title_and_writes_nothing() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;

        let err = file(&db, vid, new_finding(FindingCategory::Recall, "Airbag"))
            .await
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "category: a recall comes only from an NHTSA recall check and cannot be filed by hand"
        );
        let err = file(&db, vid, new_finding(FindingCategory::Other, "  "))
            .await
            .unwrap_err();
        assert_eq!(err.to_string(), "title: must not be blank");

        assert_eq!(row_count::<research_finding::Entity>(&db).await, 0);
        assert_eq!(row_count::<research_report::Entity>(&db).await, 0);
    }

    #[tokio::test]
    async fn a_finding_moves_from_open_to_planned_to_done_with_its_job_and_service() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let id = seed(&db, vid, "Water pump fails near 60k").await;

        let job = plan_job(&db, vid, id, None).await;
        let planned = get(&db, vid, id).await.unwrap();
        assert_eq!(planned.status, ShownFindingStatus::Planned);
        assert_eq!(
            planned.work_item,
            Some(FindingJob {
                id: job,
                title: "Replace the water pump".into(),
                status: JobStatus::Planned,
                appointment_id: None,
            })
        );
        assert!(planned.done_service.is_none());

        let service = service_svc::create(
            &db,
            vid,
            NewServiceRecord {
                service_date: date("2026-03-25"),
                work_item_ids: Some(vec![job]),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let done = get(&db, vid, id).await.unwrap();
        assert_eq!(done.status, ShownFindingStatus::Done);
        let job_shown = done.work_item.unwrap();
        assert_eq!((job_shown.id, job_shown.status), (job, JobStatus::Done));
        let by = done.done_service.unwrap();
        assert_eq!(
            (by.id, by.service_date),
            (service.record.id, date("2026-03-25"))
        );
        // Completing a job from a finding performs no recall.
        assert_eq!(
            row_count::<crate::entities::service_recall_link::Entity>(&db).await,
            0
        );
    }

    #[tokio::test]
    async fn a_job_done_by_hand_holds_the_finding_at_done_until_it_is_planned_or_dropped() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let id = seed(&db, vid, "Water pump fails near 60k").await;
        let job = plan_job(&db, vid, id, None).await;

        set_job_status(&db, vid, job, JobStatus::Done).await;
        let read = get(&db, vid, id).await.unwrap();
        assert_eq!(read.status, ShownFindingStatus::Done);
        assert!(read.done_service.is_none());
        let err = update(&db, vid, id, mark(FiledFindingStatus::Open))
            .await
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            format!(
                "Research finding {id} has a job marked done, so it stays done until that job is \
                 planned again or dropped"
            )
        );

        // A mark the owner sets wins over the done job.
        let dismissed = update(&db, vid, id, mark(FiledFindingStatus::Dismissed))
            .await
            .unwrap();
        assert_eq!(dismissed.status, ShownFindingStatus::Dismissed);
        update(&db, vid, id, mark(FiledFindingStatus::Done))
            .await
            .unwrap();

        // The finding stays done with a planned job, because it is marked so.
        set_job_status(&db, vid, job, JobStatus::Planned).await;
        assert_eq!(
            get(&db, vid, id).await.unwrap().status,
            ShownFindingStatus::Done
        );
        let open = update(&db, vid, id, mark(FiledFindingStatus::Open))
            .await
            .unwrap();
        assert_eq!(open.status, ShownFindingStatus::Planned);

        set_job_status(&db, vid, job, JobStatus::Dropped).await;
        let read = get(&db, vid, id).await.unwrap();
        assert_eq!(
            (read.status, read.work_item),
            (ShownFindingStatus::Open, None)
        );
    }

    #[tokio::test]
    async fn the_job_shown_is_the_booked_one_then_the_oldest_planned_then_the_latest_done() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let id = seed(&db, vid, "Water pump fails near 60k").await;
        let shown = async || get(&db, vid, id).await.unwrap().work_item.unwrap().id;

        let first = plan_job(&db, vid, id, None).await;
        let second = plan_job(&db, vid, id, None).await;
        assert_eq!(shown().await, first);

        let visit = appointment::create(
            &db,
            vid,
            NewAppointment {
                purpose: "60k service".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .id;
        let booked = plan_job(&db, vid, id, Some(visit)).await;
        assert_eq!(shown().await, booked);

        set_job_status(&db, vid, booked, JobStatus::Dropped).await;
        set_job_status(&db, vid, first, JobStatus::Done).await;
        assert_eq!(shown().await, second);
        set_job_status(&db, vid, second, JobStatus::Done).await;
        assert_eq!(shown().await, second);
    }

    #[tokio::test]
    async fn update_changes_what_is_sent_clears_with_none_and_leaves_the_rest() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let id = file(
            &db,
            vid,
            NewFinding {
                category: FindingCategory::ForumReport,
                title: "Pump".into(),
                description: Some("Leaks".into()),
                source_url: Some("https://example.com/t".into()),
                severity: Some(FindingSeverity::Optional),
            },
        )
        .await
        .unwrap()
        .id;

        let read = update(
            &db,
            vid,
            id,
            UpdateFinding {
                title: Some("Water pump fails near 60k".into()),
                category: Some(FindingCategory::KnownIssue),
                severity: Some(Some(FindingSeverity::Critical)),
                source_url: Some(None),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(read.title, "Water pump fails near 60k");
        assert_eq!(read.category, FindingCategory::KnownIssue);
        assert_eq!(read.severity, Some(FindingSeverity::Critical));
        assert_eq!(read.source_url, None);
        assert_eq!(read.description.as_deref(), Some("Leaks"));
        assert_eq!(read.status, ShownFindingStatus::Open);

        let read = update(
            &db,
            vid,
            id,
            UpdateFinding {
                description: Some(Some("  ".into())),
                severity: Some(None),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!((read.description, read.severity), (None, None));
        assert_eq!(read.title, "Water pump fails near 60k");
    }

    #[tokio::test]
    async fn update_refuses_the_recall_category_and_a_blank_title_and_writes_nothing() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let id = seed(&db, vid, "Pump").await;
        let stored = async || {
            research_finding::Entity::find_by_id(id)
                .one(&db)
                .await
                .unwrap()
        };
        let before = stored().await;

        let err = update(
            &db,
            vid,
            id,
            UpdateFinding {
                title: Some("Recall".into()),
                category: Some(FindingCategory::Recall),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert_eq!(
            err.to_string(),
            "category: a finding cannot become a recall, which comes only from an NHTSA recall \
             check"
        );
        let err = update(
            &db,
            vid,
            id,
            UpdateFinding {
                title: Some(" ".into()),
                status: Some(FiledFindingStatus::Done),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert_eq!(err.to_string(), "title: must not be blank");

        assert_eq!(stored().await, before);
    }

    #[tokio::test]
    async fn list_is_newest_first_and_filters_by_category_status_and_planned_work() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let pump = seed(&db, vid, "Pump").await;
        let coil = seed(&db, vid, "Coil packs").await;
        let intake = file(
            &db,
            vid,
            new_finding(FindingCategory::UpgradeIdea, "Intake"),
        )
        .await
        .unwrap()
        .id;
        let bulletin = file(
            &db,
            vid,
            new_finding(FindingCategory::ServiceBulletin, "Sunroof drain"),
        )
        .await
        .unwrap()
        .id;
        plan_job(&db, vid, coil, None).await;
        update(&db, vid, intake, mark(FiledFindingStatus::Dismissed))
            .await
            .unwrap();
        // Another vehicle's finding is on no list of this vehicle.
        let other = seed_vehicle(&db).await;
        seed(&db, other, "Other car").await;

        let listed = async |filter| {
            let views = list(&db, vid, filter).await.unwrap();
            views.into_iter().map(|v| v.id).collect::<Vec<i32>>()
        };
        assert_eq!(
            listed(FindingFilter::default()).await,
            vec![bulletin, intake, coil, pump]
        );
        assert_eq!(
            listed(FindingFilter {
                category: Some(FindingCategory::KnownIssue),
                ..Default::default()
            })
            .await,
            vec![coil, pump]
        );
        assert_eq!(
            listed(FindingFilter {
                status: Some(ShownFindingStatus::Open),
                ..Default::default()
            })
            .await,
            vec![bulletin, pump]
        );
        assert_eq!(
            listed(FindingFilter {
                status: Some(ShownFindingStatus::Planned),
                ..Default::default()
            })
            .await,
            vec![coil]
        );
        assert_eq!(
            listed(FindingFilter {
                has_planned_work: Some(false),
                limit: Some(2),
                ..Default::default()
            })
            .await,
            vec![bulletin, intake]
        );
        assert_eq!(
            listed(FindingFilter {
                has_planned_work: Some(true),
                ..Default::default()
            })
            .await,
            vec![coil]
        );
    }

    #[tokio::test]
    async fn list_refuses_a_limit_of_zero_the_recall_category_and_a_missing_vehicle() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;

        let err = list(
            &db,
            vid,
            FindingFilter {
                limit: Some(0),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert_eq!(err.to_string(), "limit: must be at least 1");
        let err = list(
            &db,
            vid,
            FindingFilter {
                category: Some(FindingCategory::Recall),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert_eq!(
            err.to_string(),
            "category: recalls are listed with the vehicle's recalls, not with its findings"
        );
        let err = list(&db, 999, FindingFilter::default()).await.unwrap_err();
        assert_eq!(err.to_string(), "Vehicle 999 not found");
    }

    #[tokio::test]
    async fn the_finding_list_and_the_recall_list_never_show_each_others_rows() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let recall_id = seed_recall(&db, vid, "24V110000", "Fuel pump").await;
        let finding_id = seed(&db, vid, "Water pump fails near 60k").await;

        let findings = list(&db, vid, FindingFilter::default()).await.unwrap();
        assert_eq!(titles(&findings), vec!["Water pump fails near 60k"]);
        assert_eq!(findings[0].id, finding_id);
        let recalls = recall::list(&db, vid).await.unwrap();
        let recall_ids: Vec<i32> = recalls.iter().map(|r| r.finding_id).collect();
        assert_eq!(recall_ids, vec![recall_id]);
    }

    // A wrong-vehicle id and a recall's id must read exactly as an id that
    // names nothing, so no function here says which ids exist.
    #[tokio::test]
    async fn another_vehicles_finding_a_recall_and_a_missing_id_are_the_same_not_found() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let other = seed_vehicle(&db).await;
        let foreign = seed(&db, other, "Other car's finding").await;
        let recall_id = seed_recall(&db, vid, "24V110000", "Fuel pump").await;
        let job = plan_job(&db, vid, recall_id, None).await;
        let stored = async || {
            let rows = research_finding::Entity::find()
                .order_by_asc(research_finding::Column::Id)
                .all(&db)
                .await
                .unwrap();
            (rows, work_item_svc::get(&db, vid, job).await.unwrap())
        };
        let before = stored().await;
        let change = || UpdateFinding {
            title: Some("Changed".into()),
            status: Some(FiledFindingStatus::Dismissed),
            ..Default::default()
        };

        for id in [foreign, recall_id, 9999] {
            let expected = format!("Research finding {id} not found");
            let errors = [
                get(&db, vid, id).await.unwrap_err(),
                update(&db, vid, id, change()).await.unwrap_err(),
                delete(&db, vid, id).await.unwrap_err(),
            ];
            for err in errors {
                assert!(matches!(err, DomainError::NotFound(_)), "{err:?}");
                assert_eq!(err.to_string(), expected);
            }
        }
        assert_eq!(stored().await, before);

        // A job that names a finding the vehicle does not have reads the same.
        let err = work_item_svc::create(
            &db,
            vid,
            NewWorkItem {
                title: "x".into(),
                research_finding_id: Some(9999),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert_eq!(err.to_string(), "Research finding 9999 not found");
    }

    #[tokio::test]
    async fn deleting_a_finding_keeps_its_jobs_and_names_the_ones_it_unlinked() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let id = seed(&db, vid, "Water pump fails near 60k").await;
        let kept = seed(&db, vid, "Coil packs").await;
        let planned = plan_job(&db, vid, id, None).await;
        let done = plan_job(&db, vid, id, None).await;
        let dropped = plan_job(&db, vid, id, None).await;
        let elsewhere = plan_job(&db, vid, kept, None).await;
        set_job_status(&db, vid, done, JobStatus::Done).await;
        set_job_status(&db, vid, dropped, JobStatus::Dropped).await;
        let report = report_of(&db, id).await.id;

        let out = delete(&db, vid, id).await.unwrap();

        assert_eq!(
            (out.id, out.title.as_str()),
            (id, "Water pump fails near 60k")
        );
        let unlinked: Vec<(i32, JobStatus)> = out
            .unlinked_work_items
            .iter()
            .map(|j| (j.id, j.status))
            .collect();
        assert_eq!(
            unlinked,
            vec![
                (planned, JobStatus::Planned),
                (done, JobStatus::Done),
                (dropped, JobStatus::Dropped)
            ]
        );
        for (job, status) in unlinked {
            let job = work_item_svc::get(&db, vid, job).await.unwrap();
            assert_eq!((job.research_finding_id, job.status), (None, status));
            assert_eq!(job.title, "Replace the water pump");
        }
        let other_job = work_item_svc::get(&db, vid, elsewhere).await.unwrap();
        assert_eq!(other_job.research_finding_id, Some(kept));
        // The report still holds a finding, so it stays.
        assert_eq!(report_of(&db, kept).await.id, report);
        let left = list(&db, vid, FindingFilter::default()).await.unwrap();
        assert_eq!(titles(&left), vec!["Coil packs"]);
    }

    #[tokio::test]
    async fn deleting_the_last_finding_of_a_report_deletes_the_report_and_no_other() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let recall_id = seed_recall(&db, vid, "24V110000", "Fuel pump").await;
        let first = seed(&db, vid, "Pump").await;
        let second = seed(&db, vid, "Coil packs").await;
        assert_eq!(row_count::<research_report::Entity>(&db).await, 2);

        assert_empty(&delete(&db, vid, first).await.unwrap().unlinked_work_items);
        assert_eq!(row_count::<research_report::Entity>(&db).await, 2);

        delete(&db, vid, second).await.unwrap();
        let reports = research_report::Entity::find().all(&db).await.unwrap();
        let kinds: Vec<_> = reports.iter().map(|r| r.report_type).collect();
        assert_eq!(kinds, vec![Some(ReportType::RecallsOnly)]);
        let recalls = recall::list(&db, vid).await.unwrap();
        assert_eq!(recalls[0].finding_id, recall_id);

        // The next filing starts a report again.
        let again = seed(&db, vid, "Pump").await;
        assert_eq!(
            report_of(&db, again).await.report_type,
            Some(ReportType::ExternalResearch)
        );
    }

    #[tokio::test]
    async fn the_waiting_findings_are_open_or_planned_off_an_appointment_most_severe_first() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let with = async |title: &str, severity| {
            let filed = NewFinding {
                severity,
                ..new_finding(FindingCategory::KnownIssue, title)
            };
            file(&db, vid, filed).await.unwrap().id
        };
        let critical = Some(FindingSeverity::Critical);
        with("Older critical", critical).await;
        with("No severity", None).await;
        let planned = with("Planned", Some(FindingSeverity::Recommended)).await;
        let booked = with("Booked", critical).await;
        let dismissed = with("Dismissed", critical).await;
        let marked_done = with("Marked done", critical).await;
        with("Informational", Some(FindingSeverity::Informational)).await;
        with("Optional", Some(FindingSeverity::Optional)).await;
        with("Newer critical", critical).await;
        seed_recall(&db, vid, "24V110000", "Fuel pump").await;
        seed(&db, seed_vehicle(&db).await, "Other car").await;

        let job = plan_job(&db, vid, planned, None).await;
        let visit = appointment::create(
            &db,
            vid,
            NewAppointment {
                purpose: "60k service".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .id;
        plan_job(&db, vid, booked, Some(visit)).await;
        update(&db, vid, dismissed, mark(FiledFindingStatus::Dismissed))
            .await
            .unwrap();
        update(&db, vid, marked_done, mark(FiledFindingStatus::Done))
            .await
            .unwrap();

        let waiting = open(&db, vid).await.unwrap();

        assert_eq!(
            titles(&waiting.findings),
            vec![
                "Newer critical",
                "Older critical",
                "Planned",
                "Optional",
                "Informational",
                "No severity"
            ]
        );
        assert_eq!(waiting.total, 6);
        let shown = waiting.findings[2].work_item.as_ref().unwrap();
        assert_eq!((shown.id, shown.status), (job, JobStatus::Planned));

        // A finding whose job is done by hand has stopped waiting.
        set_job_status(&db, vid, job, JobStatus::Done).await;
        assert_eq!(open(&db, vid).await.unwrap().total, 5);
    }

    #[tokio::test]
    async fn the_waiting_list_holds_the_first_ten_and_counts_them_all() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        for n in 0..OPEN_FINDINGS_SHOWN + 2 {
            seed(&db, vid, &format!("Finding {n}")).await;
        }

        let waiting = open(&db, vid).await.unwrap();

        assert_eq!(waiting.total, OPEN_FINDINGS_SHOWN + 2);
        assert_eq!(waiting.findings.len(), OPEN_FINDINGS_SHOWN);
        assert_eq!(waiting.findings[0].title, "Finding 11");
        assert_eq!(waiting.findings[9].title, "Finding 2");
    }

    // Deletes racing on one file from several pools all succeed: each opens
    // with the write lock, so its count of the report's findings reads what
    // the others committed.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_deletes_all_succeed_and_the_emptied_report_goes() {
        let (_dir, pools) = crate::test_support::race_pools().await;
        let first = &pools[0];
        let vid = seed_vehicle(first).await;
        let mut ids = Vec::new();
        for n in 0..pools.len() * 2 {
            let id = seed(first, vid, &format!("Finding {n}")).await;
            plan_job(first, vid, id, None).await;
            ids.push(id);
        }

        let results = crate::test_support::race(&pools, |i, db| {
            let id = ids[i];
            async move {
                delete(&db, vid, id)
                    .await
                    .map(|d| d.unlinked_work_items.len())
            }
        })
        .await;

        assert!(results.iter().all(|r| matches!(r, Ok(1))), "{results:?}");
        assert_eq!(row_count::<research_finding::Entity>(first).await, 0);
        assert_eq!(row_count::<research_report::Entity>(first).await, 0);
        assert_eq!(
            row_count::<work_item::Entity>(first).await,
            u64::try_from(ids.len()).unwrap()
        );
    }
}
