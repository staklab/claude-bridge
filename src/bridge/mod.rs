//! User-scoped desktop installation and runtime. The SIWC protocol remains in its provider.
mod install;
mod launcher;
mod service;
#[cfg(test)]
mod tests;

use anyhow::{Context, Result, bail, ensure};
pub use install::{detect_claude, install, install_claude, plan, uninstall};
pub use launcher::run;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
pub use service::{ensure_service, run_daemon, service_status, stop_service};
use std::{
    fs,
    io::Write,
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process::Command,
};

pub const PRODUCT: &str = "org.claudebridge.desktop";
pub const VERSION: &str = "0.2.0";
pub const SERVICE: &str = "org.claudebridge.agent";

#[derive(Clone)]
pub struct Paths {
    pub home: PathBuf,
    pub root: PathBuf,
}
impl Paths {
    pub fn new(home: PathBuf) -> Self {
        Self {
            root: home.join("Library/Application Support/Claude Bridge"),
            home,
        }
    }
    pub fn current() -> Result<Self> {
        let home = std::env::var_os("HOME").context("无法确定用户目录")?;
        let home = PathBuf::from(home);
        ensure!(home.is_absolute() && home.is_dir(), "用户目录不可用");
        Ok(Self::new(home))
    }
    pub fn bin(&self) -> PathBuf {
        self.root.join("bin")
    }
    pub fn launcher(&self) -> PathBuf {
        self.home.join(".local/bin/claude-gpt")
    }
    pub fn runtime(&self) -> PathBuf {
        self.root.join("runtime")
    }
    pub fn config(&self) -> PathBuf {
        self.root.join("settings.json")
    }
    pub fn agent(&self) -> PathBuf {
        self.home
            .join("Library/LaunchAgents")
            .join(format!("{SERVICE}.plist"))
    }
    pub fn prepare(&self) -> Result<()> {
        for path in [
            &self.root,
            &self.bin(),
            &self.runtime(),
            &self.root.join("logs"),
            &self.root.join("config"),
        ] {
            private_dir(path, &self.home)?;
        }
        Ok(())
    }
    pub fn proxy(&self) -> PathBuf {
        self.bin().join("claude-code-proxy")
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub version: u32,
    pub model: Option<String>,
    pub tested_model: Option<String>,
    pub app_path: Option<PathBuf>,
    pub claude_path: Option<PathBuf>,
    pub auth_directory: Option<PathBuf>,
    pub installation_id: String,
    pub plan_notice_seen: bool,
    pub installed_version: Option<String>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            version: 1,
            model: None,
            tested_model: None,
            app_path: None,
            claude_path: None,
            auth_directory: None,
            installation_id: uuid::Uuid::new_v4().to_string(),
            plan_notice_seen: false,
            installed_version: None,
        }
    }
}
impl Settings {
    pub fn load(paths: &Paths) -> Result<Self> {
        match fs::read(paths.config()) {
            Ok(bytes) => serde_json::from_slice(&bytes).context("应用配置无法读取；未覆盖原文件"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.into()),
        }
    }
    pub fn save(&self, paths: &Paths) -> Result<()> {
        paths.prepare()?;
        atomic_write(&paths.config(), &serde_json::to_vec_pretty(self)?, 0o600)
    }
    pub fn auth_dir(&self, paths: &Paths) -> PathBuf {
        self.auth_directory
            .clone()
            .unwrap_or_else(|| paths.root.join("config"))
    }
}

pub fn private_dir(path: &Path, home: &Path) -> Result<()> {
    ensure!(path.starts_with(home), "安装位置必须位于当前用户目录");
    let mut current = home.to_path_buf();
    for component in path.strip_prefix(home)?.components() {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) => ensure!(
                !metadata.file_type().is_symlink()
                    && metadata.is_dir()
                    && metadata.uid() == fs::metadata(home)?.uid(),
                "目录冲突：{}",
                current.display()
            ),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => fs::create_dir(&current)?,
            Err(e) => return Err(e.into()),
        }
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

pub fn atomic_write(path: &Path, bytes: &[u8], mode: u32) -> Result<()> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        ensure!(
            metadata.is_file() && !metadata.file_type().is_symlink() && metadata.nlink() == 1,
            "文件冲突：{}",
            path.display()
        );
    }
    let parent = path.parent().context("缺少父目录")?;
    let temporary = parent.join(format!(".bridge-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| -> Result<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        Ok(())
    })();
    let _ = fs::remove_file(&temporary);
    result
}

pub fn proxy_command(paths: &Paths, settings: &Settings) -> Command {
    let mut command = Command::new(paths.proxy());
    command
        .env("CCP_CONFIG_DIR", settings.auth_dir(paths))
        .env("CCP_SIWC_APP_NAME", "Claude Bridge")
        .env("XDG_STATE_HOME", paths.root.join("logs"));
    command
}

