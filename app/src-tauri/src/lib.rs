//! 朱比特和它的朋友们 · 核心逻辑
//! 管理内置 Python 环境（初始化 / pip / notebook 服务 / 一键重置）

use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::{Component, Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

const PYTHON_VERSION: &str = "3.9.7";

/* ================= 在线更新（全部走 GitHub，无自建服务） ================= */

/// GitHub 最新正式版（该端点天然排除 alpha/beta 预发布）
const GITHUB_LATEST_RELEASE: &str =
    "https://api.github.com/repos/aifrens/jupyter-bento/releases/latest";
/// 热修复清单（仓库内维护；可用 JUPITER_HOTFIX_URL 覆盖以便本地调试）
const HOTFIX_MANIFEST_URL: &str =
    "https://raw.githubusercontent.com/aifrens/jupyter-bento/main/hotfix/manifest.json";

#[derive(Clone, Serialize)]
struct UpdateInfo {
    latest_version: Option<String>,
    release_notes: Option<String>,
    release_url: Option<String>,
    patches: Vec<PatchInfo>,
}

#[derive(Clone, Serialize, serde::Deserialize)]
struct PatchInfo {
    id: String,
    title: String,
    description: String,
}

#[derive(serde::Deserialize)]
struct HotfixManifest {
    patches: Vec<ManifestPatch>,
}

#[derive(Clone, serde::Deserialize)]
struct ManifestPatch {
    id: String,
    title: String,
    description: String,
    applies_to: Vec<String>,
    files: Vec<ManifestFile>,
}

#[derive(Clone, serde::Deserialize)]
struct ManifestFile {
    /// env 内 site-packages 下的文件名（目标路径按平台 site-packages 目录解析）
    name: String,
    url: String,
    sha256: String,
}

#[derive(Clone, Serialize, serde::Deserialize)]
struct AppliedPatch {
    sha256: String,
    applied_at: String,
}

/// 通过系统 curl 发起 HTTPS 请求（macOS / Win10 1803+ 均自带，零新增依赖）
fn http_fetch(url: &str) -> Result<String, String> {
    let out = quiet_command(Path::new("curl"))
        .args([
            "-fsSL",
            "--max-time",
            "20",
            "-H",
            "User-Agent: jupiter-updater",
            url,
        ])
        .output()
        .map_err(|e| format!("curl 执行失败：{e}"))?;
    if !out.status.success() {
        return Err(format!("请求失败（HTTP 状态 {:?}）", out.status.code()));
    }
    String::from_utf8(out.stdout).map_err(|e| format!("响应非 UTF-8：{e}"))
}

fn semver_tuple(v: &str) -> (u32, u32, u32) {
    let base = v.trim_start_matches('v').split('-').next().unwrap_or("0");
    let mut it = base.split('.');
    (
        it.next().and_then(|s| s.parse().ok()).unwrap_or(0),
        it.next().and_then(|s| s.parse().ok()).unwrap_or(0),
        it.next().and_then(|s| s.parse().ok()).unwrap_or(0),
    )
}

fn is_prerelease(v: &str) -> bool {
    v.contains('-')
}

/// 判断远端正式版是否比当前版本更新：
/// 基线版本更高，或基线相同但当前是预发布（如 1.0.1-alpha.1 < 1.0.1 正式版）
fn is_newer_release(latest: &str, current: &str) -> bool {
    let lt = semver_tuple(latest);
    let ct = semver_tuple(current);
    lt > ct || (lt == ct && is_prerelease(current) && !is_prerelease(latest))
}

fn applied_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("applied-patches.json"))
}

fn load_applied(app: &AppHandle) -> std::collections::HashMap<String, AppliedPatch> {
    applied_path(app)
        .ok()
        .and_then(|p| fs::read(p).ok())
        .and_then(|d| serde_json::from_slice(&d).ok())
        .unwrap_or_default()
}

fn save_applied(app: &AppHandle, map: &std::collections::HashMap<String, AppliedPatch>) {
    if let Ok(p) = applied_path(app) {
        if let Ok(json) = serde_json::to_vec_pretty(map) {
            let _ = fs::write(p, json);
        }
    }
}

/// 按平台解析 env 内 site-packages 目录
fn site_packages_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let env = env_dir(app)?;
    let unix = env.join("lib").join("python3.9").join("site-packages");
    if unix.exists() {
        return Ok(unix);
    }
    let win = env.join("Lib").join("site-packages");
    if win.exists() {
        return Ok(win);
    }
    Err("未找到环境的 site-packages 目录".into())
}

fn sha256_file(p: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let data = fs::read(p).map_err(|e| e.to_string())?;
    Ok(format!("{:x}", Sha256::digest(&data)))
}

/// 补丁文件在磁盘上是否完整（用户重置/手改后需要重放）
fn patch_file_intact(app: &AppHandle, patch: &ManifestPatch) -> bool {
    let sp = match site_packages_dir(app) {
        Ok(d) => d,
        Err(_) => return false,
    };
    patch.files.iter().all(|f| {
        let p = sp.join(&f.name);
        p.exists() && sha256_file(&p).map(|h| h == f.sha256).unwrap_or(false)
    })
}

fn hotfix_url() -> String {
    std::env::var("JUPITER_HOTFIX_URL").unwrap_or_else(|_| HOTFIX_MANIFEST_URL.to_string())
}

#[tauri::command]
async fn check_updates(app: AppHandle) -> Result<UpdateInfo, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut info = UpdateInfo {
            latest_version: None,
            release_notes: None,
            release_url: None,
            patches: vec![],
        };
        let current = env!("CARGO_PKG_VERSION");

        // 1) 最新正式版（GitHub latest 端点天然排除 alpha/beta 预发布）
        if let Ok(body) = http_fetch(GITHUB_LATEST_RELEASE) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&body) {
                let tag = v.get("tag_name").and_then(|s| s.as_str()).unwrap_or("");
                if is_newer_release(tag, current) {
                    info.latest_version = Some(tag.trim_start_matches('v').to_string());
                    info.release_notes = v
                        .get("body")
                        .and_then(|s| s.as_str())
                        .map(|s| s.to_string());
                    info.release_url = v
                        .get("html_url")
                        .and_then(|s| s.as_str())
                        .map(|s| s.to_string());
                }
            }
        }

        // 2) 热修复清单（仓库内维护）
        if let Ok(body) = http_fetch(&hotfix_url()) {
            if let Ok(m) = serde_json::from_str::<HotfixManifest>(&body) {
                let applied = load_applied(&app);
                for p in m.patches {
                    if !p.applies_to.iter().any(|v| v == current) {
                        continue;
                    }
                    let already = p
                        .files
                        .first()
                        .map(|f| {
                            applied.get(&p.id).map(|a| a.sha256.as_str()) == Some(f.sha256.as_str())
                        })
                        .unwrap_or(false);
                    if already && patch_file_intact(&app, &p) {
                        continue;
                    }
                    if patch_file_intact(&app, &p) {
                        continue; // 磁盘已完好（含快照自带补丁）
                    }
                    info.patches.push(PatchInfo {
                        id: p.id,
                        title: p.title,
                        description: p.description,
                    });
                }
            }
        }
        Ok(info)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn apply_patch(app: AppHandle, id: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let body = http_fetch(&hotfix_url())?;
        let m: HotfixManifest =
            serde_json::from_str(&body).map_err(|e| format!("清单解析失败：{e}"))?;
        let patch = m
            .patches
            .into_iter()
            .find(|p| p.id == id)
            .ok_or_else(|| "清单中找不到该补丁".to_string())?;
        let sp = site_packages_dir(&app)?;

        for f in &patch.files {
            let content = http_fetch(&f.url)?;
            let tmp = sp.join(format!(".{}.tmp", f.name));
            let dest = sp.join(&f.name);
            fs::write(&tmp, content.as_bytes()).map_err(|e| format!("写入失败：{e}"))?;
            let hash = sha256_file(&tmp)?;
            if hash != f.sha256 {
                let _ = fs::remove_file(&tmp);
                return Err(format!("{} 校验不符（可能下载损坏），未应用", f.name));
            }
            fs::rename(&tmp, &dest).map_err(|e| format!("就位失败：{e}"))?;
        }

        let mut applied = load_applied(&app);
        if let Some(f) = patch.files.first() {
            applied.insert(
                patch.id.clone(),
                AppliedPatch {
                    sha256: f.sha256.clone(),
                    applied_at: format!("{:?}", std::time::SystemTime::now()),
                },
            );
            save_applied(&app, &applied);
        }
        Ok(patch.title)
    })
    .await
    .map_err(|e| e.to_string())?
}

