use orbit_domain::{
    Action, Membership, Policy, PolicyError, Role, TargetRole, WorkspaceRole, platform::Id,
};

#[test]
fn workspace_requires_exactly_one_owner() {
    let workspace_id = Id::new_v7();
    let first_owner = Membership::new(workspace_id, Id::new_v7(), WorkspaceRole::Owner);
    let second_owner = Membership::new(workspace_id, Id::new_v7(), WorkspaceRole::Owner);
    let member = Membership::new(workspace_id, Id::new_v7(), WorkspaceRole::Member);

    assert_eq!(
        Policy::validate_owner_count(&[first_owner.clone(), member.clone()]),
        Ok(())
    );
    assert_eq!(
        Policy::validate_owner_count(&[member]),
        Err(PolicyError::ExactlyOneOwnerRequired)
    );
    assert_eq!(
        Policy::validate_owner_count(&[first_owner, second_owner]),
        Err(PolicyError::ExactlyOneOwnerRequired)
    );
}

#[test]
fn owner_must_transfer_ownership_before_leaving() {
    assert_eq!(
        Policy::authorize(Role::Owner, Action::LeaveWorkspace, TargetRole::Owner),
        Err(PolicyError::TransferOwnershipRequired)
    );
}

#[test]
fn admin_cannot_remove_or_change_owner() {
    assert_eq!(
        Policy::authorize(Role::Admin, Action::RemoveMembership, TargetRole::Owner),
        Err(PolicyError::OwnerProtected)
    );
    assert_eq!(
        Policy::authorize(Role::Admin, Action::ChangeMembershipRole, TargetRole::Owner),
        Err(PolicyError::OwnerProtected)
    );
}

#[test]
fn member_can_create_edit_delete_and_restore_task_content() {
    let content_actions = [
        Action::CreateProject,
        Action::EditProject,
        Action::DeleteProject,
        Action::RestoreProject,
        Action::CreateStatus,
        Action::EditStatus,
        Action::DeleteStatus,
        Action::CreateTask,
        Action::EditTask,
        Action::DeleteTask,
        Action::RestoreTask,
        Action::CreateComment,
        Action::EditComment,
        Action::DeleteComment,
        Action::CreateAttachment,
        Action::DeleteAttachment,
    ];

    for action in content_actions {
        assert_eq!(
            Policy::authorize(Role::Member, action, TargetRole::NotApplicable),
            Ok(()),
            "member should be allowed to {action:?}"
        );
    }
}

#[test]
fn workspace_authorization_rejects_cross_workspace_access() {
    let membership = Membership::new(Id::new_v7(), Id::new_v7(), WorkspaceRole::Owner);

    assert_eq!(
        Policy::authorize_in_workspace(
            &membership,
            Id::new_v7(),
            Action::CreateTask,
            TargetRole::NotApplicable,
        ),
        Err(PolicyError::WorkspaceMismatch)
    );
}
