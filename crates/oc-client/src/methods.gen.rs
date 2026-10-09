// Generated file — DO NOT EDIT by hand.
// Regenerate with: cargo run -p openapi-gen
//! Namespaced oc-client methods for all HttpApi operations.
//! Mirrors the v2 SDK namespaces; every method maps 1:1 onto a route in
//! `oc_server::routes_gen::ROUTES` (operationId parity, 141/141).
#![allow(clippy::too_many_lines, clippy::struct_excessive_bools)]
#![allow(unused_mut, clippy::must_use_candidate, clippy::vec_init_then_push)]
#![allow(clippy::doc_markdown)]

/// Every operationId covered by this client (sorted).
pub static OPERATION_IDS: &[&str] = &[
    "agent.get",
    "agent.list",
    "command.list",
    "config.get",
    "config.shells",
    "credential.activate",
    "credential.create",
    "credential.list",
    "credential.remove",
    "credential.update",
    "debug.location.evict",
    "debug.location.list",
    "event.subscribe",
    "experimental.config.update",
    "experimental.fs.write",
    "experimental.generate.text",
    "experimental.integration.wellknown.add",
    "experimental.mcp.add",
    "experimental.mcp.connect",
    "experimental.mcp.disconnect",
    "experimental.mcp.remove",
    "experimental.migration.v1.status",
    "experimental.session.export",
    "experimental.session.import",
    "experimental.session.instructions.entry.list",
    "experimental.session.instructions.entry.put",
    "experimental.session.instructions.entry.remove",
    "experimental.session.skill",
    "experimental.session.stats",
    "experimental.session.wait",
    "form.list",
    "fs.find",
    "fs.list",
    "fs.read",
    "integration.command.cancel",
    "integration.command.connect",
    "integration.command.status",
    "integration.connect.key",
    "integration.get",
    "integration.list",
    "integration.oauth.cancel",
    "integration.oauth.complete",
    "integration.oauth.connect",
    "integration.oauth.status",
    "location.get",
    "location.reload",
    "mcp.list",
    "mcp.resource.catalog",
    "model.default",
    "model.list",
    "permission.request.list",
    "permission.saved.list",
    "permission.saved.remove",
    "persistentPty.connect",
    "plugin.check",
    "plugin.list",
    "plugin.update",
    "project.list",
    "project.update",
    "provider.get",
    "provider.list",
    "pty.connect",
    "pty.connect.token",
    "pty.create",
    "pty.get",
    "pty.list",
    "pty.remove",
    "pty.update",
    "reference.list",
    "rpc.call",
    "server.connect",
    "server.experimental.persistentPty.connectToken",
    "server.experimental.persistentPty.create",
    "server.experimental.persistentPty.get",
    "server.experimental.persistentPty.handoff",
    "server.experimental.persistentPty.list",
    "server.experimental.persistentPty.read",
    "server.experimental.persistentPty.remove",
    "server.experimental.persistentPty.shutdown",
    "server.experimental.persistentPty.snapshot",
    "server.experimental.persistentPty.update",
    "server.info",
    "server.pair",
    "session.active",
    "session.background",
    "session.command",
    "session.compact",
    "session.context",
    "session.create",
    "session.diff",
    "session.environment",
    "session.fork",
    "session.form.cancel",
    "session.form.create",
    "session.form.get",
    "session.form.list",
    "session.form.reply",
    "session.generate",
    "session.get",
    "session.inbox.cancel",
    "session.inbox.list",
    "session.inbox.update",
    "session.interrupt",
    "session.list",
    "session.log",
    "session.message.get",
    "session.message.list",
    "session.move",
    "session.permission.create",
    "session.permission.get",
    "session.permission.list",
    "session.permission.reply",
    "session.prompt",
    "session.remove",
    "session.revert.clear",
    "session.revert.commit",
    "session.revert.stage",
    "session.shell",
    "session.switchAgent",
    "session.switchModel",
    "session.synthetic",
    "session.update",
    "session.view",
    "shell.create",
    "shell.get",
    "shell.list",
    "shell.output",
    "shell.remove",
    "skill.list",
    "vcs.base",
    "vcs.branch.list",
    "vcs.diff",
    "vcs.get",
    "vcs.init",
    "vcs.status",
    "websearch.providers",
    "websearch.query",
    "worktree.create",
    "worktree.list",
    "worktree.refresh",
    "worktree.remove",
];

// Namespace `agent` (mirrors the v2 SDK method group).
pub mod agent {
    pub struct AgentApi<'a> {
        pub(crate) client: &'a crate::Client,
    }

    impl AgentApi<'_> {
        /// GET /api/agent — operationId `agent.list`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded
        pub async fn list(
            &self,
            query: &crate::types::AgentListQuery,
        ) -> Result<crate::types::AgentListResponse, crate::ClientError> {
            let mut pairs: Vec<(String, String)> = Vec::new();
            if let Some(loc) = &query.location {
                if let Some(dir) = &loc.directory {
                    pairs.push(("location[directory]".to_owned(), dir.clone()));
                }
            }
            let mut hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_json(&crate::transport::Call {
                    method: reqwest::Method::GET,
                    path: "/api/agent",
                    path_params: &[],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::None,
                    success: 200,
                    declared: &[400, 401],
                })
                .await
        }

        /// GET /api/agent/{agentID} — operationId `agent.get`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `404` AgentNotFoundErrorEncoded
        pub async fn get(
            &self,
            agent_id: &str,
            query: &crate::types::AgentGetQuery,
        ) -> Result<crate::types::AgentGetResponse, crate::ClientError> {
            let mut pairs: Vec<(String, String)> = Vec::new();
            if let Some(loc) = &query.location {
                if let Some(dir) = &loc.directory {
                    pairs.push(("location[directory]".to_owned(), dir.clone()));
                }
            }
            let mut hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_json(&crate::transport::Call {
                    method: reqwest::Method::GET,
                    path: "/api/agent/{agentID}",
                    path_params: &[("agentID", agent_id)],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::None,
                    success: 200,
                    declared: &[400, 401, 404],
                })
                .await
        }
    }

    impl crate::Client {
        /// Namespace `agent` operations.
        #[must_use]
        pub fn agent(&self) -> AgentApi<'_> {
            AgentApi { client: self }
        }
    }
}

// Namespace `command` (mirrors the v2 SDK method group).
pub mod command {
    pub struct CommandApi<'a> {
        pub(crate) client: &'a crate::Client,
    }

    impl CommandApi<'_> {
        /// GET /api/command — operationId `command.list`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded
        pub async fn list(
            &self,
            query: &crate::types::CommandListQuery,
        ) -> Result<crate::types::CommandListResponse, crate::ClientError> {
            let mut pairs: Vec<(String, String)> = Vec::new();
            if let Some(loc) = &query.location {
                if let Some(dir) = &loc.directory {
                    pairs.push(("location[directory]".to_owned(), dir.clone()));
                }
            }
            let mut hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_json(&crate::transport::Call {
                    method: reqwest::Method::GET,
                    path: "/api/command",
                    path_params: &[],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::None,
                    success: 200,
                    declared: &[400, 401],
                })
                .await
        }
    }

    impl crate::Client {
        /// Namespace `command` operations.
        #[must_use]
        pub fn command(&self) -> CommandApi<'_> {
            CommandApi { client: self }
        }
    }
}

// Namespace `config` (mirrors the v2 SDK method group).
pub mod config {
    pub struct ConfigApi<'a> {
        pub(crate) client: &'a crate::Client,
    }

    impl ConfigApi<'_> {
        /// GET /api/config — operationId `config.get`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded
        pub async fn get(
            &self,
            query: &crate::types::ConfigGetQuery,
        ) -> Result<Vec<crate::types::ConfigEntry>, crate::ClientError> {
            let mut pairs: Vec<(String, String)> = Vec::new();
            if let Some(loc) = &query.location {
                if let Some(dir) = &loc.directory {
                    pairs.push(("location[directory]".to_owned(), dir.clone()));
                }
            }
            let mut hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_json(&crate::transport::Call {
                    method: reqwest::Method::GET,
                    path: "/api/config",
                    path_params: &[],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::None,
                    success: 200,
                    declared: &[400, 401],
                })
                .await
        }

        /// GET /api/config/shell — operationId `config.shells`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded
        pub async fn shells(
            &self,
        ) -> Result<Vec<crate::types::ConfigShellOption>, crate::ClientError> {
            let pairs: Vec<(String, String)> = Vec::new();
            let hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_json(&crate::transport::Call {
                    method: reqwest::Method::GET,
                    path: "/api/config/shell",
                    path_params: &[],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::None,
                    success: 200,
                    declared: &[400, 401],
                })
                .await
        }

        /// PATCH /api/experimental/config — operationId `experimental.config.update`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded
        pub async fn update(
            &self,
            body: &crate::types::ConfigPatch,
        ) -> Result<(), crate::ClientError> {
            let pairs: Vec<(String, String)> = Vec::new();
            let hdrs: Vec<(&'static str, String)> = Vec::new();
            let body_value = crate::transport::encode_body(body)?;
            self.client
                .execute_empty(&crate::transport::Call {
                    method: reqwest::Method::PATCH,
                    path: "/api/experimental/config",
                    path_params: &[],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::Json(&body_value),
                    success: 204,
                    declared: &[400, 401],
                })
                .await
        }
    }

    impl crate::Client {
        /// Namespace `config` operations.
        #[must_use]
        pub fn config(&self) -> ConfigApi<'_> {
            ConfigApi { client: self }
        }
    }
}

// Namespace `credential` (mirrors the v2 SDK method group).
pub mod credential {
    pub struct CredentialApi<'a> {
        pub(crate) client: &'a crate::Client,
    }

    impl CredentialApi<'_> {
        /// GET /api/credential — operationId `credential.list`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded
        /// The `{{ data }}` envelope is unwrapped (v2 client parity).
        pub async fn list(&self) -> Result<Vec<crate::types::CredentialEntry>, crate::ClientError> {
            let pairs: Vec<(String, String)> = Vec::new();
            let hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_json::<crate::transport::Envelope<Vec<crate::types::CredentialEntry>>>(
                    &crate::transport::Call {
                        method: reqwest::Method::GET,
                        path: "/api/credential",
                        path_params: &[],
                        query: &pairs,
                        headers: &hdrs,
                        body: crate::transport::Body::None,
                        success: 200,
                        declared: &[400, 401],
                    },
                )
                .await
                .map(|env| env.data)
        }

        /// POST /api/credential — operationId `credential.create`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `409` ConflictErrorEncoded
        /// The `{{ data }}` envelope is unwrapped (v2 client parity).
        pub async fn create(
            &self,
            body: &crate::types::CredentialCreateInput,
        ) -> Result<crate::types::CredentialEntry, crate::ClientError> {
            let pairs: Vec<(String, String)> = Vec::new();
            let hdrs: Vec<(&'static str, String)> = Vec::new();
            let body_value = crate::transport::encode_body(body)?;
            self.client
                .execute_json::<crate::transport::Envelope<crate::types::CredentialEntry>>(
                    &crate::transport::Call {
                        method: reqwest::Method::POST,
                        path: "/api/credential",
                        path_params: &[],
                        query: &pairs,
                        headers: &hdrs,
                        body: crate::transport::Body::Json(&body_value),
                        success: 200,
                        declared: &[400, 401, 409],
                    },
                )
                .await
                .map(|env| env.data)
        }

        /// DELETE /api/credential/{credentialID} — operationId `credential.remove`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded
        pub async fn remove(&self, credential_id: &str) -> Result<(), crate::ClientError> {
            let pairs: Vec<(String, String)> = Vec::new();
            let hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_empty(&crate::transport::Call {
                    method: reqwest::Method::DELETE,
                    path: "/api/credential/{credentialID}",
                    path_params: &[("credentialID", credential_id)],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::None,
                    success: 204,
                    declared: &[400, 401],
                })
                .await
        }

        /// PATCH /api/credential/{credentialID} — operationId `credential.update`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded
        pub async fn update(
            &self,
            credential_id: &str,
            body: &crate::types::CredentialUpdateBody,
        ) -> Result<(), crate::ClientError> {
            let pairs: Vec<(String, String)> = Vec::new();
            let hdrs: Vec<(&'static str, String)> = Vec::new();
            let body_value = crate::transport::encode_body(body)?;
            self.client
                .execute_empty(&crate::transport::Call {
                    method: reqwest::Method::PATCH,
                    path: "/api/credential/{credentialID}",
                    path_params: &[("credentialID", credential_id)],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::Json(&body_value),
                    success: 204,
                    declared: &[400, 401],
                })
                .await
        }

        /// POST /api/credential/{credentialID}/activate — operationId `credential.activate`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded
        pub async fn activate(&self, credential_id: &str) -> Result<(), crate::ClientError> {
            let pairs: Vec<(String, String)> = Vec::new();
            let hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_empty(&crate::transport::Call {
                    method: reqwest::Method::POST,
                    path: "/api/credential/{credentialID}/activate",
                    path_params: &[("credentialID", credential_id)],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::None,
                    success: 204,
                    declared: &[400, 401],
                })
                .await
        }
    }

    impl crate::Client {
        /// Namespace `credential` operations.
        #[must_use]
        pub fn credential(&self) -> CredentialApi<'_> {
            CredentialApi { client: self }
        }
    }
}

// Namespace `debug` (mirrors the v2 SDK method group).
pub mod debug {
    // Namespace `debug::location` (mirrors the v2 SDK method group).
    pub mod location {
        pub struct DebugLocationApi<'a> {
            pub(crate) client: &'a crate::Client,
        }

        impl DebugLocationApi<'_> {
            /// DELETE /api/debug/location — operationId `debug.location.evict`
            /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded
            pub async fn evict(
                &self,
                query: &crate::types::DebugLocationEvictQuery,
            ) -> Result<(), crate::ClientError> {
                let mut pairs: Vec<(String, String)> = Vec::new();
                if let Some(loc) = &query.location {
                    if let Some(dir) = &loc.directory {
                        pairs.push(("location[directory]".to_owned(), dir.clone()));
                    }
                }
                let mut hdrs: Vec<(&'static str, String)> = Vec::new();
                self.client
                    .execute_empty(&crate::transport::Call {
                        method: reqwest::Method::DELETE,
                        path: "/api/debug/location",
                        path_params: &[],
                        query: &pairs,
                        headers: &hdrs,
                        body: crate::transport::Body::None,
                        success: 204,
                        declared: &[400, 401],
                    })
                    .await
            }

            /// GET /api/debug/location — operationId `debug.location.list`
            /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded
            pub async fn list(
                &self,
            ) -> Result<Vec<crate::types::LocationPublicRef>, crate::ClientError> {
                let pairs: Vec<(String, String)> = Vec::new();
                let hdrs: Vec<(&'static str, String)> = Vec::new();
                self.client
                    .execute_json(&crate::transport::Call {
                        method: reqwest::Method::GET,
                        path: "/api/debug/location",
                        path_params: &[],
                        query: &pairs,
                        headers: &hdrs,
                        body: crate::transport::Body::None,
                        success: 200,
                        declared: &[400, 401],
                    })
                    .await
            }
        }
    }

    pub struct DebugApi<'a> {
        pub(crate) client: &'a crate::Client,
    }

    impl DebugApi<'_> {
        /// Nested namespace `debug::location`.
        #[must_use]
        pub fn location(&self) -> location::DebugLocationApi<'_> {
            location::DebugLocationApi { client: self.client }
        }
    }

    impl crate::Client {
        /// Namespace `debug` operations.
        #[must_use]
        pub fn debug(&self) -> DebugApi<'_> {
            DebugApi { client: self }
        }
    }
}

// Namespace `event` (mirrors the v2 SDK method group).
pub mod event {
    pub struct EventApi<'a> {
        pub(crate) client: &'a crate::Client,
    }

    impl EventApi<'_> {
        /// GET /api/event — operationId `event.subscribe`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded
        /// SSE stream; `data` frames are JSON-in-string payloads, double-decoded by
        /// `SseStream` (server-deep-map §7).
        pub async fn subscribe(&self) -> Result<crate::sse::SseStream, crate::ClientError> {
            let pairs: Vec<(String, String)> = Vec::new();
            let hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_sse(
                    &crate::transport::Call {
                        method: reqwest::Method::GET,
                        path: "/api/event",
                        path_params: &[],
                        query: &pairs,
                        headers: &hdrs,
                        body: crate::transport::Body::None,
                        success: 200,
                        declared: &[400, 401],
                    },
                    true,
                )
                .await
        }
    }

    impl crate::Client {
        /// Namespace `event` operations.
        #[must_use]
        pub fn event(&self) -> EventApi<'_> {
            EventApi { client: self }
        }
    }
}

// Namespace `form` (mirrors the v2 SDK method group).
pub mod form {
    pub struct FormApi<'a> {
        pub(crate) client: &'a crate::Client,
    }

    impl FormApi<'_> {
        /// GET /api/form — operationId `form.list`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded
        pub async fn list(
            &self,
            query: &crate::types::FormListQuery,
        ) -> Result<crate::types::FormListResponse, crate::ClientError> {
            let mut pairs: Vec<(String, String)> = Vec::new();
            if let Some(loc) = &query.location {
                if let Some(dir) = &loc.directory {
                    pairs.push(("location[directory]".to_owned(), dir.clone()));
                }
            }
            let mut hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_json(&crate::transport::Call {
                    method: reqwest::Method::GET,
                    path: "/api/form",
                    path_params: &[],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::None,
                    success: 200,
                    declared: &[400, 401],
                })
                .await
        }
    }

    impl crate::Client {
        /// Namespace `form` operations.
        #[must_use]
        pub fn form(&self) -> FormApi<'_> {
            FormApi { client: self }
        }
    }
}

