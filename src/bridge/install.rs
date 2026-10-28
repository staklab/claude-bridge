use super::*;
use sha2::{Digest, Sha256};
use std::{os::unix::fs::symlink, time::Duration};

pub(super) const PATH_BLOCK: &str = "# >>> Claude Bridge PATH >>>\ncase \":${PATH}:\" in\n  *\":$HOME/.local/bin:\"*) ;;\n  *) export PATH=\"$HOME/.local/bin:$PATH\" ;;\nesac\n# <<< Claude Bridge PATH <<<\n";

pub fn detect_claude(paths: &Paths, settings: &Settings) -> Option<PathBuf> {
    let mut candidates = vec![
        paths.home.join(".local/bin/claude"),
        PathBuf::from("/opt/homebrew/bin/claude"),
        PathBuf::from("/usr/local/bin/claude"),
    ];
    if let Some(path) = &settings.claude_path {
        candidates.insert(0, path.clone());
    }
    if let Some(path) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&path).map(|p| p.join("claude")));
    }
    candidates.into_iter().find(|p| {
        p.is_file()
            && fs::metadata(p).is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
            && fs::canonicalize(p).ok() != fs::canonicalize(paths.bin().join("claude-gpt")).ok()
    })
}

fn shell_files(paths: &Paths) -> Result<Vec<PathBuf>> {
    let output = shell_probe(
        paths,
        &[
            "-c",
            "printf '\\nCLAUDE_BRIDGE_ZDOTDIR=%s\\n' \"${ZDOTDIR:-$HOME}\"",
        ],
    )?;
    let directory = output
        .lines()
        .find_map(|line| line.strip_prefix("CLAUDE_BRIDGE_ZDOTDIR="))
        .map(PathBuf::from)
        .context("无法检测 zsh 配置目录")?;
    ensure!(
        directory.is_absolute() && directory.starts_with(&paths.home),
        "ZDOTDIR 位于用户目录之外，请在终端设置中选择用户目录后修复安装"
    );
    Ok(vec![directory.join(".zprofile"), directory.join(".zshrc")])
}

pub(super) fn shell_probe(paths: &Paths, arguments: &[&str]) -> Result<String> {
    use std::io::{Read, Seek};
    let temporary =
        std::env::temp_dir().join(format!("claude-bridge-shell-{}", uuid::Uuid::new_v4()));
    let result = (|| -> Result<String> {
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .mode(0o600)
            .open(&temporary)?;
        let mut child = Command::new("/bin/zsh")
            .args(arguments)
            .env("HOME", &paths.home)
            .stdin(std::process::Stdio::null())
            .stdout(file.try_clone()?)
            .stderr(std::process::Stdio::null())
            .spawn()?;
        let deadline = std::time::Instant::now() + Duration::from_secs(8);
        loop {
            if child.try_wait()?.is_some() {
                break;
            }
            if std::time::Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                bail!("zsh 启动脚本耗时过长，无法自动验证终端配置");
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        file.rewind()?;
        let mut output = String::new();
        file.take(256 * 1024).read_to_string(&mut output)?;
        Ok(output)
    })();
    let _ = fs::remove_file(temporary);
    result
}

fn editable(path: &Path, home: &Path) -> Result<()> {
    if let Ok(m) = fs::symlink_metadata(path) {
        ensure!(
            m.is_file()
                && !m.file_type().is_symlink()
                && m.nlink() == 1
                && m.uid() == fs::metadata(home)?.uid(),
            "不能自动修改此 shell 配置：{}",
            path.display()
        );
        let content = fs::read_to_string(path)?;
        let start = content.matches("# >>> Claude Bridge PATH >>>").count();
        let end = content.matches("# <<< Claude Bridge PATH <<<").count();
        ensure!(
            (start == 0 && end == 0) || (start == 1 && end == 1 && content.contains(PATH_BLOCK)),
            "PATH 配置块被修改；为保留您的配置，自动安装已停止：{}",
            path.display()
        );
    }
    Ok(())
}

pub fn plan(paths: &Paths) -> Result<Value> {
    let files = shell_files(paths)?;
    for path in &files {
        editable(path, &paths.home)?;
    }
    let program = xml(&paths.bin().join("claude-gpt").to_string_lossy());
    if paths.agent().exists() {
        editable(&paths.agent(), &paths.home)?;
        ensure!(
            fs::read_to_string(paths.agent())?.contains(&program),
            "已存在同名后台服务，未覆盖"
        );
    }
    if let Ok(m) = fs::symlink_metadata(paths.launcher()) {
        ensure!(
            m.file_type().is_symlink()
                && fs::read_link(paths.launcher())? == paths.bin().join("claude-gpt"),
            "已存在其他 claude-gpt 命令，未覆盖：{}",
            paths.launcher().display()
        );
    }
    // Do not shadow another command in a later PATH directory either.
    if let Some(path) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&path) {
            let command = directory.join("claude-gpt");
            if command.exists()
                && command != paths.launcher()
                && fs::canonicalize(&command).ok()
                    != fs::canonicalize(paths.bin().join("claude-gpt")).ok()
            {
                bail!(
                    "PATH 中已有其他 claude-gpt 命令，未覆盖：{}",
                    command.display()
                );
            }
        }
    }
    Ok(
        json!({"launcher":paths.launcher(),"support":paths.root,"shell_files":files,"backup":true,"consent_required":true}),
    )
}

