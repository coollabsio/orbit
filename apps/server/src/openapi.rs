use orbit_domain::{Policy, WorkspaceRole};
use std::collections::BTreeMap;
use utoipa::openapi::extensions::ExtensionsBuilder;
use utoipa::openapi::path::{ParameterBuilder, ParameterIn};
use utoipa::openapi::schema::{ObjectBuilder, Type};
use utoipa::openapi::security::{ApiKey, ApiKeyValue, SecurityRequirement, SecurityScheme};
use utoipa::openapi::{Content, OpenApi as OpenApiDocument, Ref, RefOr, Required, ResponseBuilder};
use utoipa::{Modify, OpenApi};

pub const CONTRACT_ID: &str = "orbit-api-v1";

#[derive(OpenApi)]
#[openapi(
    info(title = "Orbit API", version = "orbit-api-v1"),
    components(schemas(
        orbit_platform::Problem,
        orbit_platform::ConflictMetadata,
        crate::task_routes::ProblemBody,
        crate::workspace_routes::ProblemBody,
        crate::attachment_routes::AttachmentProblem,
        crate::repositories::task_filter::FilterGroup,
        crate::repositories::databases::ButtonConfig
    )),
    modifiers(&ProblemDetails),
    paths(
        crate::auth_routes::setup_status,
        crate::auth_routes::setup_complete,
        crate::auth_routes::login,
        crate::auth_routes::logout,
        crate::auth_routes::me,
        crate::auth_routes::socket_ticket,
        crate::auth_routes::update_me,
        crate::auth_routes::account::delete_account,
        crate::auth_routes::account::request_email_change,
        crate::auth_routes::account::confirm_email_change,
        crate::auth_routes::put_status,
        crate::auth_routes::push::get_push_key,
        crate::auth_routes::push::list_push_subscriptions,
        crate::auth_routes::push::create_push_subscription,
        crate::auth_routes::push::delete_push_subscription,
        crate::auth_routes::push::list_apns_devices,
        crate::auth_routes::push::create_apns_device,
        crate::auth_routes::push::delete_apns_device,
        crate::auth_routes::push::send_test_push,
        crate::auth_routes::push::get_notification_preferences,
        crate::auth_routes::push::put_notification_preferences,
        crate::auth_routes::get_shortcuts,
        crate::auth_routes::put_shortcuts,
        crate::auth_routes::upload_avatar,
        crate::auth_routes::remove_avatar,
        crate::auth_routes::user_avatar,
        crate::auth_routes::change_password,
        crate::auth_routes::recovery_request,
        crate::auth_routes::auth_options,
        crate::auth_routes::registration_request,
        crate::auth_routes::registration_complete,
        crate::auth_routes::recovery_complete,
        crate::auth_routes::list_sessions,
        crate::auth_routes::revoke_session,
        crate::auth_routes::two_factor::login_second_factor,
        crate::auth_routes::two_factor::get_two_factor,
        crate::auth_routes::two_factor::setup_totp,
        crate::auth_routes::two_factor::enable_totp,
        crate::auth_routes::two_factor::disable_two_factor,
        crate::auth_routes::two_factor::regenerate_recovery_codes,
        crate::auth_routes::passkeys::list_passkeys,
        crate::auth_routes::passkeys::start_passkey_registration,
        crate::auth_routes::passkeys::finish_passkey_registration,
        crate::auth_routes::passkeys::delete_passkey,
        crate::auth_routes::passkeys::start_passkey_login,
        crate::auth_routes::passkeys::finish_passkey_login,
        crate::auth_routes::providers::start_provider_sign_in,
        crate::auth_routes::providers::provider_callback,
        crate::auth_routes::providers::exchange_provider_sign_in,
        crate::auth_routes::providers::link_provider_sign_in,
        crate::auth_routes::providers::list_connected_accounts,
        crate::auth_routes::providers::disconnect_connected_account,
        crate::workspace_routes::create_workspace,
        crate::workspace_routes::list_workspaces,
        crate::workspace_routes::get_workspace,
        crate::workspace_routes::get_workspace_storage,
        crate::workspace_routes::delete_storage_file,
        crate::workspace_routes::get_workspace_bucket,
        crate::workspace_routes::save_workspace_bucket,
        crate::workspace_routes::remove_workspace_bucket,
        crate::workspace_routes::rename_workspace,
        crate::workspace_routes::list_members,
        crate::workspace_routes::get_member_profile,
        crate::workspace_routes::put_member_note,
        crate::workspace_routes::change_member_role,
        crate::workspace_routes::remove_member,
        crate::workspace_routes::transfer_ownership,
        crate::workspace_routes::create_invitation,
        crate::workspace_routes::list_invitations,
        crate::workspace_routes::revoke_invitation,
        crate::workspace_routes::accept_invitation,
        crate::workspace_routes::preview_invitation,
        crate::workspace_routes::delete_workspace,
        crate::workspace_routes::restore_workspace,
        crate::workspace_routes::list_trash,
        crate::workspace_routes::list_audit,
        crate::workspace_routes::list_api_tokens,
        crate::workspace_routes::create_api_token,
        crate::workspace_routes::revoke_api_token,
        crate::workspace_routes::list_admin_users,
        crate::workspace_routes::get_admin_user_summary,
        crate::workspace_routes::get_instance_settings,
        crate::workspace_routes::set_registration,
        crate::workspace_routes::save_smtp,
        crate::workspace_routes::remove_smtp,
        crate::workspace_routes::send_test_email,
        crate::workspace_routes::save_s3,
        crate::workspace_routes::remove_s3,
        crate::workspace_routes::save_sign_in_provider,
        crate::workspace_routes::remove_sign_in_provider,
        crate::workspace_routes::save_storage_options,
        crate::workspace_routes::save_limits,
        crate::workspace_routes::get_job_limits,
        crate::workspace_routes::save_job_limits,
        crate::workspace_routes::start_image_compression,
        crate::workspace_routes::set_account_suspension,
        crate::workspace_routes::create_recovery_link,
        crate::workspace_routes::reset_two_factor,
        crate::workspace_routes::change_user_email,
        crate::workspace_routes::set_instance_admin,
        crate::workspace_routes::list_global_audit,
        crate::workspace_routes::export_global_audit,
        crate::workspace_routes::create_backup,
        crate::workspace_routes::list_backups,
        crate::workspace_routes::download_backup,
        crate::workspace_routes::delete_backup,
        crate::backup_item_routes::list_backup_workspaces,
        crate::backup_item_routes::list_backup_items,
        crate::backup_item_routes::restore_backup_item,
        crate::public_api::me,
        crate::public_api::projects,
        crate::public_api::create_project,
        crate::public_api::statuses,
        crate::public_api::labels,
        crate::public_api::members,
        crate::public_api::list_tasks,
        crate::public_api::get_task,
        crate::public_api::create_task,
        crate::public_api::update_task,
        crate::public_api::delete_task,
        crate::public_api::comments,
        crate::public_api::create_comment,
        crate::public_api::docs,
        crate::public_api::doc,
        crate::public_api::create_doc,
        crate::public_api::update_doc,
        crate::public_api::databases::database,
        crate::public_api::databases::query_database,
        crate::public_api::databases::create_row,
        crate::public_api::databases::update_row,
        crate::public_api::databases::trash_rows,
        crate::public_api::databases::create_property,
        crate::public_api::databases::update_property,
        crate::public_api::databases::delete_property,
        crate::public_api::create_export,
        crate::public_api::get_export,
        crate::public_api::download_export,
        crate::integration_routes::create_discord_event,
        crate::integration_routes::github_webhook,
        crate::integration_routes::github_manifest_callback,
        crate::integration_routes::github_workspace_settings,
        crate::integration_routes::github_project_settings,
        crate::integration_routes::save_github_project_connection,
        crate::integration_routes::delete_github_project_connection,
        crate::integration_routes::start_github_manifest,
        crate::task_routes::list_projects,
        crate::task_routes::create_project,
        crate::task_routes::update_project,
        crate::task_routes::delete_project,
        crate::task_routes::list_project_members,
        crate::task_routes::add_project_member,
        crate::task_routes::remove_project_member,
        crate::task_routes::restore_project,
        crate::task_routes::list_project_trash,
        crate::task_routes::purge_project,
        crate::task_routes::get_trashed_project,
        crate::task_routes::list_trashed_project_tasks,
        crate::task_routes::list_statuses,
        crate::task_routes::create_status,
        crate::task_routes::update_status,
        crate::task_routes::delete_status,
        crate::task_routes::reorder_statuses,
        crate::task_routes::list_labels,
        crate::task_routes::list_project_labels,
        crate::task_routes::create_label,
        crate::task_routes::update_label,
        crate::task_routes::delete_label,
        crate::task_routes::list_tasks,
        crate::task_routes::query_tasks,
        crate::task_routes::get_task,
        crate::task_routes::list_task_relations,
        crate::task_routes::create_task_relation,
        crate::task_routes::delete_task_relation,
        crate::task_routes::list_github_links,
        crate::task_routes::list_task_activity,
        crate::task_routes::create_task,
        crate::task_routes::update_task,
        crate::task_routes::bulk_tasks,
        crate::task_routes::reorder_tasks,
        crate::task_routes::delete_task,
        crate::task_routes::restore_task,
        crate::task_routes::list_task_trash,
        crate::task_routes::empty_task_trash,
        crate::task_routes::get_trashed_task,
        crate::task_routes::purge_task,
        crate::task_routes::list_comments,
        crate::task_routes::create_comment,
        crate::task_routes::update_comment,
        crate::task_routes::delete_comment,
        crate::task_routes::list_notifications,
        crate::task_routes::read_notification,
        crate::task_routes::read_all_notifications,
        crate::view_routes::list_views,
        crate::view_routes::create_view,
        crate::view_routes::get_view,
        crate::view_routes::update_view,
        crate::view_routes::delete_view,
        crate::view_routes::favorite_view,
        crate::view_routes::unfavorite_view,
        crate::view_routes::reorder_view_favorites,
        crate::view_routes::get_view_preference,
        crate::view_routes::put_view_preference,
        crate::attachment_routes::list_task_attachments,
        crate::attachment_routes::list_comment_attachments,
        crate::attachment_routes::upload_task_attachments,
        crate::attachment_routes::upload_comment_attachments,
        crate::attachment_routes::create_attachment_comment,
        crate::attachment_routes::delete_task_attachment,
        crate::attachment_routes::delete_comment_attachment,
        crate::attachment_routes::download_task_attachment,
        crate::attachment_routes::download_comment_attachment,
        crate::page_routes::list_pages,
        crate::page_routes::create_page,
        crate::page_routes::get_page,
        crate::page_routes::update_page,
        crate::page_routes::move_page,
        crate::page_routes::delete_page,
        crate::page_routes::restore_page,
        crate::page_routes::list_page_trash,
        crate::page_routes::get_trashed_page,
        crate::page_routes::purge_page,
        crate::page_routes::empty_page_trash,
        crate::page_routes::duplicate_page,
        crate::page_routes::search_pages,
        crate::page_routes::list_page_descendants,
        crate::page_routes::list_page_favorites,
        crate::page_routes::add_page_favorite,
        crate::page_routes::remove_page_favorite,
        crate::page_routes::move_page_favorite,
        crate::page_routes::list_page_versions,
        crate::page_routes::get_page_version,
        crate::page_routes::restore_page_version,
        crate::page_routes::set_page_lock,
        crate::page_routes::record_page_visit,
        crate::page_routes::list_recent_pages,
        crate::database_routes::get_database,
        crate::database_routes::create_database_property,
        crate::database_routes::update_database_property,
        crate::database_routes::delete_database_property,
        crate::database_routes::create_database_link,
        crate::database_routes::delete_database_link,
        crate::database_routes::create_database_view,
        crate::database_routes::update_database_view,
        crate::database_routes::delete_database_view,
        crate::database_routes::create_database_row,
        crate::database_routes::update_database_row_values,
        crate::database_routes::trash_database_rows,
        crate::database_routes::move_database_row,
        crate::database_routes::buttons::press_database_button,
        crate::database_routes::import::import_database_csv,
        crate::database_routes::forms::submit_database_form,
        crate::database_routes::formula::preview_database_formula,
        crate::database_routes::import::create_database_from_csv,
        crate::database_routes::templates::create_database_template,
        crate::database_routes::templates::update_database_template,
        crate::database_routes::templates::delete_database_template,
        crate::page_file_routes::upload_page_file,
        crate::page_file_routes::download_page_file,
        crate::page_comment_routes::list_page_threads,
        crate::page_comment_routes::create_page_thread,
        crate::page_comment_routes::delete_page_thread,
        crate::page_comment_routes::create_page_comment,
        crate::page_comment_routes::update_page_comment,
        crate::page_comment_routes::delete_page_comment,
        crate::page_comment_routes::resolve_page_thread,
        crate::page_comment_routes::reopen_page_thread,
        crate::export_routes::export_page,
        crate::workspace_export_routes::create_workspace_export,
        crate::workspace_export_routes::list_workspace_exports,
        crate::workspace_export_routes::download_workspace_export,
        crate::billing::routes::get_billing,
        crate::billing::routes::billing_checkout,
        crate::billing::routes::billing_portal,
        crate::billing::routes::stripe_webhook,
        crate::teamspace_routes::list_teamspaces,
        crate::teamspace_routes::create_teamspace,
        crate::teamspace_routes::update_teamspace,
        crate::teamspace_routes::delete_teamspace,
        crate::teamspace_routes::move_teamspace,
        crate::import_routes::create_notion_import,
        crate::import_routes::list_notion_imports,
        crate::import_routes::get_notion_import,
        crate::import_routes::start_notion_import,
        crate::import_routes::cancel_notion_import,
        crate::asana_import_routes::create_asana_import,
        crate::asana_import_routes::list_asana_imports,
        crate::asana_import_routes::get_asana_import,
        crate::asana_import_routes::start_asana_import,
        crate::asana_import_routes::cancel_asana_import,
        crate::linear_import_routes::create_linear_import,
        crate::linear_import_routes::list_linear_imports,
        crate::linear_import_routes::get_linear_import,
        crate::linear_import_routes::start_linear_import,
        crate::linear_import_routes::cancel_linear_import,
        crate::trello_import_routes::create_trello_import,
        crate::trello_import_routes::list_trello_imports,
        crate::trello_import_routes::get_trello_import,
        crate::trello_import_routes::start_trello_import,
        crate::trello_import_routes::cancel_trello_import,
        crate::obsidian_import_routes::create_obsidian_import,
        crate::obsidian_import_routes::list_obsidian_imports,
        crate::obsidian_import_routes::get_obsidian_import,
        crate::obsidian_import_routes::start_obsidian_import,
        crate::obsidian_import_routes::cancel_obsidian_import,
        crate::import_people_routes::list_import_people,
        crate::import_people_routes::update_import_person,
        crate::orbit_import_routes::create_orbit_import,
        crate::orbit_import_routes::upload_orbit_import,
        crate::orbit_import_routes::list_orbit_imports,
        crate::orbit_import_routes::get_orbit_import,
        crate::orbit_import_routes::start_orbit_import,
        crate::orbit_import_routes::cancel_orbit_import,
        crate::chat_routes::conversations::list_chat_conversations,
        crate::chat_routes::conversations::create_chat_channel,
        crate::chat_routes::conversations::update_chat_channel,
        crate::chat_routes::conversations::archive_chat_channel,
        crate::chat_routes::conversations::join_chat_channel,
        crate::chat_routes::conversations::leave_chat_channel,
        crate::chat_routes::conversations::add_chat_members,
        crate::chat_routes::conversations::remove_chat_member,
        crate::chat_routes::conversations::open_chat_dm,
        crate::chat_routes::conversations::list_chat_categories,
        crate::chat_routes::conversations::create_chat_category,
        crate::chat_routes::conversations::rename_chat_category,
        crate::chat_routes::conversations::delete_chat_category,
        crate::chat_routes::conversations::move_chat_item,
        crate::chat_routes::conversations::place_chat_channel,
        crate::chat_routes::messages::list_chat_messages,
        crate::chat_routes::messages::send_chat_message,
        crate::chat_routes::messages::forward_chat_message,
        crate::chat_routes::messages::edit_chat_message,
        crate::chat_routes::messages::delete_chat_message,
        crate::chat_routes::messages::add_chat_reaction,
        crate::chat_routes::messages::remove_chat_reaction,
        crate::chat_routes::messages::pin_chat_message,
        crate::chat_routes::messages::list_chat_pins,
        crate::chat_routes::messages::list_chat_threads,
        crate::chat_routes::messages::list_followed_chat_threads,
        crate::chat_routes::messages::list_unread_chat_threads,
        crate::chat_routes::messages::get_chat_thread,
        crate::chat_routes::state::list_chat_states,
        crate::chat_routes::state::update_chat_state,
        crate::chat_routes::state::read_chat_conversation,
        crate::chat_routes::state::mark_chat_message_unread,
        crate::chat_routes::state::read_chat_thread,
        crate::chat_routes::state::follow_chat_thread,
        crate::chat_routes::state::read_all_chat,
        crate::chat_routes::state::restore_chat_read,
        crate::chat_routes::files::upload_chat_file,
        crate::chat_routes::files::download_chat_file,
        crate::chat_routes::files::list_chat_files,
        crate::chat_routes::messages::list_chat_search,
        crate::chat_routes::links::get_chat_link_preview,
        crate::chat_routes::emoji::list_chat_emoji,
        crate::chat_routes::emoji::create_chat_emoji,
        crate::chat_routes::emoji::delete_chat_emoji,
        crate::chat_routes::emoji::get_chat_emoji_image,
        crate::chat_routes::stickers::list_chat_stickers,
        crate::chat_routes::stickers::create_chat_sticker,
        crate::chat_routes::stickers::delete_chat_sticker,
        crate::chat_routes::stickers::get_chat_sticker_image,
        crate::chat_routes::hooks::list_chat_webhooks,
        crate::chat_routes::hooks::create_chat_webhook,
        crate::chat_routes::hooks::update_chat_webhook,
        crate::chat_routes::hooks::rotate_chat_webhook,
        crate::chat_routes::hooks::delete_chat_webhook,
        crate::chat_routes::hooks::post_incoming_webhook,
        crate::chat_routes::hooks::post_action_response,
        crate::chat_routes::events::read_hook_messages,
        crate::chat_routes::interactions::click_chat_action,
        crate::chat_routes::interactions::set_chat_webhook_interactivity,
        crate::chat_routes::interactions::get_chat_webhook_signing_secret,
        crate::chat_routes::interactions::rotate_chat_webhook_signing_secret,
        crate::chat_routes::interactions::list_chat_webhook_deliveries,
    )
)]
struct ApiDocument;

