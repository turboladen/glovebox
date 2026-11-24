//! The glovebox web page: a local, server-rendered view of the same database
//! the MCP server uses.

mod actions;
mod assets;
mod guard;
mod pages;
mod view;

use std::sync::{Arc, Mutex, PoisonError};

use axum::{
    Router, middleware,
    routing::{get, post},
};
use chrono::NaiveDate;
use glovebox_shared::{db::DatabaseConnection, entities::todo};

/// What every handler reads: the database, the name of the environment the
/// process serves and the file it serves from, and the clock that says what
/// day it is. Its one mutable part, and the page's one state outside the
/// database, is `deleted_todo`, which the delete and undo posts write.
#[derive(Clone)]
pub struct AppState {
    pub db: DatabaseConnection,
    pub env_name: String,
    /// The database file's name when `GLOVEBOX_DB_PATH` chose a file other
    /// than the environment's own.
    pub db_file: Option<String>,
    pub today: fn() -> NaiveDate,
    /// The undo of the last to-do deleted on the page.
    pub deleted_todo: DeletedTodo,
}

/// The last to-do deleted on the page, kept so its banner can put it back.
/// It lasts until the next delete, its undo, or the server stopping.
#[derive(Clone, Default)]
pub struct DeletedTodo(Arc<Mutex<Option<todo::Model>>>);

// The slot holds only a row this server deleted, so the restore trusts
// nothing a form sent, and one slot is enough because a banner offers the
// undo of the last action alone. The lock is never held across an await.
// Accepted limits: one slot serves every vehicle and tab, a double-clicked
// Delete or Undo can answer with an error or another page although the
// action worked, a failed undo keeps its row without showing the banner again
// (the browser's back button reaches it), and a vehicle deleted during an
// undo gives an opaque error and writes nothing.
impl DeletedTodo {
    fn lock(&self) -> std::sync::MutexGuard<'_, Option<todo::Model>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn put(&self, deleted: todo::Model) {
        *self.lock() = Some(deleted);
    }

    // The deleted to-do `id` of vehicle `vid`, while it is the one held.
    fn get(&self, vid: i32, id: i32) -> Option<todo::Model> {
        self.lock().as_ref().filter(|t| holds(t, vid, id)).cloned()
    }

    // Takes the deleted to-do `id` of vehicle `vid` out of the slot, so a
    // second undo posted at once finds nothing to restore.
    fn take(&self, vid: i32, id: i32) -> Option<todo::Model> {
        self.lock().take_if(|t| holds(t, vid, id))
    }

    // Puts a row whose restore failed back, unless a newer delete has filled
    // the slot since.
    fn put_back(&self, deleted: todo::Model) {
        self.lock().get_or_insert(deleted);
    }
}

fn holds(deleted: &todo::Model, vid: i32, id: i32) -> bool {
    deleted.vehicle_id == vid && deleted.id == id
}

/// Builds the application over `state`. Every response, errors included,
/// carries the content security policy, and a request addressed to another
/// host or posted from another origin is refused with 403.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(pages::get_home))
        .route(
            "/vehicles/{vid}/appointments/{id}",
            get(pages::get_appointment),
        )
        .route(
            "/vehicles/{vid}/appointments/{id}/print",
            get(pages::get_sheet),
        )
        .route("/vehicles/{vid}/jobs/{id}", get(pages::get_job))
        .route("/vehicles/{vid}/services/{id}", get(pages::get_service))
        .route("/shops", get(pages::get_shops))
        .route("/shops/{id}", get(pages::get_shop))
        .route(
            "/vehicles/{vid}/needs/{id}/supplier",
            post(actions::post_supplier),
        )
        .route(
            "/vehicles/{vid}/needs/{id}/decision",
            post(actions::post_decision),
        )
        .route(
            "/vehicles/{vid}/needs/{id}/ordered",
            post(actions::post_ordered),
        )
        .route(
            "/vehicles/{vid}/needs/{id}/received",
            post(actions::post_received),
        )
        .route(
            "/vehicles/{vid}/needs/{id}/received/undo",
            post(actions::post_received_undo),
        )
        .route(
            "/vehicles/{vid}/needs/{id}/return",
            post(actions::post_return),
        )
        .route("/vehicles/{vid}/todos", post(actions::post_todo))
        .route(
            "/vehicles/{vid}/todos/{id}/done",
            post(actions::post_todo_done),
        )
        .route(
            "/vehicles/{vid}/todos/{id}/done/undo",
            post(actions::post_todo_undo),
        )
        .route(
            "/vehicles/{vid}/todos/{id}/delete",
            post(actions::post_todo_delete),
        )
        .route(
            "/vehicles/{vid}/todos/{id}/restore",
            post(actions::post_todo_restore),
        )
        .route("/assets/plex-sans-400.woff2", get(assets::regular))
        .route("/assets/plex-sans-600.woff2", get(assets::semibold))
        .fallback(pages::not_found)
        .layer(middleware::from_fn(guard::same_origin_only))
        .layer(middleware::map_response(guard::content_security_policy))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deleted(vehicle_id: i32, id: i32) -> todo::Model {
        let stamp = "2026-11-14T12:00:00Z".parse().unwrap();
        todo::Model {
            id,
            vehicle_id,
            appointment_id: None,
            title: format!("To-do {id}"),
            notes: None,
            done_on: None,
            created_at: stamp,
            updated_at: stamp,
        }
    }

    // Taking needs the same vehicle and id, and a failed restore's row goes
    // back only into an empty slot, so a newer delete keeps its undo.
    #[test]
    fn take_and_put_back_keep_the_newest_undo() {
        let slot = DeletedTodo::default();
        slot.put(deleted(1, 7));
        assert_eq!(slot.take(1, 6), None);
        assert_eq!(slot.take(2, 7), None);
        assert_eq!(slot.get(1, 7), Some(deleted(1, 7)));
        assert_eq!(slot.take(1, 7), Some(deleted(1, 7)));
        assert_eq!(slot.get(1, 7), None);

        slot.put_back(deleted(1, 7));
        assert_eq!(slot.get(1, 7), Some(deleted(1, 7)));
        slot.put(deleted(1, 8));
        slot.put_back(deleted(1, 7));
        assert_eq!(slot.get(1, 7), None);
        assert_eq!(slot.get(1, 8), Some(deleted(1, 8)));
    }
}
