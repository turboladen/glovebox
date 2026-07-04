use std::path::PathBuf;

use sea_orm::*;

use crate::{
    entities::{document, incident, part, service_record},
    error::{DomainError, DomainResult},
    inputs::document::{DocumentFilter, NewDocument, StoreDocument},
};

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

/// Validate, write the file under `files_dir`, and insert the document row.
///
/// The single storage path shared by the HTTP upload handler and the MCP
/// `attach_document` tool: size cap, filename sanitizing (traversal-proof),
/// vehicle self-guard, and the linked-entity cross-reference guard all live
/// here so neither surface can skip them. Layout on disk:
/// `{files_dir}/{vehicle_id or "general"}/{doc_type or "other"}/{timestamp}_{name}`.
pub async fn store(
    db: &impl ConnectionTrait,
    files_dir: &str,
    input: StoreDocument,
) -> DomainResult<document::Model> {
    if input.bytes.len() > MAX_FILE_BYTES {
        return Err(DomainError::BadRequest(format!(
            "File is {} bytes; the maximum is 10 MiB ({MAX_FILE_BYTES} bytes)",
            input.bytes.len()
        )));
    }
    let safe_name = sanitize_filename(&input.file_name);
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

    let vid_dir = input
        .vehicle_id
        .map_or_else(|| "general".into(), |v| v.to_string());
    let type_dir = sanitize_filename(input.doc_type.as_deref().unwrap_or("other"));
    let dir: PathBuf = [files_dir, &vid_dir, &type_dir].iter().collect();
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| DomainError::Internal(format!("failed to create files dir: {e}")))?;

    // Timestamp prefix avoids collisions between same-named uploads.
    let timestamp = chrono::Utc::now().format("%Y%m%d%H%M%S");
    let stored_name = format!("{timestamp}_{safe_name}");
    let full_path = dir.join(&stored_name);
    tokio::fs::write(&full_path, &input.bytes)
        .await
        .map_err(|e| DomainError::Internal(format!("failed to write file: {e}")))?;

    // Stored relative to the files_dir root (how /files serves it back).
    let relative_path = format!("{vid_dir}/{type_dir}/{stored_name}");
    // MAX_FILE_BYTES (10 MiB) fits i32 comfortably.
    let file_size_bytes = i32::try_from(input.bytes.len())
        .map_err(|_| DomainError::BadRequest("File too large".into()))?;

    create(
        db,
        NewDocument {
            vehicle_id: input.vehicle_id,
            title: input.title.unwrap_or_else(|| input.file_name.clone()),
            file_path: relative_path,
            file_name: input.file_name,
            mime_type: input.mime_type.or_else(|| mime_from_extension(&safe_name)),
            file_size_bytes: Some(file_size_bytes),
            doc_type: input.doc_type,
            linked_entity_type: input.linked_entity_type,
            linked_entity_id: input.linked_entity_id,
            notes: input.notes,
            extracted_text: input.extracted_text,
        },
    )
    .await
}

pub async fn delete(db: &impl ConnectionTrait, id: i32) -> DomainResult<()> {
    let result = document::Entity::delete_by_id(id).exec(db).await?;
    if result.rows_affected == 0 {
        return Err(DomainError::NotFound(format!("Document {id} not found")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::test_db;

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
        use crate::entities::vehicle;
        vehicle::ActiveModel {
            name: Set("Car".into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
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
            file_name: file_name.into(),
            bytes: b"fake pdf bytes".to_vec(),
            mime_type: None,
            doc_type: None,
            linked_entity_type: None,
            linked_entity_id: None,
            notes: None,
            extracted_text: None,
        }
    }

    #[tokio::test]
    async fn store_writes_file_and_row_with_extracted_text() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let svc_id = seed_service(&db, vid).await;
        let files = tempfile::tempdir().unwrap();

        let doc = store(
            &db,
            files.path().to_str().unwrap(),
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
            files.path().to_str().unwrap(),
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
            files.path().to_str().unwrap(),
            StoreDocument {
                bytes: vec![0u8; MAX_FILE_BYTES + 1],
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
            files.path().to_str().unwrap(),
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
                store(
                    &db,
                    files.path().to_str().unwrap(),
                    store_input(Some(vid), bad)
                )
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
            store(
                &db,
                files.path().to_str().unwrap(),
                store_input(Some(999), "a.pdf")
            )
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
        let wrong_parent = store(&db, files.path().to_str().unwrap(), link(foreign_svc))
            .await
            .unwrap_err();
        let nonexistent = store(&db, files.path().to_str().unwrap(), link(99_999))
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
    async fn store_rejects_unknown_link_type_and_half_links() {
        let db = test_db().await;
        let vid = seed_vehicle(&db).await;
        let files = tempfile::tempdir().unwrap();

        let err = store(
            &db,
            files.path().to_str().unwrap(),
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
                files.path().to_str().unwrap(),
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
                files.path().to_str().unwrap(),
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
                files.path().to_str().unwrap(),
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

    #[test]
    fn sanitize_filename_neutralizes_separators_and_keeps_safe_chars() {
        assert_eq!(
            sanitize_filename("invoice-2026_06.pdf"),
            "invoice-2026_06.pdf"
        );
        assert_eq!(sanitize_filename("../../etc/passwd"), ".._.._etc_passwd");
        assert_eq!(sanitize_filename(r"C:\temp\a b.pdf"), "C__temp_a_b.pdf");
    }
}