struct ProblemDetails;

impl Modify for ProblemDetails {
    fn modify(&self, openapi: &mut OpenApiDocument) {
        // The role table, published so generated test fixtures cannot drift from `Policy`.
        let role_permissions: serde_json::Map<String, serde_json::Value> = [
            WorkspaceRole::Owner,
            WorkspaceRole::Admin,
            WorkspaceRole::Member,
            WorkspaceRole::Guest,
        ]
        .into_iter()
        .map(|role| {
            (
                role.as_str().to_owned(),
                serde_json::json!(Policy::permissions(role)),
            )
        })
        .collect();
        openapi.extensions = Some(
            ExtensionsBuilder::new()
                .add("x-role-permissions", role_permissions)
                .build(),
        );
        openapi
            .components
            .as_mut()
            .expect("OpenAPI components exist")
            .add_security_scheme(
                "cookieAuth",
                SecurityScheme::ApiKey(ApiKey::Cookie(ApiKeyValue::new("__Host-orbit_session"))),
            );
        openapi
            .components
            .as_mut()
            .expect("OpenAPI components exist")
            .add_security_scheme(
                "bearerAuth",
                SecurityScheme::Http(utoipa::openapi::security::Http::new(
                    utoipa::openapi::security::HttpAuthScheme::Bearer,
                )),
            );

        for (route, path) in &mut openapi.paths.paths {
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
                let operation_id = operation.operation_id.clone().unwrap_or_default();
                let public_api = public_api_operation(route, &operation_id);
                let schema = if public_api {
                    "Problem"
                } else {
                    problem_schema(route)
                };
                operation.security =
                    Some(if operation_id == "create_discord_event" || public_api {
                        vec![SecurityRequirement::new("bearerAuth", Vec::<String>::new())]
                    } else if public_operation(&operation_id) {
                        vec![SecurityRequirement::default()]
                    } else if matches!(
                        operation_id.as_str(),
                        "accept_invitation"
                            | "start_provider_sign_in"
                            | "exchange_provider_sign_in"
                    ) {
                        vec![
                            SecurityRequirement::default(),
                            SecurityRequirement::new("cookieAuth", Vec::<String>::new()),
                        ]
                    } else {
                        vec![SecurityRequirement::new("cookieAuth", Vec::<String>::new())]
                    });
                operation.parameters.get_or_insert_default().push(
                    ParameterBuilder::new()
                        .name("X-Orbit-Contract")
                        .parameter_in(ParameterIn::Header)
                        .required(Required::False)
                        .description(Some(format!(
                            "Frontend contract identifier. Unsupported values return contract_mismatch; current value: {CONTRACT_ID}."
                        )))
                        .schema(Some(
                            ObjectBuilder::new().schema_type(Type::String),
                        ))
                        .build(),
                );

                let responses = if public_api {
                    public_api_responses(&operation_id)
                } else {
                    problem_responses(&operation_id)
                };
                for (status, description) in responses {
                    operation
                        .responses
                        .responses
                        .insert(status.to_owned(), problem_response(&description, schema));
                }
                add_download_media_types(&operation_id, operation);
                operation.responses.responses.insert(
                    "default".to_owned(),
                    problem_response("internal_error or another documented stable code", schema),
                );
            }
        }
    }
}

