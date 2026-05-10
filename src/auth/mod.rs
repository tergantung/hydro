use serde_json::{Value, json};

use crate::constants::{network, timing};
use crate::logging::{Direction, Logger, TransportKind};
use crate::models::AuthInput;

#[derive(Debug, Clone)]
pub struct ResolvedAuth {
    pub jwt: String,
    pub device_id: String,
}

pub async fn resolve_auth(
    auth: AuthInput,
    logger: Logger,
    session_id: String,
    proxy: Option<String>,
) -> Result<ResolvedAuth, String> {
    match auth {
        AuthInput::Jwt { jwt, device_id } => Ok(ResolvedAuth {
            jwt,
            device_id: device_id.unwrap_or_else(|| network::DEFAULT_DEVICE_ID.to_string()),
        }),
        AuthInput::AndroidDevice { device_id } => {
            let device_id = normalize_or_generate_device_id(device_id);
            let ticket =
                playfab_android_login(device_id.clone(), logger.clone(), session_id.clone(), proxy.clone())
                    .await?;
            let jwt = exchange_ticket(ticket, logger, session_id, proxy).await?;
            Ok(ResolvedAuth { jwt, device_id })
        }
        AuthInput::EmailPassword {
            email,
            password,
            device_id,
        } => {
            let device_id = device_id.unwrap_or_else(|| network::DEFAULT_DEVICE_ID.to_string());
            let ticket =
                playfab_email_login(email, password, logger.clone(), session_id.clone(), proxy.clone()).await?;
            let jwt = exchange_ticket(ticket, logger, session_id, proxy).await?;
            Ok(ResolvedAuth { jwt, device_id })
        }
    }
}

async fn playfab_android_login(
    device_id: String,
    logger: Logger,
    session_id: String,
    proxy: Option<String>,
) -> Result<String, String> {
    let body = json!({
        "AndroidDeviceId": device_id,
        "CreateAccount": true,
        "TitleId": network::PLAYFAB_TITLE_ID,
    });
    let json = post_json(
        network::PLAYFAB_ANDROID_URL,
        body,
        Vec::new(),
        logger,
        session_id,
        proxy,
    )
    .await?;
    extract_ticket(json)
}

async fn playfab_email_login(
    email: String,
    password: String,
    logger: Logger,
    session_id: String,
    proxy: Option<String>,
) -> Result<String, String> {
    let body = json!({
        "Email": email,
        "Password": password,
        "TitleId": network::PLAYFAB_TITLE_ID,
    });
    let json = post_json(
        network::PLAYFAB_EMAIL_URL,
        body,
        Vec::new(),
        logger,
        session_id,
        proxy,
    )
    .await?;
    extract_ticket(json)
}

async fn exchange_ticket(
    session_ticket: String,
    logger: Logger,
    session_id: String,
    proxy: Option<String>,
) -> Result<String, String> {
    let json = post_json(
        network::SOCIALFIRST_EXCHANGE_URL,
        json!({ "playfabToken": session_ticket }),
        vec![
            (
                "X-Sf-Client-Api-Key".to_string(),
                network::SOCIALFIRST_API_KEY.to_string(),
            ),
            (
                "X-Unity-Version".to_string(),
                network::UNITY_VERSION.to_string(),
            ),
            (
                "X-Sf-Client-Version".to_string(),
                "1.0.0".to_string(),
            ),
            (
                "X-Sf-Client-Platform".to_string(),
                "WindowsPlayer".to_string(),
            ),
        ],
        logger,
        session_id,
        proxy,
    )
    .await?;

    json.get("socialFirstToken")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| "missing socialFirstToken in auth response".to_string())
}

async fn post_json(
    url: &'static str,
    body: Value,
    headers: Vec<(String, String)>,
    logger: Logger,
    session_id: String,
    proxy: Option<String>,
) -> Result<Value, String> {
    logger.transport(
        TransportKind::Http,
        Direction::Outgoing,
        "auth_client",
        Some(&session_id),
        format!("POST {url} body={body}"),
    );

    tokio::task::spawn_blocking(move || {
        let mut config = ureq::Agent::config_builder()
            .timeout_global(Some(timing::http_timeout()));
        
        if let Some(proxy_url) = proxy {
            let normalized = normalize_proxy_url(&proxy_url);
            config = config.proxy(Some(ureq::Proxy::new(&normalized).map_err(|e| e.to_string())?));
        }

        let agent: ureq::Agent = config.build().into();

        let mut request = agent.post(url)
            .header("Content-Type", "application/json; charset=utf-8")
            .header("User-Agent", format!("UnityPlayer/{} (UnityWins/64)", network::UNITY_VERSION))
            .header("X-Unity-Version", network::UNITY_VERSION)
            .header("Accept", "*/*")
            .header("Accept-Language", "en-US,en;q=0.9")
            .header("Accept-Encoding", "gzip, br")
            .header("Connection", "keep-alive")
            .header("DNT", "1")
            .header("X-Requested-With", "com.socialfirst.pixelworlds")
            .header("Sec-Fetch-Dest", "empty")
            .header("Sec-Fetch-Mode", "cors")
            .header("Sec-Fetch-Site", "cross-site");
        for (key, value) in headers {
            request = request.header(&key, &value);
        }

        let mut response = request
            .send_json(&body)
            .map_err(|error| error.to_string())?;
        let status = response.status();
        let parsed: Value = response
            .body_mut()
            .read_json()
            .map_err(|error| error.to_string())?;

        logger.transport(
            TransportKind::Http,
            Direction::Incoming,
            "auth_client",
            Some(&session_id),
            format!("status={status} body={parsed}"),
        );

        Ok(parsed)
    })
    .await
    .map_err(|error| error.to_string())?
}

fn extract_ticket(json: Value) -> Result<String, String> {
    json.get("data")
        .and_then(|value| value.get("SessionTicket"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| "missing data.SessionTicket in PlayFab response".to_string())
}

fn normalize_or_generate_device_id(device_id: Option<String>) -> String {
    let trimmed = device_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);

    trimmed.unwrap_or_else(generate_device_id)
}

fn generate_device_id() -> String {
    let bytes: [u8; 20] = rand::random();
    let mut output = String::with_capacity(40);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(&mut output, "{byte:02x}");
    }
    output
}

fn normalize_proxy_url(input: &str) -> String {
    let input = input.trim();
    if input.is_empty() {
        return String::new();
    }

    if input.starts_with("http://") || input.starts_with("https://") || input.starts_with("socks5://") || input.starts_with("socks4://") {
        return input.to_string();
    }

    // Try parsing host:port:user:pass
    let parts: Vec<&str> = input.split(':').collect();
    if parts.len() == 4 {
        let host = parts[0];
        let port = parts[1];
        let user = parts[2];
        let pass = parts[3];
        return format!("http://{user}:{pass}@{host}:{port}");
    }

    // Try host:port
    if parts.len() == 2 {
        return format!("http://{}", input);
    }

    input.to_string()
}
