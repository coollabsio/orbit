use orbit_domain::{Membership, Permission, Policy, PolicyError, WorkspaceRole, platform::Id};

#[test]
fn workspace_requires_exactly_one_owner() {
    let workspace_id = Id::new_v7();
    let first_owner = Membership::new(workspace_id, Id::new_v7(), WorkspaceRole::Owner);
    let second_owner = Membership::new(workspace_id, Id::new_v7(), WorkspaceRole::Owner);
    let member = Membership::new(workspace_id, Id::new_v7(), WorkspaceRole::Member);

    assert_eq!(
        Policy::validate_owner_count(workspace_id, &[first_owner.clone(), member.clone()]),
        Ok(())
    );
    assert_eq!(
        Policy::validate_owner_count(workspace_id, &[member]),
        Err(PolicyError::ExactlyOneOwnerRequired)
    );
    assert_eq!(
        Policy::validate_owner_count(workspace_id, &[first_owner, second_owner]),
        Err(PolicyError::ExactlyOneOwnerRequired)
    );
}

#[test]
fn owner_count_is_scoped_when_another_workspace_has_only_a_member() {
    let workspace_a = Id::new_v7();
    let workspace_b = Id::new_v7();
    let memberships = [
        Membership::new(workspace_a, Id::new_v7(), WorkspaceRole::Owner),
        Membership::new(workspace_b, Id::new_v7(), WorkspaceRole::Member),
    ];

    assert_eq!(
        Policy::validate_owner_count(workspace_a, &memberships),
        Ok(())
    );
    assert_eq!(
        Policy::validate_owner_count(workspace_b, &memberships),
        Err(PolicyError::ExactlyOneOwnerRequired)
    );
}

#[test]
fn each_workspace_owner_is_valid_in_a_combined_membership_slice() {
    let workspace_a = Id::new_v7();
    let workspace_b = Id::new_v7();
    let memberships = [
        Membership::new(workspace_a, Id::new_v7(), WorkspaceRole::Owner),
        Membership::new(workspace_b, Id::new_v7(), WorkspaceRole::Owner),
    ];

    assert_eq!(
        Policy::validate_owner_count(workspace_a, &memberships),
        Ok(())
    );
    assert_eq!(
        Policy::validate_owner_count(workspace_b, &memberships),
        Ok(())
    );
}

#[test]
fn owner_holds_every_permission() {
    assert_eq!(Policy::permissions(WorkspaceRole::Owner), Permission::ALL);
}

#[test]
fn admin_holds_every_permission_except_the_owner_only_ones() {
    let owner_only = [Permission::WorkspaceDelete, Permission::WorkspaceTransfer];
    for permission in Permission::ALL {
        assert_eq!(
            Policy::can(WorkspaceRole::Admin, permission),
            !owner_only.contains(&permission),
            "admin and {permission:?}"
        );
    }
}

#[test]
fn member_holds_no_role_permission() {
    assert_eq!(Policy::permissions(WorkspaceRole::Member), []);
}

#[test]
fn roles_round_trip_through_their_stored_names() {
    for role in [
        WorkspaceRole::Owner,
        WorkspaceRole::Admin,
        WorkspaceRole::Member,
    ] {
        assert_eq!(WorkspaceRole::parse(role.as_str()), Some(role));
    }
    assert_eq!(WorkspaceRole::parse("guest"), None);
}