const SNAPSHOT_NAME: &str = "env-factory.tar.zst";
const RECENT_STORE_LIMIT: usize = 20;
const RECENT_LIST_LIMIT: usize = 5;
const RECENT_CALLBACK_MAX_BODY: usize = 16 * 1024;

/// 出厂内置包清单（macOS Apple Silicon 上 4 个包为 arm64 可用的小版本升级）
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
const BUILTIN: &[(&str, &str)] = &[
    ("notebook", "6.4.8"),
    ("pandas", "1.4.0"),
    ("numpy", "1.22.4"),
    ("scipy", "1.7.3"),
    ("matplotlib", "3.5.0"),
    ("seaborn", "0.11.2"),
    ("openpyxl", "3.0.9"),
    ("xlrd", "2.0.1"),
    ("Pillow", "8.4.0"),
    ("opencv-python", "4.10.0.84"),
    ("scikit-learn", "1.0.2"),
    ("xgboost", "2.1.1"),
    ("imbalanced-learn", "0.8.1"),
    ("onnxruntime", "1.12.1"),
    ("traitlets", "5.1.1"),
    ("matplotlib-inline", "0.1.3"),
];

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
const BUILTIN: &[(&str, &str)] = &[
    ("notebook", "6.4.8"),
    ("pandas", "1.3.4"),
    ("numpy", "1.22.4"),
    ("scipy", "1.7.1"),
    ("matplotlib", "3.4.3"),
    ("seaborn", "0.11.2"),
    ("openpyxl", "3.0.9"),
    ("xlrd", "2.0.1"),
    ("Pillow", "8.4.0"),
    ("opencv-python", "4.10.0.84"),
    ("scikit-learn", "0.24.2"),
    ("xgboost", "2.1.1"),
    ("imbalanced-learn", "0.8.1"),
    ("onnxruntime", "1.12.1"),
    ("traitlets", "5.1.1"),
    ("matplotlib-inline", "0.1.3"),
];

pub struct AppState {
    notebook: Mutex<Option<Child>>,
    notebook_info: Mutex<Option<NotebookInfo>>,
    /// 串行化启动、停止和重置，避免并发命令创建多个失去管理的 Notebook 进程。
    notebook_lifecycle: tauri::async_runtime::Mutex<()>,
    notebook_generation: std::sync::atomic::AtomicU64,
    recent_store: Mutex<()>,
    recent_callback: Mutex<Option<RecentCallbackServer>>,
}

#[derive(Clone, Serialize)]
pub struct NotebookInfo {
    port: u16,
    url: String,
    token: String,
    workdir: String,
}

#[derive(Clone, Serialize)]
pub struct RecentFile {
    id: String,
    workdir: String,
    name: String,
    modified_ms: u64,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct RecentEntry {
    id: String,
    workdir: String,
    path: String,
    opened_at: u64,
}

struct RecentCallbackServer {
    generation: u64,
    token: String,
    port: u16,
    stop: Arc<std::sync::atomic::AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

#[derive(serde::Deserialize)]
struct RecentCallbackPayload {
    path: String,
}

#[derive(Clone, Serialize)]
struct Progress {
    percent: u32,
    step: String,
}

#[derive(Clone, Serialize)]
struct PipLog {
    line: String,
    kind: String, // "" | "dim" | "ok" | "err"
}

/// 包来源三态：
/// builtin = 出厂内置（不可卸载）；explicit = 用户显式安装；
/// dependency = 作为依赖被连带安装（受卸载保护）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum PkgSource {
    Builtin,
    Explicit,
    Dependency,
}

impl PkgSource {
    /// 列表排序权重：直接安装 → 依赖 → 内置
    fn rank(self) -> u8 {
        match self {
            PkgSource::Explicit => 0,
            PkgSource::Dependency => 1,
            PkgSource::Builtin => 2,
        }
    }
}

#[derive(Serialize)]
pub struct Pkg {
    name: String,
    version: String,
    source: PkgSource,
    /// 当前环境中依赖此包的其他包（显示名），用于「被谁需要」展示与卸载保护
    required_by: Vec<String>,
}

#[derive(Serialize)]
pub struct EnvStatus {
    ready: bool,
    python_version: String,
}

/* ================= 路径工具 ================= */

fn env_dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("env"))
}

fn python_bin(env: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        env.join("python.exe")
    }
    #[cfg(not(windows))]
    {
        env.join("bin").join("python3")
    }
}

fn snapshot_path(app: &AppHandle) -> Result<PathBuf, String> {
    let base = app.path().resource_dir().map_err(|e| e.to_string())?;
    let direct = base.join(SNAPSHOT_NAME);
    if direct.exists() {
        return Ok(direct);
    }
    let nested = base.join("resources").join(SNAPSHOT_NAME);
    if nested.exists() {
        return Ok(nested);
    }
    Err(format!("未找到出厂环境快照：{}", direct.display()))
}

/// 展开路径开头的 ~/ 为用户主目录（Rust 不会自动展开 tilde）
fn expand_tilde(p: &str) -> PathBuf {
    if let Some(rest) = p.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")) {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(p)
}

/// 为 Jupyter 相关进程设置私有 数据/运行时/配置 目录，与用户系统 Jupyter 完全隔离
fn jupyter_env(app: &AppHandle, cmd: &mut Command) {
    if let Ok(base) = app.path().app_data_dir() {
        let data = base.join("jupyter-data");
        let rt = base.join("jupyter-runtime");
        let cfg = base.join("jupyter-config");
        fs::create_dir_all(&data).ok();
        fs::create_dir_all(&rt).ok();
        fs::create_dir_all(&cfg).ok();
        ensure_jupyter_config(&cfg);
        cmd.env("JUPYTER_DATA_DIR", &data)
            .env("JUPYTER_RUNTIME_DIR", &rt)
            .env("JUPYTER_CONFIG_DIR", &cfg);
    }
}

/// 写入应用托管的 Jupyter 配置；内容升级时覆盖旧版本，避免老用户停留在失效配置。
fn ensure_jupyter_config(cfg_dir: &Path) {
    let f = cfg_dir.join("jupyter_notebook_config.py");
    if fs::read_to_string(&f).ok().as_deref() != Some(TRUSTED_CONFIG) {
        let _ = fs::write(f, TRUSTED_CONFIG);
    }
}

/// 最近打开 handler 作为只读应用资源随包分发，不写入或污染内置 Python 环境。
fn recent_handler_parent(app: &AppHandle) -> Result<PathBuf, String> {
    let base = app.path().resource_dir().map_err(|e| e.to_string())?;
    for parent in [&base, &base.join("resources")] {
        if parent.join("jupiter_recent_handler").is_dir() {
            return Ok(parent.to_path_buf());
        }
    }
    Err("未找到最近打开事件处理模块 jupiter_recent_handler".into())
}

/// 受信任工作区配置：读取工作区 notebook 时自动执行官方签名并标记可信。
/// 服务仅监听 127.0.0.1、带 token、只服务用户指定工作目录，安全边界清晰。
const TRUSTED_CONFIG: &str = r#"# 朱比特和它的朋友们 · 自动生成，请勿手动修改
# 受信任工作区：读取 notebook 时自动执行官方签名（NotebookNotary）并标记可信，
# 等价于手动点击 "Trust Notebook"，但首次打开即可信、无需刷新页面。
from notebook.services.contents.filemanager import FileContentsManager


class TrustedFileContentsManager(FileContentsManager):
    def mark_trusted_cells(self, nb, path=""):
        try:
            if not self.notary.check_signature(nb):
                self.notary.sign(nb)
        except Exception as e:
            self.log.warning("auto-sign failed for %s: %s", path, e)
        self.notary.mark_cells(nb, True)


c.NotebookApp.contents_manager_class = TrustedFileContentsManager  # noqa: F821
c.NotebookApp.extra_services = ["jupiter_recent_handler.handlers"]  # noqa: F821
"#;

/* ---- 最近打开记录（由 Notebook 页面 handler 回调，Rust 为唯一持久化写入者） ---- */

fn recent_store_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("recent-files.json"))
}

fn load_recent_store_unlocked(app: &AppHandle) -> Vec<RecentEntry> {
    let Some(data) = recent_store_path(app)
        .ok()
        .and_then(|path| fs::read(path).ok())
    else {
        return Vec::new();
    };
    serde_json::from_slice(&data).unwrap_or_else(|error| {
        eprintln!("[UI-DIAG] recent store 读取失败，将忽略损坏内容：{error}");
        Vec::new()
    })
}

