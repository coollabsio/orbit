//! All integration tests in one test binary: one link step and one file in `target/`, not one
//! binary for each file. Run one file with `cargo test -p orbit-server --test it <file>::`.

#![recursion_limit = "256"]

mod collab_support;
mod database_fixture;
mod db_fixture;
mod fake_github;
mod shared_postgres;

mod account_api;
mod api_tokens_backends;
mod app_smoke;
mod asana_client;
mod asana_import_api;
mod attachments_api;
mod backup_items_api;
mod billing_api;
mod chat_api;
mod chat_apps;
mod chat_live;
mod chat_safety;
mod cloud_limits;
mod collab_api;
mod collab_convert;
mod collab_reconcile;
mod custom_pages_api;
mod database_api_fixes;
mod database_api_tools;
mod database_buttons;
mod database_calendar;
mod database_csv;
mod database_export;
mod database_fields;
mod database_forms;
mod database_formula_bench;
mod database_formulas;
mod database_gallery_timeline;
mod database_moves;
mod database_relations;
mod databases;
mod database_templates;
mod doc_markdown_writes;
mod email_api;
mod export_api;
mod export_markdown;
mod github_connect;
mod github_demo;
mod github_notifications;
mod github_pulls;
mod github_reviews;
mod guest_access;
mod integrations_api;
mod job_limits_api;
mod linear_client;
mod linear_import_api;
mod mail_adapters;
mod mail_admin;
mod mail_bounces;
mod mail_certificate;
mod mail_compose;
mod mail_contacts;
mod mail_content;
mod mail_deliver;
mod mail_dkim;
mod mail_domains;
mod mail_drafts;
mod mail_events;
mod mail_file_links;
mod mail_inbound;
mod mail_ingest;
mod mail_intake;
mod mail_keys;
mod mail_listener;
mod mail_mailboxes;
mod mail_providers;
mod mail_schema;
mod mail_send_limits;
mod mail_send_routes;
mod mail_send;
mod mail_threads;
mod mail_vacation;
mod markdown_import;
mod markdown_merge;
mod mcp_api;
mod notifications_api;
mod notion_client;
mod notion_convert;
mod notion_import_api;
mod oauth_api;
mod obsidian_import_api;
mod obsidian_markdown;
mod obsidian_vault;
mod openapi_contract;
mod orbit_import_api;
mod page_comments_api;
mod page_files_api;
mod page_options_api;
mod pages_api;
mod page_versions_api;
mod passkey_app_sign_in;
mod private_projects;
mod profiles_api;
mod public_api_import;
mod public_api_rate_limits;
mod public_api;
mod push_api;
mod realtime;
mod shortcuts_api;
mod sign_in_providers_api;
mod storage_api;
mod task_collab_api;
mod task_fields_tokens;
mod tasks_api;
mod teamspaces_api;
mod time_entries_api;
mod trello_client;
mod trello_import_api;
mod two_factor_api;
mod views_api;
mod webhooks_api;
mod workspace_export_api;
mod workspace_roles_api;
mod workspaces_api;
mod workspace_storage_api;

/// With `autotests = false`, cargo skips a file in `tests/` that has no `mod` line above.
#[test]
fn every_test_file_is_a_module_of_it() {
    let root = include_str!("it.rs");
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let missing: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter_map(|name| name.strip_suffix(".rs").map(str::to_owned))
        .filter(|name| name != "it" && !root.contains(&format!("\nmod {name};\n")))
        .collect();
    assert!(missing.is_empty(), "add `mod <file>;` to tests/it.rs for: {missing:?}");
}
