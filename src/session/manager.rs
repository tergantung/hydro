//! SessionManager: top-level handle that owns the per-session BotSession map.

use std::sync::Arc;
use std::sync::atomic::Ordering as AtomicOrdering;

use dashmap::DashMap;

use crate::logging::Logger;
use crate::lua_runtime::{self, LuaScriptHandle};
use crate::models::{AuthInput, LuaScriptStatusSnapshot, SessionSnapshot};

use super::SESSION_COUNTER;
use super::bot_session::BotSession;

#[derive(Debug, Clone)]
pub struct SessionManager {
    sessions: Arc<DashMap<String, Arc<BotSession>>>,
    lua_scripts: Arc<DashMap<String, LuaScriptHandle>>,
    logger: Logger,
}

impl SessionManager {
    pub fn new(logger: Logger) -> Self {
        Self {
            sessions: Arc::new(DashMap::new()),
            lua_scripts: Arc::new(DashMap::new()),
            logger,
        }
    }

    pub async fn create_session(&self, auth: AuthInput, proxy: Option<String>) -> Arc<BotSession> {
        let id = format!(
            "session-{}",
            SESSION_COUNTER.fetch_add(1, AtomicOrdering::Relaxed)
        );
        let session = BotSession::new(id.clone(), auth, self.logger.clone(), proxy).await;
        self.sessions.insert(id, session.clone());
        session
    }

    pub async fn get_session(&self, id: &str) -> Option<Arc<BotSession>> {
        self.sessions.get(id).map(|entry| entry.clone())
    }

    pub async fn list_snapshots(&self) -> Vec<SessionSnapshot> {
        let sessions: Vec<Arc<BotSession>> = self
            .sessions
            .iter()
            .map(|entry| entry.value().clone())
            .collect();
        let mut items = Vec::with_capacity(sessions.len());
        for session in sessions {
            items.push(session.snapshot().await);
        }
        items
    }

    pub async fn start_lua_script(
        &self,
        session_id: &str,
        source: String,
    ) -> Result<LuaScriptStatusSnapshot, String> {
        let session = self
            .get_session(session_id)
            .await
            .ok_or_else(|| "session not found".to_string())?;
        self.stop_lua_script(session_id).await?;
        let handle = lua_runtime::spawn_script(session, source, self.logger.clone());
        let status = handle.status.read().await.clone();
        self.lua_scripts.insert(session_id.to_string(), handle);
        Ok(status)
    }

    pub async fn stop_lua_script(
        &self,
        session_id: &str,
    ) -> Result<LuaScriptStatusSnapshot, String> {
        if let Some((_, handle)) = self.lua_scripts.remove(session_id) {
            handle.cancel.store(true, AtomicOrdering::Relaxed);
            handle.task.abort();
            Ok(handle.status.read().await.clone())
        } else {
            Ok(lua_runtime::idle_status())
        }
    }

    pub async fn lua_script_status(
        &self,
        session_id: &str,
    ) -> Result<LuaScriptStatusSnapshot, String> {
        self.get_session(session_id)
            .await
            .ok_or_else(|| "session not found".to_string())?;
        let status = if let Some(entry) = self.lua_scripts.get(session_id) {
            Some(entry.status.clone())
        } else {
            None
        };
        match status {
            Some(status) => Ok(status.read().await.clone()),
            None => Ok(lua_runtime::idle_status()),
        }
    }

    pub async fn delete_session(&self, session_id: &str) -> Result<(), String> {
        // Stop any running lua script first
        let _ = self.stop_lua_script(session_id).await;
        // Remove the session from our active map
        if self.sessions.remove(session_id).is_some() {
            Ok(())
        } else {
            Err(format!("Session {} not found", session_id))
        }
    }
}