fn write_recent_store_unlocked(app: &AppHandle, entries: &[RecentEntry]) -> Result<(), String> {
    let path = recent_store_path(app)?;
    let parent = path.parent().ok_or("最近打开记录目录无效")?;
    fs::create_dir_all(parent).map_err(|e| format!("无法创建最近打开记录目录：{e}"))?;
    let temp = parent.join("recent-files.json.tmp");
    let json = serde_json::to_vec_pretty(entries).map_err(|e| e.to_string())?;
    {
        let mut file = fs::File::create(&temp).map_err(|e| format!("无法写入最近打开记录：{e}"))?;
        file.write_all(&json)
            .map_err(|e| format!("无法写入最近打开记录：{e}"))?;
        file.sync_all()
            .map_err(|e| format!("无法同步最近打开记录：{e}"))?;
    }
    replace_file_atomically(&temp, &path)
}

fn replace_file_atomically(temp: &Path, path: &Path) -> Result<(), String> {
    // Unix 的同目录 rename 为原子替换。Windows 标准库无法覆盖既有文件，
    // 只能先删除再 rename；全进程仍由 recent_store mutex 串行，但崩溃恢复不具原子性。
    #[cfg(windows)]
    if path.exists() {
        fs::remove_file(path).map_err(|e| format!("无法替换最近打开记录：{e}"))?;
    }
    fs::rename(temp, path).map_err(|e| format!("无法替换最近打开记录：{e}"))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn normalize_workdir(path: &Path) -> Result<PathBuf, String> {
    path.canonicalize()
        .map_err(|e| format!("无法解析工作目录 {}：{e}", path.display()))
}

/// 将内部规范路径转换为适合界面和设置持久化的工作目录字符串。
/// Windows 仅在语义等价时移除 `\\?\`；依赖 verbatim 语义的路径保持原样。
fn workdir_for_display(path: &Path) -> String {
    dunce::simplified(path).display().to_string()
}

/// 只接受工作目录内的相对 .ipynb 路径；拒绝根路径、父目录、Windows 前缀和符号链接逃逸。
fn validate_notebook_path(workdir: &Path, relative: &str) -> Result<(String, PathBuf), String> {
    let normalized = relative.replace('\\', "/");
    let relative_path = Path::new(&normalized);
    if normalized.trim().is_empty()
        || relative_path.is_absolute()
        || relative_path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
        || relative_path.extension().and_then(|ext| ext.to_str()) != Some("ipynb")
    {
        return Err("RECENT_INVALID_PATH: 最近打开记录不是有效的相对 .ipynb 路径".into());
    }

    let root = normalize_workdir(workdir)?;
    let target = root.join(relative_path);
    let canonical = target
        .canonicalize()
        .map_err(|_| "RECENT_FILE_NOT_FOUND: Notebook 文件已不存在".to_string())?;
    if !canonical.starts_with(&root) || !canonical.is_file() {
        return Err("RECENT_INVALID_PATH: Notebook 文件不在当前工作目录内".into());
    }
    Ok((normalized, canonical))
}

fn record_recent_notebook(
    app: &AppHandle,
    workdir: &Path,
    relative: &str,
) -> Result<RecentEntry, String> {
    let (path, _) = validate_notebook_path(workdir, relative)?;
    let workdir = normalize_workdir(workdir)?.display().to_string();
    let opened_at = now_ms();
    let state = app.state::<AppState>();
    let _store_guard = state.recent_store.lock().unwrap();
    let mut entries = load_recent_store_unlocked(app);
    let id = match entries
        .iter_mut()
        .find(|entry| entry.workdir == workdir && entry.path == path)
    {
        Some(entry) => {
            entry.opened_at = opened_at;
            entry.id.clone()
        }
        None => {
            let id = gen_token();
            entries.push(RecentEntry {
                id: id.clone(),
                workdir: workdir.clone(),
                path: path.clone(),
                opened_at,
            });
            id
        }
    };
    entries.sort_by(|a, b| b.opened_at.cmp(&a.opened_at));
    entries.truncate(RECENT_STORE_LIMIT);
    write_recent_store_unlocked(app, &entries)?;
    Ok(RecentEntry {
        id,
        workdir,
        path,
        opened_at,
    })
}

fn http_response(stream: &mut TcpStream, status: &str, body: &str) {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
}

fn handle_recent_callback(app: &AppHandle, stream: &mut TcpStream, token: &str, workdir: &Path) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let mut reader = BufReader::new(&mut *stream);
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err()
        || !request_line.starts_with("POST /recent-open ")
    {
        http_response(stream, "404 Not Found", "not found");
        return;
    }

    let mut content_length = None;
    let mut authorized = false;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).is_err() || line.is_empty() {
            http_response(stream, "400 Bad Request", "bad request");
            return;
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            let name = name.trim();
            let value = value.trim();
            if name.eq_ignore_ascii_case("Authorization") {
                authorized = value == format!("Bearer {token}");
            } else if name.eq_ignore_ascii_case("X-Jupiter-Recent-Token") {
                authorized = value == token;
            } else if name.eq_ignore_ascii_case("Content-Length") {
                content_length = value.parse::<usize>().ok();
            }
        }
    }
    if !authorized {
        http_response(stream, "401 Unauthorized", "unauthorized");
        return;
    }
    let Some(content_length) = content_length.filter(|length| *length <= RECENT_CALLBACK_MAX_BODY)
    else {
        http_response(stream, "413 Payload Too Large", "invalid content length");
        return;
    };
    let mut body = vec![0; content_length];
    if reader.read_exact(&mut body).is_err() {
        http_response(stream, "400 Bad Request", "incomplete body");
        return;
    }
    let payload: RecentCallbackPayload = match serde_json::from_slice(&body) {
        Ok(payload) => payload,
        Err(_) => {
            http_response(stream, "400 Bad Request", "invalid json");
            return;
        }
    };
    match record_recent_notebook(app, workdir, &payload.path) {
        Ok(entry) => {
            let _ = app.emit(
                "recent-changed",
                RecentFile {
                    id: entry.id,
                    workdir: entry.workdir,
                    name: entry.path,
                    modified_ms: entry.opened_at,
                },
            );
            http_response(stream, "204 No Content", "");
        }
        Err(error) => {
            eprintln!("[UI-DIAG] recent callback 拒绝：{error}");
            http_response(stream, "400 Bad Request", "invalid notebook path");
        }
    }
}

fn start_recent_callback(
    app: &AppHandle,
    workdir: PathBuf,
    generation: u64,
) -> Result<RecentCallbackServer, String> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let token = gen_token();
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop_thread = Arc::clone(&stop);
    let token_thread = token.clone();
    let app = app.clone();
    let handle = thread::spawn(move || {
        use std::sync::atomic::Ordering;
        while !stop_thread.load(Ordering::Acquire) {
            match listener.accept() {
                Ok((mut stream, peer)) if peer.ip().is_loopback() => {
                    handle_recent_callback(&app, &mut stream, &token_thread, &workdir);
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(50));
                }
                Err(error) => {
                    eprintln!("[UI-DIAG] recent callback listener 退出：{error}");
                    break;
                }
            }
        }
    });
    Ok(RecentCallbackServer {
        generation,
        token,
        port,
        stop,
        thread: Some(handle),
    })
}

fn stop_recent_callback(state: &AppState, generation: Option<u64>) {
    use std::sync::atomic::Ordering;
    let mut slot = state.recent_callback.lock().unwrap();
    if generation.is_some_and(|expected| {
        slot.as_ref()
            .map(|server| server.generation != expected)
            .unwrap_or(true)
    }) {
        return;
    }
    let mut server = slot.take();
    drop(slot);
    if let Some(server) = server.as_mut() {
        server.stop.store(true, Ordering::Release);
        let _ = TcpStream::connect(("127.0.0.1", server.port));
        if let Some(handle) = server.thread.take() {
            let _ = handle.join();
        }
    }
}

fn quiet_command(program: &Path) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(program);
    // 双向隔离：不使用用户站点目录（避免读入本机 ~/Library/Python/3.9 等位置的包），
    // 不读取用户的 pip 配置（避免被本机 pip.conf 的 index-url/代理等干扰），
    // 清除 PYTHONPATH（防止外部环境污染内置环境的导入路径）。
    cmd.env("PYTHONNOUSERSITE", "1")
        .env(
            "PIP_CONFIG_FILE",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        )
        .env_remove("PYTHONPATH");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// PEP 503 规范化包名：小写并把 - _ . 连续分隔符折叠为单个 -。
/// 这是「内置 / 直接安装 / 依赖」三类判定的唯一比较形式——
/// 包名形式漂移（Pillow vs pillow、opencv_python vs opencv-python）
/// 不得影响分类，否则会出现把出厂包误判为用户包的回归。
fn normalize_pkg_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut prev_sep = false;
    for c in name.trim().chars() {
        if matches!(c, '-' | '_' | '.') {
            if !prev_sep {
                out.push('-');
            }
            prev_sep = true;
        } else {
            out.push(c.to_ascii_lowercase());
            prev_sep = false;
        }
    }
    out
}

