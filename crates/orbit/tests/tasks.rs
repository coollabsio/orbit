use orbit_domain::{
    DomainError, ExpectedVersion, Project, RestoreAvailability, StatusCategory, Task,
    WorkspaceDefaults,
    platform::{Id, TimestampMillis},
};

#[test]
fn workspace_defaults_create_owner_project_and_workflow() {
    let owner_id = Id::new_v7();
    let defaults = WorkspaceDefaults::new(owner_id, "Orbit", "General");

    assert_eq!(defaults.workspace.name, "Orbit");
    assert_eq!(defaults.workspace.version, 0);
    assert_eq!(defaults.owner.workspace_id, defaults.workspace.id);
    assert_eq!(defaults.owner.user_id, owner_id);
    assert_eq!(defaults.owner.role, orbit_domain::WorkspaceRole::Owner);
    assert_eq!(defaults.project.workspace_id, defaults.workspace.id);
    assert_eq!(defaults.project.name, "General");
    assert_eq!(
        defaults
            .statuses
            .iter()
            .map(|status| (status.name.as_str(), status.category))
            .collect::<Vec<_>>(),
        vec![
            ("Backlog", StatusCategory::Unstarted),
            ("Todo", StatusCategory::Unstarted),
            ("In Progress", StatusCategory::Started),
            ("Done", StatusCategory::Completed),
            ("Cancelled", StatusCategory::Cancelled),
        ]
    );
    assert!(
        defaults
            .statuses
            .iter()
            .all(|status| status.workspace_id == defaults.workspace.id
                && status.project_id == defaults.project.id
                && status.version == 0)
    );
}

#[test]
fn stale_task_update_returns_the_current_version() {
    let mut task = new_task();
    task.rename("First edit", ExpectedVersion(0)).unwrap();

    assert_eq!(
        task.rename("Stale edit", ExpectedVersion(0)),
        Err(DomainError::VersionConflict {
            expected: 0,
            current: 1,
        })
    );
    assert_eq!(task.title, "First edit");
    assert_eq!(task.version, 1);
}

#[test]
fn deleting_a_project_hides_its_active_tasks() {
    let workspace_id = Id::new_v7();
    let mut project = Project::new(workspace_id, "General", "GEN", "#5e6ad2");
    let task = Task::new(
        workspace_id,
        project.id,
        Id::new_v7(),
        "Ship it",
        Id::new_v7(),
    );

    assert!(task.is_visible(&project).unwrap());
    project
        .delete(ExpectedVersion(0), TimestampMillis::from_millis(1_000))
        .unwrap();
    assert!(!task.is_visible(&project).unwrap());
}

#[test]
fn restore_reports_a_uniqueness_conflict_without_mutating_the_project() {
    let mut project = Project::new(Id::new_v7(), "General", "GEN", "#5e6ad2");
    project
        .delete(ExpectedVersion(0), TimestampMillis::from_millis(1_000))
        .unwrap();

    assert_eq!(
        project.restore(ExpectedVersion(1), RestoreAvailability::NameConflict),
        Err(DomainError::RestoreConflict { field: "name" })
    );
    assert_eq!(project.version, 1);
    assert_eq!(
        project.deleted_at,
        Some(TimestampMillis::from_millis(1_000))
    );
}

fn new_task() -> Task {
    Task::new(
        Id::new_v7(),
        Id::new_v7(),
        Id::new_v7(),
        "Original",
        Id::new_v7(),
    )
}

#[test]
fn parent_edge_rejects_self_direct_and_three_task_cycles() {
    use orbit_domain::{MAX_PARENT_DEPTH, check_parent_edge};

    let a = Id::new_v7();
    let b = Id::new_v7();
    let c = Id::new_v7();

    // self-parenting: the walk seeds with the proposed parent
    assert_eq!(check_parent_edge(a, a, &[a]), Err(DomainError::ParentCycle));

    // direct cycle: b is already a child of a, so a -> b closes the loop
    assert_eq!(
        check_parent_edge(a, b, &[b, a]),
        Err(DomainError::ParentCycle)
    );

    // three-task cycle: a -> b -> c, now asking for c -> a
    assert_eq!(
        check_parent_edge(c, a, &[a, c, b]),
        Err(DomainError::ParentCycle)
    );

    // legal edge in an unrelated chain
    assert_eq!(check_parent_edge(c, a, &[a, b]), Ok(()));

    // the walk is bounded: ten ancestors without terminating is refused
    let chain: Vec<Id> = (0..MAX_PARENT_DEPTH).map(|_| Id::new_v7()).collect();
    assert_eq!(
        check_parent_edge(c, chain[0], &chain),
        Err(DomainError::ParentDepthExceeded {
            limit: MAX_PARENT_DEPTH
        })
    );
    // one short of the bound is still provably acyclic
    assert_eq!(
        check_parent_edge(c, chain[0], &chain[..MAX_PARENT_DEPTH - 1]),
        Ok(())
    );
}
