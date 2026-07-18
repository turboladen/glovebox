use std::path::PathBuf;

use sea_orm::*;
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::{
    config::AppConfig,
    entities::{document, incident, part, service_record},
    error::{DomainError, DomainResult},
    inputs::document::{
        DocumentDisposition, DocumentFilter, DocumentSource, NewDocument, StoreDocument,
    },
};

/// A stored document plus the idempotency signal: whether an identical file
/// (same `vehicle_id` + `content_sha256`) already existed and was returned
/// instead of writing a duplicate. `Deref` to [`document::Model`] so callers
/// read the document's fields transparently; the flattened serialization
/// surfaces `already_present` to MCP clients.
#[derive(Debug, Serialize)]
pub struct StoredDocument {
    #[serde(flatten)]
    pub document: document::Model,
    pub already_present: bool,
}

impl std::ops::Deref for StoredDocument {
    type Target = document::Model;

    fn deref(&self) -> &Self::Target {
        &self.document
    }
}

/// Hex-encoded SHA-256 of `bytes` — the content-hash dedup key.
fn sha256_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

/// Decoded-size cap for stored files: 10 MiB.
pub const MAX_FILE_BYTES: usize = 10 * 1024 * 1024;

/// Entity kinds a document may link to (`documents.linked_entity_type`).
/// Matches the vocabulary the Records UI writes (`service`/`part`/`incident`).
pub const VALID_LINKED_ENTITY_TYPES: [&str; 3] = ["service", "part", "incident"];

pub async fn list(
    db: &impl ConnectionTrait,
    filter: DocumentFilter,
) -> DomainResult<Vec<document::Model>> {
    let mut select = document::Entity::find();

    if let Some(vid) = filter.vehicle_id {
        select = select.filter(document::Column::VehicleId.eq(vid));
    }
    if let Some(ref etype) = filter.linked_entity_type {
        select = select.filter(document::Column::LinkedEntityType.eq(etype.as_str()));
    }
    if let Some(eid) = filter.linked_entity_id {
        select = select.filter(document::Column::LinkedEntityId.eq(eid));
    }

    Ok(select
        .order_by_desc(document::Column::CreatedAt)
        .all(db)
        .await?)
}

pub async fn get(db: &impl ConnectionTrait, id: i32) -> DomainResult<document::Model> {
    document::Entity::find_by_id(id)
        .one(db)
        .await?
        .ok_or_else(|| DomainError::NotFound(format!("Document {id} not found")))
}

/// Persist a document row. The file bytes must already be written to disk by
/// the caller; `input.file_path` is the stored relative path.
pub async fn create(
    db: &impl ConnectionTrait,
    input: NewDocument,
) -> DomainResult<document::Model> {
    let model = document::ActiveModel {
        vehicle_id: Set(input.vehicle_id),
        title: Set(input.title),
        file_path: Set(input.file_path),
        file_name: Set(input.file_name),
        mime_type: Set(input.mime_type),
        file_size_bytes: Set(input.file_size_bytes),
        doc_type: Set(input.doc_type),
        linked_entity_type: Set(input.linked_entity_type),
        linked_entity_id: Set(input.linked_entity_id),
        notes: Set(input.notes),
        extracted_text: Set(input.extracted_text),
        content_sha256: Set(Some(input.content_sha256)),
        ..Default::default()
    };
    Ok(model.insert(db).await?)
}