fn problem_schema(route: &str) -> &'static str {
    if route.contains("/integrations/discord") {
        "TaskProblem"
    } else if route.contains("/attachments") {
        "AttachmentProblem"
    } else if route.contains("/tasks")
        || route.contains("/projects")
        || route.contains("/labels")
        || route.contains("/views")
        || route.contains("/view-")
        || route.contains("/pages")
        || route.contains("/databases")
        || route.contains("/teamspaces")
        || route.contains("/imports/")
        || route.contains("/chat/")
        || route.contains("/billing")
    {
        "TaskProblem"
    } else if route.starts_with("/api/v1/auth") || route.starts_with("/api/v1/setup") {
        "AuthProblem"
    } else {
        "WorkspaceProblem"
    }
}

fn public_operation(operation_id: &str) -> bool {
    matches!(
        operation_id,
        "setup_status"
            | "setup_complete"
            | "login"
            | "recovery_request"
            | "recovery_complete"
            | "auth_options"
            | "registration_request"
            | "registration_complete"
            | "preview_invitation"
            | "github_webhook"
            | "post_incoming_webhook"
            | "post_action_response"
            | "read_hook_messages"
            | "github_manifest_callback"
            | "stripe_webhook"
            | "provider_callback"
            | "link_provider_sign_in"
    )
}

/// The public REST API (`crate::public_api`) under `/api/public/v1`, apart from the Discord
/// endpoint, which has its own errors.
fn public_api_operation(route: &str, operation_id: &str) -> bool {
    route.starts_with("/api/public/v1/") && operation_id != "create_discord_event"
}

/// The problem codes of the public REST API, as `crate::public_api` writes them.
fn public_api_responses(operation_id: &str) -> BTreeMap<&'static str, String> {
    let mut responses = BTreeMap::<&str, Vec<&str>>::new();
    // From the HTTP layer in front of every route.
    add_code(&mut responses, "400", "invalid_proxy_headers");
    add_code(&mut responses, "409", "contract_mismatch");
    add_code(&mut responses, "413", "request_too_large");
    add_code(&mut responses, "429", "rate_limit_exceeded");
    add_code(&mut responses, "500", "internal_error");
    add_code(&mut responses, "401", "unauthorized");
    // `GET /me` needs no scope.
    if operation_id != "public_get_me" {
        add_code(&mut responses, "403", "insufficient_scope");
    }
    add_code(&mut responses, "404", "not_found");
    if matches!(
        operation_id,
        "public_list_statuses"
            | "public_list_labels"
            | "public_list_members"
            | "public_list_tasks"
            | "public_get_task"
            | "public_list_comments"
            | "public_query_database"
            | "public_create_export"
    ) {
        add_code(&mut responses, "400", "invalid_request");
    }
    if matches!(
        operation_id,
        "public_create_export" | "public_get_export" | "public_download_export"
    ) {
        // The token's creator is no longer an owner or admin (a full export).
        add_code(&mut responses, "403", "workspace_action_forbidden");
    }
    match operation_id {
        "public_create_export" => add_code(&mut responses, "403", "origin_forbidden"),
        "public_download_export" => add_code(&mut responses, "409", "export_not_ready"),
        _ => {}
    }
    if matches!(
        operation_id,
        "public_list_statuses"
            | "public_list_labels"
            | "public_list_tasks"
            | "public_list_comments"
    ) {
        add_code(&mut responses, "400", "invalid_cursor");
    }
    let write = matches!(
        operation_id,
        "public_create_task"
            | "public_update_task"
            | "public_delete_task"
            | "public_create_comment"
            | "public_create_doc"
            | "public_update_doc"
            | "public_create_database_row"
            | "public_update_database_row"
            | "public_trash_database_rows"
    );
    if write {
        add_code(&mut responses, "400", "invalid_request");
        add_code(&mut responses, "402", "workspace_read_only");
        add_code(&mut responses, "403", "origin_forbidden");
        add_code(&mut responses, "503", "service_unavailable");
    }
    if matches!(
        operation_id,
        "public_create_task" | "public_create_doc" | "public_create_database_row"
    ) {
        add_code(&mut responses, "402", "trial_limit_reached");
    }
    if matches!(
        operation_id,
        "public_create_doc" | "public_update_doc" | "public_create_database_row"
    ) {
        add_code(&mut responses, "413", "doc_too_large");
    }
    if matches!(
        operation_id,
        "public_create_task"
            | "public_update_task"
            | "public_create_comment"
            | "public_create_doc"
            | "public_update_doc"
            | "public_query_database"
            | "public_create_database_row"
            | "public_update_database_row"
            | "public_trash_database_rows"
    ) {
        add_code(&mut responses, "422", "validation_failed");
    }
    if matches!(
        operation_id,
        "public_update_task" | "public_delete_task" | "public_update_doc"
    ) {
        add_code(&mut responses, "409", "conflict");
    }
    if matches!(
        operation_id,
        "public_create_task"
            | "public_update_task"
            | "public_delete_task"
            | "public_create_comment"
    ) {
        add_code(&mut responses, "409", "task_conflict");
    }
    match operation_id {
        "public_create_task" => {
            add_code(&mut responses, "422", "parent_cycle");
            add_code(&mut responses, "422", "parent_invalid");
        }
        "public_update_task" => {
            add_code(&mut responses, "409", "github_content_read_only");
            add_code(&mut responses, "409", "github_linked_move");
            add_code(&mut responses, "503", "task_busy");
        }
        // `permanent=true` without a workspace owner or admin.
        "public_delete_task" => add_code(&mut responses, "403", "workspace_action_forbidden"),
        "public_update_doc" => {
            add_code(&mut responses, "423", "page_locked");
            add_code(&mut responses, "503", "page_busy");
        }
        "public_create_database_row" => {
            add_code(&mut responses, "409", "database_row_limit");
            add_code(&mut responses, "409", "conflict");
            add_code(&mut responses, "503", "page_busy");
        }
        "public_update_database_row" => {
            add_code(&mut responses, "409", "conflict");
            add_code(&mut responses, "423", "page_locked");
            add_code(&mut responses, "503", "page_busy");
        }
        _ => {}
    }
    responses
        .into_iter()
        .map(|(status, codes)| (status, codes.join(", ")))
        .collect()
}

