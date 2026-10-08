//! Auth combinators (port of `src/route/auth.ts`). Definitions produce
//! headers from an [`AuthInput`]; credentials resolve lazily so env config
//! reads happen at request time.

use std::sync::Arc;

use crate::schema::{AiError, AiErrorReason, HttpOptions, ReasonFields};

/// Header map produced by auth application. Lookup is ASCII
/// case-insensitive like HTTP headers.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Headers {
    entries: Vec<(String, String)>,
}

impl Headers {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_pairs(pairs: impl IntoIterator<Item = (String, String)>) -> Self {
        let mut headers = Self::new();
        for (name, value) in pairs {
            headers.set(name, value);
        }
        headers
    }

    #[must_use]
    pub fn get(&self, name: &str) -> Option<&str> {
        self.entries.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    pub fn set(&mut self, name: impl Into<String>, value: impl Into<String>) {
        let name = name.into();
        let value = value.into();
        if let Some(entry) = self.entries.iter_mut().find(|(k, _)| k.eq_ignore_ascii_case(&name)) {
            entry.1 = value;
        } else {
            self.entries.push((name, value));
        }
    }

    pub fn remove(&mut self, name: &str) {
        self.entries.retain(|(k, _)| !k.eq_ignore_ascii_case(name));
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }
}

/// Credential resolution failure (port of `MissingCredentialError` /
/// `Config.ConfigError`).
#[derive(Debug, Clone, PartialEq)]
pub struct MissingCredentialError {
    pub source: String,
    pub message: String,
}

impl MissingCredentialError {
    fn new(source: impl Into<String>) -> Self {
        let source = source.into();
        Self { message: format!("Missing auth credential: {source}"), source }
    }

    fn not_set(name: &str) -> Self {
        Self { source: name.to_string(), message: format!("{name} is not set") }
    }
}

/// Auth application failure before it maps to `AIError::Authentication`.
pub type AuthError = MissingCredentialError;

/// Input to auth application (port of `AuthInput`).
pub struct AuthInput<'a> {
    pub request_http: Option<&'a HttpOptions>,
    pub method: &'a str,
    pub url: &'a str,
    pub body: &'a str,
    pub headers: Headers,
}

/// Credential source (port of TS `Secret`): a literal value, or an
/// environment variable read at apply time (`Auth.config`).
#[derive(Clone)]
pub enum Secret {
    Value(String),
    Env(String),
}

impl Secret {
    /// Resolve to a redacted-equivalent string; empty values are treated as
    /// missing (port of `secretEffect`).
    fn load(&self, source: &str) -> Result<String, AuthError> {
        let value = match self {
            Secret::Value(value) => value.clone(),
            Secret::Env(name) => {
                std::env::var(name).map_err(|_| AuthError::not_set(name))?.trim().to_string()
            }
        };
        if value.is_empty() {
            return Err(AuthError::new(source));
        }
        Ok(value)
    }
}

type ApplyFn = Arc<dyn Fn(&AuthInput<'_>) -> Result<Headers, AuthError> + Send + Sync>;

/// An auth definition (port of `Auth.Definition`): a function over
/// [`AuthInput`] producing headers, with combinators.
#[derive(Clone)]
pub struct Definition {
    apply: ApplyFn,
}

impl std::fmt::Debug for Definition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Auth::Definition(<fn>)")
    }
}

fn to_ai_error(error: AuthError) -> AiError {
    AiError::new(AiErrorReason::Authentication { fields: ReasonFields::new(error.message) })
}

