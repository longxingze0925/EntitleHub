use serde::Deserialize;

use crate::{SdkError, SdkResult};

pub const USAGE_ID_HEADER: &str = "x-entitlehub-usage-id";

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct AiModelSummary {
    pub id: String,
    pub object: String,
    #[serde(default)]
    pub created: i64,
    #[serde(default)]
    pub owned_by: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct AiModelListResponse {
    pub object: String,
    pub data: Vec<AiModelSummary>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AiGatewayJsonResponse {
    pub body: serde_json::Value,
    pub usage_id: Option<String>,
}

impl AiModelListResponse {
    pub fn from_json(json: &str) -> SdkResult<Self> {
        let value: serde_json::Value =
            serde_json::from_str(json).map_err(|_| SdkError::InvalidApiResponse)?;
        if let Some(error) = backend_api_error(&value) {
            return Err(error);
        }
        let response: Self =
            serde_json::from_value(value).map_err(|_| SdkError::InvalidApiResponse)?;
        response.validate()?;

        Ok(response)
    }

    fn validate(&self) -> SdkResult<()> {
        if self.object.trim().is_empty()
            || self
                .data
                .iter()
                .any(|model| model.id.trim().is_empty() || model.object.trim().is_empty())
        {
            return Err(SdkError::InvalidApiResponse);
        }

        Ok(())
    }
}

impl AiGatewayJsonResponse {
    pub fn from_json(json: &str) -> SdkResult<Self> {
        Self::from_json_with_usage_id(json, None)
    }

    pub fn from_json_with_usage_id(json: &str, usage_id: Option<&str>) -> SdkResult<Self> {
        let body = serde_json::from_str(json).map_err(|_| SdkError::InvalidApiResponse)?;
        if let Some(error) = backend_api_error(&body) {
            return Err(error);
        }
        let usage_id = usage_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned);

        Ok(Self { body, usage_id })
    }
}

pub fn build_chat_completions_body(payload: &serde_json::Value) -> SdkResult<Vec<u8>> {
    validate_ai_json_payload(payload, "chat_completions")?;
    if payload.get("stream").and_then(serde_json::Value::as_bool) == Some(true) {
        return Err(SdkError::InvalidClientRequest("stream"));
    }

    serialize_ai_json_payload(payload, "chat_completions")
}

pub fn build_image_generations_body(payload: &serde_json::Value) -> SdkResult<Vec<u8>> {
    validate_ai_json_payload(payload, "image_generations")?;
    serialize_ai_json_payload(payload, "image_generations")
}

pub fn build_video_generations_body(payload: &serde_json::Value) -> SdkResult<Vec<u8>> {
    validate_ai_json_payload(payload, "video_generations")?;
    serialize_ai_json_payload(payload, "video_generations")
}

pub fn build_embeddings_body(payload: &serde_json::Value) -> SdkResult<Vec<u8>> {
    validate_ai_json_payload(payload, "embeddings")?;
    serialize_ai_json_payload(payload, "embeddings")
}

pub fn usage_id_from_headers<'a>(headers: &'a [(String, String)]) -> Option<&'a str> {
    headers.iter().find_map(|(name, value)| {
        name.eq_ignore_ascii_case(USAGE_ID_HEADER)
            .then_some(value.trim())
            .filter(|value| !value.is_empty())
    })
}

pub fn image_urls_from_response(body: &serde_json::Value) -> Vec<String> {
    body.get("data")
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("url").and_then(serde_json::Value::as_str))
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

pub fn video_urls_from_response(body: &serde_json::Value) -> Vec<String> {
    let mut urls = Vec::new();
    collect_video_urls(body, &mut urls);
    urls
}

fn collect_video_urls(value: &serde_json::Value, urls: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(object) => {
            for key in ["url", "video_url", "output_url", "download_url"] {
                if let Some(url) = object.get(key).and_then(serde_json::Value::as_str) {
                    urls.push(url.to_owned());
                }
            }
            for nested in object.values() {
                collect_video_urls(nested, urls);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_video_urls(item, urls);
            }
        }
        _ => {}
    }
}

fn validate_ai_json_payload(payload: &serde_json::Value, field: &'static str) -> SdkResult<()> {
    let Some(object) = payload.as_object() else {
        return Err(SdkError::InvalidClientRequest(field));
    };
    let Some(model) = object
        .get("model")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Err(SdkError::InvalidClientRequest("model"));
    };
    if model.contains(char::is_control) {
        return Err(SdkError::InvalidClientRequest("model"));
    }

    Ok(())
}

