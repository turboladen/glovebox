//! To-dos: small tasks for a vehicle, optionally tied to an appointment.

use sea_orm::*;

use crate::{
    entities::todo,
    error::{DomainError, DomainResult},
    inputs::todo::{NewTodo, UpdateTodo},
};

/// Inserts a to-do for a vehicle, on an appointment when one is given. The
/// caller has already checked that the appointment belongs to the vehicle.
pub(crate) async fn insert(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    appointment_id: Option<i32>,
    title: String,
) -> DomainResult<todo::Model> {
    let model = todo::ActiveModel {
        vehicle_id: Set(vehicle_id),
        appointment_id: Set(appointment_id),
        title: Set(title),
        ..Default::default()
    };
    Ok(model.insert(db).await?)
}

/// Loads a to-do that belongs to `vehicle_id`.
///
/// # Errors
/// `NotFound` when it does not exist or belongs to another vehicle.
pub async fn require(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    id: i32,
) -> DomainResult<todo::Model> {
    todo::Entity::find_by_id(id)
        .filter(todo::Column::VehicleId.eq(vehicle_id))
        .one(db)
        .await?
        .ok_or_else(|| DomainError::NotFound(format!("To-do {id} not found")))
}

/// Adds a to-do to a vehicle, optionally on one of its appointments.
///
/// # Errors
/// `NotFound` for a vehicle or appointment that is not found, with an
/// appointment of another vehicle reading as missing; `Invalid` for a blank
/// title.
pub async fn add(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    input: NewTodo,
) -> DomainResult<todo::Model> {
    super::validate_not_blank("title", &input.title)?;
    super::vehicle::require(db, vehicle_id).await?;
    if let Some(id) = input.appointment_id {
        super::appointment::require(db, vehicle_id, id).await?;
    }
    let model = todo::ActiveModel {
        vehicle_id: Set(vehicle_id),
        appointment_id: Set(input.appointment_id),
        title: Set(input.title),
        notes: Set(input.notes),
        ..Default::default()
    };
    Ok(model.insert(db).await?)
}

/// Changes a to-do's title or notes, or moves it to another appointment of
/// the vehicle or off any appointment.
///
/// # Errors
/// `NotFound` as for [`add`] and for a to-do that is not the vehicle's;
/// `Invalid` for a blank title.
pub async fn update(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    id: i32,
    input: UpdateTodo,
) -> DomainResult<todo::Model> {
    let existing = require(db, vehicle_id, id).await?;
    if let Some(title) = &input.title {
        super::validate_not_blank("title", title)?;
    }
    if let Some(Some(appointment_id)) = input.appointment_id {
        super::appointment::require(db, vehicle_id, appointment_id).await?;
    }
    let mut active: todo::ActiveModel = existing.into();
    if let Some(v) = input.title {
        active.title = Set(v);
    }
    if let Some(v) = input.notes {
        active.notes = Set(v);
    }
    if let Some(v) = input.appointment_id {
        active.appointment_id = Set(v);
    }
    active.updated_at = Set(super::now_stamp());
    Ok(active.update(db).await?)
}

/// Edits a to-do and, when `done` is present, ticks it (`true`) or un-ticks it
/// (`false`), in one transaction. Ticking a to-do that is already done keeps
/// its original `done_on`; otherwise it is stamped with today's date.
///
/// # Errors
/// As for [`update`].
pub async fn update_with_done<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    id: i32,
    input: UpdateTodo,
    done: Option<bool>,
) -> DomainResult<todo::Model> {
    let txn = db.begin().await?;
    let edited = update(&txn, vehicle_id, id, input).await?;
    let result = match done {
        Some(true) if edited.done_on.is_some() => edited,
        Some(true) => complete(&txn, vehicle_id, id, None).await?,
        Some(false) => reopen(&txn, vehicle_id, id).await?,
        None => edited,
    };
    txn.commit().await?;
    Ok(result)
}

/// Marks a to-do done on `on` (`YYYY-MM-DD`), or today when `on` is absent.
///
/// # Errors
/// `NotFound` for a to-do that is not the vehicle's; `Invalid` for a
/// malformed date.
pub async fn complete(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    id: i32,
    on: Option<String>,
) -> DomainResult<todo::Model> {
    super::validate_optional_date("done_on", on.as_deref())?;
    let existing = require(db, vehicle_id, id).await?;
    let done_on = on.unwrap_or_else(crate::services::today_label);
    let mut active: todo::ActiveModel = existing.into();
    active.done_on = Set(Some(done_on));
    active.updated_at = Set(super::now_stamp());
    Ok(active.update(db).await?)
}

/// Marks a to-do not done.
///
/// # Errors
/// `NotFound` for a to-do that is not the vehicle's.
pub async fn reopen(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    id: i32,
) -> DomainResult<todo::Model> {
    let existing = require(db, vehicle_id, id).await?;
    let mut active: todo::ActiveModel = existing.into();
    active.done_on = Set(None);
    active.updated_at = Set(super::now_stamp());
    Ok(active.update(db).await?)
}