fn problem_responses(operation_id: &str) -> BTreeMap<&'static str, String> {
    let mut responses = BTreeMap::<&str, Vec<&str>>::new();
    add_code(&mut responses, "400", "invalid_proxy_headers");
    add_code(&mut responses, "409", "contract_mismatch");
    add_code(&mut responses, "413", "request_too_large");
    add_code(&mut responses, "500", "internal_error");
    if unsafe_operation(operation_id)
        && !matches!(
            operation_id,
            "create_discord_event"
                | "github_webhook"
                | "stripe_webhook"
                | "post_incoming_webhook"
                | "post_action_response"
        )
    {
        add_code(&mut responses, "403", "origin_forbidden");
    }
    if !public_operation(operation_id) && operation_id != "create_discord_event" {
        add_code(&mut responses, "401", "authentication_required");
    }
    if invalid_request_operation(operation_id) {
        add_code(&mut responses, "400", "invalid_request");
    }

    match operation_id {
        "setup_complete" => {
            add_code(&mut responses, "401", "invalid_setup_token");
            add_code(&mut responses, "409", "setup_unavailable");
            add_code(&mut responses, "422", "invalid_password");
        }
        "login" => {
            add_code(&mut responses, "401", "invalid_credentials");
            add_code(&mut responses, "429", "authentication_throttled");
        }
        "recovery_complete" => {
            add_code(&mut responses, "400", "invalid_recovery_token");
            add_code(&mut responses, "422", "invalid_password");
        }
        "registration_request" => {
            add_code(&mut responses, "403", "registration_closed");
            add_code(&mut responses, "422", "invalid_email");
        }
        "registration_complete" => {
            add_code(&mut responses, "400", "invalid_registration_token");
            add_code(&mut responses, "403", "registration_closed");
            add_code(&mut responses, "409", "account_exists");
            add_code(&mut responses, "422", "invalid_display_name");
            add_code(&mut responses, "422", "invalid_password");
        }
        "update_me" => {
            add_code(&mut responses, "422", "invalid_display_name");
            add_code(&mut responses, "422", "invalid_profile");
        }
        "put_status" => add_code(&mut responses, "422", "invalid_status"),
        "create_push_subscription" => add_code(&mut responses, "422", "invalid_subscription"),
        "create_apns_device" => {
            add_code(&mut responses, "422", "invalid_token");
            add_code(&mut responses, "503", "push_not_configured");
        }
        "put_shortcuts" => add_code(&mut responses, "422", "invalid_shortcuts"),
        "change_password" => {
            add_code(&mut responses, "401", "invalid_credentials");
            add_code(&mut responses, "401", "reauthentication_required");
            add_code(&mut responses, "422", "invalid_password");
        }
        "start_provider_sign_in" => {
            add_code(&mut responses, "401", "authentication_required");
            add_code(&mut responses, "404", "provider_not_configured");
            add_code(&mut responses, "422", "invalid_code_challenge");
            add_code(&mut responses, "422", "invalid_redirect");
            add_code(&mut responses, "422", "invalid_return_to");
            add_code(&mut responses, "503", "authentication_busy");
        }
        "exchange_provider_sign_in" => {
            add_code(&mut responses, "400", "invalid_handoff");
            add_code(&mut responses, "400", "provider_denied");
            add_code(&mut responses, "401", "authentication_required");
            add_code(&mut responses, "403", "registration_closed");
            add_code(&mut responses, "403", "account_email_unverified");
            add_code(&mut responses, "403", "link_needs_sign_in");
            add_code(&mut responses, "403", "account_suspended");
            add_code(&mut responses, "403", "session_mismatch");
            add_code(&mut responses, "403", "identity_mismatch");
            add_code(&mut responses, "409", "identity_in_use");
            add_code(&mut responses, "409", "identity_conflict");
            add_code(&mut responses, "409", "provider_already_connected");
            add_code(&mut responses, "422", "provider_email_unverified");
            add_code(&mut responses, "502", "provider_failed");
            add_code(&mut responses, "503", "authentication_busy");
        }
        "link_provider_sign_in" => {
            add_code(&mut responses, "400", "invalid_password_token");
            add_code(&mut responses, "401", "invalid_credentials");
            add_code(&mut responses, "403", "account_suspended");
            add_code(&mut responses, "409", "identity_conflict");
            add_code(&mut responses, "429", "authentication_throttled");
            add_code(&mut responses, "503", "authentication_busy");
        }
        "disconnect_connected_account" => {
            add_code(&mut responses, "404", "identity_not_found");
            add_code(&mut responses, "409", "last_sign_in_method");
        }
        "save_sign_in_provider" => {
            add_code(&mut responses, "403", "installation_admin_required");
            add_code(&mut responses, "404", "provider_not_found");
            add_code(&mut responses, "422", "invalid_sign_in_provider");
            add_code(&mut responses, "503", "app_key_missing");
        }
        "remove_sign_in_provider" => {
            add_code(&mut responses, "403", "installation_admin_required");
            add_code(&mut responses, "404", "provider_not_found");
        }
        "revoke_session" => add_code(&mut responses, "404", "session_not_found"),
        "preview_invitation" => add_code(&mut responses, "404", "invitation_not_found"),
        "accept_invitation" => {
            add_code(&mut responses, "403", "invitation_email_mismatch");
            add_code(&mut responses, "404", "invitation_not_found");
            add_code(&mut responses, "409", "workspace_conflict");
            add_code(&mut responses, "422", "invalid_registration");
            add_code(&mut responses, "422", "invalid_password");
            add_code(&mut responses, "429", "invitation_registration_throttled");
        }
        "create_discord_event" => {
            add_code(&mut responses, "401", "api_token_required");
            add_code(&mut responses, "401", "invalid_api_token");
            add_code(&mut responses, "403", "api_token_scope_forbidden");
            add_code(&mut responses, "409", "integration_event_conflict");
            add_code(&mut responses, "422", "validation_failed");
            add_code(&mut responses, "422", "integration_project_unavailable");
        }
        "post_action_response" => {
            add_code(&mut responses, "400", "invalid_json");
            add_code(&mut responses, "402", "workspace_read_only");
            add_code(&mut responses, "403", "chat_channel_archived");
            add_code(&mut responses, "404", "response_url_not_found");
            add_code(&mut responses, "413", "payload_too_large");
            add_code(&mut responses, "415", "unsupported_media_type");
            add_code(&mut responses, "422", "validation_failed");
            add_code(&mut responses, "429", "rate_limit_exceeded");
        }
        "read_hook_messages" => {
            add_code(&mut responses, "404", "webhook_not_found");
            add_code(&mut responses, "422", "validation_failed");
            add_code(&mut responses, "429", "rate_limit_exceeded");
        }
        "post_incoming_webhook" => {
            add_code(&mut responses, "400", "invalid_json");
            add_code(&mut responses, "402", "workspace_read_only");
            add_code(&mut responses, "403", "chat_channel_archived");
            add_code(&mut responses, "404", "webhook_not_found");
            add_code(&mut responses, "413", "payload_too_large");
            add_code(&mut responses, "415", "unsupported_media_type");
            add_code(&mut responses, "422", "validation_failed");
            add_code(&mut responses, "429", "rate_limit_exceeded");
        }
        "github_webhook" => {
            add_code(&mut responses, "401", "invalid_github_signature");
            add_code(&mut responses, "503", "app_key_missing");
            add_code(&mut responses, "503", "app_key_invalid");
            add_code(&mut responses, "409", "github_labels_ambiguous");
        }
        "get_billing" => {
            add_code(&mut responses, "404", "billing_workspace_not_found");
        }
        "billing_checkout" => {
            add_code(&mut responses, "403", "billing_owner_required");
            add_code(&mut responses, "404", "billing_workspace_not_found");
            add_code(&mut responses, "409", "subscription_active");
            add_code(&mut responses, "422", "unknown_plan");
            add_code(&mut responses, "502", "stripe_unavailable");
        }
        "billing_portal" => {
            add_code(&mut responses, "403", "billing_owner_required");
            add_code(&mut responses, "404", "billing_workspace_not_found");
            add_code(&mut responses, "409", "billing_customer_missing");
            add_code(&mut responses, "502", "stripe_unavailable");
        }
        "stripe_webhook" => {
            add_code(&mut responses, "400", "invalid_stripe_signature");
            add_code(&mut responses, "400", "invalid_stripe_event");
        }
        "github_workspace_settings" => {
            add_code(&mut responses, "404", "github_workspace_not_found");
        }
        "github_project_settings" => add_code(&mut responses, "404", "github_project_not_found"),
        "save_github_project_connection" | "delete_github_project_connection" => {
            add_code(&mut responses, "403", "github_manager_required");
            add_code(&mut responses, "404", "github_project_not_found");
            add_code(&mut responses, "422", "github_repository_not_installed");
            add_code(&mut responses, "422", "invalid_github_connection");
            add_code(&mut responses, "409", "github_label_conflict");
        }
        "start_github_manifest" => {
            add_code(&mut responses, "403", "github_manager_required");
            add_code(&mut responses, "404", "github_workspace_not_found");
            add_code(&mut responses, "409", "github_app_exists");
            add_code(&mut responses, "422", "github_https_required");
            add_code(&mut responses, "422", "invalid_github_organization");
            add_code(&mut responses, "503", "app_key_missing");
        }
        "github_manifest_callback" => {
            add_code(&mut responses, "400", "invalid_github_registration");
            add_code(&mut responses, "409", "github_app_exists");
            add_code(&mut responses, "502", "github_registration_failed");
            add_code(&mut responses, "503", "app_key_missing");
        }
        "create_workspace" => {
            add_code(&mut responses, "409", "workspace_limit_reached");
            add_code(&mut responses, "422", "invalid_workspace_name");
        }
        "get_workspace" | "get_workspace_storage" => {
            add_code(&mut responses, "404", "workspace_resource_not_found");
        }
        "rename_workspace" => {
            workspace_resource_errors(&mut responses);
            add_code(&mut responses, "422", "invalid_workspace_name");
        }
        "list_members" | "get_member_profile" => {
            add_code(&mut responses, "404", "workspace_resource_not_found");
        }
        "put_member_note" => {
            add_code(&mut responses, "404", "workspace_resource_not_found");
            add_code(&mut responses, "422", "invalid_note");
        }
        "change_member_role" | "remove_member" => {
            workspace_resource_errors(&mut responses);
            add_code(&mut responses, "409", "ownership_transfer_required");
        }
        "transfer_ownership" => {
            workspace_resource_errors(&mut responses);
            add_code(&mut responses, "409", "workspace_limit_reached");
            add_code(&mut responses, "409", "transfer_unavailable");
        }
        "restore_workspace" => {
            workspace_resource_errors(&mut responses);
            add_code(&mut responses, "409", "workspace_limit_reached");
        }
        "delete_workspace" | "delete_storage_file" => workspace_resource_errors(&mut responses),
        "create_invitation" => {
            add_code(&mut responses, "403", "workspace_action_forbidden");
            add_code(&mut responses, "404", "workspace_resource_not_found");
            add_code(&mut responses, "409", "workspace_conflict");
            add_code(&mut responses, "409", "email_not_configured");
            add_code(&mut responses, "422", "invalid_email");
            add_code(&mut responses, "422", "invalid_guest_channels");
            add_code(&mut responses, "502", "email_failed");
        }
        "get_instance_settings" | "remove_smtp" | "get_job_limits" => {
            add_code(&mut responses, "403", "installation_admin_required");
        }
        "set_registration" => {
            add_code(&mut responses, "403", "installation_admin_required");
            add_code(&mut responses, "409", "email_not_configured");
        }
        "save_smtp" => {
            add_code(&mut responses, "403", "installation_admin_required");
            add_code(&mut responses, "422", "invalid_smtp_settings");
            add_code(&mut responses, "503", "app_key_missing");
        }
        "send_test_email" => {
            add_code(&mut responses, "403", "installation_admin_required");
            add_code(&mut responses, "409", "email_not_configured");
            add_code(&mut responses, "502", "email_failed");
        }
        "get_workspace_bucket" | "remove_workspace_bucket" => {
            add_code(&mut responses, "403", "workspace_action_forbidden");
            add_code(&mut responses, "404", "workspace_resource_not_found");
        }
        "save_workspace_bucket" => {
            add_code(&mut responses, "403", "workspace_action_forbidden");
            add_code(&mut responses, "404", "workspace_resource_not_found");
            add_code(&mut responses, "409", "storage_in_use");
            add_code(&mut responses, "422", "invalid_storage_settings");
            add_code(&mut responses, "502", "storage_unreachable");
            add_code(&mut responses, "503", "app_key_missing");
        }
        "save_s3" => {
            add_code(&mut responses, "403", "installation_admin_required");
            add_code(&mut responses, "409", "storage_in_use");
            add_code(&mut responses, "422", "invalid_storage_settings");
            add_code(&mut responses, "502", "storage_unreachable");
            add_code(&mut responses, "503", "app_key_missing");
        }
        "remove_s3" => {
            add_code(&mut responses, "403", "installation_admin_required");
            add_code(&mut responses, "409", "storage_in_use");
            add_code(&mut responses, "502", "storage_unreachable");
        }
        "start_image_compression" => {
            add_code(&mut responses, "403", "installation_admin_required");
        }
        "save_limits" => {
            add_code(&mut responses, "403", "installation_admin_required");
            add_code(&mut responses, "422", "invalid_limits");
        }
        "save_job_limits" => {
            add_code(&mut responses, "403", "installation_admin_required");
            add_code(&mut responses, "422", "invalid_job_limits");
        }
        "save_storage_options" => {
            add_code(&mut responses, "403", "installation_admin_required");
            add_code(&mut responses, "409", "storage_not_configured");
            add_code(&mut responses, "409", "storage_shared");
        }
        "list_invitations" | "list_audit" | "list_api_tokens" | "create_api_token"
        | "revoke_api_token" => {
            add_code(&mut responses, "403", "workspace_action_forbidden");
            add_code(&mut responses, "404", "workspace_resource_not_found");
        }
        "revoke_invitation" => {
            add_code(&mut responses, "403", "workspace_action_forbidden");
            add_code(&mut responses, "404", "workspace_resource_not_found");
        }
        "set_account_suspension" => {
            add_code(&mut responses, "403", "installation_admin_required");
            add_code(&mut responses, "404", "workspace_resource_not_found");
            add_code(&mut responses, "404", "user_not_found");
            add_code(&mut responses, "409", "root_account");
        }
        "set_instance_admin" => {
            add_code(&mut responses, "403", "root_required");
            add_code(&mut responses, "404", "workspace_resource_not_found");
            add_code(&mut responses, "404", "user_not_found");
            add_code(&mut responses, "409", "root_account");
            add_code(&mut responses, "409", "account_suspended");
        }
        "create_recovery_link" => {
            add_code(&mut responses, "403", "installation_admin_required");
            add_code(&mut responses, "404", "workspace_resource_not_found");
            add_code(&mut responses, "404", "user_not_found");
            add_code(&mut responses, "409", "root_account");
            add_code(&mut responses, "409", "own_account");
            add_code(&mut responses, "409", "account_suspended");
        }
        "reset_two_factor" => {
            add_code(&mut responses, "403", "installation_admin_required");
            add_code(&mut responses, "404", "workspace_resource_not_found");
            add_code(&mut responses, "404", "user_not_found");
            add_code(&mut responses, "409", "root_account");
            add_code(&mut responses, "409", "own_account");
        }
        "change_user_email" => {
            add_code(&mut responses, "403", "installation_admin_required");
            add_code(&mut responses, "404", "workspace_resource_not_found");
            add_code(&mut responses, "404", "user_not_found");
            add_code(&mut responses, "409", "root_account");
            add_code(&mut responses, "409", "account_deleted");
            add_code(&mut responses, "409", "email_taken");
            add_code(&mut responses, "422", "invalid_email");
            add_code(&mut responses, "422", "same_email");
        }
        "list_admin_users" | "get_admin_user_summary" => {
            add_code(&mut responses, "403", "installation_admin_required");
        }
        "list_global_audit" | "export_global_audit" => {
            add_code(&mut responses, "403", "installation_admin_required");
        }
        "create_backup" | "list_backups" => {
            add_code(&mut responses, "403", "installation_admin_required");
        }
        "download_backup" | "delete_backup" => {
            add_code(&mut responses, "403", "installation_admin_required");
            add_code(&mut responses, "404", "backup_not_found");
        }
        "list_backup_workspaces" | "list_backup_items" | "restore_backup_item" => {
            add_code(&mut responses, "403", "installation_admin_required");
            add_code(&mut responses, "404", "backup_not_found");
            add_code(&mut responses, "422", "backup_unreadable");
            add_code(&mut responses, "503", "backup_unavailable");
            if operation_id != "list_backup_workspaces" {
                add_code(&mut responses, "404", "workspace_not_found");
            }
            if operation_id == "restore_backup_item" {
                add_code(&mut responses, "402", "trial_limit_reached");
                add_code(&mut responses, "404", "item_not_found");
                add_code(&mut responses, "409", "project_missing");
                add_code(&mut responses, "409", "teamspace_missing");
                add_code(&mut responses, "409", "database_row_limit");
                add_code(&mut responses, "413", "storage_quota_exceeded");
            }
        }
        id if attachment_list_operation(id) => {
            add_code(&mut responses, "400", "invalid_cursor");
            add_code(&mut responses, "404", "attachment_not_found");
        }
        id if attachment_upload_operation(id) => {
            add_code(&mut responses, "400", "invalid_multipart");
            if id == "upload_comment_attachments" {
                add_code(&mut responses, "403", "task_action_forbidden");
            }
            add_code(&mut responses, "404", "attachment_not_found");
            add_code(&mut responses, "413", "upload_too_large");
            add_code(&mut responses, "413", "storage_quota_exceeded");
            add_code(&mut responses, "422", "validation_failed");
        }
        id if attachment_operation(id) => {
            if id == "delete_comment_attachment" {
                add_code(&mut responses, "403", "task_action_forbidden");
            }
            add_code(&mut responses, "404", "attachment_not_found");
        }
        id if view_operation(id) => view_errors(id, &mut responses),
        "upload_page_file" => {
            add_code(&mut responses, "400", "invalid_multipart");
            add_code(&mut responses, "404", "page_file_not_found");
            add_code(&mut responses, "413", "upload_too_large");
            add_code(&mut responses, "413", "storage_quota_exceeded");
            add_code(&mut responses, "422", "validation_failed");
        }
        "download_page_file" => add_code(&mut responses, "404", "page_file_not_found"),
        "create_workspace_export" | "list_workspace_exports" | "download_workspace_export" => {
            add_code(&mut responses, "403", "workspace_action_forbidden");
            add_code(&mut responses, "404", "workspace_resource_not_found");
            if operation_id == "create_workspace_export" {
                add_code(&mut responses, "409", "export_in_progress");
            }
        }
        "export_page" => {
            add_code(&mut responses, "404", "page_not_found");
            add_code(&mut responses, "413", "export_too_many_pages");
            add_code(&mut responses, "413", "export_too_large");
        }
        id if notion_import_operation(id) => notion_import_errors(id, &mut responses),
        id if asana_import_operation(id) => asana_import_errors(id, &mut responses),
        id if linear_import_operation(id) => linear_import_errors(id, &mut responses),
        id if trello_import_operation(id) => trello_import_errors(id, &mut responses),
        id if obsidian_import_operation(id) => obsidian_import_errors(id, &mut responses),
        "list_import_people" | "update_import_person" => {
            add_code(&mut responses, "403", "workspace_action_forbidden");
            add_code(&mut responses, "404", "import_person_not_found");
            if operation_id == "update_import_person" {
                add_code(&mut responses, "422", "validation_failed");
            }
        }
        id if orbit_import_operation(id) => orbit_import_errors(id, &mut responses),
        id if task_operation(id) => task_errors(id, &mut responses),
        id if page_comment_operation(id) => page_comment_errors(id, &mut responses),
        id if page_operation(id) => page_errors(id, &mut responses),
        id if database_operation(id) => database_errors(id, &mut responses),
        id if teamspace_operation(id) => teamspace_errors(id, &mut responses),
        id if id.contains("chat") => chat_errors(id, &mut responses),
        _ => {}
    }
    // Orbit Cloud: these creates and restores are refused at the trial's limits.
    if matches!(
        operation_id,
        "create_task"
            | "restore_task"
            | "create_page"
            | "duplicate_page"
            | "restore_page"
            | "create_database_row"
            | "submit_database_form"
            | "create_invitation"
    ) {
        add_code(&mut responses, "402", "trial_limit_reached");
    }

    responses
        .into_iter()
        .map(|(status, codes)| (status, codes.join(", ")))
        .collect()
}

