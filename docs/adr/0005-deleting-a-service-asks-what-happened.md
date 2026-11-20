# ADR-0005: Deleting a service asks what happened to its work

- **Status:** Accepted
- **Date:** 2026-11-16

## Context

A service record does more than store an invoice. Recording it completes the jobs it names,
installs its parts, resolves its incidents and performs its recalls. Deleting it has to decide what
becomes of each of those, and the record alone cannot say. The owner may be deleting it because the
work never happened, or because the work did happen but this invoice is wrong: it belongs to
another shop or visit, or the owner did the work.

Before this decision, delete always undid everything except the recalls. Jobs went back to
planned, incidents reopened, and every installed part went back to purchased, including a part a
later service had already marked replaced. That part then read as an unfitted purchase that the
checklist offered to receive or return. Only recalls asked first, through a `recalls` choice of
`reopen` or `keep_done`.

## Decision

**A delete that would change completed work needs the owner's answer.** `service_record::delete`
takes `completed: Option<CompletedWork>`. When the service completed a job, installed a part,
resolved an incident or performed a recall, `None` is refused. The refusal names each item, and
nothing is written. A service that recorded none of these deletes without a choice. The MCP
argument `completed_work` replaces `recalls`, and the tool tells Claude to ask the owner rather
than choose.

**`did_not_happen` undoes the work.** Jobs return to planned, which reopens a hand-closed
appointment or takes the job off a cancelled one. Incidents go back to unresolved and recalls to
open, unless another service also resolved or performed them. An installed part goes back to
purchased with its install details cleared, and a shop-supplied one leaves its need.

**`done_another_way` keeps the work and drops only the invoice.** Jobs stay done and incidents stay
resolved. Recalls are stored as done. Parts keep their status and install date. All of them lose
their link to the deleted service.

**A replaced part stays replaced either way.** A later service took it off the car, so it was
installed at some point, whatever this invoice says. It keeps its status and its need. The same
rule applies when `correct_service` drops a part from `part_ids`: only a part that is still
installed goes back to purchased and leaves its need.

**The delete claims the service row first**, so the work named in a refusal is the work the delete
would undo.

**A correction that takes recorded work off a service asks the same question.** `correct_service`
replaces its `work_item_ids`, `part_ids` and `recall_finding_ids` lists. When the new lists leave
out a completed job, a fitted part or a performed recall, the call needs `completed_work`, is
refused naming each item without it, and passes the answer to the same release functions as the
delete. A job taken off takes the work that hangs off it: the incident and the recall it came
from, and the parts this service fitted to its needs, unless a list the call sends names that
work. A job added gets what `record_service` gives one: its incident resolved and its recall
performed. A correction that removes none of that work needs no answer.

## Alternatives considered

- **A choice for each kind of work** (jobs, parts, incidents, recalls). This is more precise, but
  the owner's answer is one fact about one invoice. A mixed answer is two corrections, each
  removing the items that share one answer.
- **Always undo, and warn in the reply.** The owner would learn what changed only after it had
  changed. A wrong guess also costs the most here, because the undo reopens appointments and
  unlinks parts.
- **Keeping the separate `recalls` choice beside the new one.** That would mean two questions about
  one fact, and they could be answered inconsistently.

## Consequences

- Claude has to ask before deleting any service that recorded work, which is most of them.
- `done_another_way` is for work with no invoice to file. When the invoice that did the work
  exists, the delete answers `did_not_happen` and that invoice is recorded, because only a
  recorded service can complete the jobs again and give the schedule items their baseline.
  Either answer removes the deleted service's schedule links.
- After `done_another_way`, a job is done with no completing service, the same state as a job set
  to done by hand. A recall it came from therefore reads done.
- Jobs, parts and recalls each have one release function that takes the answer
  (`work_item::release_completed`, `part::release_from_service`, `recall::release_from_service`),
  which `delete_service` and `correct_service` both call. Incidents have two:
  `correct_service` calls `incident::release_from_service` for the incidents of the jobs it takes
  off, and `delete_service` calls `incident::reopen_resolved` and removes all of the service's
  incident links. Each sits in the module of the apply it undoes, and
  `recall::unlink_from_service` removes a service's recall links before
  `recall::release_from_service` reads them.
- A job taken off a service takes the reminder baseline of its schedule item with it, and the
  schedule items the invoice named stay. ADR-0017 records how.
