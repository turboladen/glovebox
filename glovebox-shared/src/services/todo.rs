//! To-dos: small tasks for a vehicle, optionally tied to an appointment.

use sea_orm::{sea_query::Expr, *};

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
    input: NewTodo,
) -> DomainResult<todo::Model> {
    let model = todo::ActiveModel {
        vehicle_id: Set(vehicle_id),
        appointment_id: Set(input.appointment_id),
        title: Set(input.title),
        notes: Set(input.notes),
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
        .ok_or_else(|| not_found(id))
}

/// Adds a to-do to a vehicle, optionally on one of its appointments.
///
/// # Errors
/// `NotFound` for a vehicle or appointment that is not found, with an
/// appointment of another vehicle reading as missing; `BadRequest` for an
/// appointment that is closed or cancelled; `Invalid` for a blank title.
pub async fn add<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    input: NewTodo,
) -> DomainResult<todo::Model> {
    super::validate_not_blank("title", &input.title)?;
    super::vehicle::require(db, vehicle_id).await?;
    // Claiming the appointment first keeps a close from landing between the
    // check and the insert.
    let txn = db.begin().await?;
    if let Some(appointment_id) = input.appointment_id {
        super::appointment::claim_assignable(&txn, vehicle_id, appointment_id).await?;
    }
    let created = insert(&txn, vehicle_id, input).await?;
    txn.commit().await?;
    Ok(created)
}