fn serialize_ai_json_payload(
    payload: &serde_json::Value,
    field: &'static str,
) -> SdkResult<Vec<u8>> {
    serde_json::to_vec(payload).map_err(|_| SdkError::InvalidClientRequest(field))
}

fn backend_api_error(value: &serde_json::Value) -> Option<SdkError> {
    let object = value.as_object()?;
    if !object.contains_key("request_id") {
        return None;
    }
    let code = object.get("code")?.as_u64()?;
    if code == 0 || code > u32::MAX as u64 {
        return None;
    }
    let message = object
        .get("message")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_owned();

    Some(SdkError::ApiError(code as u32, message))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        build_chat_completions_body, build_embeddings_body, build_image_generations_body,
        build_video_generations_body, image_urls_from_response, usage_id_from_headers,
        video_urls_from_response, AiGatewayJsonResponse, AiModelListResponse,
    };

    #[test]
    fn ai_body_builders_validate_model_and_reject_streaming() {
        let chat = json!({
            "model": "gpt-test",
            "messages": [{ "role": "user", "content": "hello" }]
        });
        let image = json!({ "model": "image-test", "prompt": "logo", "n": 1 });
        let video = json!({ "model": "video-test", "prompt": "intro", "duration": 8 });
        let embeddings = json!({ "model": "embed-test", "input": "hello" });

        assert!(build_chat_completions_body(&chat).expect("chat body").len() > 10);
        assert!(
            build_image_generations_body(&image)
                .expect("image body")
                .len()
                > 10
        );
        assert!(
            build_video_generations_body(&video)
                .expect("video body")
                .len()
                > 10
        );
        assert!(
            build_embeddings_body(&embeddings)
                .expect("embedding body")
                .len()
                > 10
        );
        assert!(build_chat_completions_body(&json!({
            "model": "gpt-test",
            "stream": true
        }))
        .is_err());
        assert!(build_embeddings_body(&json!({ "input": "hello" })).is_err());
    }

    #[test]
    fn model_list_and_raw_gateway_response_parse() {
        let models = AiModelListResponse::from_json(
            r#"{
              "object": "list",
              "data": [{
                "id": "gpt-test",
                "object": "model",
                "created": 1710000000,
                "owned_by": "entitlehub"
              }]
            }"#,
        )
        .expect("models response should parse");

        assert_eq!(models.data[0].id, "gpt-test");
        assert!(AiModelListResponse::from_json(r#"{"object":"list","data":[{"id":""}]}"#).is_err());

        let response = AiGatewayJsonResponse::from_json_with_usage_id(
            r#"{"id":"chatcmpl_1","choices":[]}"#,
            Some(" usage-id "),
        )
        .expect("gateway response should parse");

        assert_eq!(response.usage_id.as_deref(), Some("usage-id"));
        assert_eq!(response.body["id"], "chatcmpl_1");

        let error = AiGatewayJsonResponse::from_json(
            r#"{
              "code": 40306,
              "message": "subscription_inactive",
              "data": null,
              "request_id": "req_1"
            }"#,
        )
        .expect_err("backend error envelope should be surfaced");
        assert!(matches!(
            error,
            crate::SdkError::ApiError(40306, message) if message == "subscription_inactive"
        ));
    }

    #[test]
    fn usage_id_and_image_urls_are_extracted_from_transport_data() {
        let headers = vec![(
            "X-EntitleHub-Usage-Id".to_owned(),
            "00000000-0000-0000-0000-000000000001".to_owned(),
        )];
        let body = json!({
            "data": [
                { "url": "https://example.com/api/ai/assets/1" },
                { "b64_json": "ignored-after-cache" }
            ]
        });

        assert_eq!(
            usage_id_from_headers(&headers),
            Some("00000000-0000-0000-0000-000000000001")
        );
        assert_eq!(
            image_urls_from_response(&body),
            vec!["https://example.com/api/ai/assets/1".to_owned()]
        );
    }

    #[test]
    fn video_urls_are_extracted_from_common_response_shapes() {
        let body = json!({
            "data": [
                { "video_url": "/api/ai/assets/video-1" },
                { "output": { "download_url": "/api/ai/assets/video-2" } }
            ]
        });

        assert_eq!(
            video_urls_from_response(&body),
            vec![
                "/api/ai/assets/video-1".to_owned(),
                "/api/ai/assets/video-2".to_owned()
            ]
        );
    }
}