/// 出厂包清单（构建快照时生成于 env/factory-manifest.json），
/// 是「内置 vs 用户安装」的唯一权威判定依据；读取失败时回退到核心锁定清单。
/// 注意：Windows 快照由 build-snapshot.ps1 生成，必须与 build-snapshot.sh
/// 一样输出该清单，否则此处静默回退、出厂包被误判为用户安装（历史事故）。
fn load_builtin_names(env: &Path) -> HashSet<String> {
    let manifest = env.join("factory-manifest.json");
    if let Ok(data) = fs::read(&manifest) {
        // 容错 UTF-8 BOM：Windows 工具链（PowerShell 重定向等）可能写入 BOM，
        // serde_json 不接受 BOM，不能因此让整份清单失效
        let data: &[u8] = if data.starts_with(b"\xef\xbb\xbf") {
            &data[3..]
        } else {
            &data[..]
        };
        if let Ok(list) = serde_json::from_slice::<Vec<serde_json::Value>>(data) {
            let set: HashSet<String> = list
                .iter()
                .filter_map(|v| v.get("name")?.as_str().map(normalize_pkg_name))
                .collect();
            if !set.is_empty() {
                return set;
            }
        }
    }
    eprintln!("[WARN] 出厂包清单缺失或损坏，回退到核心锁定清单；内置判定不完整，建议重置环境修复");
    BUILTIN.iter().map(|(n, _)| normalize_pkg_name(n)).collect()
}

/* ================= 用户显式安装记录（user-packages.json） ================= */

/// 记录文件名，存放于 env 根目录（与 factory-manifest.json 并列）。
/// 生命周期与环境目录一致：重置/重解压会整体删除 env，记录自然清空，
/// 与「重置环境时将被移除」的语义自动保持一致。
const USER_MANIFEST_NAME: &str = "user-packages.json";

fn load_user_manifest(env: &Path) -> HashSet<String> {
    fs::read(env.join(USER_MANIFEST_NAME))
        .ok()
        .and_then(|d| serde_json::from_slice::<Vec<String>>(&d).ok())
        .map(|v| v.iter().map(|s| normalize_pkg_name(s)).collect())
        .unwrap_or_default()
}

/// 尽力持久化；写盘失败仅丢失「显式安装」标记，依赖图启发式仍可兜底分类
fn save_user_manifest(env: &Path, set: &HashSet<String>) {
    let mut names: Vec<&String> = set.iter().collect();
    names.sort();
    match serde_json::to_string_pretty(&names) {
        Ok(data) => {
            if let Err(e) = fs::write(env.join(USER_MANIFEST_NAME), data) {
                eprintln!("[WARN] 写入 {USER_MANIFEST_NAME} 失败：{e}");
            }
        }
        Err(e) => eprintln!("[WARN] 序列化 {USER_MANIFEST_NAME} 失败：{e}"),
    }
}

/// 从用户输入的安装规格中提取包名头部：
/// `requests[security]`、`requests>=2`、` Pillow ` → `requests`/`requests`/`Pillow`。
/// 与探针 NAME_RE 同一规则（[A-Za-z0-9._-] 前缀），否则 pip 能装上的包
/// 却与安装记录对不上，显式安装会被误判为依赖。
fn requirement_name_head(spec: &str) -> &str {
    let trimmed = spec.trim_start();
    let end = trimmed
        .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')))
        .unwrap_or(trimmed.len());
    &trimmed[..end]
}

fn record_user_package(env: &Path, name: &str) {
    let head = requirement_name_head(name);
    if head.is_empty() {
        return;
    }
    let mut set = load_user_manifest(env);
    set.insert(normalize_pkg_name(head));
    save_user_manifest(env, &set);
}

fn unrecord_user_package(env: &Path, name: &str) {
    let mut set = load_user_manifest(env);
    if set.remove(&normalize_pkg_name(name)) {
        save_user_manifest(env, &set);
    }
}

/* ================= 环境清单与依赖图 ================= */

/// 一次性读取包列表与依赖关系的 Python 探针。
/// 用 importlib.metadata 而非 pip list：同一次进程内顺带算出依赖图，
/// 且 pip list 本身也是基于 importlib.metadata 实现，数据等价。
/// extra 依赖（extras_require，如 tests/dev）不计入依赖图。
const INVENTORY_PY: &str = r#"
import json, re
from importlib.metadata import distributions

try:
    from packaging.markers import Marker
except Exception:
    Marker = None  # packaging 不在时退化为字符串启发式（保守多留边）

def norm(n):
    return re.sub(r"[-_.]+", "-", n.strip()).lower()

NAME_RE = re.compile(r"^\s*([A-Za-z0-9._-]+)")
EXTRA_RE = re.compile(r"extra\s*==")

pkgs, requires, seen = [], {}, set()
for dist in distributions():
    name = dist.metadata.get("Name")
    if not name:
        continue
    n = norm(name)
    if n in seen:
        continue
    seen.add(n)
    pkgs.append({"name": name, "version": dist.version or ""})
    deps = set()
    try:
        reqs = dist.requires
        # 3.9 为方法、3.12+ 为属性，两种形态都要兼容
        if callable(reqs):
            reqs = reqs()
        reqs = reqs or []
    except Exception:
        reqs = []
    for r in reqs:
        head, sep, marker = r.partition(";")
        if sep:
            if EXTRA_RE.search(marker):
                continue  # 可选 extra 依赖不参与依赖图
            if Marker is not None:
                try:
                    if not Marker(marker).evaluate():
                        continue  # 环境标记不适用当前平台（如 sys_platform=='win32'）
                except Exception:
                    pass  # 标记解析失败时保守保留该边
        m = NAME_RE.match(head)
        if m:
            deps.add(norm(m.group(1)))
    deps.discard(n)
    requires[n] = sorted(deps)
print(json.dumps({"packages": pkgs, "requires": requires}, ensure_ascii=False))
"#;

#[derive(serde::Deserialize)]
struct InventoryPkg {
    name: String,
    version: String,
}

#[derive(serde::Deserialize)]
struct EnvInventory {
    packages: Vec<InventoryPkg>,
    /// 正向依赖边：规范化包名 → 它依赖的规范化包名集合
    requires: HashMap<String, Vec<String>>,
}

fn query_env_inventory(py: &Path) -> Result<EnvInventory, String> {
    let out = quiet_command(py).args(["-c", INVENTORY_PY]).output();
    let out = out.map_err(|e| format!("无法读取包列表：{e}"))?;
    if !out.status.success() {
        return Err("读取环境清单失败，环境可能已损坏，建议使用重置功能".into());
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("环境清单解析失败：{e}"))
}

/// 计算 name（规范化名）的已安装反向依赖（显示名，排序），即「被谁需要」。
fn requirers_of(
    norm_name: &str,
    requires: &HashMap<String, Vec<String>>,
    display: &HashMap<String, String>,
) -> Vec<String> {
    let mut list: Vec<String> = requires
        .iter()
        .filter(|(k, deps)| k.as_str() != norm_name && deps.iter().any(|d| d == norm_name))
        .filter_map(|(k, _)| display.get(k).cloned())
        .collect();
    list.sort_by_key(|a| a.to_lowercase());
    list
}

/// 三态分类的纯判定逻辑（与 IO 分离，便于单测）：
/// 1. 出厂清单内 → 内置；
/// 2. 安装记录内，或没有任何已安装包依赖它 → 直接安装
///    （后者兜底 notebook 内 !pip install 的顶层包；卸载后的残留依赖也会落入此类，
///    属可接受的启发式误差——保持可见且可卸载，优于误藏误判）；
/// 3. 其余 → 依赖。
fn classify_packages(
    inv: EnvInventory,
    builtins: &HashSet<String>,
    recorded: &HashSet<String>,
) -> Vec<Pkg> {
    let display: HashMap<String, String> = inv
        .packages
        .iter()
        .map(|p| (normalize_pkg_name(&p.name), p.name.clone()))
        .collect();
    let mut out: Vec<Pkg> = inv
        .packages
        .into_iter()
        .map(|p| {
            let n = normalize_pkg_name(&p.name);
            let required_by = requirers_of(&n, &inv.requires, &display);
            let source = if builtins.contains(&n) {
                PkgSource::Builtin
            } else if recorded.contains(&n) || required_by.is_empty() {
                PkgSource::Explicit
            } else {
                PkgSource::Dependency
            };
            Pkg {
                name: p.name,
                version: p.version,
                source,
                required_by,
            }
        })
        .collect();
    out.sort_by(|a, b| {
        (a.source.rank(), a.name.to_lowercase()).cmp(&(b.source.rank(), b.name.to_lowercase()))
    });
    out
}

