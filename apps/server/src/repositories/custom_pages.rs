//! Custom pages (#94): web pages that a workspace shows in its sidebar, each framed in a sandboxed iframe at
//! `/pages/{slug}`. Owners and admins (`workspace.update`) add, change and remove them; members see the pages that
//! are not `admins_only`, and guests see none. The routes check that each URL is on an operator's frame origin.

use orbit_domain::Permission;
use orbit_platform::{Database, Flag, Id, Row, SqlBuilder, TimestampMillis};
use serde::Serialize;
use serde_json::Value;
use sqlx::{Any, Transaction};
use thiserror::Error;
use utoipa::ToSchema;

use super::membership;
use super::tasks::{TaskError, record_mutation};

const COLUMNS: &str =
    "id, workspace_id, slug, name, icon, url, admins_only, version, created_at, updated_at";

#[derive(Clone, Debug, Serialize, ToSchema)]
#[schema(as = CustomPage)]
pub struct CustomPageRecord {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = String)]
    pub workspace_id: Id,
    /// The page's address in Orbit: `/pages/{slug}`. Unique in the workspace.
    pub slug: String,
    pub name: String,
    #[schema(required = true)]
    pub icon: Option<String>,
    /// The https URL the page frames.
    pub url: String,
    /// Only owners and admins see the page.
    pub admins_only: bool,
    pub version: u64,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: TimestampMillis,
    #[schema(value_type = String, format = DateTime)]
    pub updated_at: TimestampMillis,
}

/// A new page's fields, already checked by the route.
#[derive(Clone, Debug)]
pub struct NewCustomPage {
    pub slug: String,
    pub name: String,
    pub icon: Option<String>,
    pub url: String,
    pub admins_only: bool,
}

/// `None` leaves a field unchanged; `Some(None)` clears the icon.
#[derive(Clone, Debug, Default)]
pub struct CustomPageChanges {
    pub slug: Option<String>,
    pub name: Option<String>,
    pub icon: Option<Option<String>>,
    pub url: Option<String>,
    pub admins_only: Option<bool>,
}

#[derive(Debug, Error)]
pub enum CustomPageError {
    #[error("custom page was not found")]
    NotFound,
    #[error("only workspace owners and admins may manage custom pages")]
    Forbidden,
    #[error("another custom page has this slug")]
    SlugTaken,
    #[error("the workspace has the most custom pages it can have")]
    Limit,
    #[error("stale version")]
    VersionConflict { current: Box<Value> },
    #[error("stored custom page data is invalid")]
    Corrupt,
    #[error("custom page repository is unavailable")]
    Unavailable(#[from] sqlx::Error),
}

impl From<TaskError> for CustomPageError {
    fn from(error: TaskError) -> Self {
        match error {
            TaskError::Unavailable(error) => Self::Unavailable(error),
            _ => Self::Corrupt,
        }
    }
}

/// Most pages in one workspace. Each origin goes into the `frame-src` of every member, so a workspace cannot fill it.
pub const MAX_PAGES: i64 = 50;

/// 1 to 48 lowercase letters, digits and single hyphens: the page opens at `/pages/{slug}`.
#[must_use]
pub fn is_slug(value: &str) -> bool {
    (1..=48).contains(&value.len())
        && value.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| matches!(byte, b'a'..=b'z' | b'0'..=b'9'))
        })
}

/// `value` as an https URL without credentials of at most 2048 bytes, as `Url` writes it.
#[must_use]
pub fn https_url(value: &str) -> Option<url::Url> {
    url::Url::parse(value.trim()).ok().filter(|url| {
        url.scheme() == "https"
            && url.username().is_empty()
            && url.password().is_none()
            && url.as_str().len() <= 2048
    })
}

impl NewCustomPage {
    /// A page from untrusted fields (an import archive), checked as the routes check them, but without the origin
    /// check: a page whose origin is not allowed here is kept and not framed. `None` when a field is invalid.
    #[must_use]
    pub fn checked(
        slug: &str,
        name: &str,
        icon: Option<&str>,
        url: &str,
        admins_only: bool,
    ) -> Option<Self> {
        let name = name.trim();
        let icon = icon.filter(|icon| !icon.trim().is_empty());
        let fits =
            |value: &str, chars: usize| value.chars().count() <= chars && value.len() <= chars * 4;
        (is_slug(slug)
            && !name.is_empty()
            && fits(name, 60)
            && icon.is_none_or(|icon| fits(icon, 64)))
        .then_some(())?;
        Some(Self {
            slug: slug.to_owned(),
            name: name.to_owned(),
            icon: icon.map(str::to_owned),
            url: https_url(url)?.into(),
            admins_only,
        })
    }
}