// Namespace `fs` (mirrors the v2 SDK method group).
pub mod fs {
    pub struct FsApi<'a> {
        pub(crate) client: &'a crate::Client,
    }

    impl FsApi<'_> {
        /// POST /api/experimental/fs/write — operationId `experimental.fs.write`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded
        /// Binary/octet-stream endpoint: bytes pass through untyped (TODO: mime codecs).
        pub async fn write(
            &self,
            query: &crate::types::ExperimentalFsWriteQuery,
            payload: &[u8],
        ) -> Result<crate::types::ExperimentalFsWriteResponse, crate::ClientError> {
            let mut pairs: Vec<(String, String)> = Vec::new();
            if let Some(loc) = &query.location {
                if let Some(dir) = &loc.directory {
                    pairs.push(("location[directory]".to_owned(), dir.clone()));
                }
            }
            pairs.push(("path".to_owned(), query.path.clone()));
            let mut hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_json(&crate::transport::Call {
                    method: reqwest::Method::POST,
                    path: "/api/experimental/fs/write",
                    path_params: &[],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::Bytes(payload),
                    success: 200,
                    declared: &[400, 401],
                })
                .await
        }

        /// GET /api/fs/find — operationId `fs.find`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded
        pub async fn find(
            &self,
            query: &crate::types::FsFindQuery,
        ) -> Result<crate::types::FsFindResponse, crate::ClientError> {
            let mut pairs: Vec<(String, String)> = Vec::new();
            if let Some(v) = &query.limit {
                pairs.push(("limit".to_owned(), v.clone()));
            }
            if let Some(loc) = &query.location {
                if let Some(dir) = &loc.directory {
                    pairs.push(("location[directory]".to_owned(), dir.clone()));
                }
            }
            pairs.push(("query".to_owned(), query.query.clone()));
            if let Some(v) = &query.type_ {
                pairs.push(("type".to_owned(), v.as_str().to_owned()));
            }
            let mut hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_json(&crate::transport::Call {
                    method: reqwest::Method::GET,
                    path: "/api/fs/find",
                    path_params: &[],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::None,
                    success: 200,
                    declared: &[400, 401],
                })
                .await
        }

        /// GET /api/fs/list — operationId `fs.list`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded
        pub async fn list(
            &self,
            query: &crate::types::FsListQuery,
        ) -> Result<crate::types::FsListResponse, crate::ClientError> {
            let mut pairs: Vec<(String, String)> = Vec::new();
            if let Some(loc) = &query.location {
                if let Some(dir) = &loc.directory {
                    pairs.push(("location[directory]".to_owned(), dir.clone()));
                }
            }
            if let Some(v) = &query.path {
                pairs.push(("path".to_owned(), v.clone()));
            }
            let mut hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_json(&crate::transport::Call {
                    method: reqwest::Method::GET,
                    path: "/api/fs/list",
                    path_params: &[],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::None,
                    success: 200,
                    declared: &[400, 401],
                })
                .await
        }

        /// GET /api/fs/read/* — operationId `fs.read`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `404` FileNotFoundErrorEncoded
        /// Binary/octet-stream endpoint: bytes pass through untyped (TODO: mime codecs).
        pub async fn read(
            &self,
            path: &str,
            query: &crate::types::FsReadQuery,
        ) -> Result<Vec<u8>, crate::ClientError> {
            let mut pairs: Vec<(String, String)> = Vec::new();
            if let Some(loc) = &query.location {
                if let Some(dir) = &loc.directory {
                    pairs.push(("location[directory]".to_owned(), dir.clone()));
                }
            }
            let mut hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_bytes(&crate::transport::Call {
                    method: reqwest::Method::GET,
                    path: "/api/fs/read/{*path}",
                    path_params: &[("path", path)],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::None,
                    success: 200,
                    declared: &[400, 401, 404],
                })
                .await
        }
    }

    impl crate::Client {
        /// Namespace `fs` operations.
        #[must_use]
        pub fn fs(&self) -> FsApi<'_> {
            FsApi { client: self }
        }
    }
}

// Namespace `generate` (mirrors the v2 SDK method group).
pub mod generate {
    pub struct GenerateApi<'a> {
        pub(crate) client: &'a crate::Client,
    }

    impl GenerateApi<'_> {
        /// POST /api/experimental/generate — operationId `experimental.generate.text`
        /// Declared errors: `400` unknown, `401` UnauthorizedErrorEncoded, `503` ServiceUnavailableErrorEncoded
        pub async fn text(
            &self,
            body: &crate::types::ExperimentalGenerateTextBody,
        ) -> Result<crate::types::GenerateTextResponse, crate::ClientError> {
            let pairs: Vec<(String, String)> = Vec::new();
            let hdrs: Vec<(&'static str, String)> = Vec::new();
            let body_value = crate::transport::encode_body(body)?;
            self.client
                .execute_json(&crate::transport::Call {
                    method: reqwest::Method::POST,
                    path: "/api/experimental/generate",
                    path_params: &[],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::Json(&body_value),
                    success: 200,
                    declared: &[400, 401, 503],
                })
                .await
        }
    }

    impl crate::Client {
        /// Namespace `generate` operations.
        #[must_use]
        pub fn generate(&self) -> GenerateApi<'_> {
            GenerateApi { client: self }
        }
    }
}

// Namespace `integration` (mirrors the v2 SDK method group).
pub mod integration {
    // Namespace `integration::command` (mirrors the v2 SDK method group).
    pub mod command {
        pub struct IntegrationCommandApi<'a> {
            pub(crate) client: &'a crate::Client,
        }

        impl IntegrationCommandApi<'_> {
            /// POST /api/integration/{integrationID}/connect/command — operationId `integration.command.connect`
            /// Declared errors: `400` unknown, `401` UnauthorizedErrorEncoded, `404` unknown
            pub async fn connect(
                &self,
                integration_id: &str,
                query: &crate::types::IntegrationCommandConnectQuery,
                body: &crate::types::IntegrationCommandConnectBody,
            ) -> Result<crate::types::IntegrationCommandConnectResponse, crate::ClientError>
            {
                let mut pairs: Vec<(String, String)> = Vec::new();
                if let Some(loc) = &query.location {
                    if let Some(dir) = &loc.directory {
                        pairs.push(("location[directory]".to_owned(), dir.clone()));
                    }
                }
                let mut hdrs: Vec<(&'static str, String)> = Vec::new();
                let body_value = crate::transport::encode_body(body)?;
                self.client
                    .execute_json(&crate::transport::Call {
                        method: reqwest::Method::POST,
                        path: "/api/integration/{integrationID}/connect/command",
                        path_params: &[("integrationID", integration_id)],
                        query: &pairs,
                        headers: &hdrs,
                        body: crate::transport::Body::Json(&body_value),
                        success: 200,
                        declared: &[400, 401, 404],
                    })
                    .await
            }

            /// DELETE /api/integration/{integrationID}/connect/command/{attemptID} — operationId `integration.command.cancel`
            /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded
            pub async fn cancel(
                &self,
                integration_id: &str,
                attempt_id: &str,
                query: &crate::types::IntegrationCommandCancelQuery,
            ) -> Result<(), crate::ClientError> {
                let mut pairs: Vec<(String, String)> = Vec::new();
                if let Some(loc) = &query.location {
                    if let Some(dir) = &loc.directory {
                        pairs.push(("location[directory]".to_owned(), dir.clone()));
                    }
                }
                let mut hdrs: Vec<(&'static str, String)> = Vec::new();
                self.client
                    .execute_empty(&crate::transport::Call {
                        method: reqwest::Method::DELETE,
                        path: "/api/integration/{integrationID}/connect/command/{attemptID}",
                        path_params: &[
                            ("integrationID", integration_id),
                            ("attemptID", attempt_id),
                        ],
                        query: &pairs,
                        headers: &hdrs,
                        body: crate::transport::Body::None,
                        success: 204,
                        declared: &[400, 401],
                    })
                    .await
            }

            /// GET /api/integration/{integrationID}/connect/command/{attemptID} — operationId `integration.command.status`
            /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `404` unknown
            pub async fn status(
                &self,
                integration_id: &str,
                attempt_id: &str,
                query: &crate::types::IntegrationCommandStatusQuery,
            ) -> Result<crate::types::IntegrationCommandStatusResponse, crate::ClientError>
            {
                let mut pairs: Vec<(String, String)> = Vec::new();
                if let Some(loc) = &query.location {
                    if let Some(dir) = &loc.directory {
                        pairs.push(("location[directory]".to_owned(), dir.clone()));
                    }
                }
                let mut hdrs: Vec<(&'static str, String)> = Vec::new();
                self.client
                    .execute_json(&crate::transport::Call {
                        method: reqwest::Method::GET,
                        path: "/api/integration/{integrationID}/connect/command/{attemptID}",
                        path_params: &[
                            ("integrationID", integration_id),
                            ("attemptID", attempt_id),
                        ],
                        query: &pairs,
                        headers: &hdrs,
                        body: crate::transport::Body::None,
                        success: 200,
                        declared: &[400, 401, 404],
                    })
                    .await
            }
        }
    }

    // Namespace `integration::connect` (mirrors the v2 SDK method group).
    pub mod connect {
        pub struct IntegrationConnectApi<'a> {
            pub(crate) client: &'a crate::Client,
        }

        impl IntegrationConnectApi<'_> {
            /// POST /api/integration/{integrationID}/connect/key — operationId `integration.connect.key`
            /// Declared errors: `400` unknown, `401` UnauthorizedErrorEncoded, `404` IntegrationNotFoundErrorEncoded
            pub async fn key(
                &self,
                integration_id: &str,
                query: &crate::types::IntegrationConnectKeyQuery,
                body: &crate::types::IntegrationConnectKeyBody,
            ) -> Result<(), crate::ClientError> {
                let mut pairs: Vec<(String, String)> = Vec::new();
                if let Some(loc) = &query.location {
                    if let Some(dir) = &loc.directory {
                        pairs.push(("location[directory]".to_owned(), dir.clone()));
                    }
                }
                let mut hdrs: Vec<(&'static str, String)> = Vec::new();
                let body_value = crate::transport::encode_body(body)?;
                self.client
                    .execute_empty(&crate::transport::Call {
                        method: reqwest::Method::POST,
                        path: "/api/integration/{integrationID}/connect/key",
                        path_params: &[("integrationID", integration_id)],
                        query: &pairs,
                        headers: &hdrs,
                        body: crate::transport::Body::Json(&body_value),
                        success: 204,
                        declared: &[400, 401, 404],
                    })
                    .await
            }
        }
    }

    // Namespace `integration::oauth` (mirrors the v2 SDK method group).
    pub mod oauth {
        pub struct IntegrationOauthApi<'a> {
            pub(crate) client: &'a crate::Client,
        }

        impl IntegrationOauthApi<'_> {
            /// POST /api/integration/{integrationID}/connect/oauth — operationId `integration.oauth.connect`
            /// Declared errors: `400` unknown, `401` UnauthorizedErrorEncoded
            pub async fn connect(
                &self,
                integration_id: &str,
                query: &crate::types::IntegrationOauthConnectQuery,
                body: &crate::types::IntegrationOauthConnectBody,
            ) -> Result<crate::types::IntegrationOauthConnectResponse, crate::ClientError>
            {
                let mut pairs: Vec<(String, String)> = Vec::new();
                if let Some(loc) = &query.location {
                    if let Some(dir) = &loc.directory {
                        pairs.push(("location[directory]".to_owned(), dir.clone()));
                    }
                }
                let mut hdrs: Vec<(&'static str, String)> = Vec::new();
                let body_value = crate::transport::encode_body(body)?;
                self.client
                    .execute_json(&crate::transport::Call {
                        method: reqwest::Method::POST,
                        path: "/api/integration/{integrationID}/connect/oauth",
                        path_params: &[("integrationID", integration_id)],
                        query: &pairs,
                        headers: &hdrs,
                        body: crate::transport::Body::Json(&body_value),
                        success: 200,
                        declared: &[400, 401],
                    })
                    .await
            }

            /// DELETE /api/integration/{integrationID}/connect/oauth/{attemptID} — operationId `integration.oauth.cancel`
            /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded
            pub async fn cancel(
                &self,
                integration_id: &str,
                attempt_id: &str,
                query: &crate::types::IntegrationOauthCancelQuery,
            ) -> Result<(), crate::ClientError> {
                let mut pairs: Vec<(String, String)> = Vec::new();
                if let Some(loc) = &query.location {
                    if let Some(dir) = &loc.directory {
                        pairs.push(("location[directory]".to_owned(), dir.clone()));
                    }
                }
                let mut hdrs: Vec<(&'static str, String)> = Vec::new();
                self.client
                    .execute_empty(&crate::transport::Call {
                        method: reqwest::Method::DELETE,
                        path: "/api/integration/{integrationID}/connect/oauth/{attemptID}",
                        path_params: &[
                            ("integrationID", integration_id),
                            ("attemptID", attempt_id),
                        ],
                        query: &pairs,
                        headers: &hdrs,
                        body: crate::transport::Body::None,
                        success: 204,
                        declared: &[400, 401],
                    })
                    .await
            }

            /// GET /api/integration/{integrationID}/connect/oauth/{attemptID} — operationId `integration.oauth.status`
            /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `404` unknown
            pub async fn status(
                &self,
                integration_id: &str,
                attempt_id: &str,
                query: &crate::types::IntegrationOauthStatusQuery,
            ) -> Result<crate::types::IntegrationOauthStatusResponse, crate::ClientError>
            {
                let mut pairs: Vec<(String, String)> = Vec::new();
                if let Some(loc) = &query.location {
                    if let Some(dir) = &loc.directory {
                        pairs.push(("location[directory]".to_owned(), dir.clone()));
                    }
                }
                let mut hdrs: Vec<(&'static str, String)> = Vec::new();
                self.client
                    .execute_json(&crate::transport::Call {
                        method: reqwest::Method::GET,
                        path: "/api/integration/{integrationID}/connect/oauth/{attemptID}",
                        path_params: &[
                            ("integrationID", integration_id),
                            ("attemptID", attempt_id),
                        ],
                        query: &pairs,
                        headers: &hdrs,
                        body: crate::transport::Body::None,
                        success: 200,
                        declared: &[400, 401, 404],
                    })
                    .await
            }

            /// POST /api/integration/{integrationID}/connect/oauth/{attemptID}/complete — operationId `integration.oauth.complete`
            /// Declared errors: `400` unknown, `401` UnauthorizedErrorEncoded, `404` unknown
            pub async fn complete(
                &self,
                integration_id: &str,
                attempt_id: &str,
                query: &crate::types::IntegrationOauthCompleteQuery,
                body: &crate::types::IntegrationOauthCompleteBody,
            ) -> Result<(), crate::ClientError> {
                let mut pairs: Vec<(String, String)> = Vec::new();
                if let Some(loc) = &query.location {
                    if let Some(dir) = &loc.directory {
                        pairs.push(("location[directory]".to_owned(), dir.clone()));
                    }
                }
                let mut hdrs: Vec<(&'static str, String)> = Vec::new();
                let body_value = crate::transport::encode_body(body)?;
                self.client
                    .execute_empty(&crate::transport::Call {
                        method: reqwest::Method::POST,
                        path: "/api/integration/{integrationID}/connect/oauth/{attemptID}/complete",
                        path_params: &[
                            ("integrationID", integration_id),
                            ("attemptID", attempt_id),
                        ],
                        query: &pairs,
                        headers: &hdrs,
                        body: crate::transport::Body::Json(&body_value),
                        success: 204,
                        declared: &[400, 401, 404],
                    })
                    .await
            }
        }
    }

    pub struct IntegrationApi<'a> {
        pub(crate) client: &'a crate::Client,
    }

    impl IntegrationApi<'_> {
        /// POST /api/experimental/integration/wellknown — operationId `experimental.integration.wellknown.add`
        /// Declared errors: `400` unknown, `401` UnauthorizedErrorEncoded
        pub async fn wellknown_add(
            &self,
            query: &crate::types::ExperimentalIntegrationWellknownAddQuery,
            body: &crate::types::ExperimentalIntegrationWellknownAddBody,
        ) -> Result<(), crate::ClientError> {
            let mut pairs: Vec<(String, String)> = Vec::new();
            if let Some(loc) = &query.location {
                if let Some(dir) = &loc.directory {
                    pairs.push(("location[directory]".to_owned(), dir.clone()));
                }
            }
            let mut hdrs: Vec<(&'static str, String)> = Vec::new();
            let body_value = crate::transport::encode_body(body)?;
            self.client
                .execute_empty(&crate::transport::Call {
                    method: reqwest::Method::POST,
                    path: "/api/experimental/integration/wellknown",
                    path_params: &[],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::Json(&body_value),
                    success: 204,
                    declared: &[400, 401],
                })
                .await
        }

        /// GET /api/integration — operationId `integration.list`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded
        pub async fn list(
            &self,
            query: &crate::types::IntegrationListQuery,
        ) -> Result<crate::types::IntegrationListResponse, crate::ClientError> {
            let mut pairs: Vec<(String, String)> = Vec::new();
            if let Some(loc) = &query.location {
                if let Some(dir) = &loc.directory {
                    pairs.push(("location[directory]".to_owned(), dir.clone()));
                }
            }
            let mut hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_json(&crate::transport::Call {
                    method: reqwest::Method::GET,
                    path: "/api/integration",
                    path_params: &[],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::None,
                    success: 200,
                    declared: &[400, 401],
                })
                .await
        }

        /// GET /api/integration/{integrationID} — operationId `integration.get`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `404` IntegrationNotFoundErrorEncoded
        pub async fn get(
            &self,
            integration_id: &str,
            query: &crate::types::IntegrationGetQuery,
        ) -> Result<crate::types::IntegrationGetResponse, crate::ClientError> {
            let mut pairs: Vec<(String, String)> = Vec::new();
            if let Some(loc) = &query.location {
                if let Some(dir) = &loc.directory {
                    pairs.push(("location[directory]".to_owned(), dir.clone()));
                }
            }
            let mut hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_json(&crate::transport::Call {
                    method: reqwest::Method::GET,
                    path: "/api/integration/{integrationID}",
                    path_params: &[("integrationID", integration_id)],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::None,
                    success: 200,
                    declared: &[400, 401, 404],
                })
                .await
        }

        /// Nested namespace `integration::command`.
        #[must_use]
        pub fn command(&self) -> command::IntegrationCommandApi<'_> {
            command::IntegrationCommandApi { client: self.client }
        }
        /// Nested namespace `integration::connect`.
        #[must_use]
        pub fn connect(&self) -> connect::IntegrationConnectApi<'_> {
            connect::IntegrationConnectApi { client: self.client }
        }
        /// Nested namespace `integration::oauth`.
        #[must_use]
        pub fn oauth(&self) -> oauth::IntegrationOauthApi<'_> {
            oauth::IntegrationOauthApi { client: self.client }
        }
    }

    impl crate::Client {
        /// Namespace `integration` operations.
        #[must_use]
        pub fn integration(&self) -> IntegrationApi<'_> {
            IntegrationApi { client: self }
        }
    }
}

