use utoipa::openapi::{Content, OpenApi as OpenApiDocument, Ref, ResponseBuilder};
use utoipa::{Modify, OpenApi};

pub const CONTRACT_ID: &str = "orbit-api-v1";

#[derive(OpenApi)]
#[openapi(
    info(title = "Orbit API", version = "orbit-api-v1"),
    components(schemas(orbit_platform::Problem, orbit_platform::ConflictMetadata)),
    modifiers(&ProblemDetails),
    paths(
        crate::auth_routes::setup_status,
        crate::auth_routes::setup_complete,
        crate::auth_routes::login,
        crate::auth_routes::logout,
        crate::auth_routes::me,
        crate::auth_routes::recovery_request,
        crate::auth_routes::recovery_complete,
        crate::auth_routes::list_sessions,
        crate::auth_routes::revoke_session,
        crate::workspace_routes::create_workspace,
        crate::workspace_routes::list_workspaces,
        crate::workspace_routes::get_workspace,
        crate::workspace_routes::rename_workspace,
        crate::workspace_routes::list_members,
        crate::workspace_routes::change_member_role,
        crate::workspace_routes::remove_member,
        crate::workspace_routes::transfer_ownership,
        crate::workspace_routes::create_invitation,
        crate::workspace_routes::list_invitations,
        crate::workspace_routes::revoke_invitation,
        crate::workspace_routes::accept_invitation,
        crate::workspace_routes::delete_workspace,
        crate::workspace_routes::restore_workspace,
        crate::workspace_routes::list_trash,
        crate::workspace_routes::list_audit,
        crate::workspace_routes::set_account_suspension,
        crate::workspace_routes::list_global_audit,
        crate::workspace_routes::export_global_audit,
        crate::task_routes::list_projects,
        crate::task_routes::create_project,
        crate::task_routes::update_project,
        crate::task_routes::delete_project,
        crate::task_routes::restore_project,
        crate::task_routes::list_project_trash,
        crate::task_routes::list_statuses,
        crate::task_routes::create_status,
        crate::task_routes::update_status,
        crate::task_routes::delete_status,
        crate::task_routes::reorder_statuses,
        crate::task_routes::list_labels,
        crate::task_routes::create_label,
        crate::task_routes::update_label,
        crate::task_routes::delete_label,
        crate::task_routes::list_tasks,
        crate::task_routes::get_task,
        crate::task_routes::create_task,
        crate::task_routes::update_task,
        crate::task_routes::bulk_tasks,
        crate::task_routes::reorder_tasks,
        crate::task_routes::delete_task,
        crate::task_routes::restore_task,
        crate::task_routes::list_task_trash,
        crate::task_routes::list_comments,
        crate::task_routes::create_comment,
        crate::task_routes::update_comment,
        crate::task_routes::delete_comment,
        crate::attachment_routes::list_task_attachments,
        crate::attachment_routes::list_comment_attachments,
        crate::attachment_routes::upload_task_attachments,
        crate::attachment_routes::upload_comment_attachments,
        crate::attachment_routes::create_attachment_comment,
        crate::attachment_routes::delete_task_attachment,
        crate::attachment_routes::delete_comment_attachment,
        crate::attachment_routes::download_task_attachment,
        crate::attachment_routes::download_comment_attachment,
    )
)]
struct ApiDocument;

struct ProblemDetails;

impl Modify for ProblemDetails {
    fn modify(&self, openapi: &mut OpenApiDocument) {
        for path in openapi.paths.paths.values_mut() {
            for operation in [
                &mut path.get,
                &mut path.put,
                &mut path.post,
                &mut path.delete,
                &mut path.options,
                &mut path.head,
                &mut path.patch,
                &mut path.trace,
            ]
            .into_iter()
            .flatten()
            {
                operation.responses.responses.insert(
                    "default".to_owned(),
                    ResponseBuilder::new()
                        .description("RFC 9457 Problem Details response")
                        .content(
                            "application/problem+json",
                            Content::new(Some(Ref::from_schema_name("Problem"))),
                        )
                        .build()
                        .into(),
                );
            }
        }
    }
}

pub fn openapi_json() -> Result<String, serde_json::Error> {
    let mut json = serde_json::to_string_pretty(&ApiDocument::openapi())?;
    json.push('\n');
    Ok(json)
}
