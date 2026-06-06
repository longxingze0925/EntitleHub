use std::net::{IpAddr, SocketAddr};

use axum::{
    extract::{connect_info::ConnectInfo, Request, State},
    http::{HeaderMap, HeaderValue},
    middleware::Next,
    response::Response,
};

use crate::{rate_limit::INTERNAL_CLIENT_IP_HEADER, state::AppState};

pub async fn attach_client_ip(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    let peer_ip = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(address)| address.ip());
    let client_ip = resolve_client_ip(
        request.headers(),
        peer_ip,
        &state.config.security.trusted_proxies,
    );

    request.headers_mut().remove(INTERNAL_CLIENT_IP_HEADER);
    if let Ok(value) = HeaderValue::from_str(&client_ip) {
        request
            .headers_mut()
            .insert(INTERNAL_CLIENT_IP_HEADER, value);
    }

    next.run(request).await
}

fn resolve_client_ip(
    headers: &HeaderMap,
    peer_ip: Option<IpAddr>,
    trusted_proxies: &[IpAddr],
) -> String {
    let fallback = peer_ip
        .map(|ip| ip.to_string())
        .unwrap_or_else(|| "unknown".to_owned());
    let Some(peer_ip) = peer_ip else {
        return fallback;
    };
    if !trusted_proxies.contains(&peer_ip) {
        return fallback;
    }

    forwarded_for(headers)
        .or_else(|| real_ip(headers))
        .unwrap_or(fallback)
}

fn forwarded_for(headers: &HeaderMap) -> Option<String> {
    headers
        .get("X-Forwarded-For")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .and_then(parse_header_ip)
}

fn real_ip(headers: &HeaderMap) -> Option<String> {
    headers
        .get("X-Real-IP")
        .and_then(|value| value.to_str().ok())
        .and_then(parse_header_ip)
}

fn parse_header_ip(value: &str) -> Option<String> {
    value.trim().parse::<IpAddr>().ok().map(|ip| ip.to_string())
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use axum::http::{HeaderMap, HeaderValue};

    use super::resolve_client_ip;

    #[test]
    fn trusted_proxy_can_supply_forwarded_for() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Forwarded-For",
            HeaderValue::from_static("203.0.113.10, 10.0.0.1"),
        );
        headers.insert("X-Real-IP", HeaderValue::from_static("198.51.100.7"));

        assert_eq!(
            resolve_client_ip(
                &headers,
                Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
                &[IpAddr::V4(Ipv4Addr::LOCALHOST)]
            ),
            "203.0.113.10"
        );
    }

    #[test]
    fn trusted_proxy_falls_back_to_real_ip() {
        let mut headers = HeaderMap::new();
        headers.insert("X-Real-IP", HeaderValue::from_static("198.51.100.7"));

        assert_eq!(
            resolve_client_ip(
                &headers,
                Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
                &[IpAddr::V4(Ipv4Addr::LOCALHOST)]
            ),
            "198.51.100.7"
        );
    }

    #[test]
    fn untrusted_peer_cannot_spoof_forwarded_headers() {
        let mut headers = HeaderMap::new();
        headers.insert("X-Forwarded-For", HeaderValue::from_static("203.0.113.10"));

        assert_eq!(
            resolve_client_ip(
                &headers,
                Some(IpAddr::V4(Ipv4Addr::new(198, 51, 100, 7))),
                &[IpAddr::V4(Ipv4Addr::LOCALHOST)]
            ),
            "198.51.100.7"
        );
    }

    #[test]
    fn missing_peer_ip_is_unknown() {
        let headers = HeaderMap::new();

        assert_eq!(resolve_client_ip(&headers, None, &[]), "unknown");
    }
}