// Namespace `location` (mirrors the v2 SDK method group).
pub mod location {
    pub struct LocationApi<'a> {
        pub(crate) client: &'a crate::Client,
    }

    impl LocationApi<'_> {
        /// GET /api/location — operationId `location.get`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded
        pub async fn get(
            &self,
            query: &crate::types::LocationGetQuery,
        ) -> Result<crate::types::LocationPublicInfo, crate::ClientError> {
            let mut pairs: Vec<(String, String)> = Vec::new();
            if let Some(loc) = &query.location {
                if let Some(dir) = &loc.directory {
                    pairs.push(("location[directory]".to_owned(), dir.clone()));
                }
            }
            let mut hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_json(&crate::transport::Call {
                    method: reqwest::Method::GET,
                    path: "/api/location",
                    path_params: &[],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::None,
                    success: 200,
                    declared: &[400, 401],
                })
                .await
        }

        /// POST /api/location/reload — operationId `location.reload`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `503` ServiceUnavailableErrorEncoded
        pub async fn reload(&self) -> Result<(), crate::ClientError> {
            let pairs: Vec<(String, String)> = Vec::new();
            let hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_empty(&crate::transport::Call {
                    method: reqwest::Method::POST,
                    path: "/api/location/reload",
                    path_params: &[],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::None,
                    success: 204,
                    declared: &[400, 401, 503],
                })
                .await
        }
    }

    impl crate::Client {
        /// Namespace `location` operations.
        #[must_use]
        pub fn location(&self) -> LocationApi<'_> {
            LocationApi { client: self }
        }
    }
}

// Namespace `mcp` (mirrors the v2 SDK method group).
pub mod mcp {
    // Namespace `mcp::resource` (mirrors the v2 SDK method group).
    pub mod resource {
        pub struct McpResourceApi<'a> {
            pub(crate) client: &'a crate::Client,
        }

        impl McpResourceApi<'_> {
            /// GET /api/mcp/resource — operationId `mcp.resource.catalog`
            /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded
            pub async fn catalog(
                &self,
                query: &crate::types::McpResourceCatalogQuery,
            ) -> Result<crate::types::McpResourceCatalogResponse, crate::ClientError> {
                let mut pairs: Vec<(String, String)> = Vec::new();
                if let Some(loc) = &query.location {
                    if let Some(dir) = &loc.directory {
                        pairs.push(("location[directory]".to_owned(), dir.clone()));
                    }
                }
                let mut hdrs: Vec<(&'static str, String)> = Vec::new();
                self.client
                    .execute_json(&crate::transport::Call {
                        method: reqwest::Method::GET,
                        path: "/api/mcp/resource",
                        path_params: &[],
                        query: &pairs,
                        headers: &hdrs,
                        body: crate::transport::Body::None,
                        success: 200,
                        declared: &[400, 401],
                    })
                    .await
            }
        }
    }

    pub struct McpApi<'a> {
        pub(crate) client: &'a crate::Client,
    }

    impl McpApi<'_> {
        /// DELETE /api/experimental/mcp/{server} — operationId `experimental.mcp.remove`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `404` McpServerNotFoundErrorEncoded
        pub async fn remove(
            &self,
            server: &str,
            query: &crate::types::ExperimentalMcpRemoveQuery,
        ) -> Result<(), crate::ClientError> {
            let mut pairs: Vec<(String, String)> = Vec::new();
            if let Some(loc) = &query.location {
                if let Some(dir) = &loc.directory {
                    pairs.push(("location[directory]".to_owned(), dir.clone()));
                }
            }
            let mut hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_empty(&crate::transport::Call {
                    method: reqwest::Method::DELETE,
                    path: "/api/experimental/mcp/{server}",
                    path_params: &[("server", server)],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::None,
                    success: 204,
                    declared: &[400, 401, 404],
                })
                .await
        }

        /// PUT /api/experimental/mcp/{server} — operationId `experimental.mcp.add`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded
        pub async fn add(
            &self,
            server: &str,
            query: &crate::types::ExperimentalMcpAddQuery,
            body: &crate::types::ExperimentalMcpAddBody,
        ) -> Result<(), crate::ClientError> {
            let mut pairs: Vec<(String, String)> = Vec::new();
            if let Some(loc) = &query.location {
                if let Some(dir) = &loc.directory {
                    pairs.push(("location[directory]".to_owned(), dir.clone()));
                }
            }
            let mut hdrs: Vec<(&'static str, String)> = Vec::new();
            let body_value = crate::transport::encode_body(body)?;
            self.client
                .execute_empty(&crate::transport::Call {
                    method: reqwest::Method::PUT,
                    path: "/api/experimental/mcp/{server}",
                    path_params: &[("server", server)],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::Json(&body_value),
                    success: 204,
                    declared: &[400, 401],
                })
                .await
        }

        /// POST /api/experimental/mcp/{server}/connect — operationId `experimental.mcp.connect`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `404` McpServerNotFoundErrorEncoded
        pub async fn connect(
            &self,
            server: &str,
            query: &crate::types::ExperimentalMcpConnectQuery,
        ) -> Result<(), crate::ClientError> {
            let mut pairs: Vec<(String, String)> = Vec::new();
            if let Some(loc) = &query.location {
                if let Some(dir) = &loc.directory {
                    pairs.push(("location[directory]".to_owned(), dir.clone()));
                }
            }
            let mut hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_empty(&crate::transport::Call {
                    method: reqwest::Method::POST,
                    path: "/api/experimental/mcp/{server}/connect",
                    path_params: &[("server", server)],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::None,
                    success: 204,
                    declared: &[400, 401, 404],
                })
                .await
        }

        /// POST /api/experimental/mcp/{server}/disconnect — operationId `experimental.mcp.disconnect`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `404` McpServerNotFoundErrorEncoded
        pub async fn disconnect(
            &self,
            server: &str,
            query: &crate::types::ExperimentalMcpDisconnectQuery,
        ) -> Result<(), crate::ClientError> {
            let mut pairs: Vec<(String, String)> = Vec::new();
            if let Some(loc) = &query.location {
                if let Some(dir) = &loc.directory {
                    pairs.push(("location[directory]".to_owned(), dir.clone()));
                }
            }
            let mut hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_empty(&crate::transport::Call {
                    method: reqwest::Method::POST,
                    path: "/api/experimental/mcp/{server}/disconnect",
                    path_params: &[("server", server)],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::None,
                    success: 204,
                    declared: &[400, 401, 404],
                })
                .await
        }

        /// GET /api/mcp — operationId `mcp.list`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded
        pub async fn list(
            &self,
            query: &crate::types::McpListQuery,
        ) -> Result<crate::types::McpListResponse, crate::ClientError> {
            let mut pairs: Vec<(String, String)> = Vec::new();
            if let Some(loc) = &query.location {
                if let Some(dir) = &loc.directory {
                    pairs.push(("location[directory]".to_owned(), dir.clone()));
                }
            }
            let mut hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_json(&crate::transport::Call {
                    method: reqwest::Method::GET,
                    path: "/api/mcp",
                    path_params: &[],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::None,
                    success: 200,
                    declared: &[400, 401],
                })
                .await
        }

        /// Nested namespace `mcp::resource`.
        #[must_use]
        pub fn resource(&self) -> resource::McpResourceApi<'_> {
            resource::McpResourceApi { client: self.client }
        }
    }

    impl crate::Client {
        /// Namespace `mcp` operations.
        #[must_use]
        pub fn mcp(&self) -> McpApi<'_> {
            McpApi { client: self }
        }
    }
}

// Namespace `message` (mirrors the v2 SDK method group).
pub mod message {
    pub struct MessageApi<'a> {
        pub(crate) client: &'a crate::Client,
    }

    impl MessageApi<'_> {
        /// GET /api/session/{sessionID}/message — operationId `session.message.list`
        /// Declared errors: `400` unknown, `401` UnauthorizedErrorEncoded, `404` SessionNotFoundErrorEncoded, `500` UnknownErrorEncoded
        pub async fn list(
            &self,
            session_id: &str,
            query: &crate::types::SessionMessageListQuery,
        ) -> Result<crate::types::SessionMessagesResponse, crate::ClientError> {
            let mut pairs: Vec<(String, String)> = Vec::new();
            if let Some(v) = &query.cursor {
                pairs.push(("cursor".to_owned(), v.clone()));
            }
            if let Some(v) = &query.limit {
                pairs.push(("limit".to_owned(), v.clone()));
            }
            if let Some(v) = &query.order {
                pairs.push(("order".to_owned(), v.as_str().to_owned()));
            }
            if let Some(v) = &query.type_ {
                pairs.push(("type".to_owned(), v.as_str().to_owned()));
            }
            let mut hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_json(&crate::transport::Call {
                    method: reqwest::Method::GET,
                    path: "/api/session/{sessionID}/message",
                    path_params: &[("sessionID", session_id)],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::None,
                    success: 200,
                    declared: &[400, 401, 404, 500],
                })
                .await
        }

        /// GET /api/session/{sessionID}/message/{messageID} — operationId `session.message.get`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `404` unknown
        /// The `{{ data }}` envelope is unwrapped (v2 client parity).
        pub async fn get(
            &self,
            session_id: &str,
            message_id: &str,
        ) -> Result<crate::types::SessionMessageInfo, crate::ClientError> {
            let pairs: Vec<(String, String)> = Vec::new();
            let hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_json::<crate::transport::Envelope<crate::types::SessionMessageInfo>>(
                    &crate::transport::Call {
                        method: reqwest::Method::GET,
                        path: "/api/session/{sessionID}/message/{messageID}",
                        path_params: &[("sessionID", session_id), ("messageID", message_id)],
                        query: &pairs,
                        headers: &hdrs,
                        body: crate::transport::Body::None,
                        success: 200,
                        declared: &[400, 401, 404],
                    },
                )
                .await
                .map(|env| env.data)
        }
    }

    impl crate::Client {
        /// Namespace `message` operations.
        #[must_use]
        pub fn message(&self) -> MessageApi<'_> {
            MessageApi { client: self }
        }
    }
}

// Namespace `migration` (mirrors the v2 SDK method group).
pub mod migration {
    pub struct MigrationApi<'a> {
        pub(crate) client: &'a crate::Client,
    }

    impl MigrationApi<'_> {
        /// GET /api/experimental/migration/v1 — operationId `experimental.migration.v1.status`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded
        pub async fn status(
            &self,
        ) -> Result<crate::types::ExperimentalMigrationV1StatusResponse, crate::ClientError>
        {
            let pairs: Vec<(String, String)> = Vec::new();
            let hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_json(&crate::transport::Call {
                    method: reqwest::Method::GET,
                    path: "/api/experimental/migration/v1",
                    path_params: &[],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::None,
                    success: 200,
                    declared: &[400, 401],
                })
                .await
        }
    }

    impl crate::Client {
        /// Namespace `migration` operations.
        #[must_use]
        pub fn migration(&self) -> MigrationApi<'_> {
            MigrationApi { client: self }
        }
    }
}

// Namespace `model` (mirrors the v2 SDK method group).
pub mod model {
    pub struct ModelApi<'a> {
        pub(crate) client: &'a crate::Client,
    }

    impl ModelApi<'_> {
        /// GET /api/model — operationId `model.list`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `503` ServiceUnavailableErrorEncoded
        pub async fn list(
            &self,
            query: &crate::types::ModelListQuery,
        ) -> Result<crate::types::ModelListResponse, crate::ClientError> {
            let mut pairs: Vec<(String, String)> = Vec::new();
            if let Some(loc) = &query.location {
                if let Some(dir) = &loc.directory {
                    pairs.push(("location[directory]".to_owned(), dir.clone()));
                }
            }
            let mut hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_json(&crate::transport::Call {
                    method: reqwest::Method::GET,
                    path: "/api/model",
                    path_params: &[],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::None,
                    success: 200,
                    declared: &[400, 401, 503],
                })
                .await
        }

        /// GET /api/model/default — operationId `model.default`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `503` ServiceUnavailableErrorEncoded
        pub async fn default(
            &self,
            query: &crate::types::ModelDefaultQuery,
        ) -> Result<crate::types::ModelDefaultResponse, crate::ClientError> {
            let mut pairs: Vec<(String, String)> = Vec::new();
            if let Some(loc) = &query.location {
                if let Some(dir) = &loc.directory {
                    pairs.push(("location[directory]".to_owned(), dir.clone()));
                }
            }
            let mut hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_json(&crate::transport::Call {
                    method: reqwest::Method::GET,
                    path: "/api/model/default",
                    path_params: &[],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::None,
                    success: 200,
                    declared: &[400, 401, 503],
                })
                .await
        }
    }

    impl crate::Client {
        /// Namespace `model` operations.
        #[must_use]
        pub fn model(&self) -> ModelApi<'_> {
            ModelApi { client: self }
        }
    }
}

// Namespace `permission` (mirrors the v2 SDK method group).
pub mod permission {
    // Namespace `permission::request` (mirrors the v2 SDK method group).
    pub mod request {
        pub struct PermissionRequestApi<'a> {
            pub(crate) client: &'a crate::Client,
        }

        impl PermissionRequestApi<'_> {
            /// GET /api/permission/request — operationId `permission.request.list`
            /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded
            pub async fn list(
                &self,
                query: &crate::types::PermissionRequestListQuery,
            ) -> Result<crate::types::PermissionRequestListResponse, crate::ClientError>
            {
                let mut pairs: Vec<(String, String)> = Vec::new();
                if let Some(loc) = &query.location {
                    if let Some(dir) = &loc.directory {
                        pairs.push(("location[directory]".to_owned(), dir.clone()));
                    }
                }
                let mut hdrs: Vec<(&'static str, String)> = Vec::new();
                self.client
                    .execute_json(&crate::transport::Call {
                        method: reqwest::Method::GET,
                        path: "/api/permission/request",
                        path_params: &[],
                        query: &pairs,
                        headers: &hdrs,
                        body: crate::transport::Body::None,
                        success: 200,
                        declared: &[400, 401],
                    })
                    .await
            }
        }
    }

    // Namespace `permission::saved` (mirrors the v2 SDK method group).
    pub mod saved {
        pub struct PermissionSavedApi<'a> {
            pub(crate) client: &'a crate::Client,
        }

        impl PermissionSavedApi<'_> {
            /// GET /api/permission/saved — operationId `permission.saved.list`
            /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded
            /// The `{{ data }}` envelope is unwrapped (v2 client parity).
            pub async fn list(
                &self,
                query: &crate::types::PermissionSavedListQuery,
            ) -> Result<Vec<crate::types::PermissionSavedInfo>, crate::ClientError> {
                let mut pairs: Vec<(String, String)> = Vec::new();
                if let Some(v) = &query.project_id {
                    pairs.push(("projectID".to_owned(), v.clone()));
                }
                let mut hdrs: Vec<(&'static str, String)> = Vec::new();
                self.client.execute_json::<crate::transport::Envelope<Vec<crate::types::PermissionSavedInfo>>>(
            &crate::transport::Call {
                method: reqwest::Method::GET,
                path: "/api/permission/saved",
                path_params: &[],
                query: &pairs,
                headers: &hdrs,
                body: crate::transport::Body::None,
                success: 200,
                declared: &[400, 401],
            }
        )
        .await
        .map(|env| env.data)
            }

            /// DELETE /api/permission/saved/{id} — operationId `permission.saved.remove`
            /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded
            pub async fn remove(&self, id: &str) -> Result<(), crate::ClientError> {
                let pairs: Vec<(String, String)> = Vec::new();
                let hdrs: Vec<(&'static str, String)> = Vec::new();
                self.client
                    .execute_empty(&crate::transport::Call {
                        method: reqwest::Method::DELETE,
                        path: "/api/permission/saved/{id}",
                        path_params: &[("id", id)],
                        query: &pairs,
                        headers: &hdrs,
                        body: crate::transport::Body::None,
                        success: 204,
                        declared: &[400, 401],
                    })
                    .await
            }
        }
    }

    pub struct PermissionApi<'a> {
        pub(crate) client: &'a crate::Client,
    }

    impl PermissionApi<'_> {
        /// Nested namespace `permission::request`.
        #[must_use]
        pub fn request(&self) -> request::PermissionRequestApi<'_> {
            request::PermissionRequestApi { client: self.client }
        }
        /// Nested namespace `permission::saved`.
        #[must_use]
        pub fn saved(&self) -> saved::PermissionSavedApi<'_> {
            saved::PermissionSavedApi { client: self.client }
        }
    }

    impl crate::Client {
        /// Namespace `permission` operations.
        #[must_use]
        pub fn permission(&self) -> PermissionApi<'_> {
            PermissionApi { client: self }
        }
    }
}