fn add_code(
    responses: &mut BTreeMap<&'static str, Vec<&'static str>>,
    status: &'static str,
    code: &'static str,
) {
    let codes = responses.entry(status).or_default();
    if !codes.contains(&code) {
        codes.push(code);
    }
}

fn unsafe_operation(operation_id: &str) -> bool {
    !(operation_id.starts_with("list_")
        || operation_id.starts_with("get_")
        || operation_id.starts_with("download_")
        || matches!(
            operation_id,
            "me" | "setup_status" | "export_global_audit" | "search_pages" | "provider_callback"
        ))
}

fn invalid_request_operation(operation_id: &str) -> bool {
    matches!(
        operation_id,
        "create_discord_event"
            | "billing_checkout"
            | "setup_complete"
            | "login"
            | "update_me"
            | "put_status"
            | "create_push_subscription"
            | "create_apns_device"
            | "put_notification_preferences"
            | "put_shortcuts"
            | "change_password"
            | "recovery_request"
            | "recovery_complete"
            | "registration_request"
            | "registration_complete"
            | "set_registration"
            | "save_smtp"
            | "save_s3"
            | "save_sign_in_provider"
            | "start_provider_sign_in"
            | "exchange_provider_sign_in"
            | "link_provider_sign_in"
            | "save_workspace_bucket"
            | "save_storage_options"
            | "save_limits"
            | "save_job_limits"
            | "list_backup_items"
            | "restore_backup_item"
            | "create_workspace"
            | "rename_workspace"
            | "list_members"
            | "get_member_profile"
            | "put_member_note"
            | "change_member_role"
            | "remove_member"
            | "transfer_ownership"
            | "create_invitation"
            | "list_invitations"
            | "accept_invitation"
            | "preview_invitation"
            | "delete_workspace"
            | "restore_workspace"
            | "list_audit"
            | "list_api_tokens"
            | "create_api_token"
            | "revoke_api_token"
            | "set_account_suspension"
            | "set_instance_admin"
            | "list_global_audit"
            | "export_global_audit"
            | "list_projects"
            | "create_project"
            | "update_project"
            | "delete_project"
            | "list_project_members"
            | "add_project_member"
            | "remove_project_member"
            | "restore_project"
            | "list_project_trash"
            | "list_statuses"
            | "create_status"
            | "update_status"
            | "delete_status"
            | "reorder_statuses"
            | "list_labels"
            | "list_project_labels"
            | "create_label"
            | "update_label"
            | "delete_label"
            | "list_tasks"
            | "query_tasks"
            | "create_task"
            | "update_task"
            | "create_task_relation"
            | "bulk_tasks"
            | "reorder_tasks"
            | "delete_task"
            | "restore_task"
            | "list_task_trash"
            | "get_trashed_task"
            | "purge_task"
            | "get_trashed_project"
            | "list_trashed_project_tasks"
            | "list_comments"
            | "create_comment"
            | "update_comment"
            | "delete_comment"
            | "list_task_attachments"
            | "list_comment_attachments"
            | "create_view"
            | "update_view"
            | "reorder_view_favorites"
            | "put_view_preference"
            | "create_page"
            | "update_page"
            | "move_page"
            | "delete_page"
            | "restore_page"
            | "purge_page"
            | "duplicate_page"
            | "export_page"
            | "search_pages"
            | "move_page_favorite"
            | "list_page_versions"
            | "restore_page_version"
            | "set_page_lock"
            | "list_recent_pages"
            | "create_teamspace"
            | "update_teamspace"
            | "delete_teamspace"
            | "move_teamspace"
            | "create_notion_import"
            | "get_notion_import"
            | "start_notion_import"
            | "create_asana_import"
            | "get_asana_import"
            | "start_asana_import"
            | "create_linear_import"
            | "get_linear_import"
            | "start_linear_import"
            | "create_trello_import"
            | "get_trello_import"
            | "start_trello_import"
            | "get_obsidian_import"
            | "start_obsidian_import"
            | "update_import_person"
            | "create_orbit_import"
            | "get_orbit_import"
            | "start_orbit_import"
    )
}

