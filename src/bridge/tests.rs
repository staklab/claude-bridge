use super::*;
use std::{ffi::OsString, os::unix::fs::symlink};

fn home() -> (tempfile::TempDir, Paths) {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::new(dir.path().to_path_buf());
    (dir, paths)
}

#[test]
fn fresh_users_have_independent_hosts_and_no_developer_store() {
    let (_a, a) = home();
    let (_b, b) = home();
    let first = Settings::load(&a).unwrap();
    let second = Settings::load(&b).unwrap();
    assert_ne!(first.installation_id, second.installation_id);
    assert_eq!(first.auth_dir(&a), a.root.join("config"));
    assert!(first.auth_directory.is_none());
    assert!(first.model.is_none());
    first.save(&a).unwrap();
    assert_eq!(
        Settings::load(&a).unwrap().installation_id,
        first.installation_id
    );
}

#[test]
fn launchd_label_alone_does_not_authorize_stopping_another_installation() {
    let (_a, a) = home();
    let (_b, b) = home();
    let listing = format!(
        "gui/501/{SERVICE} = {{\n path = {}\n program = {}\n}}",
        a.agent().display(),
        a.bin().join("claude-gpt").display()
    );
    assert!(service::launchd_matches(&listing, &a));
    assert!(!service::launchd_matches(&listing, &b));
    assert!(!service::launchd_matches(
        &format!("{listing}\n program = /another/program"),
        &a
    ));
    assert!(!service::launchd_matches(SERVICE, &a));
}

#[test]
fn settings_are_private_and_corrupt_state_is_never_overwritten() {
    let (_dir, paths) = home();
    let settings = Settings::default();
    settings.save(&paths).unwrap();
    assert_eq!(
        fs::metadata(paths.config()).unwrap().permissions().mode() & 0o777,
        0o600
    );
    fs::write(paths.config(), b"broken data").unwrap();
    assert!(Settings::load(&paths).is_err());
    assert_eq!(fs::read(paths.config()).unwrap(), b"broken data");
}

#[test]
fn private_install_rejects_symlink_escape_and_hard_link() {
    let (_dir, paths) = home();
    paths.prepare().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    symlink(elsewhere.path(), paths.root.join("escape")).unwrap();
    assert!(private_dir(&paths.root.join("escape/subdir"), &paths.home).is_err());
    let file = paths.root.join("file");
    fs::write(&file, b"keep").unwrap();
    fs::hard_link(&file, paths.root.join("other")).unwrap();
    assert!(atomic_write(&file, b"changed", 0o600).is_err());
    assert_eq!(fs::read(file).unwrap(), b"keep");
}

#[test]
fn native_arguments_and_other_settings_survive_connection_isolation() {
    let (_dir, paths) = home();
    let source = json!({"env":{"ANTHROPIC_BASE_URL":"https://wrong.example","CLAUDE_CODE_USE_BEDROCK":"1","KEEP_ME":"yes"},"permissions":{"defaultMode":"default"}}).to_string();
    let args: Vec<OsString> = [
        "--resume",
        "conversation-id",
        "-p",
        "test",
        "--settings",
        &source,
    ]
    .iter()
    .map(OsString::from)
    .collect();
    let before = std::env::var_os("ANTHROPIC_BASE_URL");
    let cwd = std::env::current_dir().unwrap();
    let (forwarded, settings) = launcher::launch_settings(
        &paths,
        args,
        "http://127.0.0.1:12345",
        "local-test-cookie",
        "account-model",
    )
    .unwrap();
    assert_eq!(
        forwarded,
        ["--resume", "conversation-id", "-p", "test"].map(OsString::from)
    );
    assert_eq!(
        settings["env"]["ANTHROPIC_BASE_URL"],
        "http://127.0.0.1:12345"
    );
    assert_eq!(settings["env"]["CLAUDE_CODE_USE_BEDROCK"], "0");
    assert_eq!(settings["env"]["KEEP_ME"], "yes");
    assert_eq!(settings["env"]["ANTHROPIC_API_KEY"], "");
    assert_eq!(settings["env"]["ANTHROPIC_AUTH_TOKEN"], "local-test-cookie");
    assert_eq!(settings["permissions"]["defaultMode"], "default");
    assert_eq!(
        settings["env"]["ANTHROPIC_DEFAULT_HAIKU_MODEL"],
        "account-model"
    );
    assert_eq!(std::env::var_os("ANTHROPIC_BASE_URL"), before);
    assert_eq!(std::env::current_dir().unwrap(), cwd);
}

#[test]
fn settings_file_input_is_merged_without_modifying_the_file() {
    let (_dir, paths) = home();
    let file = paths.home.join("native.json");
    let original = br#"{"env":{"OTHER":"keep"},"enabledPlugins":{"example":true}}"#;
    fs::write(&file, original).unwrap();
    let (_, result) = launcher::launch_settings(
        &paths,
        vec![OsString::from(format!("--settings={}", file.display()))],
        "http://127.0.0.1:1",
        "cookie",
        "catalog-model",
    )
    .unwrap();
    assert_eq!(result["enabledPlugins"]["example"], true);
    assert_eq!(fs::read(file).unwrap(), original);
}

#[test]
fn local_cookie_is_private_persistent_and_separate_from_oauth() {
    let (_dir, paths) = home();
    let key = service::secret(&paths).unwrap();
    assert_eq!(key.len(), 64);
    assert_eq!(service::secret(&paths).unwrap(), key);
    assert!(!paths.root.join("config/siwc/state.json").exists());
    fs::set_permissions(
        paths.runtime().join("local.key"),
        fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    assert!(service::secret(&paths).is_err());
}

#[test]
fn live_session_protects_service_and_stale_session_is_cleaned() {
    let (_dir, paths) = home();
    paths.prepare().unwrap();
    let settings_file = paths.runtime().join("session-test.json");
    fs::write(&settings_file, b"{}").unwrap();
    service::lease(&paths, &settings_file).unwrap();
    assert_eq!(service::active_leases(&paths).unwrap(), 1);
    let lease = paths
        .runtime()
        .join("leases")
        .join(format!("{}.json", std::process::id()));
    atomic_write(&lease, &serde_json::to_vec(&json!({"pid":std::process::id(),"started":"different-process-start","settings_file":settings_file})).unwrap(), 0o600).unwrap();
    assert_eq!(service::active_leases(&paths).unwrap(), 0);
    assert!(!settings_file.exists());
}

#[tokio::test]
async fn foreign_listener_never_receives_credentials_and_is_not_trusted() {
    let (_dir, paths) = home();
    paths.prepare().unwrap();
    let settings = Settings::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let connection = service::Connection {
        base_url: format!("http://127.0.0.1:{port}"),
        installation_id: settings.installation_id.clone(),
        model: "test".into(),
        pid: 1,
        version: VERSION.into(),
    };
    atomic_write(
        &paths.runtime().join("service.json"),
        &serde_json::to_vec(&connection).unwrap(),
        0o600,
    )
    .unwrap();
    let response = json!({"product":PRODUCT,"connection":connection,"proof":"fake"});
    let app = axum::Router::new().route(
        "/bridge/identity",
        axum::routing::get(move |headers: http::HeaderMap| {
            let response = response.clone();
            async move {
                assert!(!headers.contains_key("authorization"));
                assert!(!headers.contains_key("x-api-key"));
                axum::Json(response)
            }
        }),
    );
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    assert!(service::probe(&paths, &settings).await.is_err());
    server.abort();
}
