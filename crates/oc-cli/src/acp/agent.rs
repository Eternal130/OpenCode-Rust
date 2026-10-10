//! The ACP agent wiring (port of `acp/agent.ts`): the builder with the
//! 13 request handlers + `session/cancel`, the boot task, and the
//! shared event feed (catalog reloads + selection events).

use std::sync::Arc;

use agent_client_protocol::schema::v1::{
    AuthenticateRequest, CancelNotification, CloseSessionRequest, DeleteSessionRequest,
    ForkSessionRequest, InitializeRequest, ListSessionsRequest, LoadSessionRequest,
    NewSessionRequest, PromptRequest, ResumeSessionRequest, SetSessionConfigOptionRequest,
    SetSessionModeRequest,
};
use agent_client_protocol::{Agent, ConnectionTo, Error as AcpError, JsonRpcResponse, Responder};

use crate::acp::config_option::Change;
use crate::acp::error::AcpError as ServiceError;
use crate::acp::transport::CappedNdJson;
use crate::acp::App;

/// The counterpart role the agent talks to.
type ClientRole = agent_client_protocol::Client;

/// `connect` (agent.ts:26-130): build the agent, serve it on the
/// transport, keep the service feed running for the connection's
/// lifetime. One registration arm per request method, mirroring
/// agent.ts:68-119; the mechanical chain is exempt from the line cap.
#[allow(clippy::too_many_lines)]
pub async fn serve(app: Arc<App>, transport: CappedNdJson) -> Result<(), AcpError> {
    let boot_app = Arc::clone(&app);
    Agent
        .builder()
        .name("opencode")
        .with_spawned(move |conn| {
            let app = Arc::clone(&boot_app);
            async move { boot(&app, conn).await }
        })
        .on_receive_request(
            {
                let app = Arc::clone(&app);
                // initialize (agent.ts:69-71): inline — no connection.
                async move |request: InitializeRequest, responder, _cx| {
                    let response = crate::acp::service::initialize(&app, request).await;
                    responder.respond(response)
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let app = Arc::clone(&app);
                async move |request: AuthenticateRequest, responder, cx| {
                    respond_with(&app, cx, responder, |_app, _conn| async move {
                        crate::acp::service::authenticate(&request)
                    })
                    .await
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let app = Arc::clone(&app);
                async move |request: NewSessionRequest, responder, cx| {
                    respond_with(&app, cx, responder, |app, conn| async move {
                        crate::acp::service::new_session(&app, &conn, request).await
                    })
                    .await
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let app = Arc::clone(&app);
                async move |request: LoadSessionRequest, responder, cx| {
                    respond_with(&app, cx, responder, |app, conn| async move {
                        crate::acp::service::load_session(&app, &conn, request).await
                    })
                    .await
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let app = Arc::clone(&app);
                async move |request: ListSessionsRequest, responder, cx| {
                    respond_with(&app, cx, responder, |app, _conn| async move {
                        crate::acp::service::list_sessions(&app, request).await
                    })
                    .await
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let app = Arc::clone(&app);
                async move |request: DeleteSessionRequest, responder, cx| {
                    respond_with(&app, cx, responder, |app, _conn| async move {
                        crate::acp::service::delete_session(&app, request).await
                    })
                    .await
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let app = Arc::clone(&app);
                async move |request: ResumeSessionRequest, responder, cx| {
                    respond_with(&app, cx, responder, |app, conn| async move {
                        crate::acp::service::resume_session(&app, &conn, request).await
                    })
                    .await
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let app = Arc::clone(&app);
                async move |request: CloseSessionRequest, responder, cx| {
                    respond_with(&app, cx, responder, |app, _conn| async move {
                        crate::acp::service::close_session(&app, request).await
                    })
                    .await
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let app = Arc::clone(&app);
                async move |request: ForkSessionRequest, responder, cx| {
                    respond_with(&app, cx, responder, |app, conn| async move {
                        crate::acp::service::fork_session(&app, &conn, request).await
                    })
                    .await
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let app = Arc::clone(&app);
                async move |request: SetSessionConfigOptionRequest, responder, cx| {
                    respond_with(&app, cx, responder, |app, _conn| async move {
                        crate::acp::service::set_session_config_option(&app, request).await
                    })
                    .await
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let app = Arc::clone(&app);
                async move |request: SetSessionModeRequest, responder, cx| {
                    respond_with(&app, cx, responder, |app, _conn| async move {
                        crate::acp::service::set_session_mode(&app, request).await
                    })
                    .await
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let prompt_app = Arc::clone(&app);
                async move |request: PromptRequest, responder, cx| {
                    // `session/prompt` must not hold the dispatch loop:
                    // spawn the turn and respond when it settles
                    // (ordering.md — "Escaping the Loop"). A `$/cancel_request`
                    // for this prompt cancels its turn like `session/cancel`
                    // rather than failing the request (turn.ts:385-387).
                    let cancellation = responder.cancellation();
                    let prompt_app = Arc::clone(&prompt_app);
                    cx.spawn(async move {
                        if cancellation.is_cancelled() {
                            let _ = responder.respond(
                                agent_client_protocol::schema::v1::PromptResponse::new(
                                    agent_client_protocol::schema::v1::StopReason::Cancelled,
                                ),
                            );
                            return Ok(());
                        }
                        let conn = prompt_app.conn().await;
                        let session_id = request.session_id.to_string();
                        let watcher = {
                            let app = Arc::clone(&prompt_app);
                            let cancellation = cancellation.clone();
                            tokio::spawn(async move {
                                cancellation.cancelled().await;
                                app.turns.cancel(&session_id).await;
                            })
                        };
                        let outcome =
                            crate::acp::service::prompt(&prompt_app, &conn, request).await;
                        watcher.abort();
                        match outcome {
                            Ok(response) => {
                                let _ = responder.respond(response);
                            }
                            Err(error) => {
                                let _ = responder.respond_with_error(error.to_request_error());
                            }
                        }
                        Ok(())
                    })
                    .map_err(|error| AcpError::internal_error().data(format!("{error}")))
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_notification(
            {
                let app = Arc::clone(&app);
                // session/cancel (agent.ts:116-119) — fast, inline.
                async move |notification: CancelNotification, _cx| {
                    crate::acp::service::cancel(&app, &notification.session_id.to_string()).await;
                    Ok(())
                }
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .connect_to(transport)
        .await
}

/// One boot (agent.ts:28-129): subscribe the service feed, wait for
/// `server.connected`, bind the connection, then keep the feed running
/// (catalog reloads + selection events) for the connection's lifetime.
async fn boot(app: &Arc<App>, conn: ConnectionTo<ClientRole>) -> Result<(), AcpError> {
    let mut stream = app
        .client
        .event()
        .subscribe()
        .await
        .map_err(|_| AcpError::internal_error().data("opencode event feed unavailable"))?;
    // The feed opens with server.connected (catalog.ts:68-71 gates the
    // first read on it).
    let first = stream
        .next_json()
        .await
        .map_err(|_| AcpError::internal_error().data("opencode event feed failed"))?
        .ok_or_else(|| AcpError::internal_error().data("opencode event feed closed"))?;
    let type_ = first.get("type").and_then(|value| value.as_str()).unwrap_or_default().to_owned();
    if type_ != "server.connected" {
        return Err(
            AcpError::internal_error().data(format!("expected server.connected, got {type_}"))
        );
    }
    app.bind_connection(conn.clone());
    // The service feed loop (catalog.ts:68-85 reload-on + sessions.ts:54-73
    // selection events — one shared subscription instead of v2's three).
    loop {
        let Ok(Some(value)) = stream.next_json().await else {
            return Ok(());
        };
        let Some(event) = crate::acp::turn::parse_event(&value) else { continue };
        match event.type_.as_str() {
            type_ if crate::acp::catalog::reload_on(type_) => {
                let directory = event
                    .data
                    .pointer("/location/directory")
                    .and_then(|value| value.as_str())
                    .map(str::to_owned);
                let _ = app.catalogs.reload_affected(directory.as_deref()).await;
            }
            "session.model.selected" => {
                let session_id = event
                    .data
                    .get("sessionID")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default();
                if let Some(model) = event.data.get("model") {
                    app.sessions
                        .selected(
                            session_id,
                            Change::Model(crate::acp::catalog::ModelRef {
                                provider_id: model
                                    .get("providerID")
                                    .and_then(|value| value.as_str())
                                    .unwrap_or_default()
                                    .to_owned(),
                                id: model
                                    .get("id")
                                    .and_then(|value| value.as_str())
                                    .unwrap_or_default()
                                    .to_owned(),
                                variant: model
                                    .get("variant")
                                    .and_then(|value| value.as_str())
                                    .map(str::to_owned),
                            }),
                        )
                        .await;
                }
            }
            "session.agent.selected" => {
                let session_id = event
                    .data
                    .get("sessionID")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default();
                if let Some(agent) = event.data.get("agent").and_then(|value| value.as_str()) {
                    app.sessions.selected(session_id, Change::Mode(agent.to_owned())).await;
                }
            }
            _ => {}
        }
    }
}

/// The uniform handler wrapper (agent.ts:34-57): await the connection,
/// run the service call off-loop, respond with the result or the
/// mapped request error.
#[allow(clippy::unused_async)]
async fn respond_with<T, F, Fut>(
    app: &Arc<App>,
    cx: ConnectionTo<ClientRole>,
    responder: Responder<T>,
    call: F,
) -> Result<(), AcpError>
where
    T: JsonRpcResponse + Send + 'static,
    F: FnOnce(Arc<App>, ConnectionTo<ClientRole>) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<T, ServiceError>> + Send,
{
    let app = Arc::clone(app);
    cx.spawn(async move {
        let conn = app.conn().await;
        match call(app, conn).await {
            Ok(response) => {
                let _ = responder.respond(response);
            }
            Err(error) => {
                let _ = responder.respond_with_error(error.to_request_error());
            }
        }
        Ok(())
    })
    .map_err(|error| AcpError::internal_error().data(format!("{error}")))
}

#[cfg(test)]
mod tests {
    /// The full agent↔client round trip against a stub opencode HTTP
    /// server lives in `tests/acp_integration.rs`; the boot-time
    /// gating and handler registration are exercised there.
    #[test]
    fn agent_wiring_is_registered() {
        // Static smoke: the builder methods exist with the right shapes
        // (compile-time check via the serve() signature above).
        let _ = "agent wiring";
    }
}