/// Keep only filesystem-safe characters; everything else (including path
/// separators, so traversal like `../` cannot survive) becomes `_`.
pub fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Best-effort MIME type from the file extension, for callers (the MCP
/// surface) that don't carry one on the wire.
fn mime_from_extension(file_name: &str) -> Option<String> {
    let ext = file_name.rsplit_once('.')?.1.to_ascii_lowercase();
    let mime = match ext.as_str() {
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "heic" => "image/heic",
        "txt" => "text/plain",
        "csv" => "text/csv",
        "md" => "text/markdown",
        "html" | "htm" => "text/html",
        _ => return None,
    };
    Some(mime.to_string())
}

/// Resolve and read a [`DocumentSource::InboxPath`] file from `inbox_dir`.
///
/// The path must be relative and contain only plain components (no `..`,
/// no root) — violations are actionable `Invalid` errors. Containment is
/// then enforced by canonicalizing BOTH the inbox root and the joined path
/// and requiring prefix containment, so a symlink inside the inbox that
/// points outside it is rejected. To avoid an existence oracle for paths
/// outside the inbox, every resolution failure — missing file, missing
/// inbox dir, unreadable file, or symlink escape — is the same `NotFound`
/// naming only the inbox-relative path (never the server's absolute path).
async fn read_inbox_file(inbox_dir: &str, source_path: &str) -> DomainResult<Vec<u8>> {
    use std::path::{Component, Path};

    let rel = Path::new(source_path);
    if rel.is_absolute() {
        return Err(DomainError::Invalid {
            field: "source_path".into(),
            message: "must be a path relative to the inbox directory, not absolute".into(),
        });
    }
    if rel.components().any(|c| !matches!(c, Component::Normal(_))) {
        return Err(DomainError::Invalid {
            field: "source_path".into(),
            message: "must not contain '..' or other non-plain path components".into(),
        });
    }

    let not_found = || {
        DomainError::NotFound(format!(
            "No file named '{source_path}' in the inbox. Save the file into the inbox directory \
             first, then pass its inbox-relative path as source_path."
        ))
    };
    let root = tokio::fs::canonicalize(inbox_dir)
        .await
        .map_err(|_| not_found())?;
    let resolved = tokio::fs::canonicalize(root.join(rel))
        .await
        .map_err(|_| not_found())?;
    if !resolved.starts_with(&root) {
        // Symlink escape: byte-identical to the missing-file error above so
        // the response never reveals whether the outside target exists.
        return Err(not_found());
    }
    tokio::fs::read(&resolved).await.map_err(|_| not_found())
}

/// Cross-reference guard for `linked_entity_type`/`linked_entity_id`: the
/// target must exist AND belong to `vehicle_id`. A wrong-vehicle target is
/// byte-identical `NotFound` to a nonexistent one (no ownership oracle).
async fn require_linked_entity(
    db: &impl ConnectionTrait,
    vehicle_id: Option<i32>,
    entity_type: &str,
    entity_id: i32,
) -> DomainResult<()> {
    let Some(vehicle_id) = vehicle_id else {
        return Err(DomainError::BadRequest(
            "linked_entity_type/linked_entity_id require a vehicle_id".into(),
        ));
    };
    let found = match entity_type {
        "service" => service_record::Entity::find_by_id(entity_id)
            .filter(service_record::Column::VehicleId.eq(vehicle_id))
            .one(db)
            .await?
            .is_some(),
        "part" => part::Entity::find_by_id(entity_id)
            .filter(part::Column::VehicleId.eq(vehicle_id))
            .one(db)
            .await?
            .is_some(),
        "incident" => incident::Entity::find_by_id(entity_id)
            .filter(incident::Column::VehicleId.eq(vehicle_id))
            .one(db)
            .await?
            .is_some(),
        other => {
            return Err(DomainError::BadRequest(format!(
                "Invalid linked_entity_type '{}'. Must be one of: {}",
                other,
                VALID_LINKED_ENTITY_TYPES.join(", ")
            )));
        }
    };
    if !found {
        // Match the owning services' missing-record messages byte-for-byte.
        let noun = match entity_type {
            "service" => "Service record",
            "part" => "Part",
            _ => "Incident",
        };
        return Err(DomainError::NotFound(format!(
            "{noun} {entity_id} not found"
        )));
    }
    Ok(())
}

/// Validate, write the file under `config.files_dir`, and insert the
/// document row.
///
/// The single storage path shared by the HTTP upload handler and the MCP
/// `attach_document` tool: size cap, filename sanitizing (traversal-proof),
/// inbox path containment, vehicle self-guard, and the linked-entity
/// cross-reference guard all live here so neither surface can skip them.
/// A [`DocumentSource::InboxPath`] file is COPIED out of `config.inbox_dir`
/// — the inbox original is deliberately left in place (the LLM may retry,
/// and the user may still want the original). Layout on disk:
/// `{files_dir}/{vehicle_id or "general"}/{doc_type or "other"}/{timestamp}_{name}`.
#[allow(clippy::too_many_lines)] // one linear validate-then-write pipeline
pub async fn store(
    db: &impl ConnectionTrait,
    config: &AppConfig,
    input: StoreDocument,
) -> DomainResult<StoredDocument> {
    let (bytes, source_basename) = match input.source {
        DocumentSource::Bytes(bytes) => (bytes, None),
        DocumentSource::InboxPath(ref path) => {
            let bytes = read_inbox_file(&config.inbox_dir, path).await?;
            let basename = std::path::Path::new(path)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned());
            (bytes, basename)
        }
    };
    // The size cap applies AFTER read, whatever the source.
    if bytes.len() > MAX_FILE_BYTES {
        return Err(DomainError::BadRequest(format!(
            "File is {} bytes; the maximum is 10 MiB ({MAX_FILE_BYTES} bytes)",
            bytes.len()
        )));
    }
    // Content-hash dedup key: computed from the exact bytes we'd write.
    let content_sha256 = sha256_hex(&bytes);
    let Some(file_name) = input.file_name.or(source_basename) else {
        return Err(DomainError::Invalid {
            field: "file_name".into(),
            message: "required when the file is passed as bytes".into(),
        });
    };
    let safe_name = sanitize_filename(&file_name);
    if safe_name.trim_matches(['_', '.']).is_empty() {
        return Err(DomainError::Invalid {
            field: "file_name".into(),
            message: "must contain at least one filename character".into(),
        });
    }

    // Self-guard the vehicle and cross-reference-guard the link target
    // BEFORE any disk write.
    if let Some(vid) = input.vehicle_id {
        crate::services::vehicle::require(db, vid).await?;
    }
    match (&input.linked_entity_type, input.linked_entity_id) {
        (Some(etype), Some(eid)) => {
            require_linked_entity(db, input.vehicle_id, etype, eid).await?;
        }
        (None, None) => {}
        _ => {
            return Err(DomainError::BadRequest(
                "linked_entity_type and linked_entity_id must be provided together".into(),
            ));
        }
    }

    // Content-hash idempotency: if an identical file already exists for this
    // vehicle, return it instead of writing a second file + row. Matches on
    // `(vehicle_id, content_sha256)` INDEPENDENT of link state — so a retry
    // loop cannot create N copies of one PDF, and an orphan (unlinked) row can
    // be ADOPTED by a later call that supplies the link. The guards above have
    // already validated the incoming link, so adoption is safe here.
    let mut hash_match =
        document::Entity::find().filter(document::Column::ContentSha256.eq(content_sha256.clone()));
    hash_match = match input.vehicle_id {
        Some(vid) => hash_match.filter(document::Column::VehicleId.eq(vid)),
        None => hash_match.filter(document::Column::VehicleId.is_null()),
    };
    if let Some(existing) = hash_match.one(db).await? {
        // Adopt the incoming link ONLY when the existing row is a true orphan
        // (no link at all). A row already linked to ANY entity is left
        // untouched — never re-point it away from its prior target.
        let is_orphan =
            existing.linked_entity_type.is_none() && existing.linked_entity_id.is_none();
        let document = if is_orphan && input.linked_entity_type.is_some() {
            let mut active: document::ActiveModel = existing.into();
            active.linked_entity_type = Set(input.linked_entity_type);
            active.linked_entity_id = Set(input.linked_entity_id);
            active.update(db).await?
        } else {
            existing
        };
        return Ok(StoredDocument {
            document,
            already_present: true,
        });
    }

    let vid_dir = input
        .vehicle_id
        .map_or_else(|| "general".into(), |v| v.to_string());
    let mut type_dir = sanitize_filename(input.doc_type.as_deref().unwrap_or("other"));
    // Same emptiness rule as file_name: a dot/underscore-only doc_type would
    // produce an un-servable /files path (it can't escape files_dir, but the
    // row would point at a path the static server refuses).
    if type_dir.trim_matches(['_', '.']).is_empty() {
        type_dir = "other".into();
    }
    let dir: PathBuf = [config.files_dir.as_str(), &vid_dir, &type_dir]
        .iter()
        .collect();
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| DomainError::Internal(format!("failed to create files dir: {e}")))?;

    // Timestamp prefix namespaces same-named uploads, but its 1-second
    // resolution is beatable by a bulk-import loop (an LLM attaching several
    // invoices in one second) — so create with O_EXCL and retry with a
    // counter suffix instead of silently overwriting another row's bytes.
    let timestamp = chrono::Utc::now().format("%Y%m%d%H%M%S");
    let mut stored_name = format!("{timestamp}_{safe_name}");
    let mut full_path = dir.join(&stored_name);
    for counter in 1..=100 {
        match tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&full_path)
            .await
        {
            Ok(mut file) => {
                use tokio::io::AsyncWriteExt;
                file.write_all(&bytes)
                    .await
                    .map_err(|e| DomainError::Internal(format!("failed to write file: {e}")))?;
                // `tokio::fs::File` buffers; dropping it without an explicit
                // flush can drop the pending write (races through under load —
                // it stored an empty file intermittently on CI). Flush before
                // the handle drops so the bytes are guaranteed on disk.
                file.flush()
                    .await
                    .map_err(|e| DomainError::Internal(format!("failed to flush file: {e}")))?;
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                if counter == 100 {
                    return Err(DomainError::Internal(
                        "failed to find a free filename after 100 attempts".into(),
                    ));
                }
                stored_name = format!("{timestamp}_{counter}_{safe_name}");
                full_path = dir.join(&stored_name);
            }
            Err(e) => {
                return Err(DomainError::Internal(format!("failed to create file: {e}")));
            }
        }
    }

    // Stored relative to the files_dir root (how /files serves it back).
    let relative_path = format!("{vid_dir}/{type_dir}/{stored_name}");
    // MAX_FILE_BYTES (10 MiB) fits i32 comfortably.
    let file_size_bytes =
        i32::try_from(bytes.len()).map_err(|_| DomainError::BadRequest("File too large".into()))?;

    let document = create(
        db,
        NewDocument {
            vehicle_id: input.vehicle_id,
            title: input.title.unwrap_or_else(|| file_name.clone()),
            file_path: relative_path,
            file_name,
            mime_type: input.mime_type.or_else(|| mime_from_extension(&safe_name)),
            file_size_bytes: Some(file_size_bytes),
            doc_type: input.doc_type,
            linked_entity_type: input.linked_entity_type,
            linked_entity_id: input.linked_entity_id,
            notes: input.notes,
            extracted_text: input.extracted_text,
            content_sha256,
        },
    )
    .await?;
    Ok(StoredDocument {
        document,
        already_present: false,
    })
}