// Changes a to-do's title or notes, or moves it to another appointment of
// the vehicle or off any appointment. It runs inside `update_with_done`'s
// transaction after the to-do's claim, which already holds SQLite's write
// lock, so the appointment check and the write see the same state.
async fn update(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    id: i32,
    input: UpdateTodo,
) -> DomainResult<todo::Model> {
    let existing = require(db, vehicle_id, id).await?;
    if let Some(title) = &input.title {
        super::validate_not_blank("title", title)?;
    }
    // Moving a to-do onto a closed or cancelled appointment is refused; one
    // already there may still be edited.
    if let Some(Some(appointment_id)) = input.appointment_id
        && existing.appointment_id != Some(appointment_id)
    {
        super::appointment::require_assignable(db, vehicle_id, appointment_id).await?;
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
    active.updated_at = Set(super::now());
    Ok(active.update(db).await?)
}

/// Edits a to-do and, when `done` is present, ticks it (`true`) or un-ticks it
/// (`false`), in one transaction. Ticking a to-do that is already done keeps
/// its original `done_on`; otherwise it is stamped with today's date.
///
/// # Errors
/// `NotFound` for a to-do that is not the vehicle's or an appointment that is
/// not found; `BadRequest` for a move onto an appointment that is closed or
/// cancelled; `Invalid` for a blank title.
pub async fn update_with_done<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    id: i32,
    input: UpdateTodo,
    done: Option<bool>,
) -> DomainResult<todo::Model> {
    let txn = db.begin().await?;
    claim(&txn, id).await?;
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

// Writes the to-do row without changing it, as `appointment::claim` does for
// an appointment, so a transaction takes the write lock before it reads.
async fn claim(txn: &impl ConnectionTrait, id: i32) -> DomainResult<()> {
    todo::Entity::update_many()
        .col_expr(todo::Column::UpdatedAt, Expr::col(todo::Column::UpdatedAt))
        .filter(todo::Column::Id.eq(id))
        .exec(txn)
        .await?;
    Ok(())
}

/// Marks a to-do done on `on`, or today when `on` is absent.
///
/// # Errors
/// `NotFound` for a to-do that is not the vehicle's.
pub async fn complete(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    id: i32,
    on: Option<chrono::NaiveDate>,
) -> DomainResult<todo::Model> {
    let existing = require(db, vehicle_id, id).await?;
    let done_on = on.unwrap_or_else(crate::services::today);
    let mut active: todo::ActiveModel = existing.into();
    active.done_on = Set(Some(done_on));
    active.updated_at = Set(super::now());
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
    active.updated_at = Set(super::now());
    Ok(active.update(db).await?)
}

/// Deletes a to-do and returns the row exactly as this call deleted it, which
/// [`restore`] can put back.
///
/// # Errors
/// `NotFound` for a to-do that is not the vehicle's.
pub async fn delete(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    id: i32,
) -> DomainResult<todo::Model> {
    // One statement deletes and returns the row, so the row returned is the
    // one deleted even when the other program edits or deletes it at once.
    todo::Entity::delete_many()
        .filter(todo::Column::Id.eq(id))
        .filter(todo::Column::VehicleId.eq(vehicle_id))
        .exec_with_returning(db)
        .await?
        .pop()
        .ok_or_else(|| not_found(id))
}

/// Puts back to-do `id`, which [`delete`] returned as `deleted`, at its own id
/// with its own title, notes, done date and creation stamp. It goes back on
/// its appointment only while that appointment is the vehicle's and open, and
/// on no appointment otherwise. With `deleted` absent, a to-do that is already
/// back is returned as it is.
///
/// # Errors
/// `NotFound` for a `deleted` row that is not to-do `id` of the vehicle, or
/// for a vehicle that is not found; `BadRequest` when `deleted` is absent and
/// the to-do is not back, or when `deleted` is given and the to-do is already
/// back.
pub async fn restore<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    vehicle_id: i32,
    id: i32,
    deleted: Option<todo::Model>,
) -> DomainResult<todo::Model> {
    // A caller that no longer holds the row, such as the page answering a
    // second undo, learns whether the to-do is already back. The kept
    // creation stamp keeps the to-do's place in its list.
    let Some(deleted) = deleted else {
        return match require(db, vehicle_id, id).await {
            Err(DomainError::NotFound(_)) => Err(DomainError::BadRequest(
                "That delete can no longer be undone.".to_owned(),
            )),
            found => found,
        };
    };
    if deleted.vehicle_id != vehicle_id || deleted.id != id {
        return Err(not_found(id));
    }
    super::vehicle::require(db, vehicle_id).await?;
    let txn = db.begin().await?;
    // The insert is the transaction's first statement, so it takes the write
    // lock before anything is read, and a row already at the id makes it
    // insert nothing instead of failing on the primary key.
    let mut restored = todo::Model {
        appointment_id: None,
        updated_at: super::now(),
        ..deleted
    };
    let row: todo::ActiveModel = restored.clone().into();
    let inserted = todo::Entity::insert(row.reset_all())
        .on_conflict_do_nothing()
        .exec_without_returning(&txn)
        .await?;
    if !matches!(inserted, TryInsertResult::Inserted(n) if n > 0) {
        return Err(DomainError::BadRequest(format!(
            "To-do {id} is already back on the list."
        )));
    }
    restored.appointment_id =
        super::appointment::open_id(&txn, vehicle_id, deleted.appointment_id).await?;
    // The inserted row already carries a fresh `updated_at`, so this update
    // sets only the appointment.
    if restored.appointment_id.is_some() {
        todo::Entity::update_many()
            .col_expr(
                todo::Column::AppointmentId,
                Expr::value(restored.appointment_id),
            )
            .filter(todo::Column::Id.eq(id))
            .exec(&txn)
            .await?;
    }
    txn.commit().await?;
    Ok(restored)
}

fn not_found(id: i32) -> DomainError {
    DomainError::NotFound(format!("To-do {id} not found"))
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
    if let Some(Some(id)) = appointment_id {
        super::appointment::require(db, vehicle_id, id).await?;
    }
    list_unchecked(db, &[vehicle_id], appointment_id, include_done).await
}

