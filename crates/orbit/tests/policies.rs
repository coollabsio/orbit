use orbit_domain::{
    Actor, Membership, MembershipDenied, Permission, Policy, PolicyError, WorkspaceRole,
    platform::Id,
};

fn actor(role: WorkspaceRole) -> Actor {
    Actor {
        user_id: Id::new_v7(),
        role,
    }
}

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

#[test]
fn only_the_author_edits_a_comment_and_managers_may_also_delete_it() {
    let author = actor(WorkspaceRole::Member);
    let other = actor(WorkspaceRole::Member);
    let admin = actor(WorkspaceRole::Admin);

    assert!(author.can_edit_comment(author.user_id));
    assert!(!admin.can_edit_comment(author.user_id));
    assert!(author.can_delete_comment(author.user_id));
    assert!(admin.can_delete_comment(author.user_id));
    assert!(!other.can_delete_comment(author.user_id));
}

#[test]
fn views_belong_to_their_owner_and_managers_edit_shared_ones() {
    let owner = actor(WorkspaceRole::Member);
    let admin = actor(WorkspaceRole::Admin);
    let other = actor(WorkspaceRole::Member);

    assert!(owner.can_edit_view(owner.user_id, false));
    assert!(admin.can_edit_view(owner.user_id, true));
    assert!(!admin.can_edit_view(owner.user_id, false));
    assert!(!other.can_edit_view(owner.user_id, true));
    assert!(owner.can_change_view_visibility(owner.user_id));
    assert!(!admin.can_change_view_visibility(owner.user_id));
}

#[test]
fn members_purge_private_pages_and_managers_purge_teamspace_pages() {
    assert!(actor(WorkspaceRole::Member).can_purge_page(true));
    assert!(!actor(WorkspaceRole::Member).can_purge_page(false));
    assert!(actor(WorkspaceRole::Admin).can_purge_page(false));
}

#[test]
fn the_owner_membership_only_moves_through_a_transfer() {
    let owner = actor(WorkspaceRole::Owner);
    let admin = actor(WorkspaceRole::Admin);
    let member = actor(WorkspaceRole::Member);
    let someone = Id::new_v7();

    assert_eq!(
        admin.change_member_role(WorkspaceRole::Member, WorkspaceRole::Owner),
        Err(MembershipDenied::TransferRequired)
    );
    assert_eq!(
        admin.change_member_role(WorkspaceRole::Owner, WorkspaceRole::Member),
        Err(MembershipDenied::OwnerProtected)
    );
    assert_eq!(
        owner.change_member_role(WorkspaceRole::Owner, WorkspaceRole::Admin),
        Err(MembershipDenied::TransferRequired)
    );
    assert_eq!(
        admin.change_member_role(WorkspaceRole::Member, WorkspaceRole::Admin),
        Ok(())
    );
    assert_eq!(
        member.change_member_role(WorkspaceRole::Member, WorkspaceRole::Admin),
        Err(MembershipDenied::RoleForbidden)
    );
    assert_eq!(
        admin.remove_member(someone, WorkspaceRole::Owner),
        Err(MembershipDenied::OwnerProtected)
    );
    assert_eq!(
        owner.remove_member(owner.user_id, WorkspaceRole::Owner),
        Err(MembershipDenied::TransferRequired)
    );
    assert!(owner.can_transfer_ownership_to(someone));
    assert!(!owner.can_transfer_ownership_to(owner.user_id));
    assert!(!admin.can_transfer_ownership_to(someone));
}

#[test]
fn members_leave_but_do_not_remove_or_invite_others() {
    let member = actor(WorkspaceRole::Member);

    assert_eq!(
        member.remove_member(member.user_id, WorkspaceRole::Member),
        Ok(())
    );
    assert_eq!(
        member.remove_member(Id::new_v7(), WorkspaceRole::Member),
        Err(MembershipDenied::RoleForbidden)
    );
    assert_eq!(
        member.invite(WorkspaceRole::Member),
        Err(MembershipDenied::RoleForbidden)
    );
    assert_eq!(
        actor(WorkspaceRole::Admin).invite(WorkspaceRole::Owner),
        Err(MembershipDenied::OwnerProtected)
    );
    assert_eq!(
        actor(WorkspaceRole::Admin).invite(WorkspaceRole::Admin),
        Ok(())
    );
}