fn notion_import_operation(operation_id: &str) -> bool {
    matches!(
        operation_id,
        "create_notion_import"
            | "list_notion_imports"
            | "get_notion_import"
            | "start_notion_import"
            | "cancel_notion_import"
    )
}

fn notion_import_errors(
    operation_id: &str,
    responses: &mut BTreeMap<&'static str, Vec<&'static str>>,
) {
    add_code(responses, "404", "notion_import_not_found");
    match operation_id {
        "create_notion_import" => {
            add_code(responses, "409", "app_key_missing");
            add_code(responses, "409", "import_in_progress");
            add_code(responses, "422", "notion_token_invalid");
            add_code(responses, "502", "notion_unavailable");
        }
        "start_notion_import" => {
            add_code(responses, "404", "page_not_found");
            add_code(responses, "404", "teamspace_not_found");
            add_code(responses, "409", "import_in_progress");
            add_code(responses, "409", "notion_import_state_conflict");
            add_code(responses, "422", "validation_failed");
            add_code(responses, "422", "notion_import_too_large");
        }
        "cancel_notion_import" => add_code(responses, "409", "notion_import_state_conflict"),
        _ => {}
    }
}

fn asana_import_operation(operation_id: &str) -> bool {
    matches!(
        operation_id,
        "create_asana_import"
            | "list_asana_imports"
            | "get_asana_import"
            | "start_asana_import"
            | "cancel_asana_import"
    )
}

fn asana_import_errors(
    operation_id: &str,
    responses: &mut BTreeMap<&'static str, Vec<&'static str>>,
) {
    add_code(responses, "404", "asana_import_not_found");
    match operation_id {
        "create_asana_import" => {
            add_code(responses, "409", "app_key_missing");
            add_code(responses, "409", "import_in_progress");
            add_code(responses, "422", "asana_token_invalid");
            add_code(responses, "502", "asana_unavailable");
        }
        "start_asana_import" => {
            add_code(responses, "409", "import_in_progress");
            add_code(responses, "409", "asana_import_state_conflict");
            add_code(responses, "422", "validation_failed");
        }
        "cancel_asana_import" => add_code(responses, "409", "asana_import_state_conflict"),
        _ => {}
    }
}

fn linear_import_operation(operation_id: &str) -> bool {
    matches!(
        operation_id,
        "create_linear_import"
            | "list_linear_imports"
            | "get_linear_import"
            | "start_linear_import"
            | "cancel_linear_import"
    )
}

fn linear_import_errors(
    operation_id: &str,
    responses: &mut BTreeMap<&'static str, Vec<&'static str>>,
) {
    add_code(responses, "404", "linear_import_not_found");
    match operation_id {
        "create_linear_import" => {
            add_code(responses, "409", "app_key_missing");
            add_code(responses, "409", "import_in_progress");
            add_code(responses, "422", "linear_token_invalid");
            add_code(responses, "502", "linear_unavailable");
        }
        "start_linear_import" => {
            add_code(responses, "409", "import_in_progress");
            add_code(responses, "409", "linear_import_state_conflict");
            add_code(responses, "422", "validation_failed");
        }
        "cancel_linear_import" => add_code(responses, "409", "linear_import_state_conflict"),
        _ => {}
    }
}

fn obsidian_import_operation(operation_id: &str) -> bool {
    matches!(
        operation_id,
        "create_obsidian_import"
            | "list_obsidian_imports"
            | "get_obsidian_import"
            | "start_obsidian_import"
            | "cancel_obsidian_import"
    )
}

fn obsidian_import_errors(
    operation_id: &str,
    responses: &mut BTreeMap<&'static str, Vec<&'static str>>,
) {
    add_code(responses, "404", "obsidian_import_not_found");
    match operation_id {
        "create_obsidian_import" => {
            add_code(responses, "409", "import_in_progress");
            add_code(responses, "413", "obsidian_archive_too_large");
            add_code(responses, "422", "obsidian_archive_invalid");
        }
        "start_obsidian_import" => {
            add_code(responses, "404", "page_not_found");
            add_code(responses, "404", "teamspace_not_found");
            add_code(responses, "409", "import_in_progress");
            add_code(responses, "409", "obsidian_import_state_conflict");
            add_code(responses, "422", "validation_failed");
            add_code(responses, "422", "obsidian_import_too_large");
        }
        "cancel_obsidian_import" => add_code(responses, "409", "obsidian_import_state_conflict"),
        _ => {}
    }
}

fn orbit_import_operation(operation_id: &str) -> bool {
    matches!(
        operation_id,
        "create_orbit_import"
            | "upload_orbit_import"
            | "list_orbit_imports"
            | "get_orbit_import"
            | "start_orbit_import"
            | "cancel_orbit_import"
    )
}

fn orbit_import_errors(
    operation_id: &str,
    responses: &mut BTreeMap<&'static str, Vec<&'static str>>,
) {
    add_code(responses, "404", "orbit_import_not_found");
    match operation_id {
        "create_orbit_import" => {
            add_code(responses, "403", "forbidden");
            add_code(responses, "409", "app_key_missing");
            add_code(responses, "409", "import_in_progress");
            add_code(responses, "422", "orbit_url_invalid");
            add_code(responses, "422", "orbit_token_invalid");
            add_code(responses, "422", "orbit_scope_missing");
            add_code(responses, "502", "orbit_unreachable");
        }
        "upload_orbit_import" => {
            add_code(responses, "409", "import_in_progress");
            add_code(responses, "413", "orbit_export_too_large");
            add_code(responses, "422", "orbit_format_unsupported");
        }
        "start_orbit_import" => {
            add_code(responses, "403", "forbidden");
            add_code(responses, "409", "import_in_progress");
            add_code(responses, "409", "orbit_import_state_conflict");
            add_code(responses, "422", "validation_failed");
        }
        "cancel_orbit_import" => add_code(responses, "409", "orbit_import_state_conflict"),
        _ => {}
    }
}