pub fn shell_status(paths: &Paths) -> Value {
    match shell_files(paths) {
        Ok(files) => {
            json!({"configured": files.iter().all(|p| fs::read_to_string(p).is_ok_and(|s| s.contains(PATH_BLOCK))), "files":files})
        }
        Err(e) => json!({"configured":false,"error":e.to_string()}),
    }
}

fn ensure_directory(path: &Path, home: &Path) -> Result<()> {
    let existing = path.exists();
    let mode = fs::metadata(path).ok().map(|m| m.permissions());
    private_dir(path, home)?;
    if existing && let Some(mode) = mode {
        fs::set_permissions(path, mode)?;
    }
    Ok(())
}

fn edit_shell(paths: &Paths, add: bool) -> Result<()> {
    for path in shell_files(paths)? {
        editable(&path, &paths.home)?;
        let old = fs::read_to_string(&path).unwrap_or_default();
        if add == old.contains(PATH_BLOCK) {
            continue;
        }
        if let Some(parent) = path.parent() {
            ensure_directory(parent, &paths.home)?;
        }
        let mode = fs::metadata(&path)
            .map(|m| m.permissions().mode() & 0o777)
            .unwrap_or(0o600);
        if path.exists() {
            let backup = path.with_file_name(format!(
                "{}.claude-bridge-backup-{}",
                path.file_name().unwrap().to_string_lossy(),
                uuid::Uuid::new_v4()
            ));
            atomic_write(&backup, old.as_bytes(), mode)?;
        }
        let new = if add {
            format!(
                "{}{}{}",
                old,
                if old.is_empty() || old.ends_with('\n') {
                    ""
                } else {
                    "\n"
                },
                PATH_BLOCK
            )
        } else {
            old.replace(PATH_BLOCK, "")
        };
        atomic_write(&path, new.as_bytes(), mode)?;
    }
    Ok(())
}