// Namespace `persistent_pty` (mirrors the v2 SDK method group).
pub mod persistent_pty {
    pub struct PersistentPtyApi<'a> {
        pub(crate) client: &'a crate::Client,
    }

    impl PersistentPtyApi<'_> {
        /// POST /api/experimental/persistent-pty/handoff — operationId `server.experimental.persistentPty.handoff`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `503` ServiceUnavailableErrorEncoded
        pub async fn handoff(
            &self,
        ) -> Result<crate::types::ServerExperimentalPersistentPtyHandoffResponse, crate::ClientError>
        {
            let pairs: Vec<(String, String)> = Vec::new();
            let hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_json(&crate::transport::Call {
                    method: reqwest::Method::POST,
                    path: "/api/experimental/persistent-pty/handoff",
                    path_params: &[],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::None,
                    success: 200,
                    declared: &[400, 401, 503],
                })
                .await
        }

        /// POST /api/experimental/persistent-pty/shutdown — operationId `server.experimental.persistentPty.shutdown`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `503` ServiceUnavailableErrorEncoded
        pub async fn shutdown(&self) -> Result<(), crate::ClientError> {
            let pairs: Vec<(String, String)> = Vec::new();
            let hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_empty(&crate::transport::Call {
                    method: reqwest::Method::POST,
                    path: "/api/experimental/persistent-pty/shutdown",
                    path_params: &[],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::None,
                    success: 204,
                    declared: &[400, 401, 503],
                })
                .await
        }

        /// DELETE /api/experimental/persistent-pty/{ptyID} — operationId `server.experimental.persistentPty.remove`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `404` PtyNotFoundErrorEncoded, `503` ServiceUnavailableErrorEncoded
        pub async fn remove(&self, pty_id: &str) -> Result<(), crate::ClientError> {
            let pairs: Vec<(String, String)> = Vec::new();
            let hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_empty(&crate::transport::Call {
                    method: reqwest::Method::DELETE,
                    path: "/api/experimental/persistent-pty/{ptyID}",
                    path_params: &[("ptyID", pty_id)],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::None,
                    success: 204,
                    declared: &[400, 401, 404, 503],
                })
                .await
        }

        /// GET /api/experimental/persistent-pty/{ptyID} — operationId `server.experimental.persistentPty.get`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `404` PtyNotFoundErrorEncoded, `503` ServiceUnavailableErrorEncoded
        /// The `{{ data }}` envelope is unwrapped (v2 client parity).
        pub async fn get(
            &self,
            pty_id: &str,
        ) -> Result<crate::types::PersistentPtyInfo, crate::ClientError> {
            let pairs: Vec<(String, String)> = Vec::new();
            let hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_json::<crate::transport::Envelope<crate::types::PersistentPtyInfo>>(
                    &crate::transport::Call {
                        method: reqwest::Method::GET,
                        path: "/api/experimental/persistent-pty/{ptyID}",
                        path_params: &[("ptyID", pty_id)],
                        query: &pairs,
                        headers: &hdrs,
                        body: crate::transport::Body::None,
                        success: 200,
                        declared: &[400, 401, 404, 503],
                    },
                )
                .await
                .map(|env| env.data)
        }

        /// PUT /api/experimental/persistent-pty/{ptyID} — operationId `server.experimental.persistentPty.update`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `404` PtyNotFoundErrorEncoded, `503` ServiceUnavailableErrorEncoded
        /// The `{{ data }}` envelope is unwrapped (v2 client parity).
        pub async fn update(
            &self,
            pty_id: &str,
            body: &crate::types::PersistentPtyUpdateInput,
        ) -> Result<crate::types::PersistentPtyInfo, crate::ClientError> {
            let pairs: Vec<(String, String)> = Vec::new();
            let hdrs: Vec<(&'static str, String)> = Vec::new();
            let body_value = crate::transport::encode_body(body)?;
            self.client
                .execute_json::<crate::transport::Envelope<crate::types::PersistentPtyInfo>>(
                    &crate::transport::Call {
                        method: reqwest::Method::PUT,
                        path: "/api/experimental/persistent-pty/{ptyID}",
                        path_params: &[("ptyID", pty_id)],
                        query: &pairs,
                        headers: &hdrs,
                        body: crate::transport::Body::Json(&body_value),
                        success: 200,
                        declared: &[400, 401, 404, 503],
                    },
                )
                .await
                .map(|env| env.data)
        }

        /// GET /api/experimental/persistent-pty/{ptyID}/connect — operationId `persistentPty.connect`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `403` ForbiddenErrorEncoded, `404` PtyNotFoundErrorEncoded, `503` ServiceUnavailableErrorEncoded
        /// TODO(pty-ws): schema models this as a boolean GET; the real endpoint is a
        /// WebSocket upgrade — WS clients live outside this crate (server-deep-map §3).
        pub async fn connect(
            &self,
            pty_id: &str,
            query: &crate::types::PersistentPtyConnectQuery,
        ) -> Result<bool, crate::ClientError> {
            let mut pairs: Vec<(String, String)> = Vec::new();
            if let Some(v) = &query.attachment_id {
                pairs.push(("attachment_id".to_owned(), v.clone()));
            }
            if let Some(v) = &query.cursor {
                pairs.push(("cursor".to_owned(), v.clone()));
            }
            if let Some(v) = &query.input_protocol {
                pairs.push(("input_protocol".to_owned(), v.clone()));
            }
            if let Some(v) = &query.role {
                pairs.push(("role".to_owned(), v.clone()));
            }
            if let Some(v) = &query.takeover {
                pairs.push(("takeover".to_owned(), v.clone()));
            }
            if let Some(v) = &query.ticket {
                pairs.push(("ticket".to_owned(), v.clone()));
            }
            let mut hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_json(&crate::transport::Call {
                    method: reqwest::Method::GET,
                    path: "/api/experimental/persistent-pty/{ptyID}/connect",
                    path_params: &[("ptyID", pty_id)],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::None,
                    success: 200,
                    declared: &[400, 401, 403, 404, 503],
                })
                .await
        }

        /// POST /api/experimental/persistent-pty/{ptyID}/connect-token — operationId `server.experimental.persistentPty.connectToken`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `403` ForbiddenErrorEncoded, `404` PtyNotFoundErrorEncoded, `503` ServiceUnavailableErrorEncoded
        /// The `{{ data }}` envelope is unwrapped (v2 client parity).
        pub async fn connect_token(
            &self,
            pty_id: &str,
            query: &crate::types::ServerExperimentalPersistentPtyConnectTokenQuery,
        ) -> Result<crate::types::PtyTicketConnectToken, crate::ClientError> {
            let mut pairs: Vec<(String, String)> = Vec::new();
            let mut hdrs: Vec<(&'static str, String)> = Vec::new();
            if let Some(v) = &query.x_opencode_ticket {
                hdrs.push(("x-opencode-ticket", v.clone()));
            }
            self.client
                .execute_json::<crate::transport::Envelope<crate::types::PtyTicketConnectToken>>(
                    &crate::transport::Call {
                        method: reqwest::Method::POST,
                        path: "/api/experimental/persistent-pty/{ptyID}/connect-token",
                        path_params: &[("ptyID", pty_id)],
                        query: &pairs,
                        headers: &hdrs,
                        body: crate::transport::Body::None,
                        success: 200,
                        declared: &[400, 401, 403, 404, 503],
                    },
                )
                .await
                .map(|env| env.data)
        }

        /// GET /api/experimental/persistent-pty/{ptyID}/snapshot — operationId `server.experimental.persistentPty.snapshot`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `404` PtyNotFoundErrorEncoded, `503` ServiceUnavailableErrorEncoded
        /// The `{{ data }}` envelope is unwrapped (v2 client parity).
        pub async fn snapshot(
            &self,
            pty_id: &str,
        ) -> Result<crate::types::PersistentPtySnapshot, crate::ClientError> {
            let pairs: Vec<(String, String)> = Vec::new();
            let hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_json::<crate::transport::Envelope<crate::types::PersistentPtySnapshot>>(
                    &crate::transport::Call {
                        method: reqwest::Method::GET,
                        path: "/api/experimental/persistent-pty/{ptyID}/snapshot",
                        path_params: &[("ptyID", pty_id)],
                        query: &pairs,
                        headers: &hdrs,
                        body: crate::transport::Body::None,
                        success: 200,
                        declared: &[400, 401, 404, 503],
                    },
                )
                .await
                .map(|env| env.data)
        }

        /// GET /api/experimental/session/{sessionID}/terminal — operationId `server.experimental.persistentPty.list`
        /// Declared errors: `400` unknown, `401` UnauthorizedErrorEncoded, `503` ServiceUnavailableErrorEncoded
        /// The `{{ data }}` envelope is unwrapped (v2 client parity).
        pub async fn list(
            &self,
            session_id: &str,
        ) -> Result<Vec<crate::types::PersistentPtyInfo>, crate::ClientError> {
            let pairs: Vec<(String, String)> = Vec::new();
            let hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_json::<crate::transport::Envelope<Vec<crate::types::PersistentPtyInfo>>>(
                    &crate::transport::Call {
                        method: reqwest::Method::GET,
                        path: "/api/experimental/session/{sessionID}/terminal",
                        path_params: &[("sessionID", session_id)],
                        query: &pairs,
                        headers: &hdrs,
                        body: crate::transport::Body::None,
                        success: 200,
                        declared: &[400, 401, 503],
                    },
                )
                .await
                .map(|env| env.data)
        }

        /// POST /api/experimental/session/{sessionID}/terminal — operationId `server.experimental.persistentPty.create`
        /// Declared errors: `400` unknown, `401` UnauthorizedErrorEncoded, `503` ServiceUnavailableErrorEncoded
        /// The `{{ data }}` envelope is unwrapped (v2 client parity).
        pub async fn create(
            &self,
            session_id: &str,
            body: &crate::types::PersistentPtyCreateInput,
        ) -> Result<crate::types::PersistentPtyInfo, crate::ClientError> {
            let pairs: Vec<(String, String)> = Vec::new();
            let hdrs: Vec<(&'static str, String)> = Vec::new();
            let body_value = crate::transport::encode_body(body)?;
            self.client
                .execute_json::<crate::transport::Envelope<crate::types::PersistentPtyInfo>>(
                    &crate::transport::Call {
                        method: reqwest::Method::POST,
                        path: "/api/experimental/session/{sessionID}/terminal",
                        path_params: &[("sessionID", session_id)],
                        query: &pairs,
                        headers: &hdrs,
                        body: crate::transport::Body::Json(&body_value),
                        success: 200,
                        declared: &[400, 401, 503],
                    },
                )
                .await
                .map(|env| env.data)
        }

        /// GET /api/experimental/session/{sessionID}/terminal/read — operationId `server.experimental.persistentPty.read`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `503` ServiceUnavailableErrorEncoded
        /// The `{{ data }}` envelope is unwrapped (v2 client parity).
        pub async fn read(
            &self,
            session_id: &str,
            query: &crate::types::ServerExperimentalPersistentPtyReadQuery,
        ) -> Result<Option<crate::types::PersistentPtyReadResult>, crate::ClientError> {
            let mut pairs: Vec<(String, String)> = Vec::new();
            if let Some(v) = &query.lines {
                pairs.push(("lines".to_owned(), v.clone()));
            }
            let mut hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client.execute_json::<crate::transport::Envelope<Option<crate::types::PersistentPtyReadResult>>>(
            &crate::transport::Call {
                method: reqwest::Method::GET,
                path: "/api/experimental/session/{sessionID}/terminal/read",
                path_params: &[("sessionID", session_id)],
                query: &pairs,
                headers: &hdrs,
                body: crate::transport::Body::None,
                success: 200,
                declared: &[400, 401, 503],
            }
        )
        .await
        .map(|env| env.data)
        }
    }

    impl crate::Client {
        /// Namespace `persistent_pty` operations.
        #[must_use]
        pub fn persistent_pty(&self) -> PersistentPtyApi<'_> {
            PersistentPtyApi { client: self }
        }
    }
}

// Namespace `plugin` (mirrors the v2 SDK method group).
pub mod plugin {
    pub struct PluginApi<'a> {
        pub(crate) client: &'a crate::Client,
    }

    impl PluginApi<'_> {
        /// GET /api/plugin — operationId `plugin.list`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded
        pub async fn list(
            &self,
            query: &crate::types::PluginListQuery,
        ) -> Result<crate::types::PluginListResponse, crate::ClientError> {
            let mut pairs: Vec<(String, String)> = Vec::new();
            if let Some(loc) = &query.location {
                if let Some(dir) = &loc.directory {
                    pairs.push(("location[directory]".to_owned(), dir.clone()));
                }
            }
            let mut hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_json(&crate::transport::Call {
                    method: reqwest::Method::GET,
                    path: "/api/plugin",
                    path_params: &[],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::None,
                    success: 200,
                    declared: &[400, 401],
                })
                .await
        }

        /// POST /api/plugin/check — operationId `plugin.check`
        /// Declared errors: `400` unknown, `401` UnauthorizedErrorEncoded
        pub async fn check(
            &self,
            query: &crate::types::PluginCheckQuery,
            body: &crate::types::PluginCheckBody,
        ) -> Result<crate::types::PluginCheckResponse, crate::ClientError> {
            let mut pairs: Vec<(String, String)> = Vec::new();
            if let Some(loc) = &query.location {
                if let Some(dir) = &loc.directory {
                    pairs.push(("location[directory]".to_owned(), dir.clone()));
                }
            }
            let mut hdrs: Vec<(&'static str, String)> = Vec::new();
            let body_value = crate::transport::encode_body(body)?;
            self.client
                .execute_json(&crate::transport::Call {
                    method: reqwest::Method::POST,
                    path: "/api/plugin/check",
                    path_params: &[],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::Json(&body_value),
                    success: 200,
                    declared: &[400, 401],
                })
                .await
        }

        /// POST /api/plugin/update — operationId `plugin.update`
        /// Declared errors: `400` unknown, `401` UnauthorizedErrorEncoded, `503` ServiceUnavailableErrorEncoded
        pub async fn update(
            &self,
            query: &crate::types::PluginUpdateQuery,
            body: &crate::types::PluginUpdateBody,
        ) -> Result<(), crate::ClientError> {
            let mut pairs: Vec<(String, String)> = Vec::new();
            if let Some(loc) = &query.location {
                if let Some(dir) = &loc.directory {
                    pairs.push(("location[directory]".to_owned(), dir.clone()));
                }
            }
            let mut hdrs: Vec<(&'static str, String)> = Vec::new();
            let body_value = crate::transport::encode_body(body)?;
            self.client
                .execute_empty(&crate::transport::Call {
                    method: reqwest::Method::POST,
                    path: "/api/plugin/update",
                    path_params: &[],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::Json(&body_value),
                    success: 204,
                    declared: &[400, 401, 503],
                })
                .await
        }
    }

    impl crate::Client {
        /// Namespace `plugin` operations.
        #[must_use]
        pub fn plugin(&self) -> PluginApi<'_> {
            PluginApi { client: self }
        }
    }
}

// Namespace `project` (mirrors the v2 SDK method group).
pub mod project {
    pub struct ProjectApi<'a> {
        pub(crate) client: &'a crate::Client,
    }

    impl ProjectApi<'_> {
        /// GET /api/project — operationId `project.list`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded
        pub async fn list(&self) -> Result<Vec<crate::types::Project>, crate::ClientError> {
            let pairs: Vec<(String, String)> = Vec::new();
            let hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_json(&crate::transport::Call {
                    method: reqwest::Method::GET,
                    path: "/api/project",
                    path_params: &[],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::None,
                    success: 200,
                    declared: &[400, 401],
                })
                .await
        }

        /// PATCH /api/project/{projectID} — operationId `project.update`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `404` ProjectNotFoundErrorEncoded
        pub async fn update(
            &self,
            project_id: &str,
            body: &crate::types::ProjectUpdateBody,
        ) -> Result<crate::types::Project, crate::ClientError> {
            let pairs: Vec<(String, String)> = Vec::new();
            let hdrs: Vec<(&'static str, String)> = Vec::new();
            let body_value = crate::transport::encode_body(body)?;
            self.client
                .execute_json(&crate::transport::Call {
                    method: reqwest::Method::PATCH,
                    path: "/api/project/{projectID}",
                    path_params: &[("projectID", project_id)],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::Json(&body_value),
                    success: 200,
                    declared: &[400, 401, 404],
                })
                .await
        }
    }

    impl crate::Client {
        /// Namespace `project` operations.
        #[must_use]
        pub fn project(&self) -> ProjectApi<'_> {
            ProjectApi { client: self }
        }
    }
}

// Namespace `provider` (mirrors the v2 SDK method group).
pub mod provider {
    pub struct ProviderApi<'a> {
        pub(crate) client: &'a crate::Client,
    }

    impl ProviderApi<'_> {
        /// GET /api/provider — operationId `provider.list`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `503` ServiceUnavailableErrorEncoded
        pub async fn list(
            &self,
            query: &crate::types::ProviderListQuery,
        ) -> Result<crate::types::ProviderListResponse, crate::ClientError> {
            let mut pairs: Vec<(String, String)> = Vec::new();
            if let Some(loc) = &query.location {
                if let Some(dir) = &loc.directory {
                    pairs.push(("location[directory]".to_owned(), dir.clone()));
                }
            }
            let mut hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_json(&crate::transport::Call {
                    method: reqwest::Method::GET,
                    path: "/api/provider",
                    path_params: &[],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::None,
                    success: 200,
                    declared: &[400, 401, 503],
                })
                .await
        }

        /// GET /api/provider/{providerID} — operationId `provider.get`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `404` ProviderNotFoundErrorEncoded, `503` ServiceUnavailableErrorEncoded
        pub async fn get(
            &self,
            provider_id: &str,
            query: &crate::types::ProviderGetQuery,
        ) -> Result<crate::types::ProviderGetResponse, crate::ClientError> {
            let mut pairs: Vec<(String, String)> = Vec::new();
            if let Some(loc) = &query.location {
                if let Some(dir) = &loc.directory {
                    pairs.push(("location[directory]".to_owned(), dir.clone()));
                }
            }
            let mut hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_json(&crate::transport::Call {
                    method: reqwest::Method::GET,
                    path: "/api/provider/{providerID}",
                    path_params: &[("providerID", provider_id)],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::None,
                    success: 200,
                    declared: &[400, 401, 404, 503],
                })
                .await
        }
    }

    impl crate::Client {
        /// Namespace `provider` operations.
        #[must_use]
        pub fn provider(&self) -> ProviderApi<'_> {
            ProviderApi { client: self }
        }
    }
}