#[derive(Clone)]
pub struct CustomPageRepository {
    database: Database,
}

impl CustomPageRepository {
    #[must_use]
    pub fn new(database: Database) -> Self {
        Self { database }
    }

    /// The pages the actor sees, by name, and whether the actor may manage them.
    pub async fn pages(
        &self,
        workspace_id: Id,
        actor_id: Id,
    ) -> Result<(Vec<CustomPageRecord>, bool), CustomPageError> {
        let mut connection = self.database.pool().acquire().await?;
        let can_manage = membership::actor(&mut *connection, workspace_id, actor_id)
            .await?
            .ok_or(CustomPageError::NotFound)?
            .can(Permission::WorkspaceUpdate);
        let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {COLUMNS} FROM custom_pages WHERE workspace_id = $1 AND ($2 = 1 OR admins_only = 0) \
             ORDER BY lower(name), id"
        )))
        .bind(workspace_id.to_string())
        .bind(i64::from(can_manage))
        .fetch_all(&mut *connection)
        .await?;
        let pages = rows
            .into_iter()
            .map(page_from_row)
            .collect::<Result<_, _>>()?;
        Ok((pages, can_manage))
    }

    /// The URLs of the pages `user_id` sees in all their workspaces: the oldest membership first, then each
    /// workspace's oldest page first. A workspace that the user joins later cannot push out the pages of the others.
    pub async fn visible_urls(&self, user_id: Id) -> Result<Vec<String>, CustomPageError> {
        let workspaces: Vec<String> = sqlx::query_scalar(
            "SELECT memberships.workspace_id FROM memberships \
             JOIN workspaces ON workspaces.id = memberships.workspace_id \
             WHERE memberships.user_id = $1 AND workspaces.deleted_at IS NULL \
             AND EXISTS (SELECT 1 FROM custom_pages WHERE custom_pages.workspace_id = memberships.workspace_id) \
             ORDER BY memberships.created_at, memberships.id",
        )
        .bind(user_id.to_string())
        .fetch_all(self.database.pool())
        .await?;
        let mut urls = Vec::new();
        for workspace in workspaces {
            let workspace_id = workspace.parse().map_err(|_| CustomPageError::Corrupt)?;
            match self.pages(workspace_id, user_id).await {
                Ok((mut pages, _)) => {
                    pages.sort_by_key(|page| (page.created_at, page.id));
                    urls.extend(pages.into_iter().map(|page| page.url));
                }
                // A guest sees no pages.
                Err(CustomPageError::NotFound) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(urls)
    }

    pub async fn create_page(
        &self,
        workspace_id: Id,
        actor_id: Id,
        page: NewCustomPage,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<CustomPageRecord, CustomPageError> {
        let id = Id::new_v7();
        let mut tx = self.database.workspace_transaction(workspace_id).await?;
        require_manager(&mut tx, workspace_id, actor_id).await?;
        require_free_slug(&mut tx, workspace_id, &page.slug, None).await?;
        if page_count(&mut tx, workspace_id).await? >= MAX_PAGES {
            return Err(CustomPageError::Limit);
        }
        sqlx::query(
            "INSERT INTO custom_pages (id, workspace_id, slug, name, icon, url, admins_only, version, created_by, \
             created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, 0, $8, $9, $10)",
        )
        .bind(id.to_string())
        .bind(workspace_id.to_string())
        .bind(&page.slug)
        .bind(&page.name)
        .bind(&page.icon)
        .bind(&page.url)
        .bind(i64::from(page.admins_only))
        .bind(actor_id.to_string())
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(&mut *tx)
        .await?;
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "custom_page.created",
            "custom_page",
            id,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(CustomPageRecord {
            id,
            workspace_id,
            slug: page.slug,
            name: page.name,
            icon: page.icon,
            url: page.url,
            admins_only: page.admins_only,
            version: 0,
            created_at: now,
            updated_at: now,
        })
    }

    /// Adds imported pages, up to [`MAX_PAGES`] in the workspace; a page whose address the workspace already uses is
    /// skipped, so a second import of the same archive adds nothing. Returns how many were added.
    pub async fn import_pages(
        &self,
        workspace_id: Id,
        actor_id: Id,
        pages: &[NewCustomPage],
        now: TimestampMillis,
    ) -> Result<u64, CustomPageError> {
        let mut tx = self.database.workspace_transaction(workspace_id).await?;
        let room = usize::try_from(MAX_PAGES - page_count(&mut tx, workspace_id).await?).unwrap_or(0);
        let pages = &pages[..pages.len().min(room)];
        if pages.is_empty() {
            return Ok(0);
        }
        let mut query = SqlBuilder::new(
            "INSERT INTO custom_pages (id, workspace_id, slug, name, icon, url, admins_only, version, created_by, \
             created_at, updated_at) VALUES ",
        );
        for (index, page) in pages.iter().enumerate() {
            if index > 0 {
                query.push(", ");
            }
            query
                .push("(")
                .push_bind(Id::new_v7().to_string())
                .push(", ")
                .push_bind(workspace_id.to_string())
                .push(", ")
                .push_bind(page.slug.clone())
                .push(", ")
                .push_bind(page.name.clone())
                .push(", ")
                .push_bind(page.icon.clone())
                .push(", ")
                .push_bind(page.url.clone())
                .push(", ")
                .push_bind(i64::from(page.admins_only))
                .push(", 0, ")
                .push_bind(actor_id.to_string())
                .push(", ")
                .push_bind(now.as_millis())
                .push(", ")
                .push_bind(now.as_millis())
                .push(")");
        }
        query.push(" ON CONFLICT DO NOTHING");
        let created = query.build().execute(&mut *tx).await?.rows_affected();
        tx.commit().await?;
        Ok(created)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update_page(
        &self,
        workspace_id: Id,
        page_id: Id,
        actor_id: Id,
        expected_version: u64,
        changes: CustomPageChanges,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<CustomPageRecord, CustomPageError> {
        let mut tx = self.database.workspace_transaction(workspace_id).await?;
        require_manager(&mut tx, workspace_id, actor_id).await?;
        let current = page_in_tx(&mut tx, workspace_id, page_id).await?;
        check_version(expected_version, &current)?;
        let page = CustomPageRecord {
            slug: changes.slug.unwrap_or_else(|| current.slug.clone()),
            name: changes.name.unwrap_or_else(|| current.name.clone()),
            icon: changes.icon.unwrap_or_else(|| current.icon.clone()),
            url: changes.url.unwrap_or_else(|| current.url.clone()),
            admins_only: changes.admins_only.unwrap_or(current.admins_only),
            version: current.version + 1,
            updated_at: now,
            ..current
        };
        require_free_slug(&mut tx, workspace_id, &page.slug, Some(page_id)).await?;
        sqlx::query(
            "UPDATE custom_pages SET slug = $1, name = $2, icon = $3, url = $4, admins_only = $5, updated_at = $6, \
             version = version + 1 WHERE id = $7 AND workspace_id = $8",
        )
        .bind(&page.slug)
        .bind(&page.name)
        .bind(&page.icon)
        .bind(&page.url)
        .bind(i64::from(page.admins_only))
        .bind(now.as_millis())
        .bind(page_id.to_string())
        .bind(workspace_id.to_string())
        .execute(&mut *tx)
        .await?;
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "custom_page.updated",
            "custom_page",
            page_id,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(page)
    }

    pub async fn delete_page(
        &self,
        workspace_id: Id,
        page_id: Id,
        actor_id: Id,
        expected_version: u64,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), CustomPageError> {
        let mut tx = self.database.workspace_transaction(workspace_id).await?;
        require_manager(&mut tx, workspace_id, actor_id).await?;
        let current = page_in_tx(&mut tx, workspace_id, page_id).await?;
        check_version(expected_version, &current)?;
        sqlx::query("DELETE FROM custom_pages WHERE id = $1 AND workspace_id = $2")
            .bind(page_id.to_string())
            .bind(workspace_id.to_string())
            .execute(&mut *tx)
            .await?;
        record_mutation(
            &mut tx,
            workspace_id,
            actor_id,
            "custom_page.deleted",
            "custom_page",
            page_id,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }
}

/// Members who are not admins get `Forbidden`; outsiders and guests get `NotFound`.
async fn require_manager(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
    actor_id: Id,
) -> Result<(), CustomPageError> {
    let actor = membership::actor(&mut **tx, workspace_id, actor_id)
        .await?
        .ok_or(CustomPageError::NotFound)?;
    if actor.can(Permission::WorkspaceUpdate) {
        Ok(())
    } else {
        Err(CustomPageError::Forbidden)
    }
}

async fn page_count(tx: &mut Transaction<'_, Any>, workspace_id: Id) -> Result<i64, CustomPageError> {
    Ok(sqlx::query_scalar("SELECT COUNT(*) FROM custom_pages WHERE workspace_id = $1")
        .bind(workspace_id.to_string())
        .fetch_one(&mut **tx)
        .await?)
}

async fn require_free_slug(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
    slug: &str,
    except: Option<Id>,
) -> Result<(), CustomPageError> {
    let Flag(taken) = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM custom_pages WHERE workspace_id = $1 AND slug = $2 \
         AND id IS DISTINCT FROM $3)",
    )
    .bind(workspace_id.to_string())
    .bind(slug)
    .bind(except.map(|id| id.to_string()))
    .fetch_one(&mut **tx)
    .await?;
    if taken {
        Err(CustomPageError::SlugTaken)
    } else {
        Ok(())
    }
}

async fn page_in_tx(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
    page_id: Id,
) -> Result<CustomPageRecord, CustomPageError> {
    let row = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM custom_pages WHERE id = $1 AND workspace_id = $2"
    )))
    .bind(page_id.to_string())
    .bind(workspace_id.to_string())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(CustomPageError::NotFound)?;
    page_from_row(row)
}

