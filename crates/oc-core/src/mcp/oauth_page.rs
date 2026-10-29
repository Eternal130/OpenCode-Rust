//! Branded HTML pages for the local OAuth callback server — port of
//! `packages/core/src/oauth/page.ts` (AUD-20①). The functions return a
//! fully self-contained HTML string with no external assets, so they work
//! offline and drop into the loopback listener's raw HTTP writer.
//!
//! The visual language mirrors the OpenCode app: the design tokens are a
//! curated subset of the OC-2 semantic tokens in
//! `packages/ui/src/styles/theme.css`, and the wordmark is the same
//! geometry as `packages/ui/src/components/logo.tsx`.

/// Friendly integration name shown as a subtitle (e.g. `"xAI"`, `"MCP"`).
pub type Provider<'a> = Option<&'a str>;

const AUTO_CLOSE_SCRIPT: &str = "setTimeout(function(){try{window.close()}catch(e){}},2500)";

/// `OauthCallbackPage.success` — the connected card (page.ts:26-36).
#[must_use]
pub fn success(provider: Provider<'_>) -> String {
    render_document(RenderDocument {
        title: "Authorization successful",
        body: render_card(RenderCard {
            status: "success",
            headline: "Authorization successful",
            message: match provider {
                Some(provider) => format!("OpenCode is now connected to {provider}."),
                None => "OpenCode is now authorized.".to_owned(),
            },
            detail: None,
            footnote: "You can close this window.",
        }),
        script: Some(AUTO_CLOSE_SCRIPT),
    })
}

/// `OauthCallbackPage.error` — the failure card with a detail block
/// (page.ts:38-49).
#[must_use]
pub fn error(detail: &str, provider: Provider<'_>) -> String {
    render_document(RenderDocument {
        title: "Authorization failed",
        body: render_card(RenderCard {
            status: "error",
            headline: "Authorization failed",
            message: match provider {
                Some(provider) => {
                    format!("OpenCode couldn't finish connecting to {}.", escape_html(provider))
                }
                None => "OpenCode couldn't complete authorization.".to_owned(),
            },
            detail: Some(detail),
            footnote: "Close this window and try again from OpenCode.",
        }),
        script: None,
    })
}

struct RenderCard<'a> {
    status: &'a str,
    headline: &'a str,
    message: String,
    detail: Option<&'a str>,
    footnote: &'a str,
}

fn render_card(input: RenderCard<'_>) -> String {
    let detail = input.detail.map(str::trim).filter(|detail| !detail.is_empty());
    let detail_attr = if detail.is_some() { "" } else { " hidden" };
    let detail_body = detail.map(escape_html).unwrap_or_default();
    format!(
        r#"<main class="card" id="oc-card" data-status="{status}" role="status" aria-live="polite">
      <div class="brand">{WORDMARK}</div>
      <div class="status" aria-hidden="true">
        <span class="icon icon-pending">{ICON_SPINNER}</span>
        <span class="icon icon-success">{ICON_CHECK}</span>
        <span class="icon icon-error">{ICON_CROSS}</span>
      </div>
      <h1 class="headline" id="oc-headline">{headline}</h1>
      <p class="message" id="oc-message">{message}</p>
      <pre class="detail" id="oc-detail"{detail_attr}>{detail_body}</pre>
      <p class="footnote" id="oc-footnote">{footnote}</p>
    </main>"#,
        status = input.status,
        headline = escape_html(input.headline),
        message = input.message,
        footnote = escape_html(input.footnote),
    )
}

struct RenderDocument<'a> {
    title: &'a str,
    body: String,
    script: Option<&'a str>,
}

fn render_document(input: RenderDocument<'_>) -> String {
    let script = match input.script {
        Some(script) => format!("\n    <script>{script}</script>"),
        None => String::new(),
    };
    format!(
        r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <meta name="robots" content="noindex" />
    <title>{title} · OpenCode</title>
    <style>{styles}</style>
  </head>
  <body>
    {body}{script}
  </body>
</html>"#,
        title = escape_html(input.title),
        styles = &*STYLES,
        body = input.body,
        script = script,
    )
}

/// `escapeHtml` (page.ts:113-121).
fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

