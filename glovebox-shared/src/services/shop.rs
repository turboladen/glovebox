use sea_orm::*;

use crate::{
    entities::shop,
    error::{DomainError, DomainResult},
};

async fn get(db: &impl ConnectionTrait, id: i32) -> DomainResult<shop::Model> {
    shop::Entity::find_by_id(id)
        .one(db)
        .await?
        .ok_or_else(|| DomainError::NotFound(format!("Shop {id} not found")))
}

/// Refuses a shop id that names no shop, with `NotFound`; `None` passes.
// The shop directory is global, so an id is checked for existence only. A
// missing shop would otherwise surface as an opaque foreign-key failure.
pub(crate) async fn require_if_named(
    db: &impl ConnectionTrait,
    shop_id: Option<i32>,
) -> DomainResult<()> {
    if let Some(id) = shop_id {
        get(db, id).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::test_db;

    #[tokio::test]
    async fn get_missing_is_not_found() {
        let db = test_db().await;
        assert!(matches!(
            get(&db, 999).await.unwrap_err(),
            DomainError::NotFound(_)
        ));
    }
}
