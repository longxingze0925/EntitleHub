use axum::{Extension, Json};
use serde::Serialize;
use uuid::Uuid;

use crate::{
    error::ApiResponse, http::request_id::RequestId, modules::auth::session::AdminContext,
};

#[derive(Debug, Serialize)]
pub struct MeResponse {
    pub user: MeUser,
    pub tenant: MeTenant,
    pub roles: Vec<String>,
    pub permissions: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct MeUser {
    pub id: Uuid,
    pub email: String,
    pub name: String,
    pub email_verified: bool,
    pub mfa_enabled: bool,
}

#[derive(Debug, Serialize)]
pub struct MeTenant {
    pub id: Uuid,
    pub name: String,
}

pub async fn me(
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
) -> Json<ApiResponse<MeResponse>> {
    Json(ApiResponse::ok(
        MeResponse {
            user: MeUser {
                id: admin.team_member_id,
                email: admin.email,
                name: admin.name,
                email_verified: admin.email_verified,
                mfa_enabled: admin.mfa_enabled,
            },
            tenant: MeTenant {
                id: admin.tenant_id,
                name: admin.tenant_name,
            },
            roles: admin.roles,
            permissions: admin.permissions,
        },
        request_id.to_string(),
    ))
}
