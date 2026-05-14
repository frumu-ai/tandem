use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::app::state::channel_user_capabilities::{
    ChannelEnrollmentCodeRecord, ChannelUserCapabilityRecord, StoredCommandTier,
};
use crate::AppState;

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub(crate) enum ChannelEnrollRequest {
    Issue {
        channel: String,
        user_id: String,
        tier: StoredCommandTier,
        #[serde(default)]
        ttl_seconds: Option<u64>,
        #[serde(default)]
        issued_by: Option<String>,
    },
    Confirm {
        pairing_code: String,
        #[serde(default)]
        enrolled_by: Option<String>,
    },
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum ChannelEnrollResponse {
    CodeIssued {
        pairing_code: String,
        expires_at_ms: u64,
        enrollment: ChannelEnrollmentCodeRecord,
    },
    Enrolled {
        capability: ChannelUserCapabilityRecord,
    },
}

pub(crate) async fn channel_enroll(
    State(state): State<AppState>,
    Json(input): Json<ChannelEnrollRequest>,
) -> Response {
    match input {
        ChannelEnrollRequest::Issue {
            channel,
            user_id,
            tier,
            ttl_seconds,
            issued_by,
        } => {
            if channel.trim().is_empty() || user_id.trim().is_empty() {
                return enrollment_error(
                    StatusCode::BAD_REQUEST,
                    "channel and user_id are required",
                );
            }
            let enrollment = state
                .issue_channel_enrollment_code(
                    channel.trim().to_ascii_lowercase(),
                    user_id.trim().to_string(),
                    tier,
                    ttl_seconds.map(|seconds| seconds.saturating_mul(1000)),
                    issued_by,
                )
                .await;
            Json(ChannelEnrollResponse::CodeIssued {
                pairing_code: enrollment.code.clone(),
                expires_at_ms: enrollment.expires_at_ms,
                enrollment,
            })
            .into_response()
        }
        ChannelEnrollRequest::Confirm {
            pairing_code,
            enrolled_by,
        } => match state
            .confirm_channel_enrollment_code(&pairing_code, enrolled_by)
            .await
        {
            Ok(capability) => Json(ChannelEnrollResponse::Enrolled { capability }).into_response(),
            Err(error) if error.to_string().contains("expired") => {
                enrollment_error(StatusCode::GONE, &error.to_string())
            }
            Err(error) => enrollment_error(StatusCode::NOT_FOUND, &error.to_string()),
        },
    }
}

fn enrollment_error(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({ "error": message }))).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tandem_channels::config::ChannelSecurityProfile;

    #[tokio::test]
    async fn issue_and_confirm_enrolls_telegram_user_for_approval() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = AppState::new_starting("test".to_string(), true);
        state.channel_user_capabilities_path = dir.path().join("channel_user_capabilities.json");

        let response = channel_enroll(
            State(state.clone()),
            Json(ChannelEnrollRequest::Issue {
                channel: "telegram".to_string(),
                user_id: "4242".to_string(),
                tier: StoredCommandTier::Approve,
                ttl_seconds: Some(60),
                issued_by: Some("operator".to_string()),
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);

        let issued = state
            .channel_enrollment_codes
            .read()
            .await
            .values()
            .next()
            .cloned()
            .expect("code stored");
        let response = channel_enroll(
            State(state.clone()),
            Json(ChannelEnrollRequest::Confirm {
                pairing_code: issued.code,
                enrolled_by: Some("desktop".to_string()),
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert!(
            state
                .channel_user_can_approve("telegram", "4242", ChannelSecurityProfile::PublicDemo)
                .await
        );
    }
}