// Curated subset of OC-2 tokens (packages/ui/src/styles/theme.css). Default
// is light; dark applies via prefers-color-scheme. The [data-theme]
// selectors let a host force a scheme without changing the default.
const LIGHT_VARS: &str = r#"
    --oc-bg: #f8f8f8;
    --oc-card: #fcfcfc;
    --oc-text-strong: #171717;
    --oc-text-base: #6f6f6f;
    --oc-text-weak: #8f8f8f;
    --oc-border-weak: #e5e5e5;
    --oc-icon-strong: #171717;
    --oc-icon-base: #8f8f8f;
    --oc-icon-weak: #dbdbdb;
    --oc-success: #2dba26;
    --oc-error: #ed4831;
    --oc-detail-bg: #fff8f6;
    --oc-detail-border: #fdc3b7;
    --oc-shadow: 0 16px 48px -6px rgba(0,0,0,.10), 0 6px 12px -2px rgba(0,0,0,.05), 0 1px 2px rgba(0,0,0,.06);"#;

const DARK_VARS: &str = r#"
    --oc-bg: #101010;
    --oc-card: #161616;
    --oc-text-strong: rgba(255,255,255,.936);
    --oc-text-base: rgba(255,255,255,.618);
    --oc-text-weak: rgba(255,255,255,.422);
    --oc-border-weak: #282828;
    --oc-icon-strong: #ededed;
    --oc-icon-base: #7e7e7e;
    --oc-icon-weak: #343434;
    --oc-success: #12c905;
    --oc-error: #fc533a;
    --oc-detail-bg: #28110c;
    --oc-detail-border: #6a1206;
    --oc-shadow: 0 16px 48px -6px rgba(0,0,0,.55), 0 6px 12px -2px rgba(0,0,0,.35), 0 1px 2px rgba(0,0,0,.4);"#;

static STYLES: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
    r#"
  :root { color-scheme: light dark;{{LIGHT}}
    --oc-font-sans: ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
    --oc-font-mono: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", "Courier New", monospace;
  }
  @media (prefers-color-scheme: dark) { :root:not([data-theme="light"]) {{{DARK}} } }
  :root[data-theme="dark"] {{{DARK}} }
  :root[data-theme="light"] {{{LIGHT}} }

  * { box-sizing: border-box; }
  html, body { margin: 0; height: 100%; }
  body {
    min-height: 100vh;
    display: grid;
    place-items: center;
    padding: 24px;
    background: var(--oc-bg);
    color: var(--oc-text-base);
    font-family: var(--oc-font-sans);
    line-height: 1.5;
    -webkit-font-smoothing: antialiased;
    text-rendering: optimizeLegibility;
  }
  .card {
    width: min(100%, 28rem);
    padding: 2.25rem 2rem 1.75rem;
    background: var(--oc-card);
    border: 1px solid var(--oc-border-weak);
    border-radius: 14px;
    box-shadow: var(--oc-shadow);
    text-align: center;
  }
  .brand { display: flex; justify-content: center; margin-bottom: 1.75rem; }
  .brand svg { height: 19px; width: auto; }
  .status { display: flex; justify-content: center; margin-bottom: 1.125rem; }
  .icon { display: none; line-height: 0; }
  .icon svg { display: block; }
  .card[data-status="pending"] .icon-pending,
  .card[data-status="success"] .icon-success,
  .card[data-status="error"] .icon-error { display: block; }
  .icon-success { color: var(--oc-success); }
  .icon-error { color: var(--oc-error); }
  .icon-pending { color: var(--oc-text-weak); }
  .headline { margin: 0; font-size: 1.1875rem; font-weight: 500; line-height: 1.3; letter-spacing: -0.012em; color: var(--oc-text-strong); }
  .message { margin: 0.5rem 0 0; font-size: 0.9375rem; color: var(--oc-text-base); }
  .detail {
    margin: 1.25rem 0 0;
    padding: 0.75rem 0.875rem;
    text-align: left;
    font-family: var(--oc-font-mono);
    font-size: 0.8125rem;
    line-height: 1.55;
    color: var(--oc-text-strong);
    background: var(--oc-detail-bg);
    border: 1px solid var(--oc-detail-border);
    border-radius: 8px;
    white-space: pre-wrap;
    word-break: break-word;
    max-height: 9.5rem;
    overflow: auto;
  }
  .detail[hidden] { display: none; }
  .footnote { margin: 1.5rem 0 0; font-size: 0.8125rem; color: var(--oc-text-weak); }
  .spinner { animation: oc-spin 0.8s linear infinite; transform-origin: center; }
  @keyframes oc-spin { to { transform: rotate(360deg); } }
  @media (prefers-reduced-motion: reduce) { .spinner { animation: none; } }