fn xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub fn install(paths: &Paths, app: &Path, consent: bool) -> Result<Value> {
    ensure!(consent, "首次安装需要在界面确认 PATH 配置与备份");
    let preview = plan(paths)?;
    ensure!(
        app.extension().is_some_and(|s| s == "app"),
        "请选择完整的 Claude Bridge.app"
    );
    let resources = app.join("Contents/Resources");
    let manifest: Value = serde_json::from_slice(
        &fs::read(resources.join("manifest.json")).context("安装包缺少完整性清单")?,
    )?;
    ensure!(
        manifest["product"] == PRODUCT && manifest["version"] == VERSION,
        "安装包版本不匹配"
    );
    let mut payload = Vec::new();
    for name in ["claude-gpt", "claude-code-proxy"] {
        let bytes = fs::read(resources.join("bin").join(name)).context("安装包缺少运行组件")?;
        let digest = format!("{:x}", Sha256::digest(&bytes));
        ensure!(
            manifest["binaries"][name] == digest,
            "安装包组件校验失败：{name}"
        );
        payload.push((name, bytes));
    }
    paths.prepare()?;
    let mut settings = Settings::load(paths)?;
    // A running daemon keeps its old executable mapping; never truncate it in place.
    for (name, bytes) in payload {
        atomic_write(&paths.bin().join(name), &bytes, 0o700)?;
    }
    ensure_directory(paths.launcher().parent().unwrap(), &paths.home)?;
    if fs::symlink_metadata(paths.launcher()).is_err() {
        symlink(paths.bin().join("claude-gpt"), paths.launcher())?;
    }
    edit_shell(paths, true)?;
    ensure_directory(paths.agent().parent().unwrap(), &paths.home)?;
    let program = xml(&paths.bin().join("claude-gpt").to_string_lossy());
    let home = xml(&paths.home.to_string_lossy());
    let log = xml(&paths.root.join("logs/service.log").to_string_lossy());
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict><key>Label</key><string>{SERVICE}</string><key>ProgramArguments</key><array><string>{program}</string><string>__bridge</string><string>daemon</string></array><key>EnvironmentVariables</key><dict><key>HOME</key><string>{home}</string><key>PATH</key><string>/usr/bin:/bin:/usr/sbin:/sbin</string></dict><key>RunAtLoad</key><false/><key>KeepAlive</key><false/><key>ProcessType</key><string>Background</string><key>StandardOutPath</key><string>{log}</string><key>StandardErrorPath</key><string>{log}</string></dict></plist>
"#
    );
    atomic_write(&paths.agent(), plist.as_bytes(), 0o600)?;
    settings.app_path = Some(app.to_path_buf());
    settings.claude_path = detect_claude(paths, &settings);
    settings.installed_version = Some(VERSION.into());
    settings.save(paths)?;
    super::service::secret(paths)?;
    Ok(json!({"installed":true,"plan":preview,"restart_terminal":true}))
}

pub async fn uninstall(paths: &Paths, consent: bool) -> Result<Value> {
    ensure!(consent, "请确认卸载启动器（保留账号和 Claude Code 会话）");
    let settings = Settings::load(paths)?;
    stop_service(paths, &settings).await?;
    if fs::read_link(paths.launcher()).ok() == Some(paths.bin().join("claude-gpt")) {
        fs::remove_file(paths.launcher())?;
    }
    edit_shell(paths, false)?;
    if paths.agent().exists()
        && fs::read_to_string(paths.agent())?
            .contains(&xml(&paths.bin().join("claude-gpt").to_string_lossy()))
    {
        fs::remove_file(paths.agent())?;
    }
    for name in ["claude-gpt", "claude-code-proxy"] {
        let _ = fs::remove_file(paths.bin().join(name));
    }
    Ok(json!({"uninstalled":true,"authorization_preserved":true,"claude_sessions_preserved":true}))
}

pub async fn install_claude(paths: &Paths, consent: bool) -> Result<Value> {
    ensure!(consent, "安装第三方 Claude Code 需要您的同意");
    if let Some(path) = detect_claude(paths, &Settings::load(paths)?) {
        return Ok(json!({"installed":true,"path":path,"reused":true}));
    }
    // Download the official installer at runtime; no Anthropic binary is distributed.
    eprintln!("正在从 Anthropic 官方渠道下载安装器…");
    let response = reqwest::Client::builder()
        .https_only(true)
        .build()?
        .get("https://claude.ai/install.sh")
        .timeout(Duration::from_secs(60))
        .send()
        .await?
        .error_for_status()?;
    let script = response.bytes().await?;
    ensure!(
        script.len() < 1024 * 1024 && script.starts_with(b"#!/"),
        "官方安装器格式异常"
    );
    paths.prepare()?;
    let file = paths
        .runtime()
        .join(format!("installer-{}.sh", uuid::Uuid::new_v4()));
    atomic_write(&file, &script, 0o600)?;
    eprintln!("正在安装 Claude Code 原生稳定版；下载期间请保持此窗口打开…");
    let result = Command::new("/bin/bash")
        .arg(&file)
        .arg("stable")
        .env("HOME", &paths.home)
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    let _ = fs::remove_file(file);
    ensure!(
        result?.success(),
        "Anthropic 官方安装失败，请检查网络后重试。原有数据已保留。"
    );
    let mut settings = Settings::load(paths)?;
    let path = detect_claude(paths, &settings)
        .context("官方安装已结束，但未检测到可执行程序；请重试检测")?;
    settings.claude_path = Some(path.clone());
    settings.save(paths)?;
    Ok(json!({"installed":true,"path":path}))
}
