//! User-triggered downloads of the whisper.cpp engine and ggml models into the app data dir.

use futures_util::StreamExt;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Pinned whisper.cpp release whose Windows assets ship `whisper-server.exe`.
const WHISPER_TAG: &str = "b5130";
const CPU_ASSET: &str = "whisper-blas-bin-x64.zip";
const GPU_ASSET: &str = "whisper-cublas-11.8.0-bin-x64.zip";

pub const MODELS: &[&str] = &["base.en", "small.en-q5_1", "small.en", "medium.en-q5_0", "large-v3-turbo-q5_0"];

pub fn engine_dir(root: &Path, gpu: bool) -> PathBuf {
    root.join(if gpu { "engine-gpu" } else { "engine-cpu" })
}

pub fn models_dir(root: &Path) -> PathBuf {
    root.join("models")
}

/// Finds whisper-server.exe anywhere inside the extracted release.
pub fn find_server_exe(dir: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut subdirs = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            subdirs.push(path);
        } else if path
            .file_name()
            .map(|n| n.to_string_lossy().eq_ignore_ascii_case("whisper-server.exe"))
            .unwrap_or(false)
        {
            return Some(path);
        }
    }
    subdirs.iter().find_map(|d| find_server_exe(d))
}

async fn download(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    mut progress: impl FnMut(u64, Option<u64>),
) -> Result<(), String> {
    let response = client.get(url).send().await.map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("Download failed ({}) for {url}", response.status()));
    }
    let total = response.content_length();
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let part = dest.with_extension("part");
    let mut file = std::fs::File::create(&part).map_err(|e| e.to_string())?;
    let mut done = 0u64;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| e.to_string())?;
        file.write_all(&chunk).map_err(|e| e.to_string())?;
        done += chunk.len() as u64;
        progress(done, total);
    }
    drop(file);
    std::fs::rename(&part, dest).map_err(|e| e.to_string())
}

pub async fn download_engine(
    client: &reqwest::Client,
    root: &Path,
    gpu: bool,
    progress: impl FnMut(u64, Option<u64>),
) -> Result<PathBuf, String> {
    let asset = if gpu { GPU_ASSET } else { CPU_ASSET };
    let url = format!("https://github.com/ggml-org/whisper.cpp/releases/download/{WHISPER_TAG}/{asset}");
    let dir = engine_dir(root, gpu);
    let zip_path = root.join(asset);
    download(client, &url, &zip_path, progress).await?;

    let extract_dir = dir.clone();
    let zip_for_task = zip_path.clone();
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let _ = std::fs::remove_dir_all(&extract_dir);
        let file = std::fs::File::open(&zip_for_task).map_err(|e| e.to_string())?;
        let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
        archive.extract(&extract_dir).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;
    let _ = std::fs::remove_file(&zip_path);

    find_server_exe(&dir).ok_or_else(|| "whisper-server.exe not found in the downloaded engine".into())
}

pub async fn download_model(
    client: &reqwest::Client,
    root: &Path,
    model: &str,
    progress: impl FnMut(u64, Option<u64>),
) -> Result<PathBuf, String> {
    if !MODELS.contains(&model) {
        return Err(format!("Unknown model {model}"));
    }
    let url = format!("https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-{model}.bin");
    let dest = super::stt::model_path(&models_dir(root), model);
    download(client, &url, &dest, progress).await?;
    Ok(dest)
}