"#
    .replace("{{LIGHT}}", LIGHT_VARS)
    .replace("{{DARK}}", DARK_VARS)
});

// OpenCode wordmark — same path geometry as
// packages/ui/src/components/logo.tsx (Logo).
const WORDMARK: &str = r#"<svg class="wordmark" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 234 42" fill="none" aria-label="OpenCode" role="img">
        <path d="M18 30H6V18H18V30Z" fill="var(--oc-icon-weak)" />
        <path d="M18 12H6V30H18V12ZM24 36H0V6H24V36Z" fill="var(--oc-icon-base)" />
        <path d="M48 30H36V18H48V30Z" fill="var(--oc-icon-weak)" />
        <path d="M36 30H48V12H36V30ZM54 36H36V42H30V6H54V36Z" fill="var(--oc-icon-base)" />
        <path d="M84 24V30H66V24H84Z" fill="var(--oc-icon-weak)" />
        <path d="M84 24H66V30H84V36H60V6H84V24ZM66 18H78V12H66V18Z" fill="var(--oc-icon-base)" />
        <path d="M108 36H96V18H108V36Z" fill="var(--oc-icon-weak)" />
        <path d="M108 12H96V36H90V6H108V12ZM114 36H108V12H114V36Z" fill="var(--oc-icon-base)" />
        <path d="M144 30H126V18H144V30Z" fill="var(--oc-icon-weak)" />
        <path d="M144 12H126V30H144V36H120V6H144V12Z" fill="var(--oc-icon-strong)" />
        <path d="M168 30H156V18H168V30Z" fill="var(--oc-icon-weak)" />
        <path d="M168 12H156V30H168V12ZM174 36H150V6H174V36Z" fill="var(--oc-icon-strong)" />
        <path d="M198 30H186V18H198V30Z" fill="var(--oc-icon-weak)" />
        <path d="M198 12H186V30H198V12ZM204 36H180V6H198V0H204V36Z" fill="var(--oc-icon-strong)" />
        <path d="M234 24V30H216V24H234Z" fill="var(--oc-icon-weak)" />
        <path d="M216 12V18H228V12H216ZM234 24H216V30H234V36H210V6H234V24Z" fill="var(--oc-icon-strong)" />
      </svg>"#;

const ICON_CHECK: &str = r#"<svg viewBox="0 0 24 24" width="30" height="30" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="9" /><path d="m8.5 12.5 2.4 2.4 4.6-5.4" /></svg>"#;

const ICON_CROSS: &str = r#"<svg viewBox="0 0 24 24" width="30" height="30" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="9" /><path d="m9 9 6 6m0-6-6 6" /></svg>"#;

const ICON_SPINNER: &str = r#"<svg class="spinner" viewBox="0 0 24 24" width="30" height="30" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="12" cy="12" r="9" opacity="0.2" /><path d="M21 12a9 9 0 0 0-9-9" /></svg>"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oauth_page_success_shape() {
        let page = success(Some("MCP"));
        assert!(page.starts_with("<!doctype html>"));
        assert!(page.contains("<title>Authorization successful · OpenCode</title>"));
        assert!(page.contains("OpenCode is now connected to MCP."));
        assert!(page.contains("data-status=\"success\""));
        assert!(page.contains("You can close this window."));
        assert!(page.contains(AUTO_CLOSE_SCRIPT));
        assert!(page.contains("--oc-success: #2dba26;"));
    }

    #[test]
    fn oauth_page_error_shape_and_escaping() {
        let page = error("bad <code> & 'state'", Some("prov<&>ider"));
        assert!(page.contains("<title>Authorization failed · OpenCode</title>"));
        assert!(page.contains("OpenCode couldn't finish connecting to prov&lt;&amp;&gt;ider."));
        assert!(page.contains("bad &lt;code&gt; &amp; &#39;state&#39;"));
        assert!(page.contains("data-status=\"error\""));
        assert!(!page.contains(" hidden"));
        assert!(!page.contains("<script>"));
    }

    #[test]
    fn oauth_page_no_provider_strings() {
        assert!(success(None).contains("OpenCode is now authorized."));
        assert!(error("x", None).contains("OpenCode couldn't complete authorization."));
    }
}