pub async fn delete(db: &impl ConnectionTrait, id: i32) -> DomainResult<()> {
    let result = document::Entity::delete_by_id(id).exec(db).await?;
    if result.rows_affected == 0 {
        return Err(DomainError::NotFound(format!("Document {id} not found")));
    }
    Ok(())
}

/// `notes` with `line` appended on a new line (or alone when there were none).
fn append_note(notes: Option<String>, line: &str) -> String {
    match notes {
        Some(existing) if !existing.trim().is_empty() => format!("{existing}\n{line}"),
        _ => line.to_string(),
    }
}

/// The provenance line written onto a document when its link is cleared.
fn unlink_note(entity_type: &str, entity_id: i32) -> String {
    let date = chrono::Utc::now().format("%Y-%m-%d");
    format!("Unlinked from {entity_type} #{entity_id} on {date} (record deleted)")
}

/// Handle the documents linked to an entity that is being deleted. `Keep`
/// clears both link fields and appends a provenance note (the row becomes a
/// true orphan, which the content-hash dedup path in [`store`] can re-adopt
/// on a later import); `Delete` removes the rows and returns their
/// `file_path`s so the caller can remove the files AFTER its transaction
/// commits (file removal is not transactional — an orphaned file on disk is
/// acceptable, a DB row pointing at a deleted record is not).
///
/// Selects on the link fields only, NOT `vehicle_id`: `vehicle_id` is
/// nullable on documents, the caller already vehicle-scoped the entity being
/// deleted, and the invariant this fn guarantees is "no dangling link
/// survives" — the link itself is the selector.
pub async fn detach_or_delete_for_entity(
    db: &impl ConnectionTrait,
    entity_type: &str,
    entity_id: i32,
    mode: DocumentDisposition,
) -> DomainResult<Vec<String>> {
    let linked = document::Entity::find()
        .filter(document::Column::LinkedEntityType.eq(entity_type))
        .filter(document::Column::LinkedEntityId.eq(entity_id))
        .all(db)
        .await?;

    match mode {
        DocumentDisposition::Keep => {
            let note = unlink_note(entity_type, entity_id);
            for doc in linked {
                let notes = doc.notes.clone();
                let mut active: document::ActiveModel = doc.into();
                active.linked_entity_type = Set(None);
                active.linked_entity_id = Set(None);
                active.notes = Set(Some(append_note(notes, &note)));
                active.update(db).await?;
            }
            Ok(Vec::new())
        }
        DocumentDisposition::Delete => {
            let file_paths: Vec<String> = linked.iter().map(|d| d.file_path.clone()).collect();
            let ids: Vec<i32> = linked.iter().map(|d| d.id).collect();
            if !ids.is_empty() {
                document::Entity::delete_many()
                    .filter(document::Column::Id.is_in(ids))
                    .exec(db)
                    .await?;
            }
            Ok(file_paths)
        }
    }
}

/// Clear a document's entity link (the `DocumentsTab` "Unlink" action, also the
/// cleanup for pre-existing orphans whose target is already gone). Appends the
/// same provenance note as the delete-with-keep path; succeeds as a no-op when
/// the document is already unlinked.
pub async fn unlink(db: &impl ConnectionTrait, id: i32) -> DomainResult<document::Model> {
    let doc = get(db, id).await?;
    let (Some(etype), Some(eid)) = (doc.linked_entity_type.clone(), doc.linked_entity_id) else {
        return Ok(doc);
    };
    let note = unlink_note(&etype, eid);
    let notes = doc.notes.clone();
    let mut active: document::ActiveModel = doc.into();
    active.linked_entity_type = Set(None);
    active.linked_entity_id = Set(None);
    active.notes = Set(Some(append_note(notes, &note)));
    Ok(active.update(db).await?)
}