/* ================= 快照解压（初始化 & 重置共用） ================= */

fn extract_snapshot(app: &AppHandle, dest: &Path) -> Result<(), String> {
    let snapshot = snapshot_path(app)?;
    let tmp = dest.parent().ok_or("环境目录无效")?.join("env-extracting");
    if tmp.exists() {
        fs::remove_dir_all(&tmp).map_err(|e| e.to_string())?;
    }
    fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;

    let file = fs::File::open(&snapshot).map_err(|e| format!("无法打开快照：{e}"))?;
    let decoder = zstd::stream::read::Decoder::new(file).map_err(|e| e.to_string())?;
    let mut archive = tar::Archive::new(decoder);

    let mut count: u32 = 0;
    let entries = archive.entries().map_err(|e| e.to_string())?;
    for entry in entries {
        let mut entry = entry.map_err(|e| format!("快照损坏：{e}"))?;
        // 防御：跳过 macOS AppleDouble 元数据文件（._*），
        // 避免污染 site-packages 被 Python 误读（如 ._xxx.pth 导致启动崩溃）
        let is_appledouble = entry
            .path()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().starts_with("._")))
            .unwrap_or(false);
        if is_appledouble {
            continue;
        }
        entry
            .unpack_in(&tmp)
            .map_err(|e| format!("解压失败：{e}"))?;
        count += 1;
        if count % 500 == 0 {
            let _ = app.emit(
                "setup-progress",
                Progress {
                    // 出厂环境约 2 万个条目，按此估算进度
                    percent: (8 + count / 250).min(92),
                    step: "正在解压内置 Python 3.9.7 环境…".into(),
                },
            );
        }
    }

    let extracted = tmp.join("python");
    if !extracted.exists() {
        let _ = fs::remove_dir_all(&tmp);
        return Err("快照内容异常：缺少 python 目录".into());
    }
    if dest.exists() {
        fs::remove_dir_all(dest).map_err(|e| e.to_string())?;
    }
    fs::rename(&extracted, dest).map_err(|e| format!("环境就位失败：{e}"))?;
    let _ = fs::remove_dir_all(&tmp);
    Ok(())
}

/* ================= Tauri 命令 ================= */

#[tauri::command]
async fn ensure_env(app: AppHandle) -> Result<EnvStatus, String> {
    let env = env_dir(&app)?;
    if python_bin(&env).exists() {
        return Ok(EnvStatus {
            ready: true,
            python_version: PYTHON_VERSION.into(),
        });
    }
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _ = app2.emit(
            "setup-progress",
            Progress {
                percent: 5,
                step: "正在解压内置 Python 3.9.7 环境…".into(),
            },
        );
        extract_snapshot(&app2, &env)?;
        let _ = app2.emit(
            "setup-progress",
            Progress {
                percent: 96,
                step: "正在校验环境完整性…".into(),
            },
        );
        if !python_bin(&env).exists() {
            return Err("环境校验失败，请尝试重置或重新安装应用".to_string());
        }
        let _ = app2.emit(
            "setup-progress",
            Progress {
                percent: 100,
                step: "完成".into(),
            },
        );
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(EnvStatus {
        ready: true,
        python_version: PYTHON_VERSION.into(),
    })
}

#[tauri::command]
async fn list_packages(app: AppHandle) -> Result<Vec<Pkg>, String> {
    let env = env_dir(&app)?;
    let py = python_bin(&env);
    let builtins = load_builtin_names(&env);
    let recorded = load_user_manifest(&env);
    let inv = tauri::async_runtime::spawn_blocking(move || query_env_inventory(&py))
        .await
        .map_err(|e| e.to_string())??;
    Ok(classify_packages(inv, &builtins, &recorded))
}

#[tauri::command]
async fn install_package(
    app: AppHandle,
    name: String,
    version: Option<String>,
    index_url: String,
) -> Result<(), String> {
    let env = env_dir(&app)?;
    let py = python_bin(&env);
    if !py.exists() {
        return Err("环境不存在，请先完成初始化".into());
    }
    let spec = match version {
        Some(v) if !v.trim().is_empty() => format!("{}=={}", name.trim(), v.trim()),
        _ => name.trim().to_string(),
    };
    let result = tauri::async_runtime::spawn_blocking(move || {
        let mut child = quiet_command(&py)
            .args([
                "-m",
                "pip",
                "install",
                "--index-url",
                &index_url,
                "--disable-pip-version-check",
                &spec,
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("无法启动 pip：{e}"))?;

        let emit = |app: &AppHandle, line: String, kind: &str| {
            let _ = app.emit(
                "pip-log",
                PipLog {
                    line,
                    kind: kind.into(),
                },
            );
        };
        emit(&app, format!("Looking in indexes: {index_url}"), "dim");

        let mut readers = vec![];
        if let Some(stdout) = child.stdout.take() {
            let app = app.clone();
            readers.push(thread::spawn(move || {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    let kind = if line.starts_with("Successfully installed") {
                        "ok"
                    } else {
                        ""
                    };
                    let _ = app.emit(
                        "pip-log",
                        PipLog {
                            line,
                            kind: kind.into(),
                        },
                    );
                }
            }));
        }
        if let Some(stderr) = child.stderr.take() {
            let app = app.clone();
            readers.push(thread::spawn(move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    let kind = if line.contains("ERROR") { "err" } else { "dim" };
                    let _ = app.emit(
                        "pip-log",
                        PipLog {
                            line,
                            kind: kind.into(),
                        },
                    );
                }
            }));
        }
        let status = child.wait().map_err(|e| e.to_string())?;
        for r in readers {
            let _ = r.join();
        }
        if status.success() {
            Ok(())
        } else {
            Err(format!("pip 退出码 {}", status.code().unwrap_or(-1)))
        }
    })
    .await
    .map_err(|e| e.to_string())?;
    if result.is_ok() {
        // pip 不保存「谁请求了安装」，显式安装记录只能由应用自行维护
        record_user_package(&env, &name);
    }
    result
}

#[tauri::command]
async fn uninstall_package(app: AppHandle, name: String) -> Result<(), String> {
    let env = env_dir(&app)?;
    if load_builtin_names(&env).contains(&normalize_pkg_name(&name)) {
        return Err("内置包不可卸载（重置环境可恢复出厂状态）".into());
    }
    let py = python_bin(&env);
    // 卸载保护：被其他已安装包依赖的包不可直接卸载，
    // 否则用户卸掉 cycler 之类底层依赖会让 matplotlib 静默坏掉
    let norm = normalize_pkg_name(&name);
    let py2 = py.clone();
    let inv = tauri::async_runtime::spawn_blocking(move || query_env_inventory(&py2))
        .await
        .map_err(|e| e.to_string())??;
    let display: HashMap<String, String> = inv
        .packages
        .iter()
        .map(|p| (normalize_pkg_name(&p.name), p.name.clone()))
        .collect();
    let requirers = requirers_of(&norm, &inv.requires, &display);
    if !requirers.is_empty() {
        return Err(format!(
            "无法卸载 {name}：{list} 依赖它，卸载会导致这些包不可用。如需移除，请先卸载上述包。",
            list = requirers.join("、")
        ));
    }
    let pip_name = name.clone();
    let out = tauri::async_runtime::spawn_blocking(move || {
        quiet_command(&py)
            .args([
                "-m",
                "pip",
                "uninstall",
                "-y",
                "--disable-pip-version-check",
                &pip_name,
            ])
            .output()
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    if out.status.success() {
        unrecord_user_package(&env, &name);
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).into_owned())
    }
}

