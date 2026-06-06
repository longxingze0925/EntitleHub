use serde::de::DeserializeOwned;

use crate::{SdkError, SdkResult};

#[derive(Debug, Clone, PartialEq)]
pub struct ApiResponseData<T> {
    pub data: T,
    pub request_id: String,
}

#[derive(Debug, serde::Deserialize)]
struct ApiEnvelope<T> {
    code: u32,
    message: String,
    data: Option<T>,
    request_id: Option<String>,
}

pub fn parse_api_response_data<T>(json: &str) -> SdkResult<ApiResponseData<T>>
where
    T: DeserializeOwned,
{
    let envelope: ApiEnvelope<T> =
        serde_json::from_str(json).map_err(|_| SdkError::InvalidApiResponse)?;
    if envelope.code != 0 {
        return Err(SdkError::ApiError(envelope.code, envelope.message));
    }

    Ok(ApiResponseData {
        data: envelope.data.ok_or(SdkError::InvalidApiResponse)?,
        request_id: envelope.request_id.unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use crate::SdkError;

    use super::parse_api_response_data;

    #[derive(Debug, PartialEq, Deserialize)]
    struct Payload {
        value: String,
    }

    #[test]
    fn parse_api_response_data_extracts_success_payload() {
        let response = parse_api_response_data::<Payload>(
            r#"{
              "code": 0,
              "message": "ok",
              "data": { "value": "ok" },
              "request_id": "req_1"
            }"#,
        )
        .expect("api response should parse");

        assert_eq!(response.data.value, "ok");
        assert_eq!(response.request_id, "req_1");
    }

    #[test]
    fn parse_api_response_data_rejects_error_or_missing_data() {
        let error = parse_api_response_data::<Payload>(
            r#"{
              "code": 40001,
              "message": "validation_failed",
              "data": null,
              "request_id": "req_1"
            }"#,
        )
        .expect_err("api error should return error");
        assert!(matches!(error, SdkError::ApiError(40001, _)));

        assert!(parse_api_response_data::<Payload>(
            r#"{
              "code": 0,
              "message": "ok",
              "data": null,
              "request_id": "req_1"
            }"#,
        )
        .is_err());
    }
}
