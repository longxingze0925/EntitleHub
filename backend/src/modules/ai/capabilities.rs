use serde_json::{json, Value};

use crate::error::AppError;

const DEFAULT_MAX_VIDEO_SECONDS: i64 = 3600;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelCapabilities {
    pub ratios: Vec<String>,
    pub resolutions: Vec<String>,
    pub durations: Vec<i64>,
    pub default_duration_seconds: Option<i64>,
    pub image_counts: Vec<i64>,
    pub max_images: Option<i64>,
    pub input_modes: Vec<String>,
    pub max_reference_images: Option<i64>,
    pub max_reference_videos: Option<i64>,
    pub max_reference_audios: Option<i64>,
    pub supports_reference_video: bool,
    pub supports_reference_audio: bool,
    pub supports_first_frame: bool,
    pub supports_last_frame: bool,
    pub accepted_mime_types: Vec<String>,
    pub max_asset_size_mb: Option<i64>,
}

impl ModelCapabilities {
    pub fn from_config(config: &Value) -> Result<Self, AppError> {
        let source = config.get("capabilities").unwrap_or(config);
        Ok(Self {
            ratios: string_list(source, &["ratios", "aspect_ratios"])?,
            resolutions: string_list(source, &["resolutions", "sizes"])?,
            durations: positive_int_list(source, &["durations", "duration_seconds_options"])?,
            default_duration_seconds: first_positive_int(
                source,
                &["default_duration_seconds", "duration_seconds", "seconds"],
            )?,
            image_counts: positive_int_list(source, &["image_counts", "counts"])?,
            max_images: first_positive_int(source, &["max_images", "max_image_count"])?,
            input_modes: string_list(source, &["inputModes", "input_modes"])?,
            max_reference_images: first_positive_int(
                source,
                &["maxReferenceImages", "max_reference_images"],
            )?,
            max_reference_videos: first_positive_int(
                source,
                &["maxReferenceVideos", "max_reference_videos"],
            )?,
            max_reference_audios: first_positive_int(
                source,
                &["maxReferenceAudios", "max_reference_audios"],
            )?,
            supports_reference_video: optional_bool(
                source,
                &["supportsReferenceVideo", "supports_reference_video"],
            )?
            .unwrap_or(false),
            supports_reference_audio: optional_bool(
                source,
                &["supportsReferenceAudio", "supports_reference_audio"],
            )?
            .unwrap_or(false),
            supports_first_frame: optional_bool(
                source,
                &["supportsFirstFrame", "supports_first_frame"],
            )?
            .unwrap_or(false),
            supports_last_frame: optional_bool(
                source,
                &["supportsLastFrame", "supports_last_frame"],
            )?
            .unwrap_or(false),
            accepted_mime_types: string_list(
                source,
                &["acceptedMimeTypes", "accepted_mime_types"],
            )?,
            max_asset_size_mb: first_positive_int(
                source,
                &["maxAssetSizeMb", "max_asset_size_mb"],
            )?,
        })
    }

    pub fn to_public_json(&self) -> Value {
        json!({
            "ratios": self.ratios,
            "resolutions": self.resolutions,
            "durations": self.durations,
            "default_duration_seconds": self.default_duration_seconds,
            "image_counts": self.image_counts,
            "max_images": self.max_images,
            "inputModes": self.input_modes,
            "maxReferenceImages": self.max_reference_images,
            "maxReferenceVideos": self.max_reference_videos,
            "maxReferenceAudios": self.max_reference_audios,
            "supportsReferenceVideo": self.supports_reference_video,
            "supportsReferenceAudio": self.supports_reference_audio,
            "supportsFirstFrame": self.supports_first_frame,
            "supportsLastFrame": self.supports_last_frame,
            "acceptedMimeTypes": self.accepted_mime_types,
            "maxAssetSizeMb": self.max_asset_size_mb,
        })
    }
}

pub fn validate_capabilities_config(config: &Value) -> Result<(), AppError> {
    ModelCapabilities::from_config(config).map(|_| ())
}

pub fn model_capabilities_json(config: &Value) -> Result<Value, AppError> {
    Ok(ModelCapabilities::from_config(config)?.to_public_json())
}

pub fn validate_image_payload(
    payload: &Value,
    config: &Value,
    fallback_max_count: i64,
) -> Result<(), AppError> {
    let capabilities = ModelCapabilities::from_config(config)?;
    validate_optional_string_choice(payload, &["ratio", "aspect_ratio"], &capabilities.ratios)?;
    validate_optional_string_choice(payload, &["resolution", "size"], &capabilities.resolutions)?;

    let count = image_count(payload, fallback_max_count)?;
    if !capabilities.image_counts.is_empty() && !capabilities.image_counts.contains(&count) {
        return Err(AppError::validation_failed(format!(
            "image count n must be one of {}",
            join_i64(&capabilities.image_counts)
        )));
    }
    if let Some(max_images) = capabilities.max_images {
        if count > max_images {
            return Err(AppError::validation_failed(format!(
                "image count n must be less than or equal to {max_images}"
            )));
        }
    }

    Ok(())
}