// Namespace `pty` (mirrors the v2 SDK method group).
pub mod pty {
    pub struct PtyApi<'a> {
        pub(crate) client: &'a crate::Client,
    }

    impl PtyApi<'_> {
        /// GET /api/pty — operationId `pty.list`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded
        pub async fn list(
            &self,
            query: &crate::types::PtyListQuery,
        ) -> Result<crate::types::PtyListResponse, crate::ClientError> {
            let mut pairs: Vec<(String, String)> = Vec::new();
            if let Some(loc) = &query.location {
                if let Some(dir) = &loc.directory {
                    pairs.push(("location[directory]".to_owned(), dir.clone()));
                }
            }
            let mut hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_json(&crate::transport::Call {
                    method: reqwest::Method::GET,
                    path: "/api/pty",
                    path_params: &[],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::None,
                    success: 200,
                    declared: &[400, 401],
                })
                .await
        }

        /// POST /api/pty — operationId `pty.create`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded
        pub async fn create(
            &self,
            query: &crate::types::PtyCreateQuery,
            body: &crate::types::PtyCreateBody,
        ) -> Result<crate::types::PtyCreateResponse, crate::ClientError> {
            let mut pairs: Vec<(String, String)> = Vec::new();
            if let Some(loc) = &query.location {
                if let Some(dir) = &loc.directory {
                    pairs.push(("location[directory]".to_owned(), dir.clone()));
                }
            }
            let mut hdrs: Vec<(&'static str, String)> = Vec::new();
            let body_value = crate::transport::encode_body(body)?;
            self.client
                .execute_json(&crate::transport::Call {
                    method: reqwest::Method::POST,
                    path: "/api/pty",
                    path_params: &[],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::Json(&body_value),
                    success: 200,
                    declared: &[400, 401],
                })
                .await
        }

        /// DELETE /api/pty/{ptyID} — operationId `pty.remove`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `404` PtyNotFoundErrorEncoded
        pub async fn remove(
            &self,
            pty_id: &str,
            query: &crate::types::PtyRemoveQuery,
        ) -> Result<(), crate::ClientError> {
            let mut pairs: Vec<(String, String)> = Vec::new();
            if let Some(loc) = &query.location {
                if let Some(dir) = &loc.directory {
                    pairs.push(("location[directory]".to_owned(), dir.clone()));
                }
            }
            let mut hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_empty(&crate::transport::Call {
                    method: reqwest::Method::DELETE,
                    path: "/api/pty/{ptyID}",
                    path_params: &[("ptyID", pty_id)],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::None,
                    success: 204,
                    declared: &[400, 401, 404],
                })
                .await
        }

        /// GET /api/pty/{ptyID} — operationId `pty.get`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `404` PtyNotFoundErrorEncoded
        pub async fn get(
            &self,
            pty_id: &str,
            query: &crate::types::PtyGetQuery,
        ) -> Result<crate::types::PtyGetResponse, crate::ClientError> {
            let mut pairs: Vec<(String, String)> = Vec::new();
            if let Some(loc) = &query.location {
                if let Some(dir) = &loc.directory {
                    pairs.push(("location[directory]".to_owned(), dir.clone()));
                }
            }
            let mut hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_json(&crate::transport::Call {
                    method: reqwest::Method::GET,
                    path: "/api/pty/{ptyID}",
                    path_params: &[("ptyID", pty_id)],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::None,
                    success: 200,
                    declared: &[400, 401, 404],
                })
                .await
        }

        /// PUT /api/pty/{ptyID} — operationId `pty.update`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `404` PtyNotFoundErrorEncoded
        pub async fn update(
            &self,
            pty_id: &str,
            query: &crate::types::PtyUpdateQuery,
            body: &crate::types::PtyUpdateBody,
        ) -> Result<crate::types::PtyUpdateResponse, crate::ClientError> {
            let mut pairs: Vec<(String, String)> = Vec::new();
            if let Some(loc) = &query.location {
                if let Some(dir) = &loc.directory {
                    pairs.push(("location[directory]".to_owned(), dir.clone()));
                }
            }
            let mut hdrs: Vec<(&'static str, String)> = Vec::new();
            let body_value = crate::transport::encode_body(body)?;
            self.client
                .execute_json(&crate::transport::Call {
                    method: reqwest::Method::PUT,
                    path: "/api/pty/{ptyID}",
                    path_params: &[("ptyID", pty_id)],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::Json(&body_value),
                    success: 200,
                    declared: &[400, 401, 404],
                })
                .await
        }

        /// GET /api/pty/{ptyID}/connect — operationId `pty.connect`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `403` ForbiddenErrorEncoded, `404` PtyNotFoundErrorEncoded
        /// TODO(pty-ws): schema models this as a boolean GET; the real endpoint is a
        /// WebSocket upgrade — WS clients live outside this crate (server-deep-map §3).
        pub async fn connect(
            &self,
            pty_id: &str,
            query: &crate::types::PtyConnectQuery,
        ) -> Result<bool, crate::ClientError> {
            let mut pairs: Vec<(String, String)> = Vec::new();
            if let Some(v) = &query.cursor {
                pairs.push(("cursor".to_owned(), v.clone()));
            }
            if let Some(v) = &query.location_directory {
                pairs.push(("location[directory]".to_owned(), v.clone()));
            }
            if let Some(v) = &query.ticket {
                pairs.push(("ticket".to_owned(), v.clone()));
            }
            let mut hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_json(&crate::transport::Call {
                    method: reqwest::Method::GET,
                    path: "/api/pty/{ptyID}/connect",
                    path_params: &[("ptyID", pty_id)],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::None,
                    success: 200,
                    declared: &[400, 401, 403, 404],
                })
                .await
        }

        /// POST /api/pty/{ptyID}/connect-token — operationId `pty.connect.token`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `403` ForbiddenErrorEncoded, `404` PtyNotFoundErrorEncoded
        pub async fn connect_token(
            &self,
            pty_id: &str,
            query: &crate::types::PtyConnectTokenQuery,
        ) -> Result<crate::types::PtyConnectTokenResponse, crate::ClientError> {
            let mut pairs: Vec<(String, String)> = Vec::new();
            if let Some(loc) = &query.location {
                if let Some(dir) = &loc.directory {
                    pairs.push(("location[directory]".to_owned(), dir.clone()));
                }
            }
            let mut hdrs: Vec<(&'static str, String)> = Vec::new();
            if let Some(v) = &query.x_opencode_ticket {
                hdrs.push(("x-opencode-ticket", v.clone()));
            }
            self.client
                .execute_json(&crate::transport::Call {
                    method: reqwest::Method::POST,
                    path: "/api/pty/{ptyID}/connect-token",
                    path_params: &[("ptyID", pty_id)],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::None,
                    success: 200,
                    declared: &[400, 401, 403, 404],
                })
                .await
        }
    }

    impl crate::Client {
        /// Namespace `pty` operations.
        #[must_use]
        pub fn pty(&self) -> PtyApi<'_> {
            PtyApi { client: self }
        }
    }
}

// Namespace `reference` (mirrors the v2 SDK method group).
pub mod reference {
    pub struct ReferenceApi<'a> {
        pub(crate) client: &'a crate::Client,
    }

    impl ReferenceApi<'_> {
        /// GET /api/reference — operationId `reference.list`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded
        pub async fn list(
            &self,
            query: &crate::types::ReferenceListQuery,
        ) -> Result<crate::types::ReferenceListResponse, crate::ClientError> {
            let mut pairs: Vec<(String, String)> = Vec::new();
            if let Some(loc) = &query.location {
                if let Some(dir) = &loc.directory {
                    pairs.push(("location[directory]".to_owned(), dir.clone()));
                }
            }
            let mut hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_json(&crate::transport::Call {
                    method: reqwest::Method::GET,
                    path: "/api/reference",
                    path_params: &[],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::None,
                    success: 200,
                    declared: &[400, 401],
                })
                .await
        }
    }

    impl crate::Client {
        /// Namespace `reference` operations.
        #[must_use]
        pub fn reference(&self) -> ReferenceApi<'_> {
            ReferenceApi { client: self }
        }
    }
}

// Namespace `rpc` (mirrors the v2 SDK method group).
pub mod rpc {
    pub struct RpcApi<'a> {
        pub(crate) client: &'a crate::Client,
    }

    impl RpcApi<'_> {
        /// POST /api/rpc/{rpcID}/{method} — operationId `rpc.call`
        /// Declared errors: `400` unknown, `401` UnauthorizedErrorEncoded, `500` RpcInternalErrorEncoded
        pub async fn call(
            &self,
            rpc_id: &str,
            method: &str,
            query: &crate::types::RpcCallQuery,
            body: &crate::types::RpcInput,
        ) -> Result<crate::types::RpcOutput, crate::ClientError> {
            let mut pairs: Vec<(String, String)> = Vec::new();
            if let Some(loc) = &query.location {
                if let Some(dir) = &loc.directory {
                    pairs.push(("location[directory]".to_owned(), dir.clone()));
                }
            }
            let mut hdrs: Vec<(&'static str, String)> = Vec::new();
            let body_value = crate::transport::encode_body(body)?;
            self.client
                .execute_json(&crate::transport::Call {
                    method: reqwest::Method::POST,
                    path: "/api/rpc/{rpcID}/{method}",
                    path_params: &[("rpcID", rpc_id), ("method", method)],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::Json(&body_value),
                    success: 200,
                    declared: &[400, 401, 500],
                })
                .await
        }
    }

    impl crate::Client {
        /// Namespace `rpc` operations.
        #[must_use]
        pub fn rpc(&self) -> RpcApi<'_> {
            RpcApi { client: self }
        }
    }
}

// Namespace `server` (mirrors the v2 SDK method group).
pub mod server {
    pub struct ServerApi<'a> {
        pub(crate) client: &'a crate::Client,
    }

    impl ServerApi<'_> {
        /// GET /api/info — operationId `server.info`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded
        pub async fn info(&self) -> Result<crate::types::ServerInfo, crate::ClientError> {
            let pairs: Vec<(String, String)> = Vec::new();
            let hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_json(&crate::transport::Call {
                    method: reqwest::Method::GET,
                    path: "/api/info",
                    path_params: &[],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::None,
                    success: 200,
                    declared: &[400, 401],
                })
                .await
        }

        /// POST /api/pair — operationId `server.pair`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded
        pub async fn pair(&self) -> Result<crate::types::PairingCode, crate::ClientError> {
            let pairs: Vec<(String, String)> = Vec::new();
            let hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_json(&crate::transport::Call {
                    method: reqwest::Method::POST,
                    path: "/api/pair",
                    path_params: &[],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::None,
                    success: 200,
                    declared: &[400, 401],
                })
                .await
        }

        /// GET /auth/connect/{code} — operationId `server.connect`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` unknown
        pub async fn connect(
            &self,
            code: &str,
        ) -> Result<crate::types::PairingSession, crate::ClientError> {
            let pairs: Vec<(String, String)> = Vec::new();
            let hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_json(&crate::transport::Call {
                    method: reqwest::Method::GET,
                    path: "/auth/connect/{code}",
                    path_params: &[("code", code)],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::None,
                    success: 200,
                    declared: &[400, 401],
                })
                .await
        }
    }

    impl crate::Client {
        /// Namespace `server` operations.
        #[must_use]
        pub fn server(&self) -> ServerApi<'_> {
            ServerApi { client: self }
        }
    }
}

// Namespace `session` (mirrors the v2 SDK method group).
pub mod session {
    // Namespace `session::form` (mirrors the v2 SDK method group).
    pub mod form {
        pub struct SessionFormApi<'a> {
            pub(crate) client: &'a crate::Client,
        }

