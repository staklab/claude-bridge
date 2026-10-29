use super::*;
use axum::{
    Json, Router,
    body::Body,
    extract::{Query, State},
    http::{Request, StatusCode},
    middleware::{Next, from_fn_with_state},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use futures_util::StreamExt;
use hmac::{Hmac, Mac};
use rand::RngCore;
use sha2::Sha256;
use std::{
    collections::HashMap,
    fs::File,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

#[derive(Clone, Serialize, Deserialize)]
pub struct Connection {
    pub base_url: String,
    pub installation_id: String,
    pub model: String,
    pub pid: u32,
    pub version: String,
}

pub(super) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn secret(paths: &Paths) -> Result<String> {
    paths.prepare()?;
    let file = paths.runtime().join("local.key");
    if !file.exists() {
        let mut bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut bytes);
        let mut handle = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&file)?;
        handle.write_all(hex(&bytes).as_bytes())?;
    }
    let metadata = fs::symlink_metadata(&file)?;
    ensure!(
        metadata.is_file()
            && !metadata.file_type().is_symlink()
            && metadata.nlink() == 1
            && metadata.permissions().mode() & 0o077 == 0,
        "本地服务密钥权限异常，请使用修复安装"
    );
    let key = fs::read_to_string(file)?;
    ensure!(
        key.len() == 64 && key.bytes().all(|b| b.is_ascii_hexdigit()),
        "本地服务密钥格式异常"
    );
    Ok(key)
}

fn signature(key: &str, challenge: &str, connection: &Connection) -> Result<Vec<u8>> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key.as_bytes())?;
    mac.update(PRODUCT.as_bytes());
    mac.update(challenge.as_bytes());
    mac.update(&serde_json::to_vec(connection)?);
    Ok(mac.finalize().into_bytes().to_vec())
}

fn lock_file(paths: &Paths, name: &str) -> Result<File> {
    paths.prepare()?;
    let file = paths.runtime().join(name);
    if let Ok(m) = fs::symlink_metadata(&file) {
        ensure!(
            !m.file_type().is_symlink() && m.is_file() && m.nlink() == 1,
            "服务锁文件冲突"
        );
    }
    Ok(fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(file)?)
}

pub(super) async fn probe(paths: &Paths, settings: &Settings) -> Result<Connection> {
    let connection: Connection =
        serde_json::from_slice(&fs::read(paths.runtime().join("service.json"))?)?;
    let url = url::Url::parse(&connection.base_url)?;
    ensure!(
        url.scheme() == "http"
            && url.host_str() == Some("127.0.0.1")
            && url.port().is_some()
            && url.path() == "/"
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none(),
        "服务地址不是本地回环地址"
    );
    ensure!(
        connection.installation_id == settings.installation_id,
        "服务安装身份不匹配"
    );
    let challenge = uuid::Uuid::new_v4().to_string();
    let response: Value = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()?
        .get(format!("{}/bridge/identity", connection.base_url))
        .query(&[("challenge", &challenge)])
        .timeout(Duration::from_secs(2))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    // An arbitrary listener never receives the local cookie or any OAuth credentials.
    ensure!(
        response["product"] == PRODUCT
            && response["connection"] == serde_json::to_value(&connection)?,
        "端口被其他程序占用"
    );
    let expected = signature(&secret(paths)?, &challenge, &connection)?;
    let supplied = response["proof"].as_str().context("服务缺少身份校验")?;
    let mut verifier = Hmac::<Sha256>::new_from_slice(b"Claude Bridge proof comparison")?;
    verifier.update(hex(&expected).as_bytes());
    let mut actual = Hmac::<Sha256>::new_from_slice(b"Claude Bridge proof comparison")?;
    actual.update(supplied.as_bytes());
    actual
        .verify_slice(&verifier.finalize().into_bytes())
        .context("服务身份校验失败")?;
    Ok(connection)
}

fn domain(paths: &Paths) -> Result<String> {
    Ok(format!("gui/{}", fs::metadata(&paths.home)?.uid()))
}

#[derive(PartialEq, Eq)]
enum JobOwner {
    Absent,
    ThisInstallation,
    OtherInstallation,
}

pub(super) fn launchd_matches(output: &str, paths: &Paths) -> bool {
    let programs: Vec<_> = output
        .lines()
        .filter_map(|line| line.trim().strip_prefix("program = "))
        .collect();
    let plists: Vec<_> = output
        .lines()
        .filter_map(|line| line.trim().strip_prefix("path = "))
        .collect();
    programs == [paths.bin().join("claude-gpt").to_string_lossy().as_ref()]
        && plists == [paths.agent().to_string_lossy().as_ref()]
}