#[tauri::command]
async fn start_notebook(
    app: AppHandle,
    state: State<'_, AppState>,
    workdir: String,
    open_browser: Option<bool>,
) -> Result<NotebookInfo, String> {
    let _lifecycle_guard = state.notebook_lifecycle.lock().await;
    let should_open = open_browser.unwrap_or(true);
    if let Some(info) = state.notebook_info.lock().unwrap().clone() {
        let requested_workdir = expand_tilde(&workdir);
        let requested_workdir = normalize_workdir(&requested_workdir)?;
        if requested_workdir != PathBuf::from(&info.workdir) {
            return Err("服务正在另一个工作目录运行，请先停止服务".into());
        }
        if should_open {
            let _ = tauri_plugin_opener::open_url(&info.url, None::<&str>);
        }
        return Ok(info);
    }
    let env = env_dir(&app)?;
    let py = python_bin(&env);
    if !py.exists() {
        return Err("环境不存在，请先完成初始化".into());
    }
    let handler_parent = recent_handler_parent(&app)?;
    let dir = expand_tilde(&workdir);
    fs::create_dir_all(&dir).map_err(|e| format!("无法创建工作目录 {}：{e}", dir.display()))?;
    let dir = normalize_workdir(&dir)?;
    // 在启动 callback 前完成所有可能失败的准备，避免错误返回时遗留 listener 线程。
    let notebook_port_listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let port = notebook_port_listener
        .local_addr()
        .map_err(|e| e.to_string())?
        .port();
    drop(notebook_port_listener);
    let token = gen_token();

    // notebook 的 stderr 写入日志文件，启动失败时可透出真实原因
    let log_path = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("notebook.log");
    let log_file = fs::File::create(&log_path).ok();
    let mut cmd = quiet_command(&py);
    jupyter_env(&app, &mut cmd);
    // 指向快照预建的 matplotlib 字体缓存（避免首次绘图扫描全系统字体）
    let mpl_cfg = env.join("mpl-config");
    fs::create_dir_all(&mpl_cfg).ok();
    cmd.env("MPLCONFIGDIR", &mpl_cfg);
    let generation = state
        .notebook_generation
        .fetch_add(1, std::sync::atomic::Ordering::AcqRel)
        + 1;
    let callback = start_recent_callback(&app, dir.clone(), generation)?;
    let callback_url = format!("http://127.0.0.1:{}/recent-open", callback.port);
    let callback_token = callback.token.clone();
    *state.recent_callback.lock().unwrap() = Some(callback);
    cmd.env("JUPITER_RECENT_CALLBACK_URL", &callback_url)
        .env("JUPITER_RECENT_CALLBACK_TOKEN", &callback_token)
        .env("PYTHONPATH", &handler_parent);
    cmd.args([
        "-m",
        "notebook",
        "--no-browser",
        "--ip=127.0.0.1",
        &format!("--port={port}"),
        &format!("--NotebookApp.token={token}"),
        &format!("--notebook-dir={}", dir.display()),
    ])
    .stdout(Stdio::null());
    match log_file {
        Some(f) => {
            cmd.stderr(Stdio::from(f));
        }
        None => {
            cmd.stderr(Stdio::null());
        }
    }
    let child = match cmd.spawn() {
        Ok(child) => child,
        Err(error) => {
            stop_recent_callback(&state, Some(generation));
            return Err(format!("无法启动 Notebook：{error}"));
        }
    };
    *state.notebook.lock().unwrap() = Some(child);

    // 等待服务端口就绪（最多 20 秒），同时侦测进程是否提前退出
    let app2 = app.clone();
    let ready = match tauri::async_runtime::spawn_blocking(move || {
        let st = app2.state::<AppState>();
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            if TcpStream::connect(("127.0.0.1", port)).is_ok() {
                return true;
            }
            {
                let mut g = st.notebook.lock().unwrap();
                let alive = match g.as_mut() {
                    Some(c) => matches!(c.try_wait(), Ok(None)),
                    None => false,
                };
                if !alive {
                    return false; // 进程已退出或被移除
                }
            }
            thread::sleep(Duration::from_millis(150));
        }
        false
    })
    .await
    {
        Ok(ready) => ready,
        Err(error) => {
            stop_notebook_inner(&state);
            return Err(error.to_string());
        }
    };

    if !ready {
        stop_notebook_inner(&state);
        let tail = fs::read_to_string(&log_path)
            .ok()
            .map(|s| {
                s.chars()
                    .rev()
                    .take(500)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect::<String>()
            })
            .filter(|s| !s.trim().is_empty());
        return Err(match tail {
            Some(t) => format!("Notebook 服务启动失败：{}", t.trim()),
            None => "Notebook 服务启动超时，请尝试重置环境".to_string(),
        });
    }

    let info = NotebookInfo {
        port,
        token: token.clone(),
        url: format!("http://127.0.0.1:{port}/tree?token={token}"),
        workdir: dir.display().to_string(),
    };
    *state.notebook_info.lock().unwrap() = Some(info.clone());
    // 后台监视进程退出，通知前端更新状态
    let app3 = app.clone();
    thread::spawn(move || {
        let st = app3.state::<AppState>();
        let instance_exited = loop {
            thread::sleep(Duration::from_millis(500));
            if st
                .notebook_generation
                .load(std::sync::atomic::Ordering::Acquire)
                != generation
            {
                break false;
            }
            let mut g = st.notebook.lock().unwrap();
            if st
                .notebook_generation
                .load(std::sync::atomic::Ordering::Acquire)
                != generation
            {
                break false;
            }
            match g.as_mut() {
                None => break false,
                Some(c) => match c.try_wait() {
                    Ok(None) => {}
                    Ok(Some(_)) => {
                        *g = None;
                        *st.notebook_info.lock().unwrap() = None;
                        drop(g);
                        stop_recent_callback(&st, Some(generation));
                        break true;
                    }
                    Err(_) => break false,
                },
            }
        };
        if instance_exited {
            let _ = app3.emit("notebook-exit", ());
        }
    });

    // 后台预热：静默导入全部重量级库，把动态库读入系统磁盘缓存，
    // 显著加速「首个内核启动 + 第一次 import」（对用户完全无感）
    {
        let app5 = app.clone();
        let env_w = env.clone();
        thread::spawn(move || {
            let py = python_bin(&env_w);
            let mut cmd = quiet_command(&py);
            jupyter_env(&app5, &mut cmd);
            let mpl = env_w.join("mpl-config");
            fs::create_dir_all(&mpl).ok();
            cmd.env("MPLCONFIGDIR", &mpl);
            let _ = cmd
                .arg("-c")
                .arg("import numpy,pandas,scipy,matplotlib.pyplot,seaborn,sklearn,xgboost,imblearn,cv2,onnxruntime,openpyxl,xlrd,PIL")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            eprintln!("[UI-DIAG] 后台预热完成（首次运行加速）");
        });
    }

    if should_open {
        let _ = tauri_plugin_opener::open_url(&info.url, None::<&str>);
    }
    Ok(info)
}

#[tauri::command]
async fn stop_notebook(state: State<'_, AppState>) -> Result<(), String> {
    let _lifecycle_guard = state.notebook_lifecycle.lock().await;
    stop_notebook_inner(&state);
    Ok(())
}

#[tauri::command]
fn notebook_status(state: State<'_, AppState>) -> Option<NotebookInfo> {
    state.notebook_info.lock().unwrap().clone()
}

#[tauri::command]
fn open_notebook_url(state: State<'_, AppState>) -> Result<(), String> {
    let info = state.notebook_info.lock().unwrap().clone();
    match info {
        Some(i) => tauri_plugin_opener::open_url(&i.url, None::<&str>).map_err(|e| e.to_string()),
        None => Err("服务未运行".into()),
    }
}

/// 在系统默认浏览器中打开外部链接（仅放行 http/https，防止滥用）
#[tauri::command]
fn open_external_url(url: String) -> Result<(), String> {
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return Err("仅支持 http(s) 链接".into());
    }
    tauri_plugin_opener::open_url(&url, None::<&str>).map_err(|e| e.to_string())
}

/// 按后端持久化记录打开 Notebook；不信任前端传入路径。
#[tauri::command]
fn open_recent_notebook(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    let info = state
        .notebook_info
        .lock()
        .unwrap()
        .clone()
        .ok_or("服务未运行")?;
    let entry = {
        let _store_guard = state.recent_store.lock().unwrap();
        load_recent_store_unlocked(&app)
            .into_iter()
            .find(|entry| entry.id == id)
            .ok_or("RECENT_NOT_FOUND: 最近打开记录不存在")?
    };
    let current_workdir = normalize_workdir(Path::new(&info.workdir))
        .map_err(|e| format!("RECENT_INVALID_PATH: {e}"))?;
    let entry_workdir = normalize_workdir(Path::new(&entry.workdir))
        .map_err(|e| format!("RECENT_INVALID_PATH: {e}"))?;
    if entry_workdir != current_workdir {
        return Err("RECENT_WORKDIR_MISMATCH: 最近打开记录不属于当前工作目录".into());
    }
    let (path, _) = validate_notebook_path(&current_workdir, &entry.path)?;
    let enc = path
        .split('/')
        .map(|segment| urlencoding::encode(segment).into_owned())
        .collect::<Vec<_>>()
        .join("/");
    let url = format!(
        "http://127.0.0.1:{}/notebooks/{}?token={}",
        info.port, enc, info.token
    );
    tauri_plugin_opener::open_url(&url, None::<&str>).map_err(|e| e.to_string())
}

