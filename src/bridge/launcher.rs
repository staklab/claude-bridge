use super::*;
use std::{ffi::OsString, os::unix::process::CommandExt};

pub fn run() -> Result<()> {
    let paths = Paths::current()?;
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    if args.first().is_some_and(|s| s == "__bridge") {
        let action = args.get(1).and_then(|s| s.to_str()).unwrap_or("status");
        if action == "auth-login" {
            let settings = Settings::load(&paths)?;
            let error = proxy_command(&paths, &settings)
                .args(["siwc", "auth", "login", "--consent"])
                .exec();
            return Err(error.into());
        }
        let settings = Settings::load(&paths)?;
        if action == "daemon" {
            // This binary has not created a runtime or any threads yet. Only the standalone
            // daemon inherits these process-local settings; the user's shell is untouched.
            unsafe {
                std::env::set_var("CCP_CONFIG_DIR", settings.auth_dir(&paths));
                std::env::set_var("XDG_STATE_HOME", paths.root.join("logs"));
                std::env::set_var("CCP_ALIAS_PROVIDER", "siwc");
                std::env::remove_var("CCP_LOG_VERBOSE");
                std::env::set_var("CCP_TRAFFIC_LOG", "0");
            }
        }
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;
        let value = runtime.block_on(async {
            let consent = args.iter().any(|s| s == "--consent");
            let parameter = |index: usize| {
                args.get(index)
                    .and_then(|s| s.to_str())
                    .context("缺少操作参数")
            };
            match action {
                "status" => status(&paths).await,
                "install-plan" => plan(&paths),
                "install" => install(&paths, Path::new(parameter(2)?), consent),
                "install-claude" => install_claude(&paths, consent).await,
                "uninstall" => uninstall(&paths, consent).await,
                "daemon" => {
                    run_daemon(&paths, &settings).await?;
                    Ok(json!({"stopped":true}))
                }
                "models" => models(&paths, &settings),
                "set-model" => {
                    select_model(&paths, parameter(2)?.into()).await?;
                    Ok(json!({"saved":true}))
                }
                "test" => test_connection(&paths).await,
                "start" => {
                    let c = ensure_service(&paths, &settings).await?;
                    Ok(json!({"running":true,"model":c.model,"verified":true}))
                }
                "stop" => {
                    stop_service(&paths, &settings).await?;
                    Ok(json!({"stopped":true}))
                }
                "notice-seen" => {
                    let mut s = settings;
                    s.plan_notice_seen = true;
                    s.save(&paths)?;
                    Ok(json!({"saved":true}))
                }
                "use-existing-auth" => {
                    ensure!(consent, "使用此 Mac 已有 SIWC 授权需要您的确认");
                    let mut s = settings;
                    let state_file = s.auth_dir(&paths).join("siwc/state.json");
                    let has_registration = if state_file.exists() {
                        let state: Value = serde_json::from_slice(&fs::read(state_file)?)?;
                        !state["profiles"]
                            .as_object()
                            .is_some_and(|profiles| profiles.is_empty())
                    } else {
                        false
                    };
                    ensure!(
                        s.auth_directory.is_none() && !has_registration,
                        "应用已有账号记录，请使用原账号重新授权"
                    );
                    let directory = paths.home.join(".config/claude-code-proxy");
                    ensure!(
                        directory.join("siwc/state.json").is_file(),
                        "未检测到已有 SIWC 授权"
                    );
                    s.auth_directory = Some(directory);
                    s.save(&paths)?;
                    Ok(json!({"linked":true,"credentials_copied":false}))
                }
                "verify-shell" => verify_shell(&paths),
                _ => bail!("未知应用操作"),
            }
        })?;
        println!("{}", serde_json::to_string(&value)?);
        return Ok(());
    }
    let result = native(&paths, args);
    if result.is_err() {
        open_app(&paths);
    }
    result
}

pub(super) fn verify_shell(paths: &Paths) -> Result<Value> {
    let output = install::shell_probe(
        paths,
        &[
            "-lic",
            "printf '\\nCLAUDE_BRIDGE_PATH=%s\\n' \"$(command -v claude-gpt)\"",
        ],
    )?;
    let expected = format!("CLAUDE_BRIDGE_PATH={}", paths.launcher().display());
    let found = output.lines().any(|s| s == expected);
    Ok(json!({"verified":found,"new_terminal_required":true}))
}