pub fn validate_video_payload(payload: &Value, config: &Value) -> Result<(), AppError> {
    let capabilities = ModelCapabilities::from_config(config)?;
    validate_optional_string_choice(payload, &["ratio", "aspect_ratio"], &capabilities.ratios)?;
    validate_optional_string_choice(payload, &["resolution", "size"], &capabilities.resolutions)?;
    validate_optional_string_choice(
        payload,
        &["inputMode", "input_mode"],
        &capabilities.input_modes,
    )?;

    let seconds = requested_video_seconds(payload, config, DEFAULT_MAX_VIDEO_SECONDS)?;
    if !capabilities.durations.is_empty() && !capabilities.durations.contains(&seconds) {
        return Err(AppError::validation_failed(format!(
            "video duration must be one of {} seconds",
            join_i64(&capabilities.durations)
        )));
    }

    Ok(())
}

pub fn validate_input_mode(input_mode: &str, config: &Value) -> Result<(), AppError> {
    let capabilities = ModelCapabilities::from_config(config)?;
    if capabilities.input_modes.is_empty()
        || capabilities
            .input_modes
            .iter()
            .any(|item| item.eq_ignore_ascii_case(input_mode))
    {
        return Ok(());
    }

    Err(AppError::validation_failed(format!(
        "inputMode must be one of {}",
        capabilities.input_modes.join(", ")
    )))
}

pub fn image_count(payload: &Value, fallback_max_count: i64) -> Result<i64, AppError> {
    let Some(value) = payload.get("n") else {
        return Ok(1);
    };
    let Some(count) = value.as_i64() else {
        return Err(AppError::validation_failed(
            "image count n must be an integer",
        ));
    };
    let max_count = fallback_max_count.max(1);
    if !(1..=max_count).contains(&count) {
        return Err(AppError::validation_failed(format!(
            "image count n must be between 1 and {max_count}"
        )));
    }

    Ok(count)
}

pub fn requested_video_seconds(
    payload: &Value,
    config: &Value,
    fallback_max_seconds: i64,
) -> Result<i64, AppError> {
    let capabilities = ModelCapabilities::from_config(config)?;
    let seconds = optional_video_seconds(payload)?
        .or_else(|| {
            payload
                .get("video")
                .and_then(|video| optional_video_seconds(video).ok())
                .flatten()
        })
        .or(capabilities.default_duration_seconds)
        .unwrap_or(8);
    let max_seconds = fallback_max_seconds.max(1);
    if !(1..=max_seconds).contains(&seconds) {
        return Err(AppError::validation_failed(format!(
            "video duration must be between 1 and {max_seconds} seconds"
        )));
    }

    Ok(seconds)
}