pub fn proxy_json(paths: &Paths, settings: &Settings, args: &[&str]) -> Result<Value> {
    let output = proxy_command(paths, settings)
        .args(args)
        .output()
        .context("应用运行组件尚未安装，请在向导中修复安装")?;
    ensure!(output.status.success(), "{}", safe_failure(&output.stderr));
    serde_json::from_slice(&output.stdout).context("应用组件返回了无法识别的数据")
}
fn safe_failure(bytes: &[u8]) -> String {
    // Child diagnostics must never return an OAuth authorization URL or credential.
    let text = String::from_utf8_lossy(bytes);
    if text.contains("token")
        || text.contains("Bearer")
        || text.contains("id_token_hint")
        || text.contains("https://auth.openai.com")
    {
        return "授权状态需要检查。请在 Claude Bridge 中重新授权；现有账号记录已保留。".into();
    }
    text.lines()
        .filter(|s| !s.trim_start().starts_with('{'))
        .collect::<Vec<_>>()
        .join("\n")
        .chars()
        .take(700)
        .collect()
}

pub fn account_status(paths: &Paths, settings: &Settings) -> Value {
    match proxy_json(paths, settings, &["siwc", "auth", "status"]) {
        Ok(value) => value["profiles"].as_array().and_then(|p| p.iter().find(|p| p["selected"] == true)).map(|p| json!({"signed_in":p["signed_in"],"plan_permission":p["plan_permission"],"email":p["email"],"reauthorization_required":p["reauthorization_required"]})).unwrap_or_else(|| json!({"signed_in":false,"plan_permission":false})),
        Err(_) => json!({"signed_in":false,"plan_permission":false}),
    }
}

pub fn models(paths: &Paths, settings: &Settings) -> Result<Value> {
    proxy_json(paths, settings, &["siwc", "models"])
}

pub async fn status(paths: &Paths) -> Result<Value> {
    let settings = Settings::load(paths)?;
    let installed = paths.proxy().is_file() && paths.bin().join("claude-gpt").is_file();
    let claude = detect_claude(paths, &settings);
    Ok(
        json!({"product":PRODUCT,"version":VERSION,"installed":installed,
        "installed_version":settings.installed_version,"launcher_installed":fs::read_link(paths.launcher()).ok().as_deref() == Some(paths.bin().join("claude-gpt").as_path()),
        "claude_path":claude,"model":settings.model,"tested_model":settings.tested_model,
        "account":if installed { account_status(paths, &settings) } else { json!({"signed_in":false,"plan_permission":false}) },
        "service":service_status(paths, &settings).await,"plan_notice_seen":settings.plan_notice_seen,
        "shell":install::shell_status(paths),"legacy_authorization_available":paths.home.join(".config/claude-code-proxy/siwc/state.json").is_file()}),
    )
}

pub fn open_app(paths: &Paths) {
    if let Ok(settings) = Settings::load(paths)
        && let Some(app) = settings.app_path
    {
        let _ = Command::new("/usr/bin/open").arg(app).spawn();
    }
}

pub async fn select_model(paths: &Paths, slug: String) -> Result<()> {
    let mut settings = Settings::load(paths)?;
    let catalog = models(paths, &settings)?;
    ensure!(
        catalog
            .as_array()
            .is_some_and(|items| items.iter().any(|m| m["id"] == slug)),
        "该模型不在此账号的当前目录中，请刷新后选择"
    );
    stop_service(paths, &settings).await?;
    settings.model = Some(slug);
    settings.tested_model = None;
    settings.save(paths)
}

pub async fn test_connection(paths: &Paths) -> Result<Value> {
    let mut settings = Settings::load(paths)?;
    let connection = ensure_service(paths, &settings).await?;
    let cookie = service::secret(paths)?;
    let response = reqwest::Client::builder().no_proxy().build()?.post(format!("{}/v1/messages", connection.base_url))
        .header("x-api-key", cookie).json(&json!({"model":"siwc","stream":false,"max_tokens":32,"messages":[{"role":"user","content":"Reply exactly: BRIDGE OK"}]}))
        .timeout(std::time::Duration::from_secs(120)).send().await?;
    let status = response.status();
    let value: Value = response.json().await.context("连接测试没有返回有效结果")?;
    if !status.is_success() {
        let code = value["error"]["code"]
            .as_str()
            .unwrap_or("connection_failed");
        let safe_code: String = code
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
            .take(128)
            .collect();
        bail!(
            "连接测试失败（{safe_code} / HTTP {}）。请检查账号权限、模型或 ChatGPT Usage。",
            status.as_u16()
        );
    }
    let text: String = value["content"]
        .as_array()
        .context("缺少完成的文本")?
        .iter()
        .filter_map(|b| b["text"].as_str())
        .collect();
    ensure!(
        value["stop_reason"] == "end_turn" && text.trim() == "BRIDGE OK",
        "模型未完成预期轻量测试，请重新选择模型或重试"
    );
    settings.tested_model = settings.model.clone();
    settings.save(paths)?;
    Ok(json!({"completed":true,"model":settings.model,"request_id":value["upstream_request_id"]}))
}