pub(super) fn launch_settings(
    paths: &Paths,
    args: Vec<OsString>,
    base: &str,
    cookie: &str,
    model: &str,
) -> Result<(Vec<OsString>, Value)> {
    let mut forwarded = Vec::new();
    let mut settings = json!({});
    let mut index = 0;
    while index < args.len() {
        if args[index] == "--" {
            forwarded.extend_from_slice(&args[index..]);
            break;
        }
        let argument = args[index].to_string_lossy();
        let source = if argument == "--settings" {
            index += 1;
            Some(
                args.get(index)
                    .context("--settings 缺少参数")?
                    .to_string_lossy()
                    .to_string(),
            )
        } else {
            argument.strip_prefix("--settings=").map(str::to_string)
        };
        if let Some(source) = source {
            let value: Value = if source.trim_start().starts_with('{') {
                serde_json::from_str(&source)?
            } else {
                serde_json::from_slice(&fs::read(&source).context("无法读取 --settings 文件")?)?
            };
            ensure!(value.is_object(), "--settings 必须是 JSON 对象");
            for (k, v) in value.as_object().unwrap() {
                settings[k] = v.clone();
            }
        } else {
            forwarded.push(args[index].clone());
        }
        index += 1;
    }
    if !settings["env"].is_object() {
        settings["env"] = json!({});
    }
    for (name, value) in connection_environment(base, cookie, model) {
        settings["env"][name] = json!(value);
    }
    // Explicit command-line settings take priority over user/project settings without editing them.
    settings["model"] = json!(model);
    let _ = paths;
    Ok((forwarded, settings))
}

fn connection_environment<'a>(
    base: &'a str,
    cookie: &'a str,
    model: &'a str,
) -> Vec<(&'static str, &'a str)> {
    vec![
        ("ANTHROPIC_BASE_URL", base),
        // Bearer gateway authentication avoids native Claude's interactive API-key
        // approval screen and cannot reuse the user's Anthropic billing key.
        ("ANTHROPIC_API_KEY", ""),
        ("ANTHROPIC_AUTH_TOKEN", cookie),
        ("ANTHROPIC_MODEL", model),
        ("ANTHROPIC_DEFAULT_HAIKU_MODEL", model),
        ("ANTHROPIC_DEFAULT_SONNET_MODEL", model),
        ("ANTHROPIC_DEFAULT_OPUS_MODEL", model),
        ("ANTHROPIC_SMALL_FAST_MODEL", model),
        ("CLAUDE_CODE_USE_BEDROCK", "0"),
        ("CLAUDE_CODE_USE_VERTEX", "0"),
        ("CLAUDE_CODE_USE_FOUNDRY", "0"),
        ("CLAUDE_CODE_OAUTH_TOKEN", ""),
        ("ANTHROPIC_CUSTOM_HEADERS", ""),
        ("CLAUDE_CODE_API_KEY_HELPER_TTL_MS", "0"),
        ("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1"),
        ("DISABLE_LOGIN_COMMAND", "1"),
        ("DISABLE_LOGOUT_COMMAND", "1"),
        ("NO_PROXY", "localhost,127.0.0.1,::1"),
        ("no_proxy", "localhost,127.0.0.1,::1"),
    ]
}

fn native(paths: &Paths, args: Vec<OsString>) -> Result<()> {
    ensure!(
        !args.iter().take_while(|arg| *arg != "--").any(|arg| {
            let value = arg.to_string_lossy();
            ["--cloud", "--teleport", "--remote-control", "--environment"]
                .iter()
                .any(|flag| value == *flag || value.starts_with(&format!("{flag}=")))
        }),
        "Claude GPT 仅使用本机 SIWC 连接。远程 Claude 服务请单独运行 claude 并完成 Anthropic 认证。"
    );
    ensure!(
        !args.first().is_some_and(|arg| [
            "auth",
            "setup-token",
            "remote-control",
            "web",
            "install"
        ]
        .iter()
        .any(|name| arg == name)),
        "Claude GPT 使用本机 ChatGPT 授权。请在 Claude Bridge 中登录；如需 Anthropic 原生认证或远程模式，请单独运行 claude。"
    );
    let settings = Settings::load(paths)?;
    let claude = detect_claude(paths, &settings)
        .context("未检测到 Claude Code，请在 Claude Bridge 中一键安装 Anthropic 官方版本")?;
    if args.len() == 1 && matches!(args[0].to_str(), Some("--version" | "--help" | "-h")) {
        return Err(Command::new(claude).args(args).exec().into());
    }
    ensure!(
        settings.model.is_some() && settings.tested_model == settings.model,
        "请先打开 Claude Bridge，完成 ChatGPT 登录、选择模型和测试连接"
    );
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let connection = runtime.block_on(ensure_service(paths, &settings))?;
    drop(runtime);
    let cookie = service::secret(paths)?;
    let (mut args, value) = launch_settings(
        paths,
        args,
        &connection.base_url,
        &cookie,
        &connection.model,
    )?;
    let settings_file = paths
        .runtime()
        .join(format!("session-{}.json", uuid::Uuid::new_v4()));
    atomic_write(&settings_file, &serde_json::to_vec(&value)?, 0o600)?;
    args.splice(
        0..0,
        [
            OsString::from("--settings"),
            settings_file.clone().into_os_string(),
        ],
    );
    service::lease(paths, &settings_file)?;
    let mut command = Command::new(claude);
    command.args(args);
    for (name, value) in connection_environment(&connection.base_url, &cookie, &connection.model) {
        command.env(name, value);
    }
    // exec preserves terminal behavior, working directory, exit code and the PID used by the lease.
    let error = command.exec();
    let _ = fs::remove_file(settings_file);
    Err(error.into())
}