/// 默认工作目录：<用户主目录>/Jupiter/notebooks（不存在则创建）
#[tauri::command]
fn default_workdir(app: AppHandle) -> Result<String, String> {
    let home = app.path().home_dir().map_err(|e| e.to_string())?;
    let dir = home.join("Jupiter").join("notebooks");
    fs::create_dir_all(&dir).map_err(|e| format!("无法创建工作目录：{e}"))?;
    Ok(workdir_for_display(&normalize_workdir(&dir)?))
}

/// 校验并返回有效工作目录：
/// 1. 保存的路径存在 → 直接使用
/// 2. 不存在但可创建 → 创建后使用
/// 3. 位于未挂载卷（如外接硬盘已拔出）或创建失败 → 回退默认目录
///    （不在 /Volumes 下创建"影子目录"，否则真实卷挂载时会被遮蔽）
#[tauri::command]
fn ensure_workdir(app: AppHandle, saved: Option<String>) -> Result<String, String> {
    let home = app.path().home_dir().map_err(|e| e.to_string())?;
    let default = home.join("Jupiter").join("notebooks");
    let candidate = saved
        .filter(|s| !s.trim().is_empty())
        .map(|s| expand_tilde(&s))
        .unwrap_or_else(|| default.clone());

    if candidate.is_dir() {
        return Ok(workdir_for_display(&normalize_workdir(&candidate)?));
    }
    // 检测 /Volumes/<卷名>/... 且卷根不存在 → 卷未挂载，直接回退
    let s = candidate.to_string_lossy();
    if let Some(rest) = s.strip_prefix("/Volumes/") {
        let vol_name = rest.split('/').next().unwrap_or("");
        if !vol_name.is_empty() && !Path::new("/Volumes").join(vol_name).exists() {
            fs::create_dir_all(&default).map_err(|e| format!("无法创建工作目录：{e}"))?;
            return Ok(workdir_for_display(&normalize_workdir(&default)?));
        }
    }
    match fs::create_dir_all(&candidate) {
        Ok(_) => Ok(workdir_for_display(&normalize_workdir(&candidate)?)),
        Err(_) => {
            fs::create_dir_all(&default).map_err(|e| format!("无法创建工作目录：{e}"))?;
            Ok(workdir_for_display(&normalize_workdir(&default)?))
        }
    }
}

/// 当前工作目录的最近打开列表，按 Notebook 页面成功打开时间倒序。
#[tauri::command]
fn list_recent_notebooks(app: AppHandle, workdir: String) -> Vec<RecentFile> {
    let state = app.state::<AppState>();
    let _store_guard = state.recent_store.lock().unwrap();
    let dir = expand_tilde(&workdir);
    let Ok(workdir) = normalize_workdir(&dir).map(|path| path.display().to_string()) else {
        return Vec::new();
    };
    let entries = load_recent_store_unlocked(&app);
    let retained: Vec<RecentEntry> = entries
        .iter()
        .filter(|entry| validate_notebook_path(Path::new(&entry.workdir), &entry.path).is_ok())
        .cloned()
        .collect();
    if retained.len() != entries.len() {
        if let Err(error) = write_recent_store_unlocked(&app, &retained) {
            eprintln!("[UI-DIAG] 无法压缩失效的最近打开记录：{error}");
        }
    }
    retained
        .into_iter()
        .filter(|entry| entry.workdir == workdir)
        .take(RECENT_LIST_LIMIT)
        .map(|entry| RecentFile {
            id: entry.id,
            workdir: entry.workdir,
            name: entry.path,
            modified_ms: entry.opened_at,
        })
        .collect()
}

/// 调试模式：仅开发构建（tauri dev，debug_assertions）或显式设置
/// JUPITER_SELFTEST 环境变量时开启；打包产物永远关闭。
#[tauri::command]
fn debug_mode() -> bool {
    cfg!(debug_assertions) || std::env::var("JUPITER_SELFTEST").is_ok()
}

/// 环境真实路径（设置页展示用）
#[tauri::command]
fn get_env_path(app: AppHandle) -> Result<String, String> {
    Ok(env_dir(&app)?.display().to_string())
}

