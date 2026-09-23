use orbit_platform::{Id, TestDatabase, TimestampMillis};
use orbit_server::audit::{self, AuditOutcome};

#[tokio::test]
async fn successful_workspace_writes_have_transactional_ordered_notifications() {
    let db = TestDatabase::new().await.unwrap();
    let workspace = Id::new_v7();
    let mut tx = db.pool().begin().await.unwrap();
    for _ in 0..2 {
        audit::record(
            &mut tx,
            workspace,
            None,
            "task.updated",
            AuditOutcome::Success,
            "task",
            Some(Id::new_v7()),
            "test",
            serde_json::json!({}),
            TimestampMillis::now(),
        )
        .await
        .unwrap();
    }
    tx.commit().await.unwrap();
    let sequences: Vec<i64> = sqlx::query_scalar(
        "SELECT sequence FROM outbox_events WHERE workspace_id = ? ORDER BY sequence",
    )
    .bind(workspace.to_string())
    .fetch_all(db.pool())
    .await
    .unwrap();
    assert_eq!(sequences, vec![1, 2]);
    let mut tx = db.pool().begin().await.unwrap();
    audit::record(
        &mut tx,
        workspace,
        None,
        "task.updated",
        AuditOutcome::Success,
        "task",
        None,
        "test",
        serde_json::json!({}),
        TimestampMillis::now(),
    )
    .await
    .unwrap();
    tx.rollback().await.unwrap();
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM outbox_events")
        .fetch_one(db.pool())
        .await
        .unwrap();
    assert_eq!(count, 2);
}

#[tokio::test]
async fn replay_resyncs_expired_invalid_and_missing_cursors_and_is_workspace_scoped() {
    let db = TestDatabase::new().await.unwrap();
    let workspace = Id::new_v7();
    let mut tx = db.pool().begin().await.unwrap();
    audit::record(
        &mut tx,
        workspace,
        None,
        "task.updated",
        AuditOutcome::Success,
        "task",
        None,
        "test",
        serde_json::json!({}),
        TimestampMillis::now(),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    use orbit_server::realtime::replay;
    for cursor in [None, Some(-1), Some(2)] {
        let event = replay(&db, workspace, cursor).await.unwrap();
        assert_eq!(event.kind, "resync_required");
        assert!(event.workspaces_changed);
    }
    let events = replay(&db, workspace, Some(0)).await.unwrap();
    assert_eq!(events.kind, "workspace.changed");
    assert_eq!(events.sequence, "1");
    assert!(!events.workspaces_changed);
    assert!(!events.profile_changed);
    assert_eq!(replay(&db, Id::new_v7(), None).await.unwrap().sequence, "0");
    sqlx::query("DELETE FROM outbox_events")
        .execute(db.pool())
        .await
        .unwrap();
    assert_eq!(
        replay(&db, workspace, Some(0)).await.unwrap().kind,
        "resync_required"
    );
    assert_eq!(
        replay(&db, workspace, Some(1)).await.unwrap().kind,
        "workspace.changed"
    );
}

#[tokio::test]
async fn replay_marks_workspace_list_changes_without_marking_task_changes() {
    let db = TestDatabase::new().await.unwrap();
    let workspace = Id::new_v7();
    for (action, resource) in [("task.updated", "task"), ("workspace.updated", "workspace")] {
        let mut tx = db.pool().begin().await.unwrap();
        audit::record(
            &mut tx,
            workspace,
            None,
            action,
            AuditOutcome::Success,
            resource,
            None,
            "test",
            serde_json::json!({}),
            TimestampMillis::now(),
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
    }
    let first = orbit_server::realtime::replay(&db, workspace, Some(0))
        .await
        .unwrap();
    assert!(first.workspaces_changed);
    let second = orbit_server::realtime::replay(&db, workspace, Some(1))
        .await
        .unwrap();
    assert!(second.workspaces_changed);
    let current = orbit_server::realtime::replay(&db, workspace, Some(2))
        .await
        .unwrap();
    assert!(!current.workspaces_changed);
    assert!(!current.profile_changed);
    let mut tx = db.pool().begin().await.unwrap();
    audit::record(
        &mut tx,
        workspace,
        None,
        "member.profile_updated",
        AuditOutcome::Success,
        "user",
        None,
        "test",
        serde_json::json!({}),
        TimestampMillis::now(),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let profile = orbit_server::realtime::replay(&db, workspace, Some(2))
        .await
        .unwrap();
    assert!(profile.profile_changed);
    assert!(!profile.workspaces_changed);
}

#[tokio::test]
async fn subscription_requires_current_session_and_workspace_membership() {
    use orbit_server::{
        realtime::authorized,
        repositories::identity::{IdentityRepository, SetupRequest},
    };
    let db = TestDatabase::new().await.unwrap();
    let identity = IdentityRepository::new((*db).clone());
    let now = TimestampMillis::now();
    identity
        .store_setup_token(
            "setup-secret",
            TimestampMillis::from_millis(now.as_millis() + 60000),
        )
        .await
        .unwrap();
    let setup = identity
        .complete_setup(
            SetupRequest {
                token: "setup-secret".into(),
                email: "owner@example.com".into(),
                display_name: "Owner".into(),
                password_hash: orbit_platform::PasswordService::default()
                    .hash("correct horse battery")
                    .unwrap(),
                workspace_name: "Orbit".into(),
                project_name: "General".into(),
            },
            now,
        )
        .await
        .unwrap();
    let session = identity
        .authenticate_session(&setup.session.token, now)
        .await
        .unwrap();
    assert!(
        authorized(&db, setup.workspace_id, session.id)
            .await
            .unwrap()
    );
    assert!(!authorized(&db, Id::new_v7(), session.id).await.unwrap());
    sqlx::query("UPDATE users SET suspended_at=? WHERE id=?")
        .bind(now.as_millis())
        .bind(setup.user_id.to_string())
        .execute(db.pool())
        .await
        .unwrap();
    assert!(
        !authorized(&db, setup.workspace_id, session.id)
            .await
            .unwrap()
    );
    sqlx::query("UPDATE users SET suspended_at=NULL WHERE id=?")
        .bind(setup.user_id.to_string())
        .execute(db.pool())
        .await
        .unwrap();
    sqlx::query("UPDATE sessions SET revoked_at=? WHERE id=?")
        .bind(now.as_millis())
        .bind(session.id.to_string())
        .execute(db.pool())
        .await
        .unwrap();
    assert!(
        !authorized(&db, setup.workspace_id, session.id)
            .await
            .unwrap()
    );
}
