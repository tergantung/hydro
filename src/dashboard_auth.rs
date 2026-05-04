use std::{
    collections::HashSet,
    fs,
    path::PathBuf,
    sync::Arc,
};

use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
};
use rand_core::OsRng;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredDashboardUser {
    password_hash: String,
}

#[derive(Debug, Default)]
struct DashboardAuthState {
    user: Option<StoredDashboardUser>,
    active_tokens: HashSet<String>,
}

#[derive(Debug, Clone)]
pub struct DashboardAuthManager {
    state: Arc<RwLock<DashboardAuthState>>,
    user_path: PathBuf,
}

#[derive(Debug, Clone, Copy)]
pub struct DashboardAuthStatus {
    pub registered: bool,
    pub authenticated: bool,
}

impl DashboardAuthManager {
    pub fn new(user_path: PathBuf) -> Result<Self, String> {
        let user = if user_path.exists() {
            let contents = fs::read_to_string(&user_path).map_err(|error| {
                format!("failed to read {}: {error}", user_path.display())
            })?;
            Some(serde_json::from_str(&contents).map_err(|error| {
                format!("failed to parse {}: {error}", user_path.display())
            })?)
        } else {
            None
        };

        Ok(Self {
            state: Arc::new(RwLock::new(DashboardAuthState {
                user,
                active_tokens: HashSet::new(),
            })),
            user_path,
        })
    }

    pub async fn status(&self, token: Option<&str>) -> DashboardAuthStatus {
        let state = self.state.read().await;
        DashboardAuthStatus {
            registered: state.user.is_some(),
            authenticated: token
                .filter(|value| !value.trim().is_empty())
                .map(|value| state.active_tokens.contains(value))
                .unwrap_or(false),
        }
    }

    pub async fn register(&self, password: String) -> Result<String, String> {
        validate_password(&password)?;

        let user_path = self.user_path.clone();
        let mut state = self.state.write().await;
        if state.user.is_some() {
            return Err("dashboard password already exists".to_string());
        }

        let stored_user =
            tokio::task::spawn_blocking(move || create_user_record(password, user_path))
                .await
                .map_err(|error| error.to_string())??;

        let token = generate_session_token();
        state.user = Some(stored_user);
        state.active_tokens.clear();
        state.active_tokens.insert(token.clone());
        Ok(token)
    }

    pub async fn login(&self, password: String) -> Result<String, String> {
        let stored_user = {
            let state = self.state.read().await;
            state
                .user
                .clone()
                .ok_or_else(|| "dashboard password is not set yet".to_string())?
        };

        tokio::task::spawn_blocking(move || verify_user_password(password, stored_user.password_hash))
            .await
            .map_err(|error| error.to_string())??;

        let token = generate_session_token();
        self.state.write().await.active_tokens.insert(token.clone());
        Ok(token)
    }

    pub async fn logout(&self, token: &str) {
        self.state.write().await.active_tokens.remove(token);
    }

    pub async fn is_authorized(&self, token: Option<&str>) -> bool {
        self.status(token).await.authenticated
    }
}

fn create_user_record(password: String, user_path: PathBuf) -> Result<StoredDashboardUser, String> {
    let salt = SaltString::generate(&mut OsRng);
    let password_hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|error| error.to_string())?
        .to_string();

    let stored_user = StoredDashboardUser { password_hash };

    let contents = serde_json::to_string_pretty(&stored_user).map_err(|error| error.to_string())?;
    fs::write(&user_path, contents)
        .map_err(|error| format!("failed to write {}: {error}", user_path.display()))?;

    Ok(stored_user)
}

fn verify_user_password(password: String, password_hash: String) -> Result<(), String> {
    let parsed_hash = PasswordHash::new(&password_hash).map_err(|error| error.to_string())?;
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .map_err(|_| "invalid password".to_string())
}

fn validate_password(password: &str) -> Result<(), String> {
    if password.trim().is_empty() {
        return Err("password is required".to_string());
    }
    if password.len() < 8 {
        return Err("password must be at least 8 characters".to_string());
    }
    Ok(())
}

fn generate_session_token() -> String {
    let bytes: [u8; 32] = rand::random();
    let mut output = String::with_capacity(64);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(&mut output, "{byte:02x}");
    }
    output
}