fn job_owner(paths: &Paths) -> Result<JobOwner> {
    let output = Command::new("/bin/launchctl")
        .args(["print", &format!("{}/{SERVICE}", domain(paths)?)])
        .output()?;
    if !output.status.success() {
        return Ok(JobOwner::Absent);
    }
    Ok(
        if launchd_matches(&String::from_utf8_lossy(&output.stdout), paths) {
            JobOwner::ThisInstallation
        } else {
            JobOwner::OtherInstallation
        },
    )
}

pub async fn service_status(paths: &Paths, settings: &Settings) -> Value {
    match probe(paths, settings).await {
        Ok(c) => {
            json!({"running":true,"verified":true,"model":c.model,"version":c.version,"active_sessions":active_leases(paths).unwrap_or_default()})
        }
        Err(_) => {
            json!({"running":false,"verified":false,"active_sessions":active_leases(paths).unwrap_or_default()})
        }
    }
}

pub async fn ensure_service(paths: &Paths, settings: &Settings) -> Result<Connection> {
    let model = settings
        .model
        .as_ref()
        .context("请打开 Claude Bridge，选择此账号可用的 GPT 模型并测试连接")?;
    let lock = lock_file(paths, "startup.lock")?;
    let until = Instant::now() + Duration::from_secs(35);
    while lock.try_lock().is_err() {
        ensure!(Instant::now() < until, "后台服务正在启动，请稍后重试");
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    if let Ok(connection) = probe(paths, settings).await {
        ensure!(
            &connection.model == model,
            "后台服务正在使用其他模型，请结束当前会话并在应用中重新选择模型"
        );
        return Ok(connection);
    }
    ensure!(
        paths.agent().is_file() && paths.proxy().is_file(),
        "请先在 Claude Bridge 中完成安装"
    );
    let account = account_status(paths, settings);
    ensure!(
        account["signed_in"] == true && account["plan_permission"] == true,
        "ChatGPT 授权缺失或需要重新授权，请打开 Claude Bridge → Continue with ChatGPT"
    );
    let domain = domain(paths)?;
    let job = format!("{domain}/{SERVICE}");
    let owner = job_owner(paths)?;
    ensure!(
        owner != JobOwner::OtherInstallation,
        "同名后台任务属于其他安装位置，未接管。请从对应的 Claude Bridge 安装中管理服务。"
    );
    if owner == JobOwner::Absent {
        ensure!(
            Command::new("/bin/launchctl")
                .arg("bootstrap")
                .arg(&domain)
                .arg(paths.agent())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()?
                .success(),
            "无法注册用户级按需服务。请在系统设置 → 通用 → 登录项与扩展中检查后台权限，再点击修复安装"
        );
    }
    ensure!(
        Command::new("/bin/launchctl")
            .args(["kickstart", &job])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()?
            .success(),
        "无法启动后台服务，请在应用中修复安装"
    );
    for _ in 0..100 {
        if let Ok(connection) = probe(paths, settings).await {
            ensure!(&connection.model == model, "服务模型与配置不一致");
            return Ok(connection);
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    bail!(
        "后台服务未能就绪，请打开 Claude Bridge 修复安装或重新授权。安全日志位于应用支持目录的 logs 文件夹。"
    )
}

pub async fn stop_service(paths: &Paths, settings: &Settings) -> Result<()> {
    ensure!(
        active_leases(paths)? == 0,
        "仍有 Claude GPT 会话运行，请先退出会话后再更换模型、修复服务或卸载启动器"
    );
    // Only unload our exact plist. Never kill a PID merely read from a file.
    if paths.agent().exists() {
        let plist = fs::read_to_string(paths.agent())?;
        ensure!(
            plist.contains(SERVICE)
                && plist.contains(
                    &paths
                        .bin()
                        .join("claude-gpt")
                        .to_string_lossy()
                        .replace('&', "&amp;")
                ),
            "后台服务文件身份不匹配"
        );
        // launchctl bootout with a plist uses its Label, not its original path.
        // Compare the loaded job too, so an isolated HOME cannot unload another install.
        if job_owner(paths)? == JobOwner::ThisInstallation {
            let _ = Command::new("/bin/launchctl")
                .arg("bootout")
                .arg(domain(paths)?)
                .arg(paths.agent())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        }
    }
    if probe(paths, settings).await.is_ok() {
        bail!("后台服务仍在处理请求，请稍后重试");
    }
    let _ = fs::remove_file(paths.runtime().join("service.json"));
    Ok(())
}

pub(super) fn process_start(pid: u32) -> Option<String> {
    let output = Command::new("/bin/ps")
        .args(["-p", &pid.to_string(), "-o", "lstart="])
        .output()
        .ok()?;
    let value = String::from_utf8(output.stdout).ok()?.trim().to_string();
    (output.status.success() && !value.is_empty()).then_some(value)
}

pub fn lease(paths: &Paths, settings_file: &Path) -> Result<()> {
    private_dir(&paths.runtime().join("leases"), &paths.home)?;
    let pid = std::process::id();
    atomic_write(
        &paths.runtime().join("leases").join(format!("{pid}.json")),
        &serde_json::to_vec(
            &json!({"pid":pid,"started":process_start(pid),"settings_file":settings_file}),
        )?,
        0o600,
    )
}

pub(super) fn active_leases(paths: &Paths) -> Result<usize> {
    let directory = paths.runtime().join("leases");
    if !directory.exists() {
        return Ok(0);
    }
    let mut count = 0;
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let Ok(bytes) = fs::read(entry.path()) else {
            continue;
        };
        let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
            continue;
        };
        let pid = value["pid"].as_u64().and_then(|p| u32::try_from(p).ok());
        if pid
            .and_then(process_start)
            .as_deref()
            .is_some_and(|start| Some(start) == value["started"].as_str())
        {
            count += 1;
        } else {
            if let Some(file) = value["settings_file"].as_str() {
                let file = Path::new(file);
                if file.parent() == Some(paths.runtime().as_path())
                    && file
                        .file_name()
                        .is_some_and(|s| s.to_string_lossy().starts_with("session-"))
                {
                    let _ = fs::remove_file(file);
                }
            }
            let _ = fs::remove_file(entry.path());
        }
    }
    Ok(count)
}

#[derive(Clone)]
struct DaemonState {
    connection: Connection,
    key: String,
    active: Arc<AtomicUsize>,
    touched: Arc<Mutex<Instant>>,
}
struct InFlight(Arc<AtomicUsize>);
impl Drop for InFlight {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

async fn identity(
    State(state): State<DaemonState>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let Some(challenge) = query
        .get("challenge")
        .filter(|s| s.len() == 36 && uuid::Uuid::parse_str(s).is_ok())
    else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    match signature(&state.key, challenge, &state.connection) {
        Ok(proof) => {
            Json(json!({"product":PRODUCT,"connection":state.connection,"proof":hex(&proof)}))
                .into_response()
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[derive(Clone)]
struct UsageCheck {
    provider: Arc<crate::providers::chatgpt_siwc::SiwcProvider>,
    gate: Arc<tokio::sync::Mutex<()>>,
}

async fn check_usage(State(state): State<UsageCheck>) -> Response {
    use crate::Provider;
    let Ok(_guard) = state.gate.try_lock() else {
        return (
            StatusCode::CONFLICT,
            [("x-should-retry", "false")],
            Json(json!({"error":{"code":"bridge_usage_check_in_progress","message":"A usage check is already in progress"}})),
        )
            .into_response();
    };
    // This route is protected by the same local authentication and Origin check
    // as inference. Ordinary Claude Code retries cannot clear the quota pause.
    state.provider.resume_plan_usage();
    let request = serde_json::from_value(usage_test_request()).expect("static test request");
    state
        .provider
        .handle_messages(
            request,
            crate::RequestContext {
                req_id: uuid::Uuid::new_v4().to_string(),
                session_id: None,
                session_seq: None,
                provider: "siwc".into(),
                traffic: None,
                monitor: None,
            },
        )
        .await
}

fn usage_test_request() -> Value {
    json!({"model":"siwc","stream":false,"max_tokens":32,"messages":[{"role":"user","content":"Reply exactly: BRIDGE OK"}]})
}

pub(crate) fn usage_test_router(
    provider: Arc<crate::providers::chatgpt_siwc::SiwcProvider>,
) -> Router {
    Router::new()
        .route("/bridge/test", post(check_usage))
        .with_state(UsageCheck {
            provider,
            gate: Default::default(),
        })
}

async fn protect(State(state): State<DaemonState>, request: Request<Body>, next: Next) -> Response {
    if request.headers().contains_key("origin") {
        return StatusCode::FORBIDDEN.into_response();
    }
    if request.uri().path() == "/bridge/identity" {
        return next.run(request).await;
    }
    let cookie = request
        .headers()
        .get("x-api-key")
        .and_then(|h| h.to_str().ok())
        .or_else(|| {
            request
                .headers()
                .get("authorization")
                .and_then(|h| h.to_str().ok())
                .and_then(|s| s.strip_prefix("Bearer "))
        });
    if cookie != Some(state.key.as_str()) {
        return (StatusCode::UNAUTHORIZED, Json(json!({"error":{"type":"authentication_error","message":"Open Claude Bridge to repair the local connection","code":"bridge_local_auth_required"}}))).into_response();
    }
    state.active.fetch_add(1, Ordering::SeqCst);
    let guard = InFlight(state.active.clone());
    *state.touched.lock().unwrap() = Instant::now();
    let response = next.run(request).await;
    let (parts, body) = response.into_parts();
    let stream = body.into_data_stream().map(move |chunk| {
        let _keep = &guard;
        *state.touched.lock().unwrap() = Instant::now();
        chunk
    });
    Response::from_parts(parts, Body::from_stream(stream))
}

pub async fn run_daemon(paths: &Paths, settings: &Settings) -> Result<()> {
    let lock = lock_file(paths, "daemon.lock")?;
    lock.try_lock().context("后台服务已经运行")?;
    let model = settings.model.clone().context("尚未选择模型")?;
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
    let connection = Connection {
        base_url: format!("http://127.0.0.1:{}", listener.local_addr()?.port()),
        installation_id: settings.installation_id.clone(),
        model: model.clone(),
        pid: std::process::id(),
        version: VERSION.into(),
    };
    let state = DaemonState {
        connection: connection.clone(),
        key: secret(paths)?,
        active: Arc::new(AtomicUsize::new(0)),
        touched: Arc::new(Mutex::new(Instant::now())),
    };
    let provider = Arc::new(crate::providers::chatgpt_siwc::SiwcProvider::new(
        "default".into(),
        Some(model),
    )?);
    let registry = crate::Registry::from_providers(
        crate::config::AliasProvider::Siwc,
        [provider.clone() as Arc<dyn crate::Provider>],
    );
    let usage_check = usage_test_router(provider);
    let router = Router::new()
        .route("/bridge/identity", get(identity))
        .with_state(state.clone())
        .merge(usage_check)
        .merge(crate::server::app(Arc::new(registry)))
        .layer(from_fn_with_state(state.clone(), protect));
    atomic_write(
        &paths.runtime().join("service.json"),
        &serde_json::to_vec(&connection)?,
        0o600,
    )?;
    let shutdown_paths = paths.clone();
    let shutdown = async move {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("signal handler");
        loop {
            tokio::select! {
                _ = term.recv() => break,
                _ = tokio::time::sleep(Duration::from_secs(30)) => {
                    if state.active.load(Ordering::SeqCst) == 0 && state.touched.lock().unwrap().elapsed() > Duration::from_secs(300) && active_leases(&shutdown_paths).unwrap_or(1) == 0 { break; }
                }
            }
        }
    };
    let result = axum::serve(
        listener,
        router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown)
    .await;
    let _ = fs::remove_file(paths.runtime().join("service.json"));
    result.context("本地服务结束")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower::ServiceExt;

    #[tokio::test]
    async fn usage_recheck_requires_local_cookie_and_rejects_browser_origins() {
        let state = DaemonState {
            connection: Connection {
                base_url: "http://127.0.0.1:12345".into(),
                installation_id: "fixture-installation".into(),
                model: "fixture-model".into(),
                pid: 1,
                version: VERSION.into(),
            },
            key: "fixture-local-cookie".into(),
            active: Default::default(),
            touched: Arc::new(Mutex::new(Instant::now())),
        };
        let app = Router::new()
            .route("/bridge/test", post(|| async { StatusCode::OK }))
            .layer(from_fn_with_state(state.clone(), protect));
        for (key, origin, expected) in [
            (None, None, StatusCode::UNAUTHORIZED),
            (Some("wrong-cookie"), None, StatusCode::UNAUTHORIZED),
            (
                Some("fixture-local-cookie"),
                Some("http://localhost"),
                StatusCode::FORBIDDEN,
            ),
            (Some("fixture-local-cookie"), None, StatusCode::OK),
        ] {
            let mut request = Request::builder().method("POST").uri("/bridge/test");
            if let Some(key) = key {
                request = request.header("x-api-key", key);
            }
            if let Some(origin) = origin {
                request = request.header("origin", origin);
            }
            let response = app
                .clone()
                .oneshot(request.body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), expected);
            axum::body::to_bytes(response.into_body(), 1024)
                .await
                .unwrap();
        }
        assert_eq!(state.active.load(Ordering::SeqCst), 0);
    }
}