/// Deletes a to-do.
///
/// # Errors
/// `NotFound` for a to-do that is not the vehicle's.
pub async fn delete(db: &impl ConnectionTrait, vehicle_id: i32, id: i32) -> DomainResult<()> {
    require(db, vehicle_id, id).await?.delete(db).await?;
    Ok(())
}

/// Lists a vehicle's to-dos, oldest first. `appointment_id` of `None` lists
/// every to-do, `Some(None)` those on no appointment, and `Some(Some(id))`
/// those on that appointment. Done to-dos appear only with `include_done`.
///
/// # Errors
/// `NotFound` for a vehicle or appointment that is not found.
pub async fn list(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    appointment_id: Option<Option<i32>>,
    include_done: bool,
) -> DomainResult<Vec<todo::Model>> {
    super::vehicle::require(db, vehicle_id).await?;
    let mut query = todo::Entity::find().filter(todo::Column::VehicleId.eq(vehicle_id));
    match appointment_id {
        None => {}
        Some(None) => query = query.filter(todo::Column::AppointmentId.is_null()),
        Some(Some(id)) => {
            super::appointment::require(db, vehicle_id, id).await?;
            query = query.filter(todo::Column::AppointmentId.eq(id));
        }
    }
    if !include_done {
        query = query.filter(todo::Column::DoneOn.is_null());
    }
    Ok(query
        .order_by_asc(todo::Column::CreatedAt)
        .order_by_asc(todo::Column::Id)
        .all(db)
        .await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        error::DomainError,
        inputs::{
            appointment::NewAppointment,
            todo::{NewTodo, UpdateTodo},
        },
        services::appointment,
        test_support::{VehicleFixture, test_db},
    };

    fn new_todo(title: &str, appointment_id: Option<i32>) -> NewTodo {
        NewTodo {
            title: title.into(),
            appointment_id,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn todo_complete_and_reopen_round_trip() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let t = add(&db, v, new_todo("Wash car", None)).await.unwrap();
        assert!(t.done_on.is_none());
        let done = complete(&db, v, t.id, Some("2026-11-10".into()))
            .await
            .unwrap();
        assert_eq!(done.done_on.as_deref(), Some("2026-11-10"));
        let today = complete(&db, v, t.id, None).await.unwrap();
        assert_eq!(today.done_on, Some(crate::services::today_label()));
        let bad = complete(&db, v, t.id, Some("11/10".into()))
            .await
            .unwrap_err();
        assert!(matches!(bad, DomainError::Invalid { ref field, .. } if field == "done_on"));
        let reopened = reopen(&db, v, t.id).await.unwrap();
        assert!(reopened.done_on.is_none());
        assert!(list(&db, v, None, false).await.unwrap().len() == 1);
        complete(&db, v, t.id, None).await.unwrap();
        assert!(list(&db, v, None, false).await.unwrap().is_empty());
        assert_eq!(list(&db, v, None, true).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn update_with_done_ticks_unticks_and_keeps_the_first_date() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let t = add(&db, v, new_todo("Wash", None)).await.unwrap();
        complete(&db, v, t.id, Some("2026-11-10".into()))
            .await
            .unwrap();

        let again = update_with_done(&db, v, t.id, UpdateTodo::default(), Some(true))
            .await
            .unwrap();
        assert_eq!(again.done_on.as_deref(), Some("2026-11-10"));

        let open = update_with_done(
            &db,
            v,
            t.id,
            UpdateTodo {
                notes: Some(Some("two coats".into())),
                ..Default::default()
            },
            Some(false),
        )
        .await
        .unwrap();
        assert_eq!(open.done_on, None);
        assert_eq!(open.notes.as_deref(), Some("two coats"));

        let ticked = update_with_done(&db, v, t.id, UpdateTodo::default(), Some(true))
            .await
            .unwrap();
        assert_eq!(ticked.done_on, Some(crate::services::today_label()));
        let untouched = update_with_done(&db, v, t.id, UpdateTodo::default(), None)
            .await
            .unwrap();
        assert_eq!(untouched.done_on, ticked.done_on);
    }

    #[tokio::test]
    async fn a_refused_edit_leaves_the_tick_unapplied() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let t = add(&db, v, new_todo("Wash", None)).await.unwrap();
        let err = update_with_done(
            &db,
            v,
            t.id,
            UpdateTodo {
                title: Some(" ".into()),
                ..Default::default()
            },
            Some(true),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, DomainError::Invalid { .. }));
        assert_eq!(require(&db, v, t.id).await.unwrap().done_on, None);
    }

    #[tokio::test]
    async fn update_with_done_reads_a_wrong_vehicle_todo_as_missing() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let other = VehicleFixture::new().name("Other").insert_id(&db).await;
        let t = add(&db, v, new_todo("Wash", None)).await.unwrap();
        let missing = update_with_done(&db, other, 99_999, UpdateTodo::default(), Some(true))
            .await
            .unwrap_err();
        let wrong = update_with_done(&db, other, t.id, UpdateTodo::default(), Some(true))
            .await
            .unwrap_err();
        assert!(matches!(wrong, DomainError::NotFound(_)));
        assert_eq!(
            wrong.to_string().replace(&t.id.to_string(), "N"),
            missing.to_string().replace("99999", "N")
        );
    }

    #[tokio::test]
    async fn add_and_update_validate_the_title() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let err = add(&db, v, new_todo("  ", None)).await.unwrap_err();
        assert!(matches!(err, DomainError::Invalid { ref field, .. } if field == "title"));
        let t = add(&db, v, new_todo("Wash", None)).await.unwrap();
        let err = update(
            &db,
            v,
            t.id,
            UpdateTodo {
                title: Some(String::new()),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(err, DomainError::Invalid { .. }));
        let got = update(
            &db,
            v,
            t.id,
            UpdateTodo {
                title: Some("Wash and wax".into()),
                notes: Some(Some("two coats".into())),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(got.title, "Wash and wax");
        assert_eq!(got.notes.as_deref(), Some("two coats"));
        delete(&db, v, t.id).await.unwrap();
        assert!(matches!(
            delete(&db, v, t.id).await.unwrap_err(),
            DomainError::NotFound(_)
        ));
    }

    #[tokio::test]
    async fn list_filters_by_appointment() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let a = appointment::create(
            &db,
            v,
            NewAppointment {
                purpose: "Brakes".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let on = add(&db, v, new_todo("On", Some(a.id))).await.unwrap();
        let off = add(&db, v, new_todo("Off", None)).await.unwrap();
        assert_eq!(list(&db, v, None, true).await.unwrap().len(), 2);
        let ids = |todos: Vec<todo::Model>| todos.into_iter().map(|t| t.id).collect::<Vec<_>>();
        assert_eq!(
            ids(list(&db, v, Some(None), true).await.unwrap()),
            vec![off.id]
        );
        assert_eq!(
            ids(list(&db, v, Some(Some(a.id)), true).await.unwrap()),
            vec![on.id]
        );
        let moved = update(
            &db,
            v,
            on.id,
            UpdateTodo {
                appointment_id: Some(None),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(moved.appointment_id, None);
    }

    #[tokio::test]
    async fn todo_on_another_vehicles_appointment_reads_as_missing() {
        let db = test_db().await;
        let v1 = VehicleFixture::new().insert_id(&db).await;
        let v2 = VehicleFixture::new().name("Other").insert_id(&db).await;
        let theirs = appointment::create(
            &db,
            v2,
            NewAppointment {
                purpose: "Theirs".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let nonexistent = add(&db, v1, new_todo("x", Some(99_999))).await.unwrap_err();
        let wrong = add(&db, v1, new_todo("x", Some(theirs.id)))
            .await
            .unwrap_err();
        assert!(matches!(wrong, DomainError::NotFound(_)));
        assert_eq!(
            wrong.to_string().replace(&theirs.id.to_string(), "N"),
            nonexistent.to_string().replace("99999", "N")
        );
        let mine = add(&db, v1, new_todo("mine", None)).await.unwrap();
        let err = update(
            &db,
            v1,
            mine.id,
            UpdateTodo {
                appointment_id: Some(Some(theirs.id)),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(err, DomainError::NotFound(_)));
        let err = list(&db, v1, Some(Some(theirs.id)), true)
            .await
            .unwrap_err();
        assert!(matches!(err, DomainError::NotFound(_)));
    }

    #[tokio::test]
    async fn wrong_vehicle_todo_ids_read_as_missing() {
        let db = test_db().await;
        let v1 = VehicleFixture::new().insert_id(&db).await;
        let v2 = VehicleFixture::new().name("Other").insert_id(&db).await;
        let t = add(&db, v1, new_todo("mine", None)).await.unwrap();
        let missing = delete(&db, v1, 99_999).await.unwrap_err().to_string();
        let wrong = update(&db, v2, t.id, UpdateTodo::default())
            .await
            .unwrap_err();
        assert!(matches!(wrong, DomainError::NotFound(_)));
        assert_eq!(
            wrong.to_string().replace(&t.id.to_string(), "N"),
            missing.replace("99999", "N")
        );
        for err in [
            complete(&db, v2, t.id, None).await.unwrap_err(),
            reopen(&db, v2, t.id).await.unwrap_err(),
            delete(&db, v2, t.id).await.unwrap_err(),
        ] {
            assert!(matches!(err, DomainError::NotFound(_)));
            assert_eq!(
                err.to_string().replace(&t.id.to_string(), "N"),
                missing.replace("99999", "N")
            );
        }
        assert!(list(&db, v2, None, true).await.unwrap().is_empty());
    }
}