impl Definition {
    pub fn new(
        apply: impl Fn(&AuthInput<'_>) -> Result<Headers, AuthError> + Send + Sync + 'static,
    ) -> Self {
        Self { apply: Arc::new(apply) }
    }

    /// Apply to an input, mapping credential failures to `Authentication`.
    pub fn apply(&self, input: &AuthInput<'_>) -> Result<Headers, AiError> {
        (self.apply)(input).map_err(to_ai_error)
    }

    /// Sequence: apply `self`, then `that` over the updated input
    /// (port of `andThen`).
    #[must_use]
    pub fn and_then(&self, that: Definition) -> Definition {
        let this = self.clone();
        Definition::new(move |input| {
            let headers = (this.apply)(input)?;
            let next_input = AuthInput {
                request_http: input.request_http,
                method: input.method,
                url: input.url,
                body: input.body,
                headers,
            };
            (that.apply)(&next_input)
        })
    }

    /// Fallback: apply `that` when `self` fails (port of `orElse`).
    #[must_use]
    pub fn or_else(&self, that: Definition) -> Definition {
        let this = self.clone();
        Definition::new(move |input| (this.apply)(input).or_else(|_| (that.apply)(input)))
    }

    /// `Authorization: Bearer <secret>`.
    #[must_use]
    pub fn bearer(secret: Secret) -> Definition {
        Definition::credential(secret, |value| {
            vec![("authorization".to_string(), format!("Bearer {value}"))]
        })
    }

    /// `<name>: <secret>`.
    pub fn header(name: impl Into<String>, secret: Secret) -> Definition {
        let name = name.into();
        Definition::credential(secret, move |value| vec![(name.clone(), value)])
    }

    /// `<name>: Bearer <secret>` (port of `bearerHeader`).
    pub fn bearer_header(name: impl Into<String>, secret: Secret) -> Definition {
        let name = name.into();
        Definition::credential(secret, move |value| vec![(name.clone(), format!("Bearer {value}"))])
    }

    /// `Authorization: <scheme> <secret>` for non-Bearer schemes.
    pub fn scheme(scheme: impl Into<String>, secret: Secret) -> Definition {
        let scheme = scheme.into();
        Definition::credential(secret, move |value| {
            vec![("authorization".to_string(), format!("{scheme} {value}"))]
        })
    }

    fn credential(
        secret: Secret,
        render: impl Fn(String) -> Vec<(String, String)> + Send + Sync + 'static,
    ) -> Definition {
        Definition::new(move |input| {
            let value = secret.load("value")?;
            let mut headers = input.headers.clone();
            for (name, header_value) in render(value) {
                headers.set(name, header_value);
            }
            Ok(headers)
        })
    }

    /// Leave headers untouched (port of `Auth.none` / `passthrough`).
    #[must_use]
    pub fn none() -> Definition {
        Definition::new(|input| Ok(input.headers.clone()))
    }

    /// Merge static headers on top of the input (port of `Auth.headers`).
    pub fn headers(pairs: impl IntoIterator<Item = (String, String)>) -> Definition {
        let extra: Vec<(String, String)> = pairs.into_iter().collect();
        Definition::new(move |input| {
            let mut headers = input.headers.clone();
            for (name, value) in &extra {
                headers.set(name.clone(), value.clone());
            }
            Ok(headers)
        })
    }

    /// Remove one header (Azure strips a gateway's `authorization`).
    pub fn remove(name: impl Into<String>) -> Definition {
        let name = name.into();
        Definition::new(move |input| {
            let mut headers = input.headers.clone();
            headers.remove(&name);
            Ok(headers)
        })
    }
}

/// Credential-level fallback: resolve the first source that loads, then
/// render (port of `Credential.orElse().bearer()/header()`).
#[derive(Clone)]
pub struct Credential {
    sources: Vec<Secret>,
}

impl Credential {
    /// Try each source in order; the first non-empty value wins.
    pub fn load(&self, source: &str) -> Result<String, AuthError> {
        let mut last = AuthError::new(source);
        for secret in &self.sources {
            match secret.load(source) {
                Ok(value) => return Ok(value),
                Err(error) => last = error,
            }
        }
        Err(last)
    }

    #[must_use]
    pub fn bearer(&self) -> Definition {
        self.render(|value| vec![("authorization".to_string(), format!("Bearer {value}"))])
    }

    pub fn header(&self, name: impl Into<String>) -> Definition {
        let name = name.into();
        self.render(move |value| vec![(name.clone(), value)])
    }

    fn render(
        &self,
        render: impl Fn(String) -> Vec<(String, String)> + Send + Sync + 'static,
    ) -> Definition {
        let credential = self.clone();
        Definition::new(move |input| {
            let value = credential.load("value")?;
            let mut headers = input.headers.clone();
            for (name, header_value) in render(value) {
                headers.set(name, header_value);
            }
            Ok(headers)
        })
    }
}

/// Config-sourced credential: reads the env var at apply time.
pub fn config(name: impl Into<String>) -> Credential {
    Credential { sources: vec![Secret::Env(name.into())] }
}

/// Optional literal credential (port of `Auth.optional`).
#[must_use]
pub fn optional(secret: Option<Secret>) -> Credential {
    Credential { sources: vec![secret.unwrap_or(Secret::Value(String::new()))] }
}

/// Required literal credential (port of `Auth.value`).
pub fn value(secret: impl Into<String>) -> Credential {
    Credential { sources: vec![Secret::Value(secret.into())] }
}

/// Chain credential sources (port of `credential.orElse`).
#[must_use]
pub fn or_else(first: &Credential, second: &Credential) -> Credential {
    Credential { sources: first.sources.iter().chain(second.sources.iter()).cloned().collect() }
}

/// Standard bearer-auth resolution for providers (port of
/// `AuthOptions.bearer`): honor an explicit auth override, otherwise resolve
/// `apiKey` (option > env vars) and apply as a bearer token.
#[must_use]
pub fn bearer_from_options(
    auth: Option<&Definition>,
    api_key: Option<Secret>,
    env_vars: &[&str],
) -> Definition {
    if let Some(auth) = auth {
        return auth.clone();
    }
    let mut sources: Vec<Secret> = api_key.into_iter().collect();
    sources.extend(env_vars.iter().map(|name| Secret::Env((*name).to_string())));
    Credential { sources }.bearer()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> AuthInput<'static> {
        AuthInput {
            request_http: None,
            method: "POST",
            url: "https://example.test/v1/chat",
            body: "{}",
            headers: Headers::from_pairs([("x-existing".to_string(), "yes".to_string())]),
        }
    }

