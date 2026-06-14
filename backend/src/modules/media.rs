use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    path::Path,
    process::Stdio,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::{fs, io::AsyncWriteExt, process::Command};
use uuid::Uuid;

use crate::error::AppError;

const MAX_FF_OUTPUT_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VideoMetadata {
    pub duration_sec: i64,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub codec: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ProcessedVideo {
    pub metadata: VideoMetadata,
    pub thumbnail_bytes: Vec<u8>,
    pub thumbnail_mime_type: &'static str,
}

pub async fn process_video_bytes(
    bytes: &[u8],
    extension: &str,
) -> Result<ProcessedVideo, AppError> {
    let work_dir = std::env::temp_dir().join(format!(
        "entitlehub-media-{}-{}",
        Uuid::new_v4(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or_default()
    ));
    fs::create_dir_all(&work_dir).await.map_err(|error| {
        AppError::dependency(format!("media temp directory create failed: {error}"))
    })?;

    let result = process_video_in_dir(bytes, extension, &work_dir).await;
    if let Err(error) = fs::remove_dir_all(&work_dir).await {
        tracing::warn!(%error, path = %work_dir.display(), "media temp directory cleanup failed");
    }
    result
}

async fn process_video_in_dir(
    bytes: &[u8],
    extension: &str,
    work_dir: &Path,
) -> Result<ProcessedVideo, AppError> {
    let input_path = work_dir.join(format!("input.{}", safe_extension(extension)));
    let thumbnail_path = work_dir.join("thumbnail.jpg");
    let mut file = fs::File::create(&input_path)
        .await
        .map_err(|error| AppError::dependency(format!("media input create failed: {error}")))?;
    file.write_all(bytes)
        .await
        .map_err(|error| AppError::dependency(format!("media input write failed: {error}")))?;
    file.flush()
        .await
        .map_err(|error| AppError::dependency(format!("media input flush failed: {error}")))?;

    let metadata = probe_video(&input_path).await?;
    generate_thumbnail(&input_path, &thumbnail_path, metadata.duration_sec).await?;
    let thumbnail_bytes = fs::read(&thumbnail_path)
        .await
        .map_err(|error| AppError::dependency(format!("video thumbnail read failed: {error}")))?;
    if thumbnail_bytes.is_empty() {
        return Err(AppError::dependency("video thumbnail is empty"));
    }

    Ok(ProcessedVideo {
        metadata,
        thumbnail_bytes,
        thumbnail_mime_type: "image/jpeg",
    })
}

async fn probe_video(path: &Path) -> Result<VideoMetadata, AppError> {
    let output = Command::new("ffprobe")
        .arg("-v")
        .arg("error")
        .arg("-select_streams")
        .arg("v:0")
        .arg("-show_entries")
        .arg("format=duration:stream=width,height,codec_name")
        .arg("-of")
        .arg("json")
        .arg(path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|error| {
            AppError::dependency(format!(
                "video_probe_unavailable: ffprobe could not be started: {error}"
            ))
        })?;
    if !output.status.success() {
        return Err(AppError::dependency(format!(
            "video_probe_failed: {}",
            limited_stderr(&output.stderr)
        )));
    }
    if output.stdout.len() > MAX_FF_OUTPUT_BYTES {
        return Err(AppError::dependency(
            "video_probe_failed: ffprobe output is too large",
        ));
    }
    metadata_from_ffprobe_json(&output.stdout)
}

async fn generate_thumbnail(
    input_path: &Path,
    output_path: &Path,
    duration_sec: i64,
) -> Result<(), AppError> {
    let seek_second = thumbnail_seek_second(duration_sec);
    let output = Command::new("ffmpeg")
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-y")
        .arg("-ss")
        .arg(format!("{seek_second:.3}"))
        .arg("-i")
        .arg(input_path)
        .arg("-frames:v")
        .arg("1")
        .arg("-vf")
        .arg("scale='min(720,iw)':-2")
        .arg("-q:v")
        .arg("3")
        .arg(output_path)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|error| {
            AppError::dependency(format!(
                "video_thumbnail_unavailable: ffmpeg could not be started: {error}"
            ))
        })?;
    if !output.status.success() {
        return Err(AppError::dependency(format!(
            "video_thumbnail_failed: {}",
            limited_stderr(&output.stderr)
        )));
    }

    Ok(())
}

fn metadata_from_ffprobe_json(bytes: &[u8]) -> Result<VideoMetadata, AppError> {
    let value: Value = serde_json::from_slice(bytes).map_err(|error| {
        AppError::dependency(format!("video_probe_failed: invalid ffprobe json: {error}"))
    })?;
    let duration_sec = value
        .get("format")
        .and_then(|format| format.get("duration"))
        .and_then(value_to_positive_seconds)
        .ok_or_else(|| AppError::dependency("video_probe_failed: duration missing"))?;
    let first_stream = value
        .get("streams")
        .and_then(Value::as_array)
        .and_then(|streams| streams.first());
    let width = first_stream
        .and_then(|stream| stream.get("width"))
        .and_then(value_to_positive_i64);
    let height = first_stream
        .and_then(|stream| stream.get("height"))
        .and_then(value_to_positive_i64);
    let codec = first_stream
        .and_then(|stream| stream.get("codec_name"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);

    Ok(VideoMetadata {
        duration_sec,
        width,
        height,
        codec,
    })
}

fn thumbnail_seek_second(duration_sec: i64) -> f64 {
    if duration_sec <= 2 {
        0.0
    } else {
        ((duration_sec as f64) * 0.1).clamp(1.0, 5.0)
    }
}

fn value_to_positive_seconds(value: &Value) -> Option<i64> {
    if let Some(value) = value.as_i64() {
        return (value > 0).then_some(value);
    }
    if let Some(value) = value.as_f64() {
        return (value.is_finite() && value > 0.0).then_some(value.ceil() as i64);
    }
    value
        .as_str()
        .and_then(|value| value.trim().parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value > 0.0)
        .map(|value| value.ceil() as i64)
}

fn value_to_positive_i64(value: &Value) -> Option<i64> {
    value.as_i64().filter(|value| *value > 0)
}

fn safe_extension(extension: &str) -> &str {
    let extension = extension.trim().trim_start_matches('.');
    if !extension.is_empty()
        && extension.len() <= 12
        && extension.bytes().all(|byte| byte.is_ascii_alphanumeric())
    {
        extension
    } else {
        "bin"
    }
}

fn limited_stderr(stderr: &[u8]) -> String {
    let len = stderr.len().min(512);
    String::from_utf8_lossy(&stderr[..len]).trim().to_owned()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{metadata_from_ffprobe_json, thumbnail_seek_second};

    #[test]
    fn ffprobe_metadata_extracts_duration_and_video_stream() {
        let metadata = metadata_from_ffprobe_json(
            serde_json::to_vec(&json!({
                "streams": [
                    {"codec_name": "h264", "width": 1920, "height": 1080}
                ],
                "format": {"duration": "8.2"}
            }))
            .expect("json")
            .as_slice(),
        )
        .expect("metadata");

        assert_eq!(metadata.duration_sec, 9);
        assert_eq!(metadata.width, Some(1920));
        assert_eq!(metadata.height, Some(1080));
        assert_eq!(metadata.codec.as_deref(), Some("h264"));
    }

    #[test]
    fn thumbnail_seek_uses_early_stable_frame() {
        assert_eq!(thumbnail_seek_second(1), 0.0);
        assert_eq!(thumbnail_seek_second(8), 1.0);
        assert_eq!(thumbnail_seek_second(80), 5.0);
    }
}
