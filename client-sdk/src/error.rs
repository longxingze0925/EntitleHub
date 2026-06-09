use std::{error::Error, fmt};

pub type SdkResult<T> = Result<T, SdkError>;

#[derive(Debug)]
pub enum SdkError {
    ApiError(u32, String),
    Base64Invalid,
    DowngradeNotAllowed,
    ExpiredAccessToken,
    ExpiredRefreshToken,
    ExpiredScript,
    HashMismatch,
    InvalidAuthRequest(&'static str),
    InvalidCache,
    InvalidMachineId,
    InvalidNonce,
    InvalidJwks,
    InvalidPrivateKey,
    InvalidPublicKey,
    InvalidSession,
    InvalidSignature,
    InvalidAccessToken,
    InvalidApiResponse,
    InvalidUpdateInfo(&'static str),
    InvalidClientRequest(&'static str),
    Io(std::io::Error),
    UnsupportedSignatureAlg(String),
}

impl fmt::Display for SdkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HashMismatch => write!(f, "downloaded file hash mismatch"),
            Self::ApiError(code, message) => write!(f, "api error {code}: {message}"),
            Self::Base64Invalid => write!(f, "base64 content invalid"),
            Self::DowngradeNotAllowed => write!(f, "update version_code is older than current"),
            Self::ExpiredAccessToken => write!(f, "access token expired"),
            Self::ExpiredRefreshToken => write!(f, "refresh token expired"),
            Self::ExpiredScript => write!(f, "script expired"),
            Self::InvalidAuthRequest(field) => write!(f, "auth request invalid: {field}"),
            Self::InvalidAccessToken => write!(f, "access token invalid"),
            Self::InvalidApiResponse => write!(f, "api response invalid"),
            Self::InvalidCache => write!(f, "cache invalid"),
            Self::InvalidClientRequest(field) => write!(f, "client request invalid: {field}"),
            Self::InvalidMachineId => write!(f, "machine id invalid"),
            Self::InvalidNonce => write!(f, "nonce invalid"),
            Self::InvalidJwks => write!(f, "jwks invalid"),
            Self::InvalidPrivateKey => write!(f, "private key invalid"),
            Self::InvalidPublicKey => write!(f, "public key invalid"),
            Self::InvalidSession => write!(f, "session invalid"),
            Self::InvalidSignature => write!(f, "signature invalid"),
            Self::InvalidUpdateInfo(field) => write!(f, "update info invalid: {field}"),
            Self::Io(error) => write!(f, "io error: {error}"),
            Self::UnsupportedSignatureAlg(alg) => {
                write!(f, "unsupported signature algorithm: {alg}")
            }
        }
    }
}

impl Error for SdkError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<std::io::Error> for SdkError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}