fn check_version(expected: u64, record: &CustomPageRecord) -> Result<(), CustomPageError> {
    if expected == record.version {
        Ok(())
    } else {
        Err(CustomPageError::VersionConflict {
            current: Box::new(serde_json::to_value(record).map_err(|_| CustomPageError::Corrupt)?),
        })
    }
}

fn page_from_row(row: sqlx::any::AnyRow) -> Result<CustomPageRecord, CustomPageError> {
    let parse_id = |value: String| value.parse::<Id>().map_err(|_| CustomPageError::Corrupt);
    Ok(CustomPageRecord {
        id: parse_id(row.get("id"))?,
        workspace_id: parse_id(row.get("workspace_id"))?,
        slug: row.get("slug"),
        name: row.get("name"),
        icon: row.get("icon"),
        url: row.get("url"),
        admins_only: row.get("admins_only"),
        version: u64::try_from(row.get::<i64, _>("version"))
            .map_err(|_| CustomPageError::Corrupt)?,
        created_at: TimestampMillis::from_millis(row.get("created_at")),
        updated_at: TimestampMillis::from_millis(row.get("updated_at")),
    })
}

#[cfg(test)]
mod tests {
    use super::NewCustomPage;

    #[test]
    fn an_imported_page_is_checked_as_the_routes_check_it() {
        let page = NewCustomPage::checked(
            "deploys",
            " Deploys ",
            Some(" "),
            "https://Grafana.example.com/d",
            true,
        )
        .unwrap();
        assert_eq!(
            (page.name.as_str(), page.icon, page.url.as_str()),
            ("Deploys", None, "https://grafana.example.com/d")
        );
        // A page whose origin is not allowed here is kept: it is not framed until the origin is allowed.
        assert!(
            NewCustomPage::checked("a", "A", None, "https://other.example.com/", false).is_some()
        );
        for (slug, name, icon, url) in [
            ("Deploys", "Deploys", None, "https://a.example.com/"),
            ("a--b", "Deploys", None, "https://a.example.com/"),
            ("deploys", "  ", None, "https://a.example.com/"),
            (
                "deploys",
                &"n".repeat(61) as &str,
                None,
                "https://a.example.com/",
            ),
            (
                "deploys",
                "Deploys",
                Some(&"i".repeat(65) as &str),
                "https://a.example.com/",
            ),
            ("deploys", "Deploys", None, "http://a.example.com/"),
            ("deploys", "Deploys", None, "javascript:alert(1)"),
            (
                "deploys",
                "Deploys",
                None,
                "https://user:secret@a.example.com/",
            ),
        ] {
            assert!(
                NewCustomPage::checked(slug, name, icon, url, false).is_none(),
                "{slug} {name} {url}"
            );
        }
    }
}