/// 应用版本号（取自构建产物元数据，界面展示与打包永远一致）
#[tauri::command]
fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[tauri::command]
async fn reset_env(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let _lifecycle_guard = state.notebook_lifecycle.lock().await;
    let emit = |percent: u32, step: &str| {
        let _ = app.emit(
            "reset-progress",
            Progress {
                percent,
                step: step.into(),
            },
        );
    };
    emit(10, "正在停止 Notebook 服务…");
    stop_notebook_inner(&state);
    let env = env_dir(&app)?;
    emit(30, "正在清除当前环境…");
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        if env.exists() {
            fs::remove_dir_all(&env).map_err(|e| format!("清除失败：{e}"))?;
        }
        let _ = app2.emit(
            "reset-progress",
            Progress {
                percent: 45,
                step: "正在恢复出厂环境快照…".into(),
            },
        );
        extract_snapshot(&app2, &env)?;
        let _ = app2.emit(
            "reset-progress",
            Progress {
                percent: 95,
                step: "正在校验环境完整性…".into(),
            },
        );
        if !python_bin(&env).exists() {
            return Err("重置后校验失败，请重新安装应用".to_string());
        }
        let _ = app2.emit(
            "reset-progress",
            Progress {
                percent: 100,
                step: "完成".into(),
            },
        );
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn pick_directory(app: AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = std::sync::mpsc::channel();
    app.dialog()
        .file()
        .set_title("选择工作目录")
        .pick_folder(move |p| {
            let _ = tx.send(p.and_then(|fp| fp.as_path().map(workdir_for_display)));
        });
    rx.recv().map_err(|e| e.to_string())
}

/// 前端诊断上报：把 WebView 内的状态打印到 stderr，便于排查白屏等问题
#[tauri::command]
fn diag_report(msg: String) {
    eprintln!("[UI-DIAG] {msg}");
}

/* ================= 内部辅助 ================= */

fn stop_notebook_inner(state: &State<'_, AppState>) {
    state
        .notebook_generation
        .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
    let mut guard = state.notebook.lock().unwrap();
    if let Some(mut child) = guard.take() {
        terminate_child(&mut child);
    }
    *state.notebook_info.lock().unwrap() = None;
    drop(guard);
    stop_recent_callback(state, None);
}

fn terminate_child(child: &mut Child) {
    #[cfg(unix)]
    {
        unsafe {
            libc::kill(child.id() as i32, libc::SIGTERM);
        }
        for _ in 0..30 {
            if matches!(child.try_wait(), Ok(Some(_))) {
                return;
            }
            thread::sleep(Duration::from_millis(100));
        }
        let _ = child.kill();
        let _ = child.wait();
    }
    #[cfg(windows)]
    {
        let _ = quiet_command(Path::new("taskkill"))
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .status();
        let _ = child.wait();
    }
}

fn gen_token() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    (0..48)
        .map(|_| format!("{:x}", rng.gen::<u8>() & 0x0f))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!("jupiter-recent-test-{}", gen_token()));
        fs::create_dir_all(root.join("nested")).unwrap();
        let notebook = root.join("nested").join("测试 notebook.ipynb");
        fs::write(&notebook, b"{}").unwrap();
        (root, notebook)
    }

    #[test]
    fn validates_nested_notebook_inside_workdir() {
        let (root, notebook) = fixture();
        let (path, canonical) =
            validate_notebook_path(&root, "nested/测试 notebook.ipynb").unwrap();
        assert_eq!(path, "nested/测试 notebook.ipynb");
        assert_eq!(canonical, notebook.canonicalize().unwrap());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_parent_absolute_non_notebook_and_missing_paths() {
        let (root, _) = fixture();
        for path in ["../escape.ipynb", "/tmp/escape.ipynb", "nested/readme.txt"] {
            assert!(validate_notebook_path(&root, path)
                .unwrap_err()
                .starts_with("RECENT_INVALID_PATH:"));
        }
        assert!(validate_notebook_path(&root, "nested/missing.ipynb")
            .unwrap_err()
            .starts_with("RECENT_FILE_NOT_FOUND:"));
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn displays_regular_windows_workdir_without_verbatim_prefix() {
        let (root, _) = fixture();
        let canonical = normalize_workdir(&root).unwrap();
        let displayed = workdir_for_display(&canonical);
        assert!(!displayed.starts_with(r"\\?\"));
        assert_eq!(PathBuf::from(displayed).canonicalize().unwrap(), canonical);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn preserves_verbatim_prefix_when_windows_path_cannot_be_simplified() {
        let reserved = Path::new(r"\\?\C:\Users\asus\CON");
        assert_eq!(
            workdir_for_display(reserved),
            reserved.display().to_string()
        );

        let segment = "a".repeat(130);
        let long = PathBuf::from(format!(r"\\?\C:\{segment}\{segment}\notebooks"));
        assert_eq!(workdir_for_display(&long), long.display().to_string());

        let unc = Path::new(r"\\?\UNC\server\share\notebooks");
        assert_eq!(workdir_for_display(unc), unc.display().to_string());
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_that_escapes_workdir() {
        use std::os::unix::fs::symlink;

        let (root, _) = fixture();
        let outside = std::env::temp_dir().join(format!("outside-{}.ipynb", gen_token()));
        fs::write(&outside, b"{}").unwrap();
        symlink(&outside, root.join("escape.ipynb")).unwrap();
        assert!(validate_notebook_path(&root, "escape.ipynb")
            .unwrap_err()
            .starts_with("RECENT_INVALID_PATH:"));
        fs::remove_dir_all(root).unwrap();
        fs::remove_file(outside).unwrap();
    }

    #[test]
    fn generated_ids_are_opaque_and_unique() {
        let first = gen_token();
        let second = gen_token();
        assert_eq!(first.len(), 48);
        assert_eq!(second.len(), 48);
        assert_ne!(first, second);
    }

    /* ---------- 包名规范化与三态分类 ---------- */

    #[test]
    fn extracts_name_head_from_requirement_spec() {
        // 用户可能在包名框输入 extras / 版本约束，记录前必须取包名头部
        assert_eq!(requirement_name_head("requests"), "requests");
        assert_eq!(requirement_name_head("requests[security]"), "requests");
        assert_eq!(requirement_name_head("requests>=2"), "requests");
        assert_eq!(requirement_name_head(" Pillow "), "Pillow");
        assert_eq!(
            requirement_name_head("matplotlib-inline==0.1.3"),
            "matplotlib-inline"
        );
        assert_eq!(requirement_name_head("[bad]"), "");
    }

    #[test]
    fn normalizes_names_per_pep503() {
        // 大小写、- _ . 分隔符差异都必须折叠到同一形式，
        // 否则出厂清单与 pip 元数据对不上时会误判分类（Windows 事故根因之一）
        assert_eq!(normalize_pkg_name("Pillow"), "pillow");
        assert_eq!(normalize_pkg_name("opencv_python"), "opencv-python");
        assert_eq!(normalize_pkg_name("zope.interface"), "zope-interface");
        assert_eq!(normalize_pkg_name("matplotlib-inline"), "matplotlib-inline");
        assert_eq!(
            normalize_pkg_name("Argon2--CFFI__Bindings"),
            "argon2-cffi-bindings"
        );
        assert_eq!(normalize_pkg_name("  requests  "), "requests");
    }

    fn inventory_fixture() -> EnvInventory {
        // 场景：notebook（内置，依赖 traitlets）、matplotlib（显式安装，依赖 cycler/pillow）、
        // cycler（连带依赖）、pillow（显式安装但也被 matplotlib 需要）、
        // tomli（notebook 内 !pip install，无记录无入边）
        let packages = [
            "notebook",
            "traitlets",
            "matplotlib",
            "cycler",
            "Pillow",
            "tomli",
        ]
        .iter()
        .map(|n| InventoryPkg {
            name: n.to_string(),
            version: "1.0".into(),
        })
        .collect();
        let requires: HashMap<String, Vec<String>> = [
            ("notebook".to_string(), vec!["traitlets".to_string()]),
            (
                "matplotlib".to_string(),
                vec!["cycler".to_string(), "pillow".to_string()],
            ),
        ]
        .into_iter()
        .collect();
        EnvInventory { packages, requires }
    }

    #[test]
    fn classifies_builtin_explicit_dependency() {
        let builtins: HashSet<String> = ["notebook", "traitlets"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let recorded: HashSet<String> = ["matplotlib", "pillow"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let pkgs = classify_packages(inventory_fixture(), &builtins, &recorded);
        let by_name: HashMap<&str, &Pkg> = pkgs.iter().map(|p| (p.name.as_str(), p)).collect();

        assert_eq!(by_name["notebook"].source, PkgSource::Builtin);
        assert_eq!(by_name["traitlets"].source, PkgSource::Builtin);
        assert_eq!(by_name["matplotlib"].source, PkgSource::Explicit);
        assert_eq!(by_name["cycler"].source, PkgSource::Dependency);
        assert_eq!(
            by_name["cycler"].required_by,
            vec!["matplotlib".to_string()]
        );
        // 显式安装记录优先于「被别人需要」：用户装过 pillow，即使 matplotlib 依赖它仍是直接安装
        assert_eq!(by_name["Pillow"].source, PkgSource::Explicit);
        assert_eq!(
            by_name["Pillow"].required_by,
            vec!["matplotlib".to_string()]
        );
        // 无记录且无入边（notebook 内 !pip install 的顶层包）→ 启发式归为直接安装
        assert_eq!(by_name["tomli"].source, PkgSource::Explicit);
    }

    #[test]
    fn unrecorded_leaf_falls_back_to_explicit() {
        // 卸载 matplotlib 后 cycler 变成无入边孤儿：保持可见且可卸载（不误藏）
        let mut inv = inventory_fixture();
        inv.packages.retain(|p| p.name != "matplotlib");
        inv.requires.remove("matplotlib");
        let builtins = HashSet::new();
        let recorded = HashSet::new();
        let pkgs = classify_packages(inv, &builtins, &recorded);
        let cycler = pkgs.iter().find(|p| p.name == "cycler").unwrap();
        assert_eq!(cycler.source, PkgSource::Explicit);
        assert!(cycler.required_by.is_empty());
    }

    #[test]
    fn builtin_manifest_tolerates_utf8_bom() {
        // Windows 工具链可能写出带 BOM 的 JSON，不能让整份清单失效
        let root = std::env::temp_dir().join(format!("jupiter-manifest-test-{}", gen_token()));
        fs::create_dir_all(&root).unwrap();
        let mut data = b"\xef\xbb\xbf".to_vec();
        data.extend_from_slice(br#"[{"name":"Pillow","version":"8.4.0"}]"#);
        fs::write(root.join("factory-manifest.json"), data).unwrap();
        let names = load_builtin_names(&root);
        assert!(names.contains("pillow"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_manifest_falls_back_to_core_builtin() {
        let root = std::env::temp_dir().join(format!("jupiter-manifest-test-{}", gen_token()));
        fs::create_dir_all(&root).unwrap();
        let names = load_builtin_names(&root);
        assert_eq!(names.len(), BUILTIN.len());
        assert!(names.contains("notebook"));
        fs::remove_dir_all(root).unwrap();
    }
}

/* ================= 应用入口 ================= */

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState {
            notebook: Mutex::new(None),
            notebook_info: Mutex::new(None),
            notebook_lifecycle: tauri::async_runtime::Mutex::new(()),
            notebook_generation: std::sync::atomic::AtomicU64::new(0),
            recent_store: Mutex::new(()),
            recent_callback: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![
            ensure_env,
            list_packages,
            install_package,
            uninstall_package,
            start_notebook,
            stop_notebook,
            notebook_status,
            open_notebook_url,
            open_recent_notebook,
            open_external_url,
            check_updates,
            apply_patch,
            default_workdir,
            ensure_workdir,
            list_recent_notebooks,
            debug_mode,
            get_env_path,
            app_version,
            reset_env,
            pick_directory,
            diag_report,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    // Unix 信号兜底：kill/注销等场景下终止 Notebook 子进程，避免孤儿进程
    #[cfg(unix)]
    {
        use signal_hook::{
            consts::{SIGINT, SIGTERM},
            iterator::Signals,
        };
        let mut signals = Signals::new([SIGTERM, SIGINT]).expect("无法注册信号处理");
        let app_handle = app.handle().clone();
        thread::spawn(move || {
            for sig in signals.forever() {
                eprintln!("[UI-DIAG] 收到信号 {sig}，正在停止 Notebook 服务…");
                let st = app_handle.state::<AppState>();
                let mut guard = st.notebook.lock().unwrap();
                if let Some(mut child) = guard.take() {
                    terminate_child(&mut child);
                }
                drop(guard);
                stop_recent_callback(&st, None);
                std::process::exit(0);
            }
        });
    }

    app.run(|app_handle, event| {
        // 退出应用时同步停止 Notebook 服务，避免产生无人管理的孤儿进程
        if let tauri::RunEvent::ExitRequested { .. } = event {
            let st = app_handle.state::<AppState>();
            let mut guard = st.notebook.lock().unwrap();
            if let Some(mut child) = guard.take() {
                terminate_child(&mut child);
            }
            *st.notebook_info.lock().unwrap() = None;
            drop(guard);
            stop_recent_callback(&st, None);
        }
    });
}