fn trello_import_operation(operation_id: &str) -> bool {
    matches!(
        operation_id,
        "create_trello_import"
            | "list_trello_imports"
            | "get_trello_import"
            | "start_trello_import"
            | "cancel_trello_import"
    )
}

fn trello_import_errors(
    operation_id: &str,
    responses: &mut BTreeMap<&'static str, Vec<&'static str>>,
) {
    add_code(responses, "404", "trello_import_not_found");
    match operation_id {
        "create_trello_import" => {
            add_code(responses, "409", "app_key_missing");
            add_code(responses, "409", "import_in_progress");
            add_code(responses, "422", "trello_key_invalid");
            add_code(responses, "422", "trello_token_invalid");
            add_code(responses, "502", "trello_unavailable");
        }
        "start_trello_import" => {
            add_code(responses, "409", "import_in_progress");
            add_code(responses, "409", "trello_import_state_conflict");
            add_code(responses, "422", "validation_failed");
        }
        "cancel_trello_import" => add_code(responses, "409", "trello_import_state_conflict"),
        _ => {}
    }
}

fn workspace_resource_errors(responses: &mut BTreeMap<&'static str, Vec<&'static str>>) {
    add_code(responses, "403", "workspace_action_forbidden");
    add_code(responses, "404", "workspace_resource_not_found");
    add_code(responses, "409", "workspace_conflict");
    add_code(responses, "409", "conflict");
}

fn attachment_list_operation(operation_id: &str) -> bool {
    matches!(
        operation_id,
        "list_task_attachments" | "list_comment_attachments"
    )
}

fn attachment_upload_operation(operation_id: &str) -> bool {
    matches!(
        operation_id,
        "upload_task_attachments" | "upload_comment_attachments" | "create_attachment_comment"
    )
}

fn attachment_operation(operation_id: &str) -> bool {
    operation_id.contains("attachment")
}

fn task_operation(operation_id: &str) -> bool {
    matches!(
        operation_id,
        "list_projects"
            | "create_project"
            | "update_project"
            | "delete_project"
            | "list_project_members"
            | "add_project_member"
            | "remove_project_member"
            | "restore_project"
            | "list_project_trash"
            | "purge_project"
            | "empty_task_trash"
            | "get_trashed_project"
            | "list_trashed_project_tasks"
            | "get_trashed_task"
            | "purge_task"
            | "list_statuses"
            | "create_status"
            | "update_status"
            | "delete_status"
            | "reorder_statuses"
            | "list_labels"
            | "list_project_labels"
            | "create_label"
            | "update_label"
            | "delete_label"
            | "list_tasks"
            | "query_tasks"
            | "get_task"
            | "list_task_relations"
            | "create_task_relation"
            | "delete_task_relation"
            | "create_task"
            | "update_task"
            | "bulk_tasks"
            | "reorder_tasks"
            | "delete_task"
            | "restore_task"
            | "list_task_trash"
            | "list_comments"
            | "create_comment"
            | "update_comment"
            | "delete_comment"
    )
}

fn task_errors(operation_id: &str, responses: &mut BTreeMap<&'static str, Vec<&'static str>>) {
    add_code(responses, "404", "task_resource_not_found");
    if matches!(
        operation_id,
        "list_projects"
            | "list_project_trash"
            | "list_statuses"
            | "list_labels"
            | "list_project_labels"
            | "list_tasks"
            | "query_tasks"
            | "list_task_trash"
            | "list_trashed_project_tasks"
            | "list_comments"
    ) {
        add_code(responses, "400", "invalid_cursor");
    }
    if matches!(
        operation_id,
        "create_project"
            | "update_project"
            | "create_status"
            | "update_status"
            | "delete_status"
            | "reorder_statuses"
            | "create_label"
            | "update_label"
            | "create_task"
            | "update_task"
            | "bulk_tasks"
            | "reorder_tasks"
            | "create_comment"
            | "update_comment"
            | "create_task_relation"
            | "delete_task_relation"
    ) || operation_id == "list_tasks"
    {
        add_code(responses, "422", "validation_failed");
    }
    if operation_id == "query_tasks" {
        add_code(responses, "422", "invalid_filter");
    }
    if matches!(operation_id, "create_task" | "update_task" | "bulk_tasks") {
        add_code(responses, "422", "parent_cycle");
        add_code(responses, "422", "parent_invalid");
    }
    if matches!(
        operation_id,
        "create_project"
            | "create_status"
            | "create_label"
            | "create_task"
            | "create_comment"
            | "create_task_relation"
    ) {
        add_code(responses, "409", "task_conflict");
    }
    if matches!(
        operation_id,
        "update_project"
            | "delete_project"
            | "update_status"
            | "delete_status"
            | "reorder_statuses"
            | "update_label"
            | "delete_label"
            | "update_task"
            | "bulk_tasks"
            | "reorder_tasks"
            | "delete_task"
            | "update_comment"
            | "delete_comment"
    ) {
        add_code(responses, "409", "task_conflict");
        add_code(responses, "409", "conflict");
    }
    if matches!(operation_id, "restore_project" | "restore_task") {
        add_code(responses, "409", "task_conflict");
        add_code(responses, "409", "conflict");
        add_code(responses, "409", "restore_conflict");
    }
    if matches!(operation_id, "update_task" | "bulk_tasks") {
        add_code(responses, "409", "github_content_read_only");
        add_code(responses, "409", "github_linked_move");
    }
    if matches!(operation_id, "update_comment" | "delete_comment") {
        add_code(responses, "403", "task_action_forbidden");
    }
    if matches!(
        operation_id,
        "purge_project" | "empty_task_trash" | "purge_task"
    ) {
        add_code(responses, "403", "task_action_forbidden");
    }
    if operation_id == "purge_task" {
        add_code(responses, "409", "task_not_trashed");
    }
    if operation_id == "purge_project" {
        add_code(responses, "409", "project_not_trashed");
    }
}

fn view_operation(operation_id: &str) -> bool {
    matches!(
        operation_id,
        "list_views"
            | "create_view"
            | "get_view"
            | "update_view"
            | "delete_view"
            | "favorite_view"
            | "unfavorite_view"
            | "reorder_view_favorites"
            | "get_view_preference"
            | "put_view_preference"
    )
}

fn view_errors(operation_id: &str, responses: &mut BTreeMap<&'static str, Vec<&'static str>>) {
    add_code(responses, "404", "task_resource_not_found");
    if matches!(
        operation_id,
        "create_view"
            | "update_view"
            | "reorder_view_favorites"
            | "get_view_preference"
            | "put_view_preference"
    ) {
        add_code(responses, "422", "validation_failed");
    }
    if matches!(
        operation_id,
        "create_view" | "update_view" | "put_view_preference"
    ) {
        add_code(responses, "422", "invalid_filter");
    }
    if matches!(operation_id, "update_view" | "delete_view") {
        add_code(responses, "403", "task_action_forbidden");
    }
    if operation_id == "update_view" {
        add_code(responses, "409", "conflict");
    }
}

fn page_operation(operation_id: &str) -> bool {
    matches!(
        operation_id,
        "list_pages"
            | "create_page"
            | "get_page"
            | "list_page_descendants"
            | "update_page"
            | "move_page"
            | "delete_page"
            | "restore_page"
            | "list_page_trash"
            | "get_trashed_page"
            | "purge_page"
            | "empty_page_trash"
            | "duplicate_page"
            | "search_pages"
            | "list_page_favorites"
            | "add_page_favorite"
            | "remove_page_favorite"
            | "move_page_favorite"
            | "list_page_versions"
            | "get_page_version"
            | "restore_page_version"
            | "set_page_lock"
            | "record_page_visit"
            | "list_recent_pages"
    )
}

fn page_errors(operation_id: &str, responses: &mut BTreeMap<&'static str, Vec<&'static str>>) {
    add_code(responses, "404", "page_not_found");
    if matches!(operation_id, "create_page" | "move_page") {
        add_code(responses, "404", "teamspace_not_found");
    }
    if matches!(
        operation_id,
        "create_page" | "update_page" | "move_page" | "search_pages" | "move_page_favorite"
    ) {
        add_code(responses, "422", "validation_failed");
    }
    if matches!(
        operation_id,
        "update_page" | "move_page" | "delete_page" | "restore_page" | "purge_page"
    ) {
        add_code(responses, "409", "conflict");
    }
    if matches!(operation_id, "purge_page" | "move_page") {
        add_code(responses, "403", "workspace_action_forbidden");
    }
    if operation_id == "purge_page" {
        add_code(responses, "409", "page_not_trashed");
    }
    if operation_id == "list_page_versions" {
        add_code(responses, "400", "invalid_cursor");
        add_code(responses, "422", "validation_failed");
    }
    if matches!(operation_id, "get_page_version" | "restore_page_version") {
        add_code(responses, "404", "page_version_not_found");
    }
    if operation_id == "restore_page_version" {
        add_code(responses, "409", "conflict");
    }
    if matches!(operation_id, "update_page" | "restore_page_version") {
        add_code(responses, "423", "page_locked");
    }
    if matches!(operation_id, "create_page" | "move_page") {
        add_code(responses, "409", "database_in_database");
    }
    if operation_id == "restore_page" {
        add_code(responses, "409", "database_trashed");
    }
    if matches!(
        operation_id,
        "restore_page" | "duplicate_page" | "create_page" | "move_page"
    ) {
        add_code(responses, "409", "database_row_limit");
    }
    if operation_id == "list_recent_pages" {
        add_code(responses, "422", "validation_failed");
    }
}