/// Runs the query of [`list`] for vehicles and an appointment the caller has
/// already checked, across every vehicle in `vehicle_ids` at once.
pub(crate) async fn list_unchecked(
    db: &impl ConnectionTrait,
    vehicle_ids: &[i32],
    appointment_id: Option<Option<i32>>,
    include_done: bool,
) -> DomainResult<Vec<todo::Model>> {
    let mut query =
        todo::Entity::find().filter(todo::Column::VehicleId.is_in(vehicle_ids.iter().copied()));
    match appointment_id {
        None => {}
        Some(None) => query = query.filter(todo::Column::AppointmentId.is_null()),
        Some(Some(id)) => query = query.filter(todo::Column::AppointmentId.eq(id)),
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
        test_support::{VehicleFixture, date, race_pools, row_count, test_db},
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
        let done = complete(&db, v, t.id, Some(date("2026-11-10")))
            .await
            .unwrap();
        assert_eq!(done.done_on, Some(date("2026-11-10")));
        let today = complete(&db, v, t.id, None).await.unwrap();
        assert_eq!(today.done_on, Some(crate::services::today()));
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
        complete(&db, v, t.id, Some(date("2026-11-10")))
            .await
            .unwrap();

        let again = update_with_done(&db, v, t.id, UpdateTodo::default(), Some(true))
            .await
            .unwrap();
        assert_eq!(again.done_on, Some(date("2026-11-10")));

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
        assert_eq!(ticked.done_on, Some(crate::services::today()));
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

    async fn appointment_id(db: &DatabaseConnection, v: i32, purpose: &str) -> i32 {
        appointment::create(
            db,
            v,
            NewAppointment {
                purpose: purpose.into(),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .id
    }

    #[tokio::test]
    async fn a_todo_cannot_be_added_to_or_moved_onto_a_closed_or_cancelled_appointment() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let closed = appointment_id(&db, v, "Brakes").await;
        let cancelled = appointment_id(&db, v, "Tires").await;
        let kept = add(&db, v, new_todo("Bring the key", Some(closed)))
            .await
            .unwrap();
        appointment::close(&db, v, closed).await.unwrap();
        appointment::cancel(&db, v, cancelled).await.unwrap();
        let loose = add(&db, v, new_todo("Loose", None)).await.unwrap();

        for (id, ending) in [(closed, "closed"), (cancelled, "cancelled")] {
            let expected = format!("Appointment {id} is {ending}; reopen it first");
            let err = add(&db, v, new_todo("Late", Some(id))).await.unwrap_err();
            assert_eq!(err.to_string(), expected);
            let move_onto = UpdateTodo {
                appointment_id: Some(Some(id)),
                ..Default::default()
            };
            let err = update_with_done(&db, v, loose.id, move_onto, None)
                .await
                .unwrap_err();
            assert_eq!(err.to_string(), expected);
        }
        assert_eq!(row_count::<todo::Entity>(&db).await, 2);
        assert_eq!(
            require(&db, v, loose.id).await.unwrap().appointment_id,
            None
        );

        // A to-do already on a closed appointment can still be edited.
        let edit = UpdateTodo {
            title: Some("Bring both keys".into()),
            appointment_id: Some(Some(closed)),
            ..Default::default()
        };
        let edited = update_with_done(&db, v, kept.id, edit, None).await.unwrap();
        assert_eq!(
            (edited.title.as_str(), edited.appointment_id),
            ("Bring both keys", Some(closed))
        );
    }

    #[tokio::test]
    async fn adding_todos_while_the_appointment_closes_never_fails_on_the_lock() {
        let (_dir, pools) = race_pools().await;
        let first = &pools[0];
        let v = VehicleFixture::new().insert_id(first).await;
        let a = appointment_id(first, v, "Brakes").await;

        let mut adds = tokio::task::JoinSet::new();
        for (i, db) in pools.iter().chain(pools.iter()).cloned().enumerate() {
            adds.spawn(async move { add(&db, v, new_todo(&format!("To-do {i}"), Some(a))).await });
        }
        let closer = pools[1].clone();
        let closed = tokio::spawn(async move { appointment::close(&closer, v, a).await });
        let results = adds.join_all().await;
        closed.await.unwrap().unwrap();

        for result in &results {
            match result {
                Ok(_) => {}
                Err(DomainError::BadRequest(m)) if m.contains("is closed") => {}
                other => panic!("unexpected result {other:?}"),
            }
        }
    }

    #[tokio::test]
    async fn restore_puts_a_deleted_todo_back_as_it_was() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let a = appointment_id(&db, v, "Brakes").await;
        let first = add(&db, v, new_todo("First", Some(a))).await.unwrap();
        let t = add(
            &db,
            v,
            NewTodo {
                title: "Return the pads".into(),
                notes: Some("RMA 42".into()),
                appointment_id: Some(a),
            },
        )
        .await
        .unwrap();
        let last = add(&db, v, new_todo("Last", Some(a))).await.unwrap();
        // The web page never deletes a done to-do; ticking first covers the
        // restore contract for any caller, not a path the page takes.
        let t = complete(&db, v, t.id, Some(date("2026-11-10")))
            .await
            .unwrap();
        // An older creation stamp than its neighbors' makes it list first, so
        // the order below shows the stamp survived.
        let mut older: todo::ActiveModel = t.into();
        older.created_at = Set("2026-01-01T00:00:00Z".parse().unwrap());
        let t = older.update(&db).await.unwrap();

        let deleted = delete(&db, v, t.id).await.unwrap();
        assert_eq!(deleted, t);
        assert!(require(&db, v, t.id).await.is_err());

        let back = restore(&db, v, t.id, Some(deleted)).await.unwrap();
        assert_eq!(back.id, t.id);
        assert_eq!(
            (
                back.vehicle_id,
                back.appointment_id,
                &back.title,
                &back.notes,
                back.done_on,
                back.created_at
            ),
            (v, Some(a), &t.title, &t.notes, t.done_on, t.created_at)
        );
        assert!(back.updated_at >= t.updated_at);
        assert_eq!(require(&db, v, t.id).await.unwrap(), back);
        let ids = list(&db, v, Some(Some(a)), true)
            .await
            .unwrap()
            .into_iter()
            .map(|t| t.id)
            .collect::<Vec<_>>();
        assert_eq!(ids, vec![t.id, first.id, last.id]);

        let newer = add(&db, v, new_todo("Newer", None)).await.unwrap();
        assert!(newer.id > last.id, "{} after {}", newer.id, last.id);
    }

    #[tokio::test]
    async fn restore_leaves_a_closed_cancelled_or_deleted_appointment_off() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        for ending in ["closed", "cancelled", "deleted"] {
            let a = appointment_id(&db, v, ending).await;
            let t = add(&db, v, new_todo(ending, Some(a))).await.unwrap();
            let deleted = delete(&db, v, t.id).await.unwrap();
            match ending {
                "closed" => drop(appointment::close(&db, v, a).await.unwrap()),
                "cancelled" => drop(appointment::cancel(&db, v, a).await.unwrap()),
                _ => appointment::delete(&db, v, a).await.unwrap(),
            }
            let back = restore(&db, v, t.id, Some(deleted)).await.unwrap();
            assert_eq!((back.id, back.appointment_id), (t.id, None), "{ending}");
            assert_eq!(require(&db, v, t.id).await.unwrap().appointment_id, None);
        }
    }

    // A row naming another vehicle's open appointment comes back on no
    // appointment, as one naming a missing appointment does.
    #[tokio::test]
    async fn restore_puts_no_todo_on_another_vehicles_appointment() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let other = VehicleFixture::new().name("Other").insert_id(&db).await;
        let theirs = appointment_id(&db, other, "Theirs").await;
        let t = add(&db, v, new_todo("Wash", None)).await.unwrap();
        let mut deleted = delete(&db, v, t.id).await.unwrap();
        deleted.appointment_id = Some(theirs);
        let back = restore(&db, v, t.id, Some(deleted)).await.unwrap();
        assert_eq!(back.appointment_id, None);
        assert_eq!(require(&db, v, t.id).await.unwrap().appointment_id, None);
    }

    #[tokio::test]
    async fn restore_reads_a_wrong_vehicle_todo_as_missing() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let other = VehicleFixture::new().name("Other").insert_id(&db).await;
        let t = add(&db, v, new_todo("Wash", None)).await.unwrap();
        let deleted = delete(&db, v, t.id).await.unwrap();
        let missing = require(&db, other, 99_999).await.unwrap_err();
        let wrong = restore(&db, other, t.id, Some(deleted.clone()))
            .await
            .unwrap_err();
        assert!(matches!(wrong, DomainError::NotFound(_)));
        assert_eq!(
            wrong.to_string().replace(&t.id.to_string(), "N"),
            missing.to_string().replace("99999", "N")
        );
        assert!(require(&db, v, t.id).await.is_err());
        assert!(require(&db, other, t.id).await.is_err());
    }

    #[tokio::test]
    async fn restoring_twice_is_refused_and_changes_nothing() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let t = add(&db, v, new_todo("Wash", None)).await.unwrap();
        let deleted = delete(&db, v, t.id).await.unwrap();
        let back = restore(&db, v, t.id, Some(deleted.clone())).await.unwrap();
        let mut edited = deleted;
        edited.title = "Changed".into();
        let err = restore(&db, v, t.id, Some(edited)).await.unwrap_err();
        assert!(matches!(err, DomainError::BadRequest(_)), "{err:?}");
        assert_eq!(
            err.to_string(),
            format!("To-do {} is already back on the list.", t.id)
        );
        assert_eq!(require(&db, v, t.id).await.unwrap(), back);
    }

    #[tokio::test]
    async fn restore_without_the_row_accepts_only_a_todo_already_back() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let t = add(&db, v, new_todo("Wash", None)).await.unwrap();
        let deleted = delete(&db, v, t.id).await.unwrap();

        let err = restore(&db, v, t.id, None).await.unwrap_err();
        assert!(matches!(err, DomainError::BadRequest(_)), "{err:?}");
        assert_eq!(err.to_string(), "That delete can no longer be undone.");
        assert!(require(&db, v, t.id).await.is_err());

        let back = restore(&db, v, t.id, Some(deleted)).await.unwrap();
        assert_eq!(restore(&db, v, t.id, None).await.unwrap(), back);
    }

    #[tokio::test]
    async fn restore_refuses_a_row_that_is_not_the_named_todo() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let a = add(&db, v, new_todo("Wash", None)).await.unwrap();
        let b = add(&db, v, new_todo("Wax", None)).await.unwrap();
        let deleted = delete(&db, v, a.id).await.unwrap();
        let err = restore(&db, v, b.id, Some(deleted)).await.unwrap_err();
        assert!(matches!(err, DomainError::NotFound(_)), "{err:?}");
        assert!(require(&db, v, a.id).await.is_err());
    }

    // `restore` and its appointment check rely on SQLite never handing a
    // deleted id out again, which AUTOINCREMENT guarantees for both tables.
    #[tokio::test]
    async fn deleted_todo_and_appointment_ids_are_not_reused() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let t = add(&db, v, new_todo("Wash", None)).await.unwrap();
        delete(&db, v, t.id).await.unwrap();
        assert!(add(&db, v, new_todo("Wax", None)).await.unwrap().id > t.id);
        let a = appointment_id(&db, v, "Brakes").await;
        appointment::delete(&db, v, a).await.unwrap();
        assert!(appointment_id(&db, v, "Tires").await > a);
    }

    #[tokio::test]
    async fn restore_for_a_deleted_vehicle_is_not_found() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let t = add(&db, v, new_todo("Wash", None)).await.unwrap();
        let deleted = delete(&db, v, t.id).await.unwrap();
        crate::services::vehicle::delete_unused(&db, v)
            .await
            .unwrap();
        let err = restore(&db, v, t.id, Some(deleted)).await.unwrap_err();
        assert!(matches!(err, DomainError::NotFound(_)), "{err:?}");
        assert_eq!(row_count::<todo::Entity>(&db).await, 0);
    }

    // Ticks and edits racing on one to-do from several pools all succeed:
    // each transaction takes the write lock before it reads.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_ticks_and_edits_on_one_todo_all_succeed() {
        let (_dir, pools) = race_pools().await;
        let first = &pools[0];
        let v = VehicleFixture::new().insert_id(first).await;
        let t = add(first, v, new_todo("Wash", None)).await.unwrap();

        let mut writes = tokio::task::JoinSet::new();
        for (i, db) in pools.iter().chain(pools.iter()).cloned().enumerate() {
            writes.spawn(async move {
                let input = UpdateTodo {
                    notes: Some(Some(format!("note {i}"))),
                    ..Default::default()
                };
                update_with_done(&db, v, t.id, input, Some(i % 2 == 0)).await
            });
        }
        let results = writes.join_all().await;

        assert!(results.iter().all(Result::is_ok), "{results:?}");
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