    #[test]
    fn renders_a_direct_secret_as_bearer_auth() {
        let headers = Definition::bearer(Secret::Value("sk-test".into())).apply(&input()).unwrap();
        assert_eq!(headers.get("authorization"), Some("Bearer sk-test"));
        assert_eq!(headers.get("x-existing"), Some("yes"));
    }

    #[test]
    fn falls_back_between_credential_sources_before_rendering() {
        // PRIMARY_KEY is not set; the literal fallback wins.
        let credential = or_else(&config("OC_AI_TEST_PRIMARY_KEY"), &value("fallback-key"));
        let headers = credential.header("x-api-key").apply(&input()).unwrap();
        assert_eq!(headers.get("x-api-key"), Some("fallback-key"));
        assert_eq!(headers.get("x-existing"), Some("yes"));
    }

    #[test]
    fn composes_header_auth_in_sequence() {
        let auth = Definition::headers([("x-tenant-id".to_string(), "tenant-1".to_string())])
            .and_then(Definition::bearer(Secret::Value("gateway-token".into())));
        let headers = auth.apply(&input()).unwrap();
        assert_eq!(headers.get("x-tenant-id"), Some("tenant-1"));
        assert_eq!(headers.get("authorization"), Some("Bearer gateway-token"));
        assert_eq!(headers.get("x-existing"), Some("yes"));
    }

    #[test]
    fn renders_a_direct_secret_as_a_custom_header() {
        let headers = Definition::header("api-key", Secret::Value("direct-key".into()))
            .apply(&input())
            .unwrap();
        assert_eq!(headers.get("api-key"), Some("direct-key"));
    }

    #[test]
    fn renders_bearer_auth_into_a_custom_header() {
        let headers = Definition::bearer_header(
            "cf-aig-authorization",
            Secret::Value("gateway-token".into()),
        )
        .apply(&input())
        .unwrap();
        assert_eq!(headers.get("cf-aig-authorization"), Some("Bearer gateway-token"));
    }

    #[test]
    fn renders_a_custom_scheme_into_authorization() {
        let headers =
            Definition::scheme("Key", Secret::Value("k-1".into())).apply(&input()).unwrap();
        assert_eq!(headers.get("authorization"), Some("Key k-1"));
    }

    #[test]
    fn falls_back_between_full_auth_values() {
        let auth = Definition::bearer(Secret::Env("OC_AI_TEST_OPENAI_API_KEY".into())).or_else(
            Definition::headers([("authorization".to_string(), "Bearer supplied".to_string())]),
        );
        let headers = auth.apply(&input()).unwrap();
        assert_eq!(headers.get("authorization"), Some("Bearer supplied"));
    }

    #[test]
    fn reports_a_missing_config_credential_as_authentication_naming_the_variable() {
        let error = Definition::bearer(Secret::Env("OC_AI_TEST_MISSING_KEY".into()))
            .apply(&input())
            .unwrap_err();
        assert!(matches!(error.reason, AiErrorReason::Authentication { .. }));
        assert!(error.message().contains("OC_AI_TEST_MISSING_KEY"));
    }

    #[test]
    fn empty_secrets_count_as_missing() {
        let error = Definition::bearer(Secret::Value(String::new())).apply(&input()).unwrap_err();
        assert!(matches!(error.reason, AiErrorReason::Authentication { .. }));
        assert!(error.message().contains("Missing auth credential"));
    }

    #[test]
    fn can_intentionally_leave_auth_untouched() {
        let headers = Definition::none().apply(&input()).unwrap();
        assert_eq!(headers.get("authorization"), None);
        assert_eq!(headers.get("x-existing"), Some("yes"));
    }

    #[test]
    fn remove_strips_one_header() {
        let auth = Definition::none().and_then(Definition::remove("x-existing"));
        let headers = auth.apply(&input()).unwrap();
        assert_eq!(headers.get("x-existing"), None);
    }

    #[test]
    fn bearer_from_options_prefers_explicit_auth_over_keys() {
        let explicit = Definition::header("x-api-key", Secret::Value("explicit".into()));
        let auth = bearer_from_options(
            Some(&explicit),
            Some(Secret::Value("ignored".into())),
            &["OC_AI_TEST_X"],
        );
        let headers = auth.apply(&input()).unwrap();
        assert_eq!(headers.get("x-api-key"), Some("explicit"));
        assert_eq!(headers.get("authorization"), None);
    }

    #[test]
    fn bearer_from_options_falls_back_to_env_config() {
        std::env::set_var("OC_AI_TEST_BEARER_ENV", "env-key");
        let auth = bearer_from_options(None, None, &["OC_AI_TEST_BEARER_ENV"]);
        let headers = auth.apply(&input()).unwrap();
        assert_eq!(headers.get("authorization"), Some("Bearer env-key"));
        std::env::remove_var("OC_AI_TEST_BEARER_ENV");
    }
}