pub fn value_to_positive_seconds(value: &Value) -> Option<i64> {
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

fn optional_video_seconds(value: &Value) -> Result<Option<i64>, AppError> {
    for key in ["duration", "duration_seconds", "seconds"] {
        if let Some(raw) = value.get(key) {
            return value_to_positive_seconds(raw)
                .map(Some)
                .ok_or_else(|| AppError::validation_failed("video duration is invalid"));
        }
    }

    Ok(None)
}

fn validate_optional_string_choice(
    payload: &Value,
    keys: &[&str],
    allowed: &[String],
) -> Result<(), AppError> {
    if allowed.is_empty() {
        return Ok(());
    }
    for key in keys {
        if let Some(value) = payload.get(*key) {
            let Some(value) = value
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            else {
                return Err(AppError::validation_failed(format!("{key} is invalid")));
            };
            if !allowed.iter().any(|item| item.eq_ignore_ascii_case(value)) {
                return Err(AppError::validation_failed(format!(
                    "{key} must be one of {}",
                    allowed.join(", ")
                )));
            }
            return Ok(());
        }
    }

    Ok(())
}

fn string_list(source: &Value, keys: &[&str]) -> Result<Vec<String>, AppError> {
    let Some(value) = keys.iter().find_map(|key| source.get(*key)) else {
        return Ok(Vec::new());
    };
    let Some(items) = value.as_array() else {
        return Err(AppError::validation_failed(
            "ai model capability list must be an array",
        ));
    };
    let mut output = Vec::new();
    for item in items {
        let Some(text) = item.as_str().map(str::trim).filter(|item| !item.is_empty()) else {
            return Err(AppError::validation_failed(
                "ai model capability items must be non-empty strings",
            ));
        };
        if text.len() > 64 || text.contains('\0') {
            return Err(AppError::validation_failed(
                "ai model capability item is invalid",
            ));
        }
        if !output.iter().any(|existing: &String| existing == text) {
            output.push(text.to_owned());
        }
    }

    Ok(output)
}

fn positive_int_list(source: &Value, keys: &[&str]) -> Result<Vec<i64>, AppError> {
    let Some(value) = keys.iter().find_map(|key| source.get(*key)) else {
        return Ok(Vec::new());
    };
    let Some(items) = value.as_array() else {
        return Err(AppError::validation_failed(
            "ai model capability list must be an array",
        ));
    };
    let mut output = Vec::new();
    for item in items {
        let Some(number) = value_to_positive_seconds(item) else {
            return Err(AppError::validation_failed(
                "ai model capability numbers must be positive",
            ));
        };
        if !output.contains(&number) {
            output.push(number);
        }
    }
    output.sort_unstable();

    Ok(output)
}

fn first_positive_int(source: &Value, keys: &[&str]) -> Result<Option<i64>, AppError> {
    let Some(value) = keys.iter().find_map(|key| source.get(*key)) else {
        return Ok(None);
    };
    value_to_positive_seconds(value)
        .map(Some)
        .ok_or_else(|| AppError::validation_failed("ai model capability number must be positive"))
}

fn optional_bool(source: &Value, keys: &[&str]) -> Result<Option<bool>, AppError> {
    let Some(value) = keys.iter().find_map(|key| source.get(*key)) else {
        return Ok(None);
    };
    value
        .as_bool()
        .map(Some)
        .ok_or_else(|| AppError::validation_failed("ai model capability boolean must be valid"))
}

fn join_i64(values: &[i64]) -> String {
    values
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        image_count, model_capabilities_json, requested_video_seconds, validate_image_payload,
        validate_input_mode, validate_video_payload,
    };

    #[test]
    fn capabilities_validate_allowed_image_options() {
        let config = json!({
            "capabilities": {
                "ratios": ["1:1", "16:9"],
                "resolutions": ["1024x1024"],
                "image_counts": [1, 2]
            }
        });

        assert!(validate_image_payload(
            &json!({"ratio": "1:1", "size": "1024x1024", "n": 2}),
            &config,
            10
        )
        .is_ok());
        assert!(validate_image_payload(&json!({"ratio": "9:16", "n": 2}), &config, 10).is_err());
        assert!(validate_image_payload(&json!({"ratio": "1:1", "n": 3}), &config, 10).is_err());
    }

    #[test]
    fn capabilities_validate_allowed_video_options() {
        let config = json!({
            "capabilities": {
                "durations": [5, 8],
                "default_duration_seconds": 8,
                "ratios": ["16:9"]
            }
        });

        assert_eq!(
            requested_video_seconds(&json!({"model": "video"}), &config, 3600).expect("seconds"),
            8
        );
        assert!(validate_video_payload(&json!({"ratio": "16:9", "duration": 5}), &config).is_ok());
        assert!(validate_video_payload(&json!({"ratio": "1:1", "duration": 5}), &config).is_err());
        assert!(
            validate_video_payload(&json!({"ratio": "16:9", "duration": 10}), &config).is_err()
        );
    }

    #[test]
    fn public_capabilities_are_stable_shape() {
        let public = model_capabilities_json(&json!({
            "capabilities": {
                "sizes": ["720p"],
                "max_images": 4,
                "inputModes": ["text", "image"],
                "maxReferenceVideos": 1,
                "supportsFirstFrame": true,
                "acceptedMimeTypes": ["image/png"],
                "maxAssetSizeMb": 50
            }
        }))
        .expect("capabilities");

        assert_eq!(public["resolutions"], json!(["720p"]));
        assert_eq!(public["max_images"], json!(4));
        assert_eq!(public["inputModes"], json!(["text", "image"]));
        assert_eq!(public["maxReferenceVideos"], json!(1));
        assert_eq!(public["supportsFirstFrame"], json!(true));
        assert_eq!(public["acceptedMimeTypes"], json!(["image/png"]));
        assert_eq!(public["maxAssetSizeMb"], json!(50));
        assert!(validate_input_mode(
            "image",
            &json!({"capabilities": {"inputModes": ["text", "image"]}})
        )
        .is_ok());
        assert!(validate_input_mode(
            "frames",
            &json!({"capabilities": {"inputModes": ["text", "image"]}})
        )
        .is_err());
    }

    #[test]
    fn empty_config_keeps_backward_compatible_defaults() {
        assert!(validate_image_payload(&json!({"n": 10}), &json!({}), 10).is_ok());
        assert!(image_count(&json!({"n": 11}), 10).is_err());
    }
}