/// Remove a stored file from the files dir by its row-relative `file_path`,
/// with the same containment check the upload path enforces (a `file_path`
/// that escapes `files_dir` is rejected, not followed). A missing file is Ok —
/// the row is already gone and that is the invariant that matters.
pub async fn remove_stored_file(config: &AppConfig, file_path: &str) -> DomainResult<()> {
    let files_dir = std::path::Path::new(&config.files_dir)
        .canonicalize()
        .map_err(|e| DomainError::Internal(format!("Invalid files_dir: {e}")))?;
    let full_path = files_dir.join(file_path);
    if !full_path.exists() {
        return Ok(());
    }
    let full_path = full_path
        .canonicalize()
        .map_err(|e| DomainError::Internal(e.to_string()))?;
    if !full_path.starts_with(&files_dir) {
        return Err(DomainError::BadRequest("Invalid file path".into()));
    }
    tokio::fs::remove_file(&full_path)
        .await
        .map_err(|e| DomainError::Internal(e.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{VehicleFixture, test_db};

    fn sample(title: &str) -> NewDocument {
        NewDocument {
            vehicle_id: None,
            title: title.into(),
            file_path: format!("general/other/{title}.pdf"),
            file_name: format!("{title}.pdf"),
            mime_type: Some("application/pdf".into()),
            file_size_bytes: Some(1024),
            doc_type: Some("invoice".into()),
            linked_entity_type: None,
            linked_entity_id: None,
            notes: None,
            extracted_text: None,
            content_sha256: "0".repeat(64),
        }
    }

    #[tokio::test]
    async fn create_then_get_round_trips() {
        let db = test_db().await;
        let created = create(&db, sample("receipt")).await.unwrap();
        let fetched = get(&db, created.id).await.unwrap();
        assert_eq!(fetched.title, "receipt");
        assert_eq!(fetched.file_size_bytes, Some(1024));
    }

    #[tokio::test]
    async fn list_filters_by_linked_entity() {
        let db = test_db().await;
        create(&db, sample("a")).await.unwrap();
        let mut linked = sample("b");
        linked.linked_entity_type = Some("service_record".into());
        linked.linked_entity_id = Some(5);
        create(&db, linked).await.unwrap();

        assert_eq!(list(&db, DocumentFilter::default()).await.unwrap().len(), 2);
        let filtered = list(
            &db,
            DocumentFilter {
                linked_entity_type: Some("service_record".into()),
                linked_entity_id: Some(5),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].title, "b");
    }

    #[tokio::test]
    async fn delete_removes_row() {
        let db = test_db().await;
        let created = create(&db, sample("gone")).await.unwrap();
        delete(&db, created.id).await.unwrap();
        assert!(matches!(
            get(&db, created.id).await.unwrap_err(),
            DomainError::NotFound(_)
        ));
    }

    #[tokio::test]
    async fn get_missing_is_not_found() {
        let db = test_db().await;
        assert!(matches!(
            get(&db, 999).await.unwrap_err(),
            DomainError::NotFound(_)
        ));
    }

    // ─── store(): shared file-writing path (HTTP upload + MCP attach) ───

    async fn seed_vehicle(db: &impl ConnectionTrait) -> i32 {
        VehicleFixture::new().insert_id(db).await
    }

    async fn seed_service(db: &impl ConnectionTrait, vehicle_id: i32) -> i32 {
        service_record::ActiveModel {
            vehicle_id: Set(vehicle_id),
            service_date: Set("2026-06-01".into()),
            paid_by: Set("self".into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    fn store_input(vehicle_id: Option<i32>, file_name: &str) -> StoreDocument {
        StoreDocument {
            vehicle_id,
            title: None,
            file_name: Some(file_name.into()),
            source: DocumentSource::Bytes(b"fake pdf bytes".to_vec()),
            mime_type: None,
            doc_type: None,
            linked_entity_type: None,
            linked_entity_id: None,
            notes: None,
            extracted_text: None,
        }
    }

    fn cfg(files_dir: &std::path::Path, inbox_dir: &std::path::Path) -> AppConfig {
        AppConfig {
            db_path: String::new(),
            listen: String::new(),
            files_dir: files_dir.to_string_lossy().into_owned(),
            inbox_dir: inbox_dir.to_string_lossy().into_owned(),
            public_url: String::new(),
        }
    }

    /// Config for tests that only exercise the bytes path: files under the
    /// tempdir, inbox pointed at a nonexistent sibling.
    fn files_cfg(files: &tempfile::TempDir) -> AppConfig {
        cfg(files.path(), &files.path().join("inbox"))
    }

    #[tokio::test]
    async fn store_writes_file_and_row_with_extracted_text() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let svc_id = seed_service(&db, vid).await;
        let files = tempfile::tempdir().unwrap();

        let doc = store(
            &db,
            &files_cfg(&files),
            StoreDocument {
                title: Some("FCP invoice".into()),
                extracted_text: Some("Sachs clutch kit $899".into()),
                linked_entity_type: Some("service".into()),
                linked_entity_id: Some(svc_id),
                ..store_input(Some(vid), "invoice.pdf")
            },
        )
        .await
        .unwrap();

        assert_eq!(doc.title, "FCP invoice");
        assert_eq!(doc.file_name, "invoice.pdf");
        assert_eq!(doc.mime_type.as_deref(), Some("application/pdf"));
        assert_eq!(doc.file_size_bytes, Some(14));
        assert_eq!(doc.linked_entity_type.as_deref(), Some("service"));
        assert_eq!(doc.linked_entity_id, Some(svc_id));
        assert_eq!(doc.extracted_text.as_deref(), Some("Sachs clutch kit $899"));
        assert!(doc.file_path.starts_with(&format!("{vid}/other/")));

        let on_disk = std::fs::read(files.path().join(&doc.file_path)).unwrap();
        assert_eq!(on_disk, b"fake pdf bytes");
    }

    #[tokio::test]
    async fn store_defaults_title_to_file_name() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let files = tempfile::tempdir().unwrap();
        let doc = store(
            &db,
            &files_cfg(&files),
            store_input(Some(vid), "receipt.png"),
        )
        .await
        .unwrap();
        assert_eq!(doc.title, "receipt.png");
        assert_eq!(doc.mime_type.as_deref(), Some("image/png"));
    }

    #[tokio::test]
    async fn store_rejects_over_cap_naming_the_limit() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let files = tempfile::tempdir().unwrap();
        let err = store(
            &db,
            &files_cfg(&files),
            StoreDocument {
                source: DocumentSource::Bytes(vec![0u8; MAX_FILE_BYTES + 1]),
                ..store_input(Some(vid), "huge.bin")
            },
        )
        .await
        .unwrap_err();
        match err {
            DomainError::BadRequest(msg) => {
                assert!(msg.contains("10 MiB"), "must name the cap: {msg}");
            }
            other => panic!("expected BadRequest, got {other:?}"),
        }
        assert!(
            list(&db, DocumentFilter::default())
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn store_neutralizes_path_traversal_in_file_name() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let files = tempfile::tempdir().unwrap();
        let doc = store(
            &db,
            &files_cfg(&files),
            store_input(Some(vid), "../../etc/passwd"),
        )
        .await
        .unwrap();

        // The stored path stays inside {vid}/other/ — separators became `_`,
        // so no path component is `..`.
        assert!(doc.file_path.starts_with(&format!("{vid}/other/")));
        assert!(doc.file_path.split('/').all(|c| c != ".." && !c.is_empty()));
        let full = files.path().join(&doc.file_path).canonicalize().unwrap();
        assert!(full.starts_with(files.path().canonicalize().unwrap()));
    }

    #[tokio::test]
    async fn store_rejects_file_name_without_real_characters() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let files = tempfile::tempdir().unwrap();
        for bad in ["", "../..", "///"] {
            assert!(matches!(
                store(&db, &files_cfg(&files), store_input(Some(vid), bad))
                    .await
                    .unwrap_err(),
                DomainError::Invalid { .. }
            ));
        }
    }

    #[tokio::test]
    async fn store_rejects_missing_vehicle() {
        let db = test_db().await;
        let files = tempfile::tempdir().unwrap();
        assert!(matches!(
            store(&db, &files_cfg(&files), store_input(Some(999), "a.pdf"))
                .await
                .unwrap_err(),
            DomainError::NotFound(_)
        ));
    }

    #[tokio::test]
    async fn store_wrong_vehicle_link_is_byte_identical_to_nonexistent() {
        let db = test_db().await;
        let owner = seed_vehicle(&db).await;
        let other = seed_vehicle(&db).await;
        let foreign_svc = seed_service(&db, other).await;
        let files = tempfile::tempdir().unwrap();

        let link = |eid: i32| StoreDocument {
            linked_entity_type: Some("service".into()),
            linked_entity_id: Some(eid),
            ..store_input(Some(owner), "invoice.pdf")
        };

        // Wrong-parent regression: another vehicle's record must read exactly
        // like a nonexistent one (no ownership oracle) — and nothing persists.
        let wrong_parent = store(&db, &files_cfg(&files), link(foreign_svc))
            .await
            .unwrap_err();
        let nonexistent = store(&db, &files_cfg(&files), link(99_999))
            .await
            .unwrap_err();
        assert_eq!(
            wrong_parent.to_string(),
            format!("Service record {foreign_svc} not found")
        );
        assert_eq!(
            nonexistent
                .to_string()
                .replace("99999", &foreign_svc.to_string()),
            wrong_parent.to_string()
        );
        assert!(
            list(&db, DocumentFilter::default())
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn store_wrong_vehicle_part_and_incident_links_are_byte_identical() {
        use crate::entities::{incident, part};
        let db = test_db().await;
        let owner = seed_vehicle(&db).await;
        let other = seed_vehicle(&db).await;
        let foreign_part = part::ActiveModel {
            vehicle_id: Set(other),
            name: Set("Filter".into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap()
        .id;
        let foreign_incident = incident::ActiveModel {
            vehicle_id: Set(other),
            category: Set("noise".into()),
            title: Set("Rattle".into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap()
        .id;
        let files = tempfile::tempdir().unwrap();

        // Per-link wrong-parent regression (house rule): part and incident
        // vocabularies read exactly like nonexistent ids too.
        for (etype, noun, foreign_id) in [
            ("part", "Part", foreign_part),
            ("incident", "Incident", foreign_incident),
        ] {
            let link = |eid: i32| StoreDocument {
                linked_entity_type: Some(etype.into()),
                linked_entity_id: Some(eid),
                ..store_input(Some(owner), "invoice.pdf")
            };
            let wrong_parent = store(&db, &files_cfg(&files), link(foreign_id))
                .await
                .unwrap_err();
            let nonexistent = store(&db, &files_cfg(&files), link(99_999))
                .await
                .unwrap_err();
            assert_eq!(
                wrong_parent.to_string(),
                format!("{noun} {foreign_id} not found")
            );
            assert_eq!(
                nonexistent
                    .to_string()
                    .replace("99999", &foreign_id.to_string()),
                wrong_parent.to_string()
            );
        }
    }

    #[tokio::test]
    async fn store_same_second_same_name_never_overwrites() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let files = tempfile::tempdir().unwrap();

        // Bulk-import case: several same-named attaches inside one timestamp
        // second must land as distinct files (O_EXCL + counter suffix), each
        // row pointing at its own bytes.
        let mut paths = std::collections::HashSet::new();
        for i in 0..3 {
            let mut input = store_input(Some(vid), "invoice.pdf");
            input.source = DocumentSource::Bytes(format!("payload {i}").into_bytes());
            let doc = store(&db, &files_cfg(&files), input).await.unwrap();
            let on_disk = tokio::fs::read(files.path().join(&doc.file_path))
                .await
                .unwrap();
            assert_eq!(on_disk, format!("payload {i}").into_bytes());
            assert!(
                paths.insert(doc.file_path.clone()),
                "path reused: {}",
                doc.file_path
            );
        }
    }

    // ─── content-hash idempotency (glovebox-hwaf) ───

    /// Store the same file name with identical bytes on the same vehicle: the
    /// second store dedupes to the first — same row, no second file — instead
    /// of writing a duplicate.
    #[tokio::test]
    async fn store_identical_bytes_returns_existing_no_second_file() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let files = tempfile::tempdir().unwrap();

        let first = store(
            &db,
            &files_cfg(&files),
            store_input(Some(vid), "invoice.pdf"),
        )
        .await
        .unwrap();
        assert!(!first.already_present);

        let second = store(
            &db,
            &files_cfg(&files),
            store_input(Some(vid), "invoice.pdf"),
        )
        .await
        .unwrap();
        assert!(second.already_present, "identical bytes must dedupe");
        assert_eq!(second.id, first.id, "must return the SAME row");

        // Exactly one row and one file on disk.
        assert_eq!(list(&db, DocumentFilter::default()).await.unwrap().len(), 1);
        assert_eq!(pdf_files_under(files.path()), 1);
    }

    /// Different bytes are distinct documents even with the same name/vehicle.
    #[tokio::test]
    async fn store_different_bytes_creates_new_row() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let files = tempfile::tempdir().unwrap();

        store(
            &db,
            &files_cfg(&files),
            store_input(Some(vid), "invoice.pdf"),
        )
        .await
        .unwrap();
        let second = store(
            &db,
            &files_cfg(&files),
            StoreDocument {
                source: DocumentSource::Bytes(b"totally different bytes".to_vec()),
                ..store_input(Some(vid), "invoice.pdf")
            },
        )
        .await
        .unwrap();
        assert!(!second.already_present);
        assert_eq!(list(&db, DocumentFilter::default()).await.unwrap().len(), 2);
    }

    /// The content-hash key is per-vehicle: identical bytes on two vehicles are
    /// distinct rows (they belong to different cars).
    #[tokio::test]
    async fn store_identical_bytes_different_vehicle_are_distinct() {
        let db = test_db().await;
        let v1 = seed_vehicle(&db).await;
        let v2 = seed_vehicle(&db).await;
        let files = tempfile::tempdir().unwrap();

        store(
            &db,
            &files_cfg(&files),
            store_input(Some(v1), "invoice.pdf"),
        )
        .await
        .unwrap();
        let second = store(
            &db,
            &files_cfg(&files),
            store_input(Some(v2), "invoice.pdf"),
        )
        .await
        .unwrap();
        assert!(!second.already_present, "different vehicle must not dedupe");
        assert_eq!(list(&db, DocumentFilter::default()).await.unwrap().len(), 2);
    }

    /// Orphan adoption: a first store with no link, then the same bytes WITH a
    /// link, dedupes to the same row AND adopts the link (the orphan gets
    /// relinked) — no second file/row.
    #[tokio::test]
    async fn store_orphan_is_adopted_by_later_link() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let svc_id = seed_service(&db, vid).await;
        let files = tempfile::tempdir().unwrap();

        let orphan = store(
            &db,
            &files_cfg(&files),
            store_input(Some(vid), "invoice.pdf"),
        )
        .await
        .unwrap();
        assert!(orphan.linked_entity_type.is_none());

        let adopted = store(
            &db,
            &files_cfg(&files),
            StoreDocument {
                linked_entity_type: Some("service".into()),
                linked_entity_id: Some(svc_id),
                ..store_input(Some(vid), "invoice.pdf")
            },
        )
        .await
        .unwrap();
        assert!(adopted.already_present);
        assert_eq!(adopted.id, orphan.id);
        assert_eq!(adopted.linked_entity_type.as_deref(), Some("service"));
        assert_eq!(adopted.linked_entity_id, Some(svc_id));
        assert_eq!(list(&db, DocumentFilter::default()).await.unwrap().len(), 1);
        assert_eq!(pdf_files_under(files.path()), 1);
    }

    /// A dedup match that is ALREADY linked to a different entity is never
    /// re-pointed: the incoming link does not steal it.
    #[tokio::test]
    async fn store_dedup_never_steals_an_existing_link() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let svc_a = seed_service(&db, vid).await;
        let svc_b = seed_service(&db, vid).await;
        let files = tempfile::tempdir().unwrap();

        let first = store(
            &db,
            &files_cfg(&files),
            StoreDocument {
                linked_entity_type: Some("service".into()),
                linked_entity_id: Some(svc_a),
                ..store_input(Some(vid), "invoice.pdf")
            },
        )
        .await
        .unwrap();

        let second = store(
            &db,
            &files_cfg(&files),
            StoreDocument {
                linked_entity_type: Some("service".into()),
                linked_entity_id: Some(svc_b),
                ..store_input(Some(vid), "invoice.pdf")
            },
        )
        .await
        .unwrap();
        assert!(second.already_present);
        assert_eq!(second.id, first.id);
        assert_eq!(
            second.linked_entity_id,
            Some(svc_a),
            "existing link must NOT be stolen by a later attach"
        );
        assert_eq!(list(&db, DocumentFilter::default()).await.unwrap().len(), 1);
    }

    /// Count stored `*.pdf` files anywhere under the files dir.
    fn pdf_files_under(root: &std::path::Path) -> usize {
        fn walk(dir: &std::path::Path, acc: &mut usize) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, acc);
                } else if path.extension().is_some_and(|e| e == "pdf") {
                    *acc += 1;
                }
            }
        }
        let mut count = 0;
        walk(root, &mut count);
        count
    }

    #[tokio::test]
    async fn store_rejects_unknown_link_type_and_half_links() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let files = tempfile::tempdir().unwrap();

        let err = store(
            &db,
            &files_cfg(&files),
            StoreDocument {
                linked_entity_type: Some("build".into()),
                linked_entity_id: Some(1),
                ..store_input(Some(vid), "a.pdf")
            },
        )
        .await
        .unwrap_err();
        match err {
            DomainError::BadRequest(msg) => {
                assert!(msg.contains("service") && msg.contains("incident"), "{msg}");
            }
            other => panic!("expected BadRequest, got {other:?}"),
        }

        // Type without id (and vice versa) is a clean BadRequest.
        assert!(matches!(
            store(
                &db,
                &files_cfg(&files),
                StoreDocument {
                    linked_entity_type: Some("service".into()),
                    ..store_input(Some(vid), "a.pdf")
                },
            )
            .await
            .unwrap_err(),
            DomainError::BadRequest(_)
        ));
        assert!(matches!(
            store(
                &db,
                &files_cfg(&files),
                StoreDocument {
                    linked_entity_id: Some(1),
                    ..store_input(Some(vid), "a.pdf")
                },
            )
            .await
            .unwrap_err(),
            DomainError::BadRequest(_)
        ));
    }

    #[tokio::test]
    async fn store_link_requires_a_vehicle() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let svc_id = seed_service(&db, vid).await;
        let files = tempfile::tempdir().unwrap();
        assert!(matches!(
            store(
                &db,
                &files_cfg(&files),
                StoreDocument {
                    linked_entity_type: Some("service".into()),
                    linked_entity_id: Some(svc_id),
                    ..store_input(None, "a.pdf")
                },
            )
            .await
            .unwrap_err(),
            DomainError::BadRequest(_)
        ));
    }

    // ─── store(): inbox source (`DocumentSource::InboxPath`) ────────────

    fn inbox_input(vehicle_id: Option<i32>, source_path: &str) -> StoreDocument {
        StoreDocument {
            file_name: None,
            source: DocumentSource::InboxPath(source_path.into()),
            ..store_input(vehicle_id, "ignored")
        }
    }

    #[tokio::test]
    async fn store_inbox_copies_file_and_keeps_the_original() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let files = tempfile::tempdir().unwrap();
        let inbox = tempfile::tempdir().unwrap();
        std::fs::write(inbox.path().join("invoice.pdf"), b"real invoice bytes").unwrap();

        let doc = store(
            &db,
            &cfg(files.path(), inbox.path()),
            inbox_input(Some(vid), "invoice.pdf"),
        )
        .await
        .unwrap();

        // file_name defaults to the inbox file's basename.
        assert_eq!(doc.file_name, "invoice.pdf");
        assert_eq!(doc.title, "invoice.pdf");
        assert_eq!(doc.mime_type.as_deref(), Some("application/pdf"));
        assert_eq!(doc.file_size_bytes, Some(18));
        let on_disk = std::fs::read(files.path().join(&doc.file_path)).unwrap();
        assert_eq!(on_disk, b"real invoice bytes");
        // COPY semantics: the inbox original must still be there, intact.
        assert_eq!(
            std::fs::read(inbox.path().join("invoice.pdf")).unwrap(),
            b"real invoice bytes"
        );
    }

    #[tokio::test]
    async fn store_inbox_accepts_subdirectory_paths() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let files = tempfile::tempdir().unwrap();
        let inbox = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(inbox.path().join("scans/june")).unwrap();
        std::fs::write(inbox.path().join("scans/june/invoice.pdf"), b"scan").unwrap();

        let doc = store(
            &db,
            &cfg(files.path(), inbox.path()),
            inbox_input(Some(vid), "scans/june/invoice.pdf"),
        )
        .await
        .unwrap();
        // Basename only — the inbox subdirectories don't leak into the name.
        assert_eq!(doc.file_name, "invoice.pdf");
        assert_eq!(
            std::fs::read(files.path().join(&doc.file_path)).unwrap(),
            b"scan"
        );
    }

    #[tokio::test]
    async fn store_inbox_explicit_file_name_overrides_basename() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let files = tempfile::tempdir().unwrap();
        let inbox = tempfile::tempdir().unwrap();
        std::fs::write(inbox.path().join("upload_tmp_8341"), b"bytes").unwrap();

        let doc = store(
            &db,
            &cfg(files.path(), inbox.path()),
            StoreDocument {
                file_name: Some("fcp-invoice.pdf".into()),
                ..inbox_input(Some(vid), "upload_tmp_8341")
            },
        )
        .await
        .unwrap();
        assert_eq!(doc.file_name, "fcp-invoice.pdf");
        assert_eq!(doc.mime_type.as_deref(), Some("application/pdf"));
    }

    #[tokio::test]
    async fn store_inbox_rejects_absolute_paths() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let files = tempfile::tempdir().unwrap();
        let inbox = tempfile::tempdir().unwrap();

        let err = store(
            &db,
            &cfg(files.path(), inbox.path()),
            inbox_input(Some(vid), "/etc/passwd"),
        )
        .await
        .unwrap_err();
        match err {
            DomainError::Invalid { field, message } => {
                assert_eq!(field, "source_path");
                assert!(message.contains("relative"), "{message}");
            }
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn store_inbox_rejects_parent_dir_components() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let files = tempfile::tempdir().unwrap();
        let inbox = tempfile::tempdir().unwrap();

        for bad in ["../outside.pdf", "scans/../../outside.pdf", "./invoice.pdf"] {
            let err = store(
                &db,
                &cfg(files.path(), inbox.path()),
                inbox_input(Some(vid), bad),
            )
            .await
            .unwrap_err();
            assert!(
                matches!(&err, DomainError::Invalid { field, .. } if field == "source_path"),
                "{bad} must be Invalid(source_path), got {err:?}"
            );
        }
    }

    #[tokio::test]
    async fn store_inbox_missing_file_names_the_relative_path_only() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let files = tempfile::tempdir().unwrap();
        let inbox = tempfile::tempdir().unwrap();

        let err = store(
            &db,
            &cfg(files.path(), inbox.path()),
            inbox_input(Some(vid), "no-such.pdf"),
        )
        .await
        .unwrap_err();
        match err {
            DomainError::NotFound(msg) => {
                assert!(msg.contains("'no-such.pdf'"), "{msg}");
                // The server's absolute inbox path must not leak.
                assert!(
                    !msg.contains(inbox.path().to_str().unwrap()),
                    "absolute server path leaked: {msg}"
                );
            }
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn store_inbox_symlink_escape_reads_exactly_like_missing() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let files = tempfile::tempdir().unwrap();
        let inbox = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret.txt"), b"secret").unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("secret.txt"),
            inbox.path().join("link.pdf"),
        )
        .unwrap();

        let config = cfg(files.path(), inbox.path());
        // A symlink escaping the inbox and a plain missing file must produce
        // byte-identical NotFound errors (modulo the requested name) — no
        // oracle for whether the outside target exists.
        let escape = store(&db, &config, inbox_input(Some(vid), "link.pdf"))
            .await
            .unwrap_err();
        let missing = store(&db, &config, inbox_input(Some(vid), "missing.pdf"))
            .await
            .unwrap_err();
        assert!(matches!(escape, DomainError::NotFound(_)), "{escape:?}");
        assert_eq!(
            escape.to_string().replace("link.pdf", "missing.pdf"),
            missing.to_string()
        );
        // Nothing persisted, on disk or in the DB.
        assert!(
            list(&db, DocumentFilter::default())
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn store_inbox_rejects_over_cap_files() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let files = tempfile::tempdir().unwrap();
        let inbox = tempfile::tempdir().unwrap();
        std::fs::write(inbox.path().join("huge.bin"), vec![0u8; MAX_FILE_BYTES + 1]).unwrap();

        let err = store(
            &db,
            &cfg(files.path(), inbox.path()),
            inbox_input(Some(vid), "huge.bin"),
        )
        .await
        .unwrap_err();
        match err {
            DomainError::BadRequest(msg) => assert!(msg.contains("10 MiB"), "{msg}"),
            other => panic!("expected BadRequest, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn store_bytes_without_file_name_is_invalid() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let files = tempfile::tempdir().unwrap();
        assert!(matches!(
            store(
                &db,
                &files_cfg(&files),
                StoreDocument {
                    file_name: None,
                    ..store_input(Some(vid), "ignored")
                },
            )
            .await
            .unwrap_err(),
            DomainError::Invalid { .. }
        ));
    }

    #[test]
    fn sanitize_filename_neutralizes_separators_and_keeps_safe_chars() {
        assert_eq!(
            sanitize_filename("invoice-2026_06.pdf"),
            "invoice-2026_06.pdf"
        );
        assert_eq!(sanitize_filename("../../etc/passwd"), ".._.._etc_passwd");
        assert_eq!(sanitize_filename(r"C:\temp\a b.pdf"), "C__temp_a_b.pdf");
    }

    // ─── detach_or_delete_for_entity / unlink / remove_stored_file ───

    fn linked_sample(title: &str, etype: &str, eid: i32, notes: Option<&str>) -> NewDocument {
        NewDocument {
            linked_entity_type: Some(etype.into()),
            linked_entity_id: Some(eid),
            notes: notes.map(Into::into),
            ..sample(title)
        }
    }

    #[tokio::test]
    async fn detach_keep_clears_links_and_appends_note() {
        let db = test_db().await;
        let with_notes = create(&db, linked_sample("a", "service", 7, Some("prior note")))
            .await
            .unwrap();
        let without_notes = create(&db, linked_sample("b", "service", 7, None))
            .await
            .unwrap();
        // Same id, different entity type — must be untouched.
        let other_type = create(&db, linked_sample("c", "part", 7, None))
            .await
            .unwrap();
        let unlinked = create(&db, sample("d")).await.unwrap();

        let paths = detach_or_delete_for_entity(&db, "service", 7, DocumentDisposition::Keep)
            .await
            .unwrap();
        assert!(paths.is_empty(), "Keep mode returns no files to remove");

        let a = get(&db, with_notes.id).await.unwrap();
        assert_eq!(a.linked_entity_type, None);
        assert_eq!(a.linked_entity_id, None);
        let a_notes = a.notes.unwrap();
        assert!(a_notes.starts_with("prior note\nUnlinked from service #7 on "));
        assert!(a_notes.ends_with("(record deleted)"));

        let b = get(&db, without_notes.id).await.unwrap();
        assert_eq!(b.linked_entity_type, None);
        let b_notes = b.notes.unwrap();
        assert!(b_notes.starts_with("Unlinked from service #7 on "));

        let c = get(&db, other_type.id).await.unwrap();
        assert_eq!(c.linked_entity_type.as_deref(), Some("part"));
        assert_eq!(c.linked_entity_id, Some(7));
        assert_eq!(c.notes, None);

        let d = get(&db, unlinked.id).await.unwrap();
        assert_eq!(d.notes, None);
    }

    #[tokio::test]
    async fn detach_delete_removes_only_matching_rows_and_returns_paths() {
        let db = test_db().await;
        let doomed_a = create(&db, linked_sample("a", "service", 7, None))
            .await
            .unwrap();
        let doomed_b = create(&db, linked_sample("b", "service", 7, None))
            .await
            .unwrap();
        let other_id = create(&db, linked_sample("c", "service", 8, None))
            .await
            .unwrap();
        let other_type = create(&db, linked_sample("d", "part", 7, None))
            .await
            .unwrap();

        let mut paths = detach_or_delete_for_entity(&db, "service", 7, DocumentDisposition::Delete)
            .await
            .unwrap();
        paths.sort();
        assert_eq!(
            paths,
            vec![doomed_a.file_path.clone(), doomed_b.file_path.clone()]
        );

        assert!(matches!(
            get(&db, doomed_a.id).await.unwrap_err(),
            DomainError::NotFound(_)
        ));
        assert!(matches!(
            get(&db, doomed_b.id).await.unwrap_err(),
            DomainError::NotFound(_)
        ));
        assert!(get(&db, other_id.id).await.is_ok());
        assert!(get(&db, other_type.id).await.is_ok());
    }

    #[tokio::test]
    async fn unlink_clears_link_notes_provenance_and_is_idempotent() {
        let db = test_db().await;
        let doc = create(&db, linked_sample("orphan-me", "incident", 3, None))
            .await
            .unwrap();

        let unlinked = unlink(&db, doc.id).await.unwrap();
        assert_eq!(unlinked.linked_entity_type, None);
        assert_eq!(unlinked.linked_entity_id, None);
        let notes = unlinked.notes.clone().unwrap();
        assert!(notes.starts_with("Unlinked from incident #3 on "));

        // Already unlinked → no-op success, no second note.
        let again = unlink(&db, doc.id).await.unwrap();
        assert_eq!(again.notes.as_ref(), Some(&notes));

        assert!(matches!(
            unlink(&db, 9999).await.unwrap_err(),
            DomainError::NotFound(_)
        ));
    }

    #[tokio::test]
    async fn remove_stored_file_removes_tolerates_missing_and_rejects_escape() {
        let root = tempfile::tempdir().unwrap();
        let files_dir = root.path().join("files");
        std::fs::create_dir_all(files_dir.join("1/invoice")).unwrap();
        std::fs::write(files_dir.join("1/invoice/a.pdf"), b"bytes").unwrap();
        std::fs::write(root.path().join("secret.txt"), b"outside").unwrap();
        let config = cfg(&files_dir, &root.path().join("inbox"));

        remove_stored_file(&config, "1/invoice/a.pdf")
            .await
            .unwrap();
        assert!(!files_dir.join("1/invoice/a.pdf").exists());

        // Already gone: fine — the row is what matters.
        remove_stored_file(&config, "1/invoice/a.pdf")
            .await
            .unwrap();

        // A file_path escaping files_dir is refused, not followed.
        assert!(matches!(
            remove_stored_file(&config, "../secret.txt")
                .await
                .unwrap_err(),
            DomainError::BadRequest(_)
        ));
        assert!(root.path().join("secret.txt").exists());
    }

    /// The glovebox-9fbj ⇄ glovebox-hwaf interaction: delete-with-Keep leaves
    /// a TRUE orphan (both link fields cleared), which the content-hash dedup
    /// in `store` adopts on reimport instead of duplicating.
    #[tokio::test]
    async fn service_delete_keep_leaves_orphan_that_dedup_readopts() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let svc_id = seed_service(&db, vid).await;
        let files = tempfile::tempdir().unwrap();
        let config = files_cfg(&files);

        let doc = store(
            &db,
            &config,
            StoreDocument {
                linked_entity_type: Some("service".into()),
                linked_entity_id: Some(svc_id),
                ..store_input(Some(vid), "invoice.pdf")
            },
        )
        .await
        .unwrap();

        crate::services::service_record::delete(&db, vid, svc_id, DocumentDisposition::Keep)
            .await
            .unwrap();

        let orphan = get(&db, doc.id).await.unwrap();
        assert_eq!(orphan.linked_entity_type, None);
        assert!(orphan.notes.unwrap().contains("Unlinked from service"));

        // Reimport of the same bytes with a fresh link adopts the orphan.
        let new_svc = seed_service(&db, vid).await;
        let readopted = store(
            &db,
            &config,
            StoreDocument {
                linked_entity_type: Some("service".into()),
                linked_entity_id: Some(new_svc),
                ..store_input(Some(vid), "invoice.pdf")
            },
        )
        .await
        .unwrap();
        assert!(readopted.already_present);
        assert_eq!(readopted.id, doc.id);
        assert_eq!(readopted.linked_entity_id, Some(new_svc));
    }

    #[tokio::test]
    async fn service_delete_cascade_removes_doc_rows_and_returns_paths() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let svc_id = seed_service(&db, vid).await;
        let files = tempfile::tempdir().unwrap();

        let doc = store(
            &db,
            &files_cfg(&files),
            StoreDocument {
                linked_entity_type: Some("service".into()),
                linked_entity_id: Some(svc_id),
                ..store_input(Some(vid), "invoice.pdf")
            },
        )
        .await
        .unwrap();

        let paths =
            crate::services::service_record::delete(&db, vid, svc_id, DocumentDisposition::Delete)
                .await
                .unwrap();
        assert_eq!(paths, vec![doc.file_path.clone()]);
        assert!(matches!(
            get(&db, doc.id).await.unwrap_err(),
            DomainError::NotFound(_)
        ));
    }
}