fn database_operation(operation_id: &str) -> bool {
    matches!(
        operation_id,
        "get_database"
            | "create_database_property"
            | "update_database_property"
            | "delete_database_property"
            | "create_database_view"
            | "update_database_view"
            | "delete_database_view"
            | "create_database_row"
            | "update_database_row_values"
            | "trash_database_rows"
            | "move_database_row"
            | "import_database_csv"
            | "create_database_from_csv"
            | "submit_database_form"
            | "create_database_template"
            | "update_database_template"
            | "delete_database_template"
    )
}

fn database_errors(operation_id: &str, responses: &mut BTreeMap<&'static str, Vec<&'static str>>) {
    add_code(responses, "404", "page_not_found");
    if operation_id == "get_database" {
        return;
    }
    add_code(responses, "400", "invalid_request");
    add_code(responses, "402", "workspace_read_only");
    add_code(responses, "422", "validation_failed");
    if operation_id.contains("property") || operation_id.contains("view") {
        add_code(responses, "423", "page_locked");
    }
    if matches!(
        operation_id,
        "update_database_property" | "delete_database_property"
    ) {
        add_code(responses, "404", "database_property_not_found");
        add_code(responses, "409", "database_title_property");
    }
    if matches!(
        operation_id,
        "update_database_view" | "delete_database_view"
    ) {
        add_code(responses, "404", "database_view_not_found");
    }
    match operation_id {
        "create_database_property" => add_code(responses, "409", "database_property_limit"),
        "create_database_view" => add_code(responses, "409", "database_view_limit"),
        "delete_database_view" => add_code(responses, "409", "database_last_view"),
        "create_database_row" => add_code(responses, "409", "database_row_limit"),
        "create_database_template" => add_code(responses, "409", "database_template_limit"),
        "update_database_template" | "delete_database_template" => {
            add_code(responses, "404", "database_template_not_found");
        }
        "import_database_csv" => {
            add_code(responses, "409", "database_row_limit");
            add_code(responses, "409", "database_property_limit");
            add_code(responses, "413", "request_too_large");
            add_code(responses, "423", "page_locked");
        }
        "create_database_from_csv" => add_code(responses, "413", "request_too_large"),
        "submit_database_form" => {
            add_code(responses, "404", "database_view_not_found");
            add_code(responses, "409", "database_form_closed");
            add_code(responses, "409", "database_row_limit");
            add_code(responses, "429", "database_form_rate_limited");
        }
        _ => {}
    }
}

fn page_comment_operation(operation_id: &str) -> bool {
    matches!(
        operation_id,
        "list_page_threads"
            | "create_page_thread"
            | "delete_page_thread"
            | "create_page_comment"
            | "update_page_comment"
            | "delete_page_comment"
            | "resolve_page_thread"
            | "reopen_page_thread"
    )
}

fn page_comment_errors(
    operation_id: &str,
    responses: &mut BTreeMap<&'static str, Vec<&'static str>>,
) {
    add_code(responses, "404", "page_not_found");
    if operation_id != "list_page_threads" && operation_id != "create_page_thread" {
        add_code(responses, "404", "page_comment_not_found");
    }
    if matches!(
        operation_id,
        "create_page_thread" | "create_page_comment" | "update_page_comment"
    ) {
        add_code(responses, "422", "validation_failed");
    }
    if matches!(
        operation_id,
        "delete_page_thread" | "update_page_comment" | "delete_page_comment"
    ) {
        add_code(responses, "403", "page_comment_forbidden");
    }
}

/// Every chat operation is in a workspace the caller may not be in, or names a conversation
/// or message the caller cannot see: both are `chat_not_found`.
fn chat_errors(operation_id: &str, responses: &mut BTreeMap<&'static str, Vec<&'static str>>) {
    add_code(responses, "404", "chat_not_found");
    if operation_id == "upload_chat_file" {
        add_code(responses, "400", "invalid_multipart");
        add_code(responses, "413", "upload_too_large");
        add_code(responses, "413", "storage_quota_exceeded");
        add_code(responses, "422", "validation_failed");
        return;
    }
    if matches!(
        operation_id,
        "download_chat_file" | "get_chat_emoji_image" | "get_chat_sticker_image"
    ) {
        return;
    }
    if operation_id == "create_chat_emoji" {
        add_code(responses, "400", "invalid_multipart");
        add_code(responses, "403", "chat_forbidden");
        add_code(responses, "409", "emoji_name_taken");
        add_code(responses, "409", "emoji_limit_reached");
        add_code(responses, "413", "emoji_too_large");
        add_code(responses, "413", "storage_quota_exceeded");
        add_code(responses, "422", "validation_failed");
        add_code(responses, "422", "invalid_emoji");
        return;
    }
    if operation_id == "create_chat_sticker" {
        add_code(responses, "400", "invalid_multipart");
        add_code(responses, "403", "chat_forbidden");
        add_code(responses, "409", "sticker_name_taken");
        add_code(responses, "409", "sticker_limit_reached");
        add_code(responses, "413", "sticker_too_large");
        add_code(responses, "413", "storage_quota_exceeded");
        add_code(responses, "422", "validation_failed");
        add_code(responses, "422", "invalid_sticker");
        return;
    }
    if matches!(operation_id, "delete_chat_emoji" | "delete_chat_sticker") {
        add_code(responses, "403", "chat_forbidden");
        return;
    }
    if operation_id == "click_chat_action" {
        add_code(responses, "403", "chat_forbidden");
        add_code(responses, "422", "validation_failed");
        add_code(responses, "409", "chat_conflict");
        add_code(responses, "502", "integration_failed");
        add_code(responses, "503", "app_key_missing");
        return;
    }
    if matches!(
        operation_id,
        "set_chat_webhook_interactivity"
            | "get_chat_webhook_signing_secret"
            | "rotate_chat_webhook_signing_secret"
            | "list_chat_webhook_deliveries"
    ) {
        add_code(responses, "403", "chat_forbidden");
        add_code(responses, "409", "chat_conflict");
        add_code(responses, "422", "validation_failed");
        add_code(responses, "503", "app_key_missing");
        return;
    }
    if operation_id.ends_with("_chat_webhook") || operation_id == "list_chat_webhooks" {
        add_code(responses, "403", "chat_forbidden");
        if matches!(operation_id, "create_chat_webhook" | "update_chat_webhook") {
            add_code(responses, "422", "validation_failed");
        }
        if operation_id == "create_chat_webhook" {
            add_code(responses, "409", "chat_conflict");
        }
        return;
    }
    if !operation_id.starts_with("list_") {
        add_code(responses, "422", "validation_failed");
    }
    if matches!(operation_id, "send_chat_message" | "edit_chat_message") {
        add_code(responses, "422", "chat_message_too_long");
    }
    if matches!(
        operation_id,
        "create_chat_channel" | "update_chat_channel" | "add_chat_reaction"
    ) {
        add_code(responses, "409", "chat_conflict");
    }
    if !(operation_id.starts_with("list_")
        || operation_id.starts_with("get_")
        || operation_id.starts_with("read_")
        || matches!(
            operation_id,
            "create_chat_channel" | "open_chat_dm" | "follow_chat_thread" | "restore_chat_read"
        ))
    {
        add_code(responses, "403", "chat_forbidden");
    }
}

fn teamspace_operation(operation_id: &str) -> bool {
    matches!(
        operation_id,
        "list_teamspaces"
            | "create_teamspace"
            | "update_teamspace"
            | "delete_teamspace"
            | "move_teamspace"
    )
}

fn teamspace_errors(operation_id: &str, responses: &mut BTreeMap<&'static str, Vec<&'static str>>) {
    add_code(responses, "404", "teamspace_not_found");
    if matches!(
        operation_id,
        "create_teamspace" | "update_teamspace" | "move_teamspace"
    ) {
        add_code(responses, "422", "validation_failed");
    }
    if matches!(
        operation_id,
        "update_teamspace" | "delete_teamspace" | "move_teamspace"
    ) {
        add_code(responses, "409", "conflict");
    }
    if operation_id == "delete_teamspace" {
        add_code(responses, "403", "workspace_action_forbidden");
        add_code(responses, "409", "teamspace_not_empty");
        add_code(responses, "422", "last_teamspace");
    }
}

fn add_download_media_types(operation_id: &str, operation: &mut utoipa::openapi::path::Operation) {
    let schema = match operation_id {
        "download_task_attachment" | "download_comment_attachment" => "AttachmentDownload",
        "download_page_file" => "PageFileDownload",
        "download_chat_file" => "ChatFileDownload",
        _ => return,
    };
    let Some(RefOr::T(response)) = operation.responses.responses.get_mut("200") else {
        return;
    };
    for media_type in [
        "application/pdf",
        "image/avif",
        "image/gif",
        "image/jpeg",
        "image/png",
        "image/svg+xml",
        "image/webp",
        "text/html",
    ] {
        response.content.insert(
            media_type.to_owned(),
            Content::new(Some(Ref::from_schema_name(schema))),
        );
    }
}

fn problem_response(
    description: &str,
    schema: &str,
) -> utoipa::openapi::RefOr<utoipa::openapi::Response> {
    ResponseBuilder::new()
        .description(description)
        .content(
            "application/problem+json",
            Content::new(Some(Ref::from_schema_name(schema))),
        )
        .build()
        .into()
}

pub fn openapi_json() -> Result<String, serde_json::Error> {
    let mut json = serde_json::to_string_pretty(&ApiDocument::openapi())?;
    json.push('\n');
    Ok(json)
}