        impl SessionFormApi<'_> {
            /// GET /api/session/{sessionID}/form — operationId `session.form.list`
            /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `404` unknown
            /// The `{{ data }}` envelope is unwrapped (v2 client parity).
            pub async fn list(
                &self,
                session_id: &str,
            ) -> Result<Vec<crate::types::FormInfo>, crate::ClientError> {
                let pairs: Vec<(String, String)> = Vec::new();
                let hdrs: Vec<(&'static str, String)> = Vec::new();
                self.client
                    .execute_json::<crate::transport::Envelope<Vec<crate::types::FormInfo>>>(
                        &crate::transport::Call {
                            method: reqwest::Method::GET,
                            path: "/api/session/{sessionID}/form",
                            path_params: &[("sessionID", session_id)],
                            query: &pairs,
                            headers: &hdrs,
                            body: crate::transport::Body::None,
                            success: 200,
                            declared: &[400, 401, 404],
                        },
                    )
                    .await
                    .map(|env| env.data)
            }

            /// POST /api/session/{sessionID}/form — operationId `session.form.create`
            /// Declared errors: `400` unknown, `401` UnauthorizedErrorEncoded, `404` unknown, `409` ConflictErrorEncoded
            /// The `{{ data }}` envelope is unwrapped (v2 client parity).
            pub async fn create(
                &self,
                session_id: &str,
                body: &crate::types::FormCreatePayload,
            ) -> Result<crate::types::FormInfo, crate::ClientError> {
                let pairs: Vec<(String, String)> = Vec::new();
                let hdrs: Vec<(&'static str, String)> = Vec::new();
                let body_value = crate::transport::encode_body(body)?;
                self.client
                    .execute_json::<crate::transport::Envelope<crate::types::FormInfo>>(
                        &crate::transport::Call {
                            method: reqwest::Method::POST,
                            path: "/api/session/{sessionID}/form",
                            path_params: &[("sessionID", session_id)],
                            query: &pairs,
                            headers: &hdrs,
                            body: crate::transport::Body::Json(&body_value),
                            success: 200,
                            declared: &[400, 401, 404, 409],
                        },
                    )
                    .await
                    .map(|env| env.data)
            }

            /// DELETE /api/session/{sessionID}/form/{formID} — operationId `session.form.cancel`
            /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `404` unknown, `409` FormAlreadySettledErrorEncoded
            pub async fn cancel(
                &self,
                session_id: &str,
                form_id: &str,
                query: &crate::types::SessionFormCancelQuery,
            ) -> Result<(), crate::ClientError> {
                let mut pairs: Vec<(String, String)> = Vec::new();
                if let Some(v) = &query.message {
                    pairs.push(("message".to_owned(), v.clone()));
                }
                let mut hdrs: Vec<(&'static str, String)> = Vec::new();
                self.client
                    .execute_empty(&crate::transport::Call {
                        method: reqwest::Method::DELETE,
                        path: "/api/session/{sessionID}/form/{formID}",
                        path_params: &[("sessionID", session_id), ("formID", form_id)],
                        query: &pairs,
                        headers: &hdrs,
                        body: crate::transport::Body::None,
                        success: 204,
                        declared: &[400, 401, 404, 409],
                    })
                    .await
            }

            /// GET /api/session/{sessionID}/form/{formID} — operationId `session.form.get`
            /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `404` unknown
            /// The `{{ data }}` envelope is unwrapped (v2 client parity).
            pub async fn get(
                &self,
                session_id: &str,
                form_id: &str,
            ) -> Result<crate::types::FormDetail, crate::ClientError> {
                let pairs: Vec<(String, String)> = Vec::new();
                let hdrs: Vec<(&'static str, String)> = Vec::new();
                self.client
                    .execute_json::<crate::transport::Envelope<crate::types::FormDetail>>(
                        &crate::transport::Call {
                            method: reqwest::Method::GET,
                            path: "/api/session/{sessionID}/form/{formID}",
                            path_params: &[("sessionID", session_id), ("formID", form_id)],
                            query: &pairs,
                            headers: &hdrs,
                            body: crate::transport::Body::None,
                            success: 200,
                            declared: &[400, 401, 404],
                        },
                    )
                    .await
                    .map(|env| env.data)
            }

            /// POST /api/session/{sessionID}/form/{formID}/reply — operationId `session.form.reply`
            /// Declared errors: `400` unknown, `401` UnauthorizedErrorEncoded, `404` unknown, `409` FormAlreadySettledErrorEncoded
            pub async fn reply(
                &self,
                session_id: &str,
                form_id: &str,
                body: &crate::types::FormReply,
            ) -> Result<(), crate::ClientError> {
                let pairs: Vec<(String, String)> = Vec::new();
                let hdrs: Vec<(&'static str, String)> = Vec::new();
                let body_value = crate::transport::encode_body(body)?;
                self.client
                    .execute_empty(&crate::transport::Call {
                        method: reqwest::Method::POST,
                        path: "/api/session/{sessionID}/form/{formID}/reply",
                        path_params: &[("sessionID", session_id), ("formID", form_id)],
                        query: &pairs,
                        headers: &hdrs,
                        body: crate::transport::Body::Json(&body_value),
                        success: 204,
                        declared: &[400, 401, 404, 409],
                    })
                    .await
            }
        }
    }

    // Namespace `session::inbox` (mirrors the v2 SDK method group).
    pub mod inbox {
        pub struct SessionInboxApi<'a> {
            pub(crate) client: &'a crate::Client,
        }

        impl SessionInboxApi<'_> {
            /// GET /api/session/{sessionID}/inbox — operationId `session.inbox.list`
            /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `404` SessionNotFoundErrorEncoded
            /// The `{{ data }}` envelope is unwrapped (v2 client parity).
            pub async fn list(
                &self,
                session_id: &str,
            ) -> Result<Vec<crate::types::SessionInboxInfo>, crate::ClientError> {
                let pairs: Vec<(String, String)> = Vec::new();
                let hdrs: Vec<(&'static str, String)> = Vec::new();
                self.client.execute_json::<crate::transport::Envelope<Vec<crate::types::SessionInboxInfo>>>(
            &crate::transport::Call {
                method: reqwest::Method::GET,
                path: "/api/session/{sessionID}/inbox",
                path_params: &[("sessionID", session_id)],
                query: &pairs,
                headers: &hdrs,
                body: crate::transport::Body::None,
                success: 200,
                declared: &[400, 401, 404],
            }
        )
        .await
        .map(|env| env.data)
            }

            /// DELETE /api/session/{sessionID}/inbox/{inboxID} — operationId `session.inbox.cancel`
            /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `404` SessionNotFoundErrorEncoded
            pub async fn cancel(
                &self,
                session_id: &str,
                inbox_id: &str,
            ) -> Result<(), crate::ClientError> {
                let pairs: Vec<(String, String)> = Vec::new();
                let hdrs: Vec<(&'static str, String)> = Vec::new();
                self.client
                    .execute_empty(&crate::transport::Call {
                        method: reqwest::Method::DELETE,
                        path: "/api/session/{sessionID}/inbox/{inboxID}",
                        path_params: &[("sessionID", session_id), ("inboxID", inbox_id)],
                        query: &pairs,
                        headers: &hdrs,
                        body: crate::transport::Body::None,
                        success: 204,
                        declared: &[400, 401, 404],
                    })
                    .await
            }

            /// PATCH /api/session/{sessionID}/inbox/{inboxID} — operationId `session.inbox.update`
            /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `404` SessionNotFoundErrorEncoded, `409` ConflictErrorEncoded
            pub async fn update(
                &self,
                session_id: &str,
                inbox_id: &str,
                body: &crate::types::SessionInboxUpdateBody,
            ) -> Result<(), crate::ClientError> {
                let pairs: Vec<(String, String)> = Vec::new();
                let hdrs: Vec<(&'static str, String)> = Vec::new();
                let body_value = crate::transport::encode_body(body)?;
                self.client
                    .execute_empty(&crate::transport::Call {
                        method: reqwest::Method::PATCH,
                        path: "/api/session/{sessionID}/inbox/{inboxID}",
                        path_params: &[("sessionID", session_id), ("inboxID", inbox_id)],
                        query: &pairs,
                        headers: &hdrs,
                        body: crate::transport::Body::Json(&body_value),
                        success: 204,
                        declared: &[400, 401, 404, 409],
                    })
                    .await
            }
        }
    }

    // Namespace `session::instructions` (mirrors the v2 SDK method group).
    pub mod instructions {
        pub struct SessionInstructionsApi<'a> {
            pub(crate) client: &'a crate::Client,
        }

        impl SessionInstructionsApi<'_> {
            /// GET /api/experimental/session/{sessionID}/instructions/entries — operationId `experimental.session.instructions.entry.list`
            /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `404` unknown
            /// The `{{ data }}` envelope is unwrapped (v2 client parity).
            pub async fn entry_list(
                &self,
                session_id: &str,
            ) -> Result<Vec<crate::types::InstructionEntryInfo>, crate::ClientError> {
                let pairs: Vec<(String, String)> = Vec::new();
                let hdrs: Vec<(&'static str, String)> = Vec::new();
                self.client.execute_json::<crate::transport::Envelope<Vec<crate::types::InstructionEntryInfo>>>(
            &crate::transport::Call {
                method: reqwest::Method::GET,
                path: "/api/experimental/session/{sessionID}/instructions/entries",
                path_params: &[("sessionID", session_id)],
                query: &pairs,
                headers: &hdrs,
                body: crate::transport::Body::None,
                success: 200,
                declared: &[400, 401, 404],
            }
        )
        .await
        .map(|env| env.data)
            }

            /// DELETE /api/experimental/session/{sessionID}/instructions/entries/{key} — operationId `experimental.session.instructions.entry.remove`
            /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `404` unknown
            pub async fn entry_remove(
                &self,
                session_id: &str,
                key: &str,
            ) -> Result<(), crate::ClientError> {
                let pairs: Vec<(String, String)> = Vec::new();
                let hdrs: Vec<(&'static str, String)> = Vec::new();
                self.client
                    .execute_empty(&crate::transport::Call {
                        method: reqwest::Method::DELETE,
                        path: "/api/experimental/session/{sessionID}/instructions/entries/{key}",
                        path_params: &[("sessionID", session_id), ("key", key)],
                        query: &pairs,
                        headers: &hdrs,
                        body: crate::transport::Body::None,
                        success: 204,
                        declared: &[400, 401, 404],
                    })
                    .await
            }

            /// PUT /api/experimental/session/{sessionID}/instructions/entries/{key} — operationId `experimental.session.instructions.entry.put`
            /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `404` unknown, `413` InstructionEntryValueTooLargeErrorEncoded
            pub async fn entry_put(
                &self,
                session_id: &str,
                key: &str,
                body: &crate::types::ExperimentalSessionInstructionsEntryPutBody,
            ) -> Result<(), crate::ClientError> {
                let pairs: Vec<(String, String)> = Vec::new();
                let hdrs: Vec<(&'static str, String)> = Vec::new();
                let body_value = crate::transport::encode_body(body)?;
                self.client
                    .execute_empty(&crate::transport::Call {
                        method: reqwest::Method::PUT,
                        path: "/api/experimental/session/{sessionID}/instructions/entries/{key}",
                        path_params: &[("sessionID", session_id), ("key", key)],
                        query: &pairs,
                        headers: &hdrs,
                        body: crate::transport::Body::Json(&body_value),
                        success: 204,
                        declared: &[400, 401, 404, 413],
                    })
                    .await
            }
        }
    }

    // Namespace `session::revert` (mirrors the v2 SDK method group).
    pub mod revert {
        pub struct SessionRevertApi<'a> {
            pub(crate) client: &'a crate::Client,
        }

        impl SessionRevertApi<'_> {
            /// DELETE /api/session/{sessionID}/revert — operationId `session.revert.clear`
            /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `404` unknown, `409` SessionBusyErrorEncoded, `500` UnknownErrorEncoded
            pub async fn clear(&self, session_id: &str) -> Result<(), crate::ClientError> {
                let pairs: Vec<(String, String)> = Vec::new();
                let hdrs: Vec<(&'static str, String)> = Vec::new();
                self.client
                    .execute_empty(&crate::transport::Call {
                        method: reqwest::Method::DELETE,
                        path: "/api/session/{sessionID}/revert",
                        path_params: &[("sessionID", session_id)],
                        query: &pairs,
                        headers: &hdrs,
                        body: crate::transport::Body::None,
                        success: 204,
                        declared: &[400, 401, 404, 409, 500],
                    })
                    .await
            }

            /// POST /api/session/{sessionID}/revert/commit — operationId `session.revert.commit`
            /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `404` unknown, `409` SessionBusyErrorEncoded
            pub async fn commit(&self, session_id: &str) -> Result<(), crate::ClientError> {
                let pairs: Vec<(String, String)> = Vec::new();
                let hdrs: Vec<(&'static str, String)> = Vec::new();
                self.client
                    .execute_empty(&crate::transport::Call {
                        method: reqwest::Method::POST,
                        path: "/api/session/{sessionID}/revert/commit",
                        path_params: &[("sessionID", session_id)],
                        query: &pairs,
                        headers: &hdrs,
                        body: crate::transport::Body::None,
                        success: 204,
                        declared: &[400, 401, 404, 409],
                    })
                    .await
            }

            /// POST /api/session/{sessionID}/revert/stage — operationId `session.revert.stage`
            /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `404` unknown, `409` SessionBusyErrorEncoded, `500` UnknownErrorEncoded
            /// The `{{ data }}` envelope is unwrapped (v2 client parity).
            pub async fn stage(
                &self,
                session_id: &str,
                body: &crate::types::SessionRevertStageBody,
            ) -> Result<crate::types::SessionRevert, crate::ClientError> {
                let pairs: Vec<(String, String)> = Vec::new();
                let hdrs: Vec<(&'static str, String)> = Vec::new();
                let body_value = crate::transport::encode_body(body)?;
                self.client
                    .execute_json::<crate::transport::Envelope<crate::types::SessionRevert>>(
                        &crate::transport::Call {
                            method: reqwest::Method::POST,
                            path: "/api/session/{sessionID}/revert/stage",
                            path_params: &[("sessionID", session_id)],
                            query: &pairs,
                            headers: &hdrs,
                            body: crate::transport::Body::Json(&body_value),
                            success: 200,
                            declared: &[400, 401, 404, 409, 500],
                        },
                    )
                    .await
                    .map(|env| env.data)
            }
        }
    }

    pub struct SessionApi<'a> {
        pub(crate) client: &'a crate::Client,
    }

    impl SessionApi<'_> {
        /// POST /api/experimental/session/import — operationId `experimental.session.import`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `404` SessionNotFoundErrorEncoded, `409` ConflictErrorEncoded
        /// The `{{ data }}` envelope is unwrapped (v2 client parity).
        pub async fn import(
            &self,
            body: &crate::types::ExperimentalSessionImportBody,
        ) -> Result<crate::types::SessionInfo, crate::ClientError> {
            let pairs: Vec<(String, String)> = Vec::new();
            let hdrs: Vec<(&'static str, String)> = Vec::new();
            let body_value = crate::transport::encode_body(body)?;
            self.client
                .execute_json::<crate::transport::Envelope<crate::types::SessionInfo>>(
                    &crate::transport::Call {
                        method: reqwest::Method::POST,
                        path: "/api/experimental/session/import",
                        path_params: &[],
                        query: &pairs,
                        headers: &hdrs,
                        body: crate::transport::Body::Json(&body_value),
                        success: 200,
                        declared: &[400, 401, 404, 409],
                    },
                )
                .await
                .map(|env| env.data)
        }

        /// GET /api/experimental/session/stats — operationId `experimental.session.stats`
        /// Declared errors: `400` unknown, `401` UnauthorizedErrorEncoded
        /// The `{{ data }}` envelope is unwrapped (v2 client parity).
        pub async fn stats(
            &self,
            query: &crate::types::ExperimentalSessionStatsQuery,
        ) -> Result<crate::types::SessionStatsInfo, crate::ClientError> {
            let mut pairs: Vec<(String, String)> = Vec::new();
            if let Some(v) = &query.from {
                pairs.push(("from".to_owned(), v.clone()));
            }
            if let Some(v) = &query.project {
                pairs.push(("project".to_owned(), v.clone()));
            }
            if let Some(v) = &query.timezone {
                pairs.push(("timezone".to_owned(), v.clone()));
            }
            if let Some(v) = &query.to {
                pairs.push(("to".to_owned(), v.clone()));
            }
            if let Some(v) = &query.tools {
                pairs.push(("tools".to_owned(), v.as_str().to_owned()));
            }
            let mut hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_json::<crate::transport::Envelope<crate::types::SessionStatsInfo>>(
                    &crate::transport::Call {
                        method: reqwest::Method::GET,
                        path: "/api/experimental/session/stats",
                        path_params: &[],
                        query: &pairs,
                        headers: &hdrs,
                        body: crate::transport::Body::None,
                        success: 200,
                        declared: &[400, 401],
                    },
                )
                .await
                .map(|env| env.data)
        }

        /// GET /api/experimental/session/{sessionID}/export — operationId `experimental.session.export`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `404` SessionNotFoundErrorEncoded, `500` UnknownErrorEncoded
        /// The `{{ data }}` envelope is unwrapped (v2 client parity).
        pub async fn export(
            &self,
            session_id: &str,
            query: &crate::types::ExperimentalSessionExportQuery,
        ) -> Result<crate::types::SessionTransferData, crate::ClientError> {
            let mut pairs: Vec<(String, String)> = Vec::new();
            if let Some(v) = &query.sanitize {
                pairs.push(("sanitize".to_owned(), v.as_str().to_owned()));
            }
            let mut hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_json::<crate::transport::Envelope<crate::types::SessionTransferData>>(
                    &crate::transport::Call {
                        method: reqwest::Method::GET,
                        path: "/api/experimental/session/{sessionID}/export",
                        path_params: &[("sessionID", session_id)],
                        query: &pairs,
                        headers: &hdrs,
                        body: crate::transport::Body::None,
                        success: 200,
                        declared: &[400, 401, 404, 500],
                    },
                )
                .await
                .map(|env| env.data)
        }

        /// GET /api/experimental/session/{sessionID}/log — operationId `session.log`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `404` SessionNotFoundErrorEncoded
        /// SSE stream; `data` frames are JSON-in-string payloads, double-decoded by
        /// `SseStream` (server-deep-map §7).
        pub async fn log(
            &self,
            session_id: &str,
            query: &crate::types::SessionLogQuery,
        ) -> Result<crate::sse::SseStream, crate::ClientError> {
            let mut pairs: Vec<(String, String)> = Vec::new();
            if let Some(v) = &query.after {
                pairs.push(("after".to_owned(), v.clone()));
            }
            if let Some(v) = &query.follow {
                pairs.push(("follow".to_owned(), v.as_str().to_owned()));
            }
            let mut hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_sse(
                    &crate::transport::Call {
                        method: reqwest::Method::GET,
                        path: "/api/experimental/session/{sessionID}/log",
                        path_params: &[("sessionID", session_id)],
                        query: &pairs,
                        headers: &hdrs,
                        body: crate::transport::Body::None,
                        success: 200,
                        declared: &[400, 401, 404],
                    },
                    true,
                )
                .await
        }

        /// POST /api/experimental/session/{sessionID}/skill — operationId `experimental.session.skill`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `404` unknown
        pub async fn skill(
            &self,
            session_id: &str,
            body: &crate::types::ExperimentalSessionSkillBody,
        ) -> Result<(), crate::ClientError> {
            let pairs: Vec<(String, String)> = Vec::new();
            let hdrs: Vec<(&'static str, String)> = Vec::new();
            let body_value = crate::transport::encode_body(body)?;
            self.client
                .execute_empty(&crate::transport::Call {
                    method: reqwest::Method::POST,
                    path: "/api/experimental/session/{sessionID}/skill",
                    path_params: &[("sessionID", session_id)],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::Json(&body_value),
                    success: 204,
                    declared: &[400, 401, 404],
                })
                .await
        }

        /// POST /api/experimental/session/{sessionID}/wait — operationId `experimental.session.wait`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `404` unknown, `503` ServiceUnavailableErrorEncoded
        pub async fn wait(&self, session_id: &str) -> Result<(), crate::ClientError> {
            let pairs: Vec<(String, String)> = Vec::new();
            let hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_empty(&crate::transport::Call {
                    method: reqwest::Method::POST,
                    path: "/api/experimental/session/{sessionID}/wait",
                    path_params: &[("sessionID", session_id)],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::None,
                    success: 204,
                    declared: &[400, 401, 404, 503],
                })
                .await
        }

        /// GET /api/session — operationId `session.list`
        /// Declared errors: `400` unknown, `401` UnauthorizedErrorEncoded
        pub async fn list(
            &self,
            query: &crate::types::SessionListQuery,
        ) -> Result<crate::types::SessionsResponse, crate::ClientError> {
            let mut pairs: Vec<(String, String)> = Vec::new();
            if let Some(v) = &query.cursor {
                pairs.push(("cursor".to_owned(), v.clone()));
            }
            if let Some(v) = &query.directory {
                pairs.push(("directory".to_owned(), v.clone()));
            }
            if let Some(v) = &query.limit {
                pairs.push(("limit".to_owned(), v.clone()));
            }
            if let Some(v) = &query.order {
                pairs.push(("order".to_owned(), v.as_str().to_owned()));
            }
            if let Some(v) = &query.parent_id {
                pairs.push(("parentID".to_owned(), v.clone()));
            }
            if let Some(v) = &query.project {
                pairs.push(("project".to_owned(), v.clone()));
            }
            if let Some(v) = &query.search {
                pairs.push(("search".to_owned(), v.clone()));
            }
            if let Some(v) = &query.subpath {
                pairs.push(("subpath".to_owned(), v.clone()));
            }
            let mut hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_json(&crate::transport::Call {
                    method: reqwest::Method::GET,
                    path: "/api/session",
                    path_params: &[],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::None,
                    success: 200,
                    declared: &[400, 401],
                })
                .await
        }

        /// POST /api/session — operationId `session.create`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `404` SessionNotFoundErrorEncoded
        /// The `{{ data }}` envelope is unwrapped (v2 client parity).
        pub async fn create(
            &self,
            body: &crate::types::SessionCreateBody,
        ) -> Result<crate::types::SessionInfo, crate::ClientError> {
            let pairs: Vec<(String, String)> = Vec::new();
            let hdrs: Vec<(&'static str, String)> = Vec::new();
            let body_value = crate::transport::encode_body(body)?;
            self.client
                .execute_json::<crate::transport::Envelope<crate::types::SessionInfo>>(
                    &crate::transport::Call {
                        method: reqwest::Method::POST,
                        path: "/api/session",
                        path_params: &[],
                        query: &pairs,
                        headers: &hdrs,
                        body: crate::transport::Body::Json(&body_value),
                        success: 200,
                        declared: &[400, 401, 404],
                    },
                )
                .await
                .map(|env| env.data)
        }

        /// GET /api/session/active — operationId `session.active`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded
        /// The `{{ data }}` envelope is unwrapped (v2 client parity).
        pub async fn active(
            &self,
        ) -> Result<std::collections::BTreeMap<String, serde_json::Value>, crate::ClientError>
        {
            let pairs: Vec<(String, String)> = Vec::new();
            let hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client.execute_json::<crate::transport::Envelope<std::collections::BTreeMap<String, serde_json::Value>>>(
            &crate::transport::Call {
                method: reqwest::Method::GET,
                path: "/api/session/active",
                path_params: &[],
                query: &pairs,
                headers: &hdrs,
                body: crate::transport::Body::None,
                success: 200,
                declared: &[400, 401],
            }
        )
        .await
        .map(|env| env.data)
        }

        /// DELETE /api/session/{sessionID} — operationId `session.remove`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `404` SessionNotFoundErrorEncoded
        pub async fn remove(&self, session_id: &str) -> Result<(), crate::ClientError> {
            let pairs: Vec<(String, String)> = Vec::new();
            let hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_empty(&crate::transport::Call {
                    method: reqwest::Method::DELETE,
                    path: "/api/session/{sessionID}",
                    path_params: &[("sessionID", session_id)],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::None,
                    success: 204,
                    declared: &[400, 401, 404],
                })
                .await
        }

        /// GET /api/session/{sessionID} — operationId `session.get`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `404` SessionNotFoundErrorEncoded
        /// The `{{ data }}` envelope is unwrapped (v2 client parity).
        pub async fn get(
            &self,
            session_id: &str,
        ) -> Result<crate::types::SessionInfo, crate::ClientError> {
            let pairs: Vec<(String, String)> = Vec::new();
            let hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_json::<crate::transport::Envelope<crate::types::SessionInfo>>(
                    &crate::transport::Call {
                        method: reqwest::Method::GET,
                        path: "/api/session/{sessionID}",
                        path_params: &[("sessionID", session_id)],
                        query: &pairs,
                        headers: &hdrs,
                        body: crate::transport::Body::None,
                        success: 200,
                        declared: &[400, 401, 404],
                    },
                )
                .await
                .map(|env| env.data)
        }

        /// PATCH /api/session/{sessionID} — operationId `session.update`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `404` unknown
        pub async fn update(
            &self,
            session_id: &str,
            body: &crate::types::SessionUpdateBody,
        ) -> Result<(), crate::ClientError> {
            let pairs: Vec<(String, String)> = Vec::new();
            let hdrs: Vec<(&'static str, String)> = Vec::new();
            let body_value = crate::transport::encode_body(body)?;
            self.client
                .execute_empty(&crate::transport::Call {
                    method: reqwest::Method::PATCH,
                    path: "/api/session/{sessionID}",
                    path_params: &[("sessionID", session_id)],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::Json(&body_value),
                    success: 204,
                    declared: &[400, 401, 404],
                })
                .await
        }

        /// POST /api/session/{sessionID}/agent — operationId `session.switchAgent`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `404` unknown
        pub async fn switch_agent(
            &self,
            session_id: &str,
            body: &crate::types::SessionSwitchAgentBody,
        ) -> Result<(), crate::ClientError> {
            let pairs: Vec<(String, String)> = Vec::new();
            let hdrs: Vec<(&'static str, String)> = Vec::new();
            let body_value = crate::transport::encode_body(body)?;
            self.client
                .execute_empty(&crate::transport::Call {
                    method: reqwest::Method::POST,
                    path: "/api/session/{sessionID}/agent",
                    path_params: &[("sessionID", session_id)],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::Json(&body_value),
                    success: 204,
                    declared: &[400, 401, 404],
                })
                .await
        }

        /// POST /api/session/{sessionID}/background — operationId `session.background`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `404` unknown
        pub async fn background(&self, session_id: &str) -> Result<(), crate::ClientError> {
            let pairs: Vec<(String, String)> = Vec::new();
            let hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_empty(&crate::transport::Call {
                    method: reqwest::Method::POST,
                    path: "/api/session/{sessionID}/background",
                    path_params: &[("sessionID", session_id)],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::None,
                    success: 204,
                    declared: &[400, 401, 404],
                })
                .await
        }

        /// POST /api/session/{sessionID}/command — operationId `session.command`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `404` unknown, `500` CommandExecutionErrorEncoded
        pub async fn command(
            &self,
            session_id: &str,
            body: &crate::types::SessionCommandBody,
        ) -> Result<(), crate::ClientError> {
            let pairs: Vec<(String, String)> = Vec::new();
            let hdrs: Vec<(&'static str, String)> = Vec::new();
            let body_value = crate::transport::encode_body(body)?;
            self.client
                .execute_empty(&crate::transport::Call {
                    method: reqwest::Method::POST,
                    path: "/api/session/{sessionID}/command",
                    path_params: &[("sessionID", session_id)],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::Json(&body_value),
                    success: 204,
                    declared: &[400, 401, 404, 500],
                })
                .await
        }

        /// POST /api/session/{sessionID}/compact — operationId `session.compact`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `404` unknown, `409` ConflictErrorEncoded
        /// The `{{ data }}` envelope is unwrapped (v2 client parity).
        pub async fn compact(
            &self,
            session_id: &str,
            body: &crate::types::SessionCompactBody,
        ) -> Result<crate::types::SessionInboxCompaction, crate::ClientError> {
            let pairs: Vec<(String, String)> = Vec::new();
            let hdrs: Vec<(&'static str, String)> = Vec::new();
            let body_value = crate::transport::encode_body(body)?;
            self.client
                .execute_json::<crate::transport::Envelope<crate::types::SessionInboxCompaction>>(
                    &crate::transport::Call {
                        method: reqwest::Method::POST,
                        path: "/api/session/{sessionID}/compact",
                        path_params: &[("sessionID", session_id)],
                        query: &pairs,
                        headers: &hdrs,
                        body: crate::transport::Body::Json(&body_value),
                        success: 200,
                        declared: &[400, 401, 404, 409],
                    },
                )
                .await
                .map(|env| env.data)
        }

        /// GET /api/session/{sessionID}/context — operationId `session.context`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `404` SessionNotFoundErrorEncoded, `500` UnknownErrorEncoded
        /// The `{{ data }}` envelope is unwrapped (v2 client parity).
        pub async fn context(
            &self,
            session_id: &str,
        ) -> Result<Vec<crate::types::SessionMessageInfo>, crate::ClientError> {
            let pairs: Vec<(String, String)> = Vec::new();
            let hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_json::<crate::transport::Envelope<Vec<crate::types::SessionMessageInfo>>>(
                    &crate::transport::Call {
                        method: reqwest::Method::GET,
                        path: "/api/session/{sessionID}/context",
                        path_params: &[("sessionID", session_id)],
                        query: &pairs,
                        headers: &hdrs,
                        body: crate::transport::Body::None,
                        success: 200,
                        declared: &[400, 401, 404, 500],
                    },
                )
                .await
                .map(|env| env.data)
        }

        /// GET /api/session/{sessionID}/diff — operationId `session.diff`
        /// Declared errors: `400` unknown, `401` UnauthorizedErrorEncoded, `404` unknown, `500` UnknownErrorEncoded
        /// The `{{ data }}` envelope is unwrapped (v2 client parity).
        pub async fn diff(
            &self,
            session_id: &str,
            query: &crate::types::SessionDiffQuery,
        ) -> Result<Vec<crate::types::FileDiffInfo>, crate::ClientError> {
            let mut pairs: Vec<(String, String)> = Vec::new();
            if let Some(v) = &query.context {
                pairs.push(("context".to_owned(), v.clone()));
            }
            if let Some(v) = &query.from {
                pairs.push(("from".to_owned(), v.clone()));
            }
            if let Some(v) = &query.to {
                pairs.push(("to".to_owned(), v.clone()));
            }
            let mut hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_json::<crate::transport::Envelope<Vec<crate::types::FileDiffInfo>>>(
                    &crate::transport::Call {
                        method: reqwest::Method::GET,
                        path: "/api/session/{sessionID}/diff",
                        path_params: &[("sessionID", session_id)],
                        query: &pairs,
                        headers: &hdrs,
                        body: crate::transport::Body::None,
                        success: 200,
                        declared: &[400, 401, 404, 500],
                    },
                )
                .await
                .map(|env| env.data)
        }

        /// PUT /api/session/{sessionID}/environment — operationId `session.environment`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `404` SessionNotFoundErrorEncoded
        pub async fn environment(
            &self,
            session_id: &str,
            body: &crate::types::SessionEnvironmentBody,
        ) -> Result<(), crate::ClientError> {
            let pairs: Vec<(String, String)> = Vec::new();
            let hdrs: Vec<(&'static str, String)> = Vec::new();
            let body_value = crate::transport::encode_body(body)?;
            self.client
                .execute_empty(&crate::transport::Call {
                    method: reqwest::Method::PUT,
                    path: "/api/session/{sessionID}/environment",
                    path_params: &[("sessionID", session_id)],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::Json(&body_value),
                    success: 204,
                    declared: &[400, 401, 404],
                })
                .await
        }

        /// POST /api/session/{sessionID}/fork — operationId `session.fork`
        /// Declared errors: `400` unknown, `401` UnauthorizedErrorEncoded, `404` unknown
        /// The `{{ data }}` envelope is unwrapped (v2 client parity).
        pub async fn fork(
            &self,
            session_id: &str,
            body: &crate::types::SessionForkBody,
        ) -> Result<crate::types::SessionInfo, crate::ClientError> {
            let pairs: Vec<(String, String)> = Vec::new();
            let hdrs: Vec<(&'static str, String)> = Vec::new();
            let body_value = crate::transport::encode_body(body)?;
            self.client
                .execute_json::<crate::transport::Envelope<crate::types::SessionInfo>>(
                    &crate::transport::Call {
                        method: reqwest::Method::POST,
                        path: "/api/session/{sessionID}/fork",
                        path_params: &[("sessionID", session_id)],
                        query: &pairs,
                        headers: &hdrs,
                        body: crate::transport::Body::Json(&body_value),
                        success: 200,
                        declared: &[400, 401, 404],
                    },
                )
                .await
                .map(|env| env.data)
        }

        /// POST /api/session/{sessionID}/generate — operationId `session.generate`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `404` unknown, `503` ServiceUnavailableErrorEncoded
        pub async fn generate(
            &self,
            session_id: &str,
            body: &crate::types::SessionGenerateBody,
        ) -> Result<crate::types::SessionGenerateResponse, crate::ClientError> {
            let pairs: Vec<(String, String)> = Vec::new();
            let hdrs: Vec<(&'static str, String)> = Vec::new();
            let body_value = crate::transport::encode_body(body)?;
            self.client
                .execute_json(&crate::transport::Call {
                    method: reqwest::Method::POST,
                    path: "/api/session/{sessionID}/generate",
                    path_params: &[("sessionID", session_id)],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::Json(&body_value),
                    success: 200,
                    declared: &[400, 401, 404, 503],
                })
                .await
        }

        /// POST /api/session/{sessionID}/interrupt — operationId `session.interrupt`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `404` unknown
        pub async fn interrupt(
            &self,
            session_id: &str,
            query: &crate::types::SessionInterruptQuery,
        ) -> Result<crate::types::SessionInterruptResponse, crate::ClientError> {
            let mut pairs: Vec<(String, String)> = Vec::new();
            if let Some(v) = &query.resume {
                pairs.push(("resume".to_owned(), v.as_str().to_owned()));
            }
            let mut hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_json(&crate::transport::Call {
                    method: reqwest::Method::POST,
                    path: "/api/session/{sessionID}/interrupt",
                    path_params: &[("sessionID", session_id)],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::None,
                    success: 200,
                    declared: &[400, 401, 404],
                })
                .await
        }

        /// POST /api/session/{sessionID}/model — operationId `session.switchModel`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `404` unknown
        pub async fn switch_model(
            &self,
            session_id: &str,
            body: &crate::types::SessionSwitchModelBody,
        ) -> Result<(), crate::ClientError> {
            let pairs: Vec<(String, String)> = Vec::new();
            let hdrs: Vec<(&'static str, String)> = Vec::new();
            let body_value = crate::transport::encode_body(body)?;
            self.client
                .execute_empty(&crate::transport::Call {
                    method: reqwest::Method::POST,
                    path: "/api/session/{sessionID}/model",
                    path_params: &[("sessionID", session_id)],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::Json(&body_value),
                    success: 204,
                    declared: &[400, 401, 404],
                })
                .await
        }

        /// POST /api/session/{sessionID}/move — operationId `session.move`
        /// Declared errors: `400` unknown, `401` UnauthorizedErrorEncoded, `404` SessionNotFoundErrorEncoded
        pub async fn move_(
            &self,
            session_id: &str,
            body: &crate::types::SessionMoveBody,
        ) -> Result<(), crate::ClientError> {
            let pairs: Vec<(String, String)> = Vec::new();
            let hdrs: Vec<(&'static str, String)> = Vec::new();
            let body_value = crate::transport::encode_body(body)?;
            self.client
                .execute_empty(&crate::transport::Call {
                    method: reqwest::Method::POST,
                    path: "/api/session/{sessionID}/move",
                    path_params: &[("sessionID", session_id)],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::Json(&body_value),
                    success: 204,
                    declared: &[400, 401, 404],
                })
                .await
        }

        /// GET /api/session/{sessionID}/permission — operationId `session.permission.list`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `404` SessionNotFoundErrorEncoded
        /// The `{{ data }}` envelope is unwrapped (v2 client parity).
        pub async fn permission_list(
            &self,
            session_id: &str,
        ) -> Result<Vec<crate::types::PermissionRequest>, crate::ClientError> {
            let pairs: Vec<(String, String)> = Vec::new();
            let hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_json::<crate::transport::Envelope<Vec<crate::types::PermissionRequest>>>(
                    &crate::transport::Call {
                        method: reqwest::Method::GET,
                        path: "/api/session/{sessionID}/permission",
                        path_params: &[("sessionID", session_id)],
                        query: &pairs,
                        headers: &hdrs,
                        body: crate::transport::Body::None,
                        success: 200,
                        declared: &[400, 401, 404],
                    },
                )
                .await
                .map(|env| env.data)
        }

        /// POST /api/session/{sessionID}/permission — operationId `session.permission.create`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `404` unknown
        /// The `{{ data }}` envelope is unwrapped (v2 client parity).
        pub async fn permission_create(
            &self,
            session_id: &str,
            body: &crate::types::SessionPermissionCreateBody,
        ) -> Result<crate::types::SessionPermissionCreateResponseData, crate::ClientError> {
            let pairs: Vec<(String, String)> = Vec::new();
            let hdrs: Vec<(&'static str, String)> = Vec::new();
            let body_value = crate::transport::encode_body(body)?;
            self.client.execute_json::<crate::transport::Envelope<crate::types::SessionPermissionCreateResponseData>>(
            &crate::transport::Call {
                method: reqwest::Method::POST,
                path: "/api/session/{sessionID}/permission",
                path_params: &[("sessionID", session_id)],
                query: &pairs,
                headers: &hdrs,
                body: crate::transport::Body::Json(&body_value),
                success: 200,
                declared: &[400, 401, 404],
            }
        )
        .await
        .map(|env| env.data)
        }

        /// GET /api/session/{sessionID}/permission/{requestID} — operationId `session.permission.get`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `404` unknown
        /// The `{{ data }}` envelope is unwrapped (v2 client parity).
        pub async fn permission_get(
            &self,
            session_id: &str,
            request_id: &str,
        ) -> Result<crate::types::PermissionRequest, crate::ClientError> {
            let pairs: Vec<(String, String)> = Vec::new();
            let hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_json::<crate::transport::Envelope<crate::types::PermissionRequest>>(
                    &crate::transport::Call {
                        method: reqwest::Method::GET,
                        path: "/api/session/{sessionID}/permission/{requestID}",
                        path_params: &[("sessionID", session_id), ("requestID", request_id)],
                        query: &pairs,
                        headers: &hdrs,
                        body: crate::transport::Body::None,
                        success: 200,
                        declared: &[400, 401, 404],
                    },
                )
                .await
                .map(|env| env.data)
        }

        /// POST /api/session/{sessionID}/permission/{requestID}/reply — operationId `session.permission.reply`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `404` unknown
        pub async fn permission_reply(
            &self,
            session_id: &str,
            request_id: &str,
            body: &crate::types::SessionPermissionReplyBody,
        ) -> Result<(), crate::ClientError> {
            let pairs: Vec<(String, String)> = Vec::new();
            let hdrs: Vec<(&'static str, String)> = Vec::new();
            let body_value = crate::transport::encode_body(body)?;
            self.client
                .execute_empty(&crate::transport::Call {
                    method: reqwest::Method::POST,
                    path: "/api/session/{sessionID}/permission/{requestID}/reply",
                    path_params: &[("sessionID", session_id), ("requestID", request_id)],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::Json(&body_value),
                    success: 204,
                    declared: &[400, 401, 404],
                })
                .await
        }

        /// POST /api/session/{sessionID}/prompt — operationId `session.prompt`
        /// Declared errors: `400` unknown, `401` UnauthorizedErrorEncoded, `404` unknown, `409` ConflictErrorEncoded
        /// The `{{ data }}` envelope is unwrapped (v2 client parity).
        pub async fn prompt(
            &self,
            session_id: &str,
            body: &crate::types::SessionPromptBody,
        ) -> Result<crate::types::SessionInboxUser, crate::ClientError> {
            let pairs: Vec<(String, String)> = Vec::new();
            let hdrs: Vec<(&'static str, String)> = Vec::new();
            let body_value = crate::transport::encode_body(body)?;
            self.client
                .execute_json::<crate::transport::Envelope<crate::types::SessionInboxUser>>(
                    &crate::transport::Call {
                        method: reqwest::Method::POST,
                        path: "/api/session/{sessionID}/prompt",
                        path_params: &[("sessionID", session_id)],
                        query: &pairs,
                        headers: &hdrs,
                        body: crate::transport::Body::Json(&body_value),
                        success: 200,
                        declared: &[400, 401, 404, 409],
                    },
                )
                .await
                .map(|env| env.data)
        }

        /// POST /api/session/{sessionID}/shell — operationId `session.shell`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `404` unknown
        pub async fn shell(
            &self,
            session_id: &str,
            body: &crate::types::SessionShellBody,
        ) -> Result<(), crate::ClientError> {
            let pairs: Vec<(String, String)> = Vec::new();
            let hdrs: Vec<(&'static str, String)> = Vec::new();
            let body_value = crate::transport::encode_body(body)?;
            self.client
                .execute_empty(&crate::transport::Call {
                    method: reqwest::Method::POST,
                    path: "/api/session/{sessionID}/shell",
                    path_params: &[("sessionID", session_id)],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::Json(&body_value),
                    success: 204,
                    declared: &[400, 401, 404],
                })
                .await
        }

        /// POST /api/session/{sessionID}/synthetic — operationId `session.synthetic`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `404` unknown, `409` ConflictErrorEncoded
        /// The `{{ data }}` envelope is unwrapped (v2 client parity).
        pub async fn synthetic(
            &self,
            session_id: &str,
            body: &crate::types::SessionSyntheticBody,
        ) -> Result<crate::types::SessionInboxSynthetic, crate::ClientError> {
            let pairs: Vec<(String, String)> = Vec::new();
            let hdrs: Vec<(&'static str, String)> = Vec::new();
            let body_value = crate::transport::encode_body(body)?;
            self.client
                .execute_json::<crate::transport::Envelope<crate::types::SessionInboxSynthetic>>(
                    &crate::transport::Call {
                        method: reqwest::Method::POST,
                        path: "/api/session/{sessionID}/synthetic",
                        path_params: &[("sessionID", session_id)],
                        query: &pairs,
                        headers: &hdrs,
                        body: crate::transport::Body::Json(&body_value),
                        success: 200,
                        declared: &[400, 401, 404, 409],
                    },
                )
                .await
                .map(|env| env.data)
        }

        /// POST /api/session/{sessionID}/view — operationId `session.view`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `404` SessionNotFoundErrorEncoded
        pub async fn view(
            &self,
            session_id: &str,
            body: &crate::types::SessionViewBody,
        ) -> Result<(), crate::ClientError> {
            let pairs: Vec<(String, String)> = Vec::new();
            let hdrs: Vec<(&'static str, String)> = Vec::new();
            let body_value = crate::transport::encode_body(body)?;
            self.client
                .execute_empty(&crate::transport::Call {
                    method: reqwest::Method::POST,
                    path: "/api/session/{sessionID}/view",
                    path_params: &[("sessionID", session_id)],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::Json(&body_value),
                    success: 204,
                    declared: &[400, 401, 404],
                })
                .await
        }

        /// Nested namespace `session::form`.
        #[must_use]
        pub fn form(&self) -> form::SessionFormApi<'_> {
            form::SessionFormApi { client: self.client }
        }
        /// Nested namespace `session::inbox`.
        #[must_use]
        pub fn inbox(&self) -> inbox::SessionInboxApi<'_> {
            inbox::SessionInboxApi { client: self.client }
        }
        /// Nested namespace `session::instructions`.
        #[must_use]
        pub fn instructions(&self) -> instructions::SessionInstructionsApi<'_> {
            instructions::SessionInstructionsApi { client: self.client }
        }
        /// Nested namespace `session::revert`.
        #[must_use]
        pub fn revert(&self) -> revert::SessionRevertApi<'_> {
            revert::SessionRevertApi { client: self.client }
        }
    }

    impl crate::Client {
        /// Namespace `session` operations.
        #[must_use]
        pub fn session(&self) -> SessionApi<'_> {
            SessionApi { client: self }
        }
    }
}

// Namespace `shell` (mirrors the v2 SDK method group).
pub mod shell {
    pub struct ShellApi<'a> {
        pub(crate) client: &'a crate::Client,
    }

    impl ShellApi<'_> {
        /// GET /api/shell — operationId `shell.list`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded
        pub async fn list(
            &self,
            query: &crate::types::ShellListQuery,
        ) -> Result<crate::types::ShellListResponse, crate::ClientError> {
            let mut pairs: Vec<(String, String)> = Vec::new();
            if let Some(loc) = &query.location {
                if let Some(dir) = &loc.directory {
                    pairs.push(("location[directory]".to_owned(), dir.clone()));
                }
            }
            let mut hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_json(&crate::transport::Call {
                    method: reqwest::Method::GET,
                    path: "/api/shell",
                    path_params: &[],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::None,
                    success: 200,
                    declared: &[400, 401],
                })
                .await
        }

        /// POST /api/shell — operationId `shell.create`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded
        pub async fn create(
            &self,
            query: &crate::types::ShellCreateQuery,
            body: &crate::types::ShellCreateBody,
        ) -> Result<crate::types::ShellCreateResponse, crate::ClientError> {
            let mut pairs: Vec<(String, String)> = Vec::new();
            if let Some(loc) = &query.location {
                if let Some(dir) = &loc.directory {
                    pairs.push(("location[directory]".to_owned(), dir.clone()));
                }
            }
            let mut hdrs: Vec<(&'static str, String)> = Vec::new();
            let body_value = crate::transport::encode_body(body)?;
            self.client
                .execute_json(&crate::transport::Call {
                    method: reqwest::Method::POST,
                    path: "/api/shell",
                    path_params: &[],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::Json(&body_value),
                    success: 200,
                    declared: &[400, 401],
                })
                .await
        }

        /// DELETE /api/shell/{id} — operationId `shell.remove`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded
        pub async fn remove(
            &self,
            id: &str,
            query: &crate::types::ShellRemoveQuery,
        ) -> Result<(), crate::ClientError> {
            let mut pairs: Vec<(String, String)> = Vec::new();
            if let Some(loc) = &query.location {
                if let Some(dir) = &loc.directory {
                    pairs.push(("location[directory]".to_owned(), dir.clone()));
                }
            }
            let mut hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_empty(&crate::transport::Call {
                    method: reqwest::Method::DELETE,
                    path: "/api/shell/{id}",
                    path_params: &[("id", id)],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::None,
                    success: 204,
                    declared: &[400, 401],
                })
                .await
        }

        /// GET /api/shell/{id} — operationId `shell.get`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `404` ShellNotFoundErrorEncoded
        pub async fn get(
            &self,
            id: &str,
            query: &crate::types::ShellGetQuery,
        ) -> Result<crate::types::ShellGetResponse, crate::ClientError> {
            let mut pairs: Vec<(String, String)> = Vec::new();
            if let Some(loc) = &query.location {
                if let Some(dir) = &loc.directory {
                    pairs.push(("location[directory]".to_owned(), dir.clone()));
                }
            }
            let mut hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_json(&crate::transport::Call {
                    method: reqwest::Method::GET,
                    path: "/api/shell/{id}",
                    path_params: &[("id", id)],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::None,
                    success: 200,
                    declared: &[400, 401, 404],
                })
                .await
        }

        /// GET /api/shell/{id}/output — operationId `shell.output`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `404` ShellNotFoundErrorEncoded
        pub async fn output(
            &self,
            id: &str,
            query: &crate::types::ShellOutputQuery,
        ) -> Result<crate::types::ShellOutputResponse, crate::ClientError> {
            let mut pairs: Vec<(String, String)> = Vec::new();
            if let Some(v) = &query.cursor {
                pairs.push(("cursor".to_owned(), v.clone()));
            }
            if let Some(v) = &query.limit {
                pairs.push(("limit".to_owned(), v.clone()));
            }
            if let Some(loc) = &query.location {
                if let Some(dir) = &loc.directory {
                    pairs.push(("location[directory]".to_owned(), dir.clone()));
                }
            }
            let mut hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_json(&crate::transport::Call {
                    method: reqwest::Method::GET,
                    path: "/api/shell/{id}/output",
                    path_params: &[("id", id)],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::None,
                    success: 200,
                    declared: &[400, 401, 404],
                })
                .await
        }
    }

    impl crate::Client {
        /// Namespace `shell` operations.
        #[must_use]
        pub fn shell(&self) -> ShellApi<'_> {
            ShellApi { client: self }
        }
    }
}

// Namespace `skill` (mirrors the v2 SDK method group).
pub mod skill {
    pub struct SkillApi<'a> {
        pub(crate) client: &'a crate::Client,
    }

    impl SkillApi<'_> {
        /// GET /api/skill — operationId `skill.list`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded
        pub async fn list(
            &self,
            query: &crate::types::SkillListQuery,
        ) -> Result<crate::types::SkillListResponse, crate::ClientError> {
            let mut pairs: Vec<(String, String)> = Vec::new();
            if let Some(loc) = &query.location {
                if let Some(dir) = &loc.directory {
                    pairs.push(("location[directory]".to_owned(), dir.clone()));
                }
            }
            let mut hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_json(&crate::transport::Call {
                    method: reqwest::Method::GET,
                    path: "/api/skill",
                    path_params: &[],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::None,
                    success: 200,
                    declared: &[400, 401],
                })
                .await
        }
    }

    impl crate::Client {
        /// Namespace `skill` operations.
        #[must_use]
        pub fn skill(&self) -> SkillApi<'_> {
            SkillApi { client: self }
        }
    }
}

// Namespace `vcs` (mirrors the v2 SDK method group).
pub mod vcs {
    // Namespace `vcs::branch` (mirrors the v2 SDK method group).
    pub mod branch {
        pub struct VcsBranchApi<'a> {
            pub(crate) client: &'a crate::Client,
        }

        impl VcsBranchApi<'_> {
            /// GET /api/vcs/branch — operationId `vcs.branch.list`
            /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded
            pub async fn list(
                &self,
                query: &crate::types::VcsBranchListQuery,
            ) -> Result<crate::types::VcsBranchListResponse, crate::ClientError> {
                let mut pairs: Vec<(String, String)> = Vec::new();
                if let Some(v) = &query.limit {
                    pairs.push(("limit".to_owned(), v.clone()));
                }
                if let Some(loc) = &query.location {
                    if let Some(dir) = &loc.directory {
                        pairs.push(("location[directory]".to_owned(), dir.clone()));
                    }
                }
                if let Some(v) = &query.search {
                    pairs.push(("search".to_owned(), v.clone()));
                }
                let mut hdrs: Vec<(&'static str, String)> = Vec::new();
                self.client
                    .execute_json(&crate::transport::Call {
                        method: reqwest::Method::GET,
                        path: "/api/vcs/branch",
                        path_params: &[],
                        query: &pairs,
                        headers: &hdrs,
                        body: crate::transport::Body::None,
                        success: 200,
                        declared: &[400, 401],
                    })
                    .await
            }
        }
    }

    pub struct VcsApi<'a> {
        pub(crate) client: &'a crate::Client,
    }

    impl VcsApi<'_> {
        /// GET /api/vcs — operationId `vcs.get`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded
        pub async fn get(
            &self,
            query: &crate::types::VcsGetQuery,
        ) -> Result<crate::types::VcsGetResponse, crate::ClientError> {
            let mut pairs: Vec<(String, String)> = Vec::new();
            if let Some(loc) = &query.location {
                if let Some(dir) = &loc.directory {
                    pairs.push(("location[directory]".to_owned(), dir.clone()));
                }
            }
            let mut hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_json(&crate::transport::Call {
                    method: reqwest::Method::GET,
                    path: "/api/vcs",
                    path_params: &[],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::None,
                    success: 200,
                    declared: &[400, 401],
                })
                .await
        }

        /// GET /api/vcs/base — operationId `vcs.base`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `503` ServiceUnavailableErrorEncoded
        pub async fn base(
            &self,
            query: &crate::types::VcsBaseQuery,
        ) -> Result<crate::types::VcsBaseResponse, crate::ClientError> {
            let mut pairs: Vec<(String, String)> = Vec::new();
            if let Some(loc) = &query.location {
                if let Some(dir) = &loc.directory {
                    pairs.push(("location[directory]".to_owned(), dir.clone()));
                }
            }
            let mut hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_json(&crate::transport::Call {
                    method: reqwest::Method::GET,
                    path: "/api/vcs/base",
                    path_params: &[],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::None,
                    success: 200,
                    declared: &[400, 401, 503],
                })
                .await
        }

        /// GET /api/vcs/diff — operationId `vcs.diff`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `503` ServiceUnavailableErrorEncoded
        pub async fn diff(
            &self,
            query: &crate::types::VcsDiffQuery,
        ) -> Result<crate::types::VcsDiffResponse, crate::ClientError> {
            let mut pairs: Vec<(String, String)> = Vec::new();
            if let Some(v) = &query.base {
                pairs.push(("base".to_owned(), v.clone()));
            }
            if let Some(v) = &query.context {
                pairs.push(("context".to_owned(), v.clone()));
            }
            if let Some(loc) = &query.location {
                if let Some(dir) = &loc.directory {
                    pairs.push(("location[directory]".to_owned(), dir.clone()));
                }
            }
            pairs.push(("mode".to_owned(), query.mode.as_str().to_owned()));
            let mut hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_json(&crate::transport::Call {
                    method: reqwest::Method::GET,
                    path: "/api/vcs/diff",
                    path_params: &[],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::None,
                    success: 200,
                    declared: &[400, 401, 503],
                })
                .await
        }

        /// POST /api/vcs/init — operationId `vcs.init`
        /// Declared errors: `400` unknown, `401` UnauthorizedErrorEncoded, `409` ConflictErrorEncoded, `501` VcsInitNotSupportedErrorEncoded, `503` ServiceUnavailableErrorEncoded
        pub async fn init(
            &self,
            query: &crate::types::VcsInitQuery,
        ) -> Result<(), crate::ClientError> {
            let mut pairs: Vec<(String, String)> = Vec::new();
            if let Some(loc) = &query.location {
                if let Some(dir) = &loc.directory {
                    pairs.push(("location[directory]".to_owned(), dir.clone()));
                }
            }
            if let Some(v) = &query.provider {
                pairs.push(("provider".to_owned(), v.clone()));
            }
            let mut hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_empty(&crate::transport::Call {
                    method: reqwest::Method::POST,
                    path: "/api/vcs/init",
                    path_params: &[],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::None,
                    success: 204,
                    declared: &[400, 401, 409, 501, 503],
                })
                .await
        }

        /// GET /api/vcs/status — operationId `vcs.status`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded
        pub async fn status(
            &self,
            query: &crate::types::VcsStatusQuery,
        ) -> Result<crate::types::VcsStatusResponse, crate::ClientError> {
            let mut pairs: Vec<(String, String)> = Vec::new();
            if let Some(loc) = &query.location {
                if let Some(dir) = &loc.directory {
                    pairs.push(("location[directory]".to_owned(), dir.clone()));
                }
            }
            let mut hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_json(&crate::transport::Call {
                    method: reqwest::Method::GET,
                    path: "/api/vcs/status",
                    path_params: &[],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::None,
                    success: 200,
                    declared: &[400, 401],
                })
                .await
        }

        /// Nested namespace `vcs::branch`.
        #[must_use]
        pub fn branch(&self) -> branch::VcsBranchApi<'_> {
            branch::VcsBranchApi { client: self.client }
        }
    }

    impl crate::Client {
        /// Namespace `vcs` operations.
        #[must_use]
        pub fn vcs(&self) -> VcsApi<'_> {
            VcsApi { client: self }
        }
    }
}

// Namespace `websearch` (mirrors the v2 SDK method group).
pub mod websearch {
    pub struct WebsearchApi<'a> {
        pub(crate) client: &'a crate::Client,
    }

    impl WebsearchApi<'_> {
        /// POST /api/websearch — operationId `websearch.query`
        /// Declared errors: `400` unknown, `401` UnauthorizedErrorEncoded, `503` ServiceUnavailableErrorEncoded
        pub async fn query(
            &self,
            query: &crate::types::WebsearchQueryQuery,
            body: &crate::types::WebsearchQueryBody,
        ) -> Result<crate::types::WebsearchQueryResponse, crate::ClientError> {
            let mut pairs: Vec<(String, String)> = Vec::new();
            if let Some(loc) = &query.location {
                if let Some(dir) = &loc.directory {
                    pairs.push(("location[directory]".to_owned(), dir.clone()));
                }
            }
            let mut hdrs: Vec<(&'static str, String)> = Vec::new();
            let body_value = crate::transport::encode_body(body)?;
            self.client
                .execute_json(&crate::transport::Call {
                    method: reqwest::Method::POST,
                    path: "/api/websearch",
                    path_params: &[],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::Json(&body_value),
                    success: 200,
                    declared: &[400, 401, 503],
                })
                .await
        }

        /// GET /api/websearch/provider — operationId `websearch.providers`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `503` ServiceUnavailableErrorEncoded
        pub async fn providers(
            &self,
            query: &crate::types::WebsearchProvidersQuery,
        ) -> Result<crate::types::WebsearchProvidersResponse, crate::ClientError> {
            let mut pairs: Vec<(String, String)> = Vec::new();
            if let Some(loc) = &query.location {
                if let Some(dir) = &loc.directory {
                    pairs.push(("location[directory]".to_owned(), dir.clone()));
                }
            }
            let mut hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_json(&crate::transport::Call {
                    method: reqwest::Method::GET,
                    path: "/api/websearch/provider",
                    path_params: &[],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::None,
                    success: 200,
                    declared: &[400, 401, 503],
                })
                .await
        }
    }

    impl crate::Client {
        /// Namespace `websearch` operations.
        #[must_use]
        pub fn websearch(&self) -> WebsearchApi<'_> {
            WebsearchApi { client: self }
        }
    }
}

// Namespace `worktree` (mirrors the v2 SDK method group).
pub mod worktree {
    pub struct WorktreeApi<'a> {
        pub(crate) client: &'a crate::Client,
    }

    impl WorktreeApi<'_> {
        /// DELETE /api/worktree — operationId `worktree.remove`
        /// Declared errors: `400` unknown, `401` UnauthorizedErrorEncoded, `404` ProjectNotFoundErrorEncoded
        pub async fn remove(
            &self,
            body: &crate::types::WorktreeRemoveInput,
        ) -> Result<(), crate::ClientError> {
            let pairs: Vec<(String, String)> = Vec::new();
            let hdrs: Vec<(&'static str, String)> = Vec::new();
            let body_value = crate::transport::encode_body(body)?;
            self.client
                .execute_empty(&crate::transport::Call {
                    method: reqwest::Method::DELETE,
                    path: "/api/worktree",
                    path_params: &[],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::Json(&body_value),
                    success: 204,
                    declared: &[400, 401, 404],
                })
                .await
        }

        /// GET /api/worktree — operationId `worktree.list`
        /// Declared errors: `400` InvalidRequestErrorEncoded, `401` UnauthorizedErrorEncoded, `404` ProjectNotFoundErrorEncoded
        pub async fn list(
            &self,
            query: &crate::types::WorktreeListQuery,
        ) -> Result<crate::types::WorktreeList, crate::ClientError> {
            let mut pairs: Vec<(String, String)> = Vec::new();
            pairs.push(("projectID".to_owned(), query.project_id.clone()));
            let mut hdrs: Vec<(&'static str, String)> = Vec::new();
            self.client
                .execute_json(&crate::transport::Call {
                    method: reqwest::Method::GET,
                    path: "/api/worktree",
                    path_params: &[],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::None,
                    success: 200,
                    declared: &[400, 401, 404],
                })
                .await
        }

        /// POST /api/worktree — operationId `worktree.create`
        /// Declared errors: `400` unknown, `401` UnauthorizedErrorEncoded, `404` ProjectNotFoundErrorEncoded
        pub async fn create(
            &self,
            body: &crate::types::WorktreeCreateInput,
        ) -> Result<crate::types::WorktreeInfo, crate::ClientError> {
            let pairs: Vec<(String, String)> = Vec::new();
            let hdrs: Vec<(&'static str, String)> = Vec::new();
            let body_value = crate::transport::encode_body(body)?;
            self.client
                .execute_json(&crate::transport::Call {
                    method: reqwest::Method::POST,
                    path: "/api/worktree",
                    path_params: &[],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::Json(&body_value),
                    success: 200,
                    declared: &[400, 401, 404],
                })
                .await
        }

        /// POST /api/worktree/refresh — operationId `worktree.refresh`
        /// Declared errors: `400` unknown, `401` UnauthorizedErrorEncoded, `404` ProjectNotFoundErrorEncoded
        pub async fn refresh(
            &self,
            body: &crate::types::WorktreeRefreshBody,
        ) -> Result<(), crate::ClientError> {
            let pairs: Vec<(String, String)> = Vec::new();
            let hdrs: Vec<(&'static str, String)> = Vec::new();
            let body_value = crate::transport::encode_body(body)?;
            self.client
                .execute_empty(&crate::transport::Call {
                    method: reqwest::Method::POST,
                    path: "/api/worktree/refresh",
                    path_params: &[],
                    query: &pairs,
                    headers: &hdrs,
                    body: crate::transport::Body::Json(&body_value),
                    success: 204,
                    declared: &[400, 401, 404],
                })
                .await
        }
    }

    impl crate::Client {
        /// Namespace `worktree` operations.
        #[must_use]
        pub fn worktree(&self) -> WorktreeApi<'_> {
            WorktreeApi { client: self }
        }
    }
}
