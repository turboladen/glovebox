# ADR-0028: Recalls come only from NHTSA

- **Status:** Accepted
- **Date:** 2026-11-24

## Context

A research finding is a fact worth acting on: a fix the community recommends before a part fails,
a failure the model is known for, a technical service bulletin. `file_research_finding` stores one
and `search_records` finds it by its text, but no tool lists, reads, corrects or deletes a
finding, so a new conversation cannot see what an earlier one saved, and a finding filed by
mistake stays.

Recalls are rows of the same table, `research_findings`, with the category `recall`. The recall
check stores them from NHTSA, and the recall service reads them with a status, links them to the
services that performed them and lets the owner mark one done or not applicable.
`file_research_finding` accepts any category, `recall` included, so a client can file a "recall"
that NHTSA never issued. Such a row has no campaign number, appears in `list_recalls` and
`whats_coming` beside the real ones, and can be named in a service's `recall_finding_ids`.

The category, the severity and a report's type are free text, compared with string literals in
three services. A category spelled `Recall` is silently not a recall.

## Decision

**Recalls come only from NHTSA.** `check_recalls` is the only writer of a finding whose category
is `recall`. Filing a finding as a recall is refused, and so is changing a finding's category to
`recall`.

**The finding tools and the recall tools never touch each other's rows.**
`list_research_findings`, `get_research_finding`, `update_research_finding` and
`delete_research_finding` (`services::finding`) work on the findings that are not recalls. To
them a recall's id is an id that names nothing: it gives the same `NotFound`, byte for byte, as a
missing id or another vehicle's finding, so the finding tools do not say which ids are recalls. A
recall is read with `list_recalls`, changed with `set_recall_status` and never deleted.
`plan_work` takes either kind in `research_finding_id`.

**The category, the severity and the report type are vocabularies** (ADR-0004):
`research_finding::FindingCategory` (`recall`, `forum_report`, `suggested_maintenance`,
`upgrade_idea`, `known_issue`, `service_bulletin`, `other`), `research_finding::FindingSeverity`
(`critical`, `recommended`, `optional`, `informational`) and `research_report::ReportType`
(`recalls_only`, `external_research`). The first four categories and the four severities are the
ones the tool's field descriptions name. `known_issue` and `service_bulletin` name two kinds the
tool description asks for, and `other` holds a fact none of the rest fits.
`m20261124_000002_finding_vocabularies` rewrites a stored value that differs from a listed one
only by ASCII case or surrounding spaces as the listed one, and a blank severity or report type as
NULL. It refuses to finish, with nothing changed, while a column holds any other value, and then
adds a CHECK per column.

**A finding reads a status that its job can decide**, the way a recall does. The stored status is
`new`, `completed` or `dismissed`, which a caller sets as `open` (still worth acting on), `done`
(acted on) or `dismissed` (decided against). A finding marked `done` or `dismissed` reads so.
One that is not marked reads `planned` while a planned job addresses it, `done` when a job from
it is done with none planned, and `open` otherwise. A read shows the job (`work_item`: a planned
one on an appointment, else the oldest planned one, else the latest done one) and the service
that completed it (`done_service`). Setting a status the finding already reads stores nothing,
so a client that sends back the `done` it read does not turn a status the job decides into a
mark that outlasts the job; `set_recall_status` follows the same rule. Setting `open` clears a
mark, and the finding then reads what its jobs say. It is refused only for a finding with no
mark that a done job holds at `done`, where nothing would change. A recall's `open` clears its
`done` or `not_applicable` mark under the same rule and the same refusal, and a recall linked to
a service refuses every change, because the link holds it at `done`.

**A search hit says which of the two it found.** `search_records` gives a finding the kind
`research_finding` and a recall the kind `recall`, both under the `research` scope, so a client
takes each id to the tools that read it.

**Deleting a finding keeps the jobs filed from it.** Each job keeps its work and its status and
stops naming the finding, and the reply lists those jobs. Two tables reference a finding:
`work_items.research_finding_id`, whose key is `ON DELETE SET NULL` and which the delete clears
itself so each job's `updated_at` is stamped, and `service_recall_links`, which cascades and
holds only recalls. A report whose last finding is deleted is deleted in the same transaction.

**`whats_coming` lists the findings still waiting** in `open_findings`: each finding that is not a
recall and reads `open`, or reads `planned` with its job on no appointment. A finding whose
planned job is on an appointment is left out, as the home page leaves out waiting work whose job
is booked (ADR-0002). The list is ordered by severity (`critical` first, a finding with none
last) and then newest first, and holds at most ten; `open_findings_total` counts them all.

## Alternatives

- **Hand-filed recalls with a required campaign number.** A recall the owner hears of before
  NHTSA lists it could be filed with its number and joined to the NHTSA row later. It needs a
  rule for which copy wins when the check finds the same campaign, and for a number typed wrong,
  and a recall of another market that NHTSA never lists would stay open forever. A recall notice
  the owner holds can be filed as a finding of the category `other`, which is planned and
  completed like any finding.
- **One set of tools for findings and recalls.** A recall's status, its service links and its
  campaign number have their own rules, and one `update` tool would have to refuse most of its
  arguments for a recall. A delete tool that reaches recalls lets a client remove a recall the
  next check would store again as open, losing a `not_applicable` mark.
- **A separate error for a recall's id.** "Use `set_recall_status`" would help a client that
  mixed the two up, but it tells which ids are recalls, and the cross-reference rule gives every
  wrong id the same answer.
- **A list of categories without `recall` for the tool argument.** The schema would not offer the
  value, and the refusal would be a bare "unknown variant". The domain refusal says where recalls
  come from.
- **Leaving stored free-text categories as they are.** A row whose category is outside the list
  cannot be read once the column is an enum.
- **Refusing to delete a finding a job names.** The owner would have to edit each job first, and
  the job's work does not depend on the finding that suggested it.

## Consequences

- On the owner's database as of the 2026-11-24 backup, `research_findings` holds five rows, all
  with the category `recall`, the severity `critical` and one `recalls_only` report, and no
  finding filed by hand. Every stored value is in its list as written, so the migration rewrites
  no row and no stored record breaks the refusal. The five recalls read as before.
- A database that holds a category outside the list, such as the `maintenance` or `note` an
  earlier client could file, does not migrate until the owner corrects the value.
- A client that filed a recall by hand gets a refusal that names the recall check. The project
  prompt and the tool description say to run `check_recalls`.
- A value added to one of the three vocabularies needs a migration that rewrites its CHECK, as
  `research_finding::FindingStatus` does.
- A finding's `report_id` and the report's type are bookkeeping, and no tool shows them.
- The web page shows no findings. The home page's waiting lists stay as ADR-0002 describes.
- A program built before this migration refuses to open a migrated database and says to rebuild.
