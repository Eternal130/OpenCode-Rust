//! Port of `packages/core/src/form.ts` — the form sessions service (§4.10,
//! §4.11): `ask` parks on a watch channel until reply/cancel/timeout; the
//! durable Form events publish through the bus (v2 has them ephemeral).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use oc_schema::form::{Answer, Field, Fields, FormValue, Info, State, When, WhenOp, WhenValue};
use oc_schema::id::FormId;
use oc_util::determinism::{Clock, IdGen};
use parking_lot::Mutex;
use tokio::sync::watch;

/// Settled-form retention (cleanup only).
pub const RETENTION_MS: u64 = 10 * 60 * 1_000;

/// `Form.NotFoundError`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("Form not found: {0}")]
pub struct NotFoundError(pub FormId);

/// `Form.AlreadySettledError`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("Form already settled: {0}")]
pub struct AlreadySettledError(pub FormId);

/// `Form.AlreadyExistsError`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("Form already exists: {0}")]
pub struct AlreadyExistsError(pub FormId);

/// `Form.InvalidAnswerError`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("Invalid answer for form {id}: {message}")]
pub struct InvalidAnswerError {
    pub id: FormId,
    pub message: String,
}

/// `Form.InvalidFormError`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("Invalid form: {message}")]
pub struct InvalidFormError {
    pub message: String,
}

/// `Form.CreateInput` — `Form.Info` minus the id.
pub type CreateInput = NewForm;

/// `Form.Info` under construction.
#[derive(Debug, Clone, PartialEq)]
pub struct NewForm {
    pub session_id: String,
    pub title: String,
    pub metadata: Option<oc_schema::common::Record>,
    pub fields: Fields,
}

/// Errors of the mutating surface.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum FormError {
    #[error(transparent)]
    NotFound(#[from] NotFoundError),
    #[error(transparent)]
    AlreadySettled(#[from] AlreadySettledError),
    #[error(transparent)]
    AlreadyExists(#[from] AlreadyExistsError),
    #[error(transparent)]
    InvalidAnswer(#[from] InvalidAnswerError),
    #[error(transparent)]
    InvalidForm(#[from] InvalidFormError),
}

struct Entry {
    form: Info,
    state: State,
    /// Wall-clock settlement time (TTL eviction; `None` while pending —
    /// ref `Cache.timeToLive`: pending → `Duration.infinity`).
    settled_at: Option<u64>,
    /// Resolves with the terminal state.
    settled: watch::Sender<Option<State>>,
}

/// The forms service (clone shares state).
#[derive(Clone)]
pub struct Forms {
    inner: Arc<Inner>,
}

struct Inner {
    ids: Arc<dyn IdGen>,
    clock: Arc<dyn Clock>,
    forms: Mutex<HashMap<FormId, Entry>>,
    bus: Option<crate::bus::Bus>,
    closed: Mutex<bool>,
}

impl Forms {
    pub fn new(ids: Arc<dyn IdGen>, clock: Arc<dyn Clock>) -> Self {
        Self {
            inner: Arc::new(Inner {
                ids,
                clock,
                forms: Mutex::new(HashMap::new()),
                bus: None,
                closed: Mutex::new(false),
            }),
        }
    }

    /// Attaches a bus for `form.created`/`form.replied`/`form.cancelled`.
    #[must_use]
    pub fn with_bus(mut self, bus: crate::bus::Bus) -> Self {
        let inner = Arc::get_mut(&mut self.inner).expect("bus must attach before cloning");
        inner.bus = Some(bus);
        self
    }

    #[must_use]
    fn now_ms(&self) -> u64 {
        u64::try_from(self.inner.clock.unix_ms().max(0)).unwrap_or(0)
    }

    /// Evict settled entries older than [`RETENTION_MS`] (ref form.ts:8
    /// `RETENTION = Duration.minutes(10)` fed to `Cache.makeWith`'s
    /// `timeToLive`: pending → infinity, settled → 10 minutes).
    fn evict_expired(&self) {
        let now = self.now_ms();
        self.inner.forms.lock().retain(|_, entry| {
            match entry.settled_at {
                Some(settled_at) => now.saturating_sub(settled_at) < RETENTION_MS,
                None => true, // pending forms never expire
            }
        });
    }

    async fn publish(&self, type_: &str, data: serde_json::Value) {
        if let Some(bus) = &self.inner.bus {
            let definition = crate::bus::registry::EventDefinition::ephemeral(type_);
            let _ = bus.publish(&definition, data, crate::bus::PublishOptions::default()).await;
        }
    }

    /// `Form.create`.
    pub async fn create(&self, input: CreateInput) -> Result<Info, FormError> {
        let id = FormId(format!("frm_{}", self.inner.ids.new_id("frm")));
        self.create_with_id(id, input).await
    }

    /// `Form.create` with an explicit id (test seam).
    pub async fn create_with_id(&self, id: FormId, input: CreateInput) -> Result<Info, FormError> {
        if let Some(invalid) = validate_fields(&input.fields) {
            return Err(InvalidFormError { message: invalid }.into());
        }
        self.evict_expired();
        let form = Info {
            id: id.clone(),
            session_id: input.session_id.clone(),
            title: input.title,
            metadata: input.metadata,
            fields: input.fields,
        };
        let (settled, _) = watch::channel(None);
        let entry = Entry { form: form.clone(), state: State::Pending, settled_at: None, settled };
        let existed = {
            let mut forms = self.inner.forms.lock();
            if forms.contains_key(&id) {
                true
            } else {
                forms.insert(id.clone(), entry);
                false
            }
        };
        if existed {
            return Err(AlreadyExistsError(id).into());
        }
        self.publish("form.created", serde_json::json!({ "form": form })).await;
        let closed = *self.inner.closed.lock();
        if closed {
            let _ = self.cancel(&id, None).await;
        }
        Ok(form)
    }

    /// `Form.ask` — create + await the terminal state. Dropping the returned
    /// future before settlement cancels the form (`onInterrupt`).
    pub async fn ask(&self, input: CreateInput) -> Result<State, FormError> {
        let id = FormId(format!("frm_{}", self.inner.ids.new_id("frm")));
        self.ask_with_id(id, input).await
    }

    /// `Form.ask` over a caller-minted id — the MCP URL-elicitation path
    /// registers the form id before parking (index.ts:239-244), so it
    /// cannot let `ask` mint its own.
    pub async fn ask_with_id(&self, id: FormId, input: CreateInput) -> Result<State, FormError> {
        self.create_with_id(id.clone(), input).await?;
        let receiver = {
            let forms = self.inner.forms.lock();
            forms
                .get(&id)
                .map(|entry| entry.settled.subscribe())
                .ok_or_else(|| NotFoundError(id.clone()))?
        };
        let outcome = AskGuard { forms: self.clone(), id: id.clone() }.run(receiver).await;
        if let Some(state) = outcome {
            Ok(state)
        } else {
            // Interrupted (future dropped) — cancel and surface the
            // terminal state if it settles synchronously.
            let _ = self.cancel(&id, None).await;
            Err(NotFoundError(id).into())
        }
    }

    /// `Form.get`.
    pub fn get(&self, id: &FormId) -> Result<Info, NotFoundError> {
        self.evict_expired();
        self.inner
            .forms
            .lock()
            .get(id)
            .map(|entry| entry.form.clone())
            .ok_or_else(|| NotFoundError(id.clone()))
    }

    /// `Form.list` — pending forms, optionally per session.
    #[must_use]
    pub fn list(&self, session_id: Option<&str>) -> Vec<Info> {
        self.evict_expired();
        self.inner
            .forms
            .lock()
            .values()
            .filter(|entry| matches!(entry.state, State::Pending))
            .filter(|entry| session_id.is_none_or(|session| entry.form.session_id == session))
            .map(|entry| entry.form.clone())
            .collect()
    }

    /// `Form.state`.
    pub fn state(&self, id: &FormId) -> Result<State, NotFoundError> {
        self.evict_expired();
        self.inner
            .forms
            .lock()
            .get(id)
            .map(|entry| entry.state.clone())
            .ok_or_else(|| NotFoundError(id.clone()))
    }

    /// `Form.reply`.
    pub async fn reply(&self, id: &FormId, answer: Answer) -> Result<(), FormError> {
        self.evict_expired();
        let form = {
            let forms = self.inner.forms.lock();
            let entry = forms.get(id).ok_or_else(|| NotFoundError(id.clone()))?;
            if !matches!(entry.state, State::Pending) {
                return Err(AlreadySettledError(id.clone()).into());
            }
            entry.form.clone()
        };
        if let Some(invalid) = validate_answer(&form.fields, &answer) {
            return Err(InvalidAnswerError { id: id.clone(), message: invalid }.into());
        }
        let next = State::Answered { answer: answer.clone() };
        {
            let mut forms = self.inner.forms.lock();
            if let Some(entry) = forms.get_mut(id) {
                entry.state = next.clone();
                entry.settled_at = Some(self.now_ms());
                let _ = entry.settled.send(Some(next.clone()));
            }
        }
        self.publish(
            "form.replied",
            serde_json::json!({ "id": id.as_str(), "sessionID": form.session_id, "answer": answer }),
        )
        .await;
        Ok(())
    }

    /// `Form.cancel`.
    pub async fn cancel(&self, id: &FormId, message: Option<&str>) -> Result<(), FormError> {
        self.evict_expired();
        let session_id = {
            let forms = self.inner.forms.lock();
            let entry = forms.get(id).ok_or_else(|| NotFoundError(id.clone()))?;
            if !matches!(entry.state, State::Pending) {
                return Err(AlreadySettledError(id.clone()).into());
            }
            entry.form.session_id.clone()
        };
        let next = State::Cancelled { message: message.map(String::from) };
        {
            let mut forms = self.inner.forms.lock();
            if let Some(entry) = forms.get_mut(id) {
                entry.state = next.clone();
                entry.settled_at = Some(self.now_ms());
                let _ = entry.settled.send(Some(next));
            }
        }
        self.publish(
            "form.cancelled",
            serde_json::json!({ "id": id.as_str(), "sessionID": session_id }),
        )
        .await;
        Ok(())
    }

    /// `Form.close` — settle every pending form (location shutdown).
    pub async fn close(&self) {
        *self.inner.closed.lock() = true;
        let pending: Vec<FormId> = self
            .inner
            .forms
            .lock()
            .values()
            .filter(|entry| matches!(entry.state, State::Pending))
            .map(|entry| entry.form.id.clone())
            .collect();
        for id in pending {
            let _ = self.cancel(&id, None).await;
        }
    }
}

/// Guards an `ask`: dropping the wait cancels the form (interrupt).
struct AskGuard {
    forms: Forms,
    id: FormId,
}

impl AskGuard {
    async fn run(self, mut receiver: watch::Receiver<Option<State>>) -> Option<State> {
        loop {
            {
                let current = receiver.borrow();
                if let Some(state) = current.as_ref() {
                    return Some(state.clone());
                }
            }
            if receiver.changed().await.is_err() {
                return None;
            }
        }
    }
}

impl Drop for AskGuard {
    fn drop(&mut self) {
        // onInterrupt → cancel (ignored when already settled). No-op outside
        // a runtime (process teardown).
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            let forms = self.forms.clone();
            let id = self.id.clone();
            handle.spawn(async move {
                let _ = forms.cancel(&id, None).await;
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Validation (form.ts:235-370)
// ---------------------------------------------------------------------------

/// `Form.validateAnswer` — answer shape against the fields.
#[must_use]
pub fn validate_answer(form: &[Field], answer: &Answer) -> Option<String> {
    let fields: HashMap<&str, &Field> =
        form.iter().map(|field| (field_key(field), field)).collect();
    for key in answer.keys() {
        if !fields.contains_key(key.as_str()) {
            return Some(format!("Unknown form field: {key}"));
        }
    }
    for field in form {
        let key = field_key(field);
        let value = answer.get(key);
        if let Field::External { .. } = field {
            if !matches!(value, Some(FormValue::Bool(true))) {
                return Some(format!("External form field must be acknowledged: {key}"));
            }
            continue;
        }
        let active = is_active(field, answer);
        match value {
            None => {
                if field_required(field).unwrap_or(false) && active {
                    return Some(format!("Missing required form field: {key}"));
                }
            }
            Some(value) => {
                if !active {
                    return Some(format!("Form field is not active: {key}"));
                }
                if let Some(invalid) = validate_field(field, value) {
                    return Some(invalid);
                }
            }
        }
    }
    None
}

fn field_key(field: &Field) -> &str {
    match field {
        Field::String { key, .. }
        | Field::Number { key, .. }
        | Field::Integer { key, .. }
        | Field::Boolean { key, .. }
        | Field::Multiselect { key, .. }
        | Field::External { key, .. } => key,
    }
}

fn field_required(field: &Field) -> Option<bool> {
    match field {
        Field::String { required, .. }
        | Field::Number { required, .. }
        | Field::Integer { required, .. }
        | Field::Boolean { required, .. }
        | Field::Multiselect { required, .. } => *required,
        Field::External { .. } => None,
    }
}

fn field_when(field: &Field) -> Option<&[When]> {
    match field {
        Field::String { when, .. }
        | Field::Number { when, .. }
        | Field::Integer { when, .. }
        | Field::Boolean { when, .. }
        | Field::Multiselect { when, .. } => when.as_deref(),
        Field::External { .. } => None,
    }
}

/// An unanswered referenced field makes the condition false for both ops.
fn is_active(field: &Field, answer: &Answer) -> bool {
    field_when(field).is_none_or(|conditions| {
        conditions.iter().all(|when| matches_when(when, answer.get(&when.key)))
    })
}

#[allow(clippy::float_cmp)] // parity: TS `===` on numbers is exact comparison
fn matches_when(when: &When, value: Option<&FormValue>) -> bool {
    let Some(value) = value else { return false };
    let hit = match value {
        FormValue::Strings(items) => items.iter().any(|item| when_value_matches(&when.value, item)),
        FormValue::String(text) => when_value_matches(&when.value, text),
        FormValue::Number(number) => {
            matches!(&when.value, WhenValue::Number(expected) if expected == number)
        }
        FormValue::Bool(boolean) => {
            matches!(&when.value, WhenValue::Bool(expected) if expected == boolean)
        }
    };
    if when.op == WhenOp::Eq {
        hit
    } else {
        !hit
    }
}

fn when_value_matches(expected: &WhenValue, actual: &str) -> bool {
    matches!(expected, WhenValue::String(value) if value == actual)
}

/// `Form.validateFields` — create-time validation of `when` references.
#[must_use]
pub fn validate_fields(fields: &[Field]) -> Option<String> {
    if fields.is_empty() {
        return Some("Form must have at least one field".into());
    }
    let mut earlier: HashMap<String, &Field> = HashMap::new();
    let mut keys: HashSet<String> = HashSet::new();
    for field in fields {
        let key = field_key(field).to_string();
        if keys.contains(&key) {
            return Some(format!("Duplicate form field key: {key}"));
        }
        keys.insert(key.clone());
        if let Field::External { .. } = field {
            continue;
        }
        for when in field_when(field).unwrap_or(&[]) {
            let Some(target) = earlier.get(&when.key) else {
                return Some(format!(
                    "Form field condition must reference an earlier field: {key} -> {}",
                    when.key
                ));
            };
            if let Some(invalid) = validate_when(when, target) {
                return Some(format!("{invalid}: {key} -> {}", when.key));
            }
        }
        earlier.insert(key, field);
    }
    None
}

fn validate_when(when: &When, target: &Field) -> Option<String> {
    let target_key = field_key(target).to_string();
    let _ = target_key;
    match target {
        Field::Boolean { .. } => {
            if !matches!(when.value, WhenValue::Bool(_)) {
                return Some("Form field condition value must be a boolean".into());
            }
            None
        }
        Field::Number { .. } | Field::Integer { .. } => {
            if !matches!(when.value, WhenValue::Number(_)) {
                return Some("Form field condition value must be a number".into());
            }
            None
        }
        // string and multiselect targets both compare against string values
        _ => {
            if !matches!(when.value, WhenValue::String(_)) {
                return Some("Form field condition value must be a string".into());
            }
            let closed = match target {
                Field::Multiselect { custom, .. } => !custom.unwrap_or(false),
                Field::String { options, custom, .. } => {
                    options.is_some() && !custom.unwrap_or(false)
                }
                _ => false,
            };
            if closed {
                let options = match target {
                    Field::Multiselect { options, .. } => options,
                    Field::String { options, .. } => options.as_deref().unwrap_or(&[]),
                    _ => &[],
                };
                let value = match &when.value {
                    WhenValue::String(value) => value.clone(),
                    _ => String::new(),
                };
                if !options.iter().any(|option| option.value == value) {
                    return Some(
                        "Form field condition value must be one of the field's options".into(),
                    );
                }
            }
            None
        }
    }
}

#[allow(clippy::too_many_lines)] // one-to-one port of form.ts field validation
fn validate_field(field: &Field, value: &FormValue) -> Option<String> {
    let key = field_key(field).to_string();
    match field {
        Field::String {
            required,
            min_length,
            max_length,
            pattern,
            format,
            options,
            custom,
            ..
        } => {
            let FormValue::String(text) = value else {
                return Some(format!("Expected string for form field: {key}"));
            };
            if required.unwrap_or(false) && text.is_empty() {
                return Some(format!("Missing required form field: {key}"));
            }
            if let Some(min) = min_length {
                if (text.chars().count() as u64) < *min {
                    return Some(format!("Form field is too short: {key}"));
                }
            }
            if let Some(max) = max_length {
                if (text.chars().count() as u64) > *max {
                    return Some(format!("Form field is too long: {key}"));
                }
            }
            if let Some(pattern) = pattern {
                match regex::Regex::new(pattern) {
                    Ok(regex) => {
                        if !regex.is_match(text) {
                            return Some(format!("Form field does not match pattern: {key}"));
                        }
                    }
                    Err(_) => return Some(format!("Form field has invalid pattern: {key}")),
                }
            }
            // The guard form would need an extra fall-through arm to stay
            // exhaustive (a guarded arm does not count for exhaustiveness).
            #[allow(clippy::collapsible_match)]
            match format {
                Some(oc_schema::form::StringFormat::Email) => {
                    if !is_email(text) {
                        return Some(format!("Expected email for form field: {key}"));
                    }
                }
                Some(oc_schema::form::StringFormat::Uri) => {
                    if !is_uri(text) {
                        return Some(format!("Expected URI for form field: {key}"));
                    }
                }
                Some(oc_schema::form::StringFormat::Date) => {
                    if !is_date(text) {
                        return Some(format!("Expected date for form field: {key}"));
                    }
                }
                Some(oc_schema::form::StringFormat::DateTime) => {
                    if !is_date_time(text) {
                        return Some(format!("Expected date-time for form field: {key}"));
                    }
                }
                None => {}
            }
            if let Some(options) = options {
                if !custom.unwrap_or(false) && !options.iter().any(|option| option.value == *text) {
                    return Some(format!("Invalid option for form field: {key}"));
                }
            }
            None
        }
        Field::Number { minimum, maximum, .. } | Field::Integer { minimum, maximum, .. } => {
            let FormValue::Number(number) = value else {
                return Some(format!("Expected number for form field: {key}"));
            };
            if !number.is_finite() {
                return Some(format!("Expected number for form field: {key}"));
            }
            if matches!(field, Field::Integer { .. }) && number.fract() != 0.0 {
                return Some(format!("Expected integer for form field: {key}"));
            }
            if let Some(minimum) = minimum {
                if number < minimum {
                    return Some(format!("Form field is too small: {key}"));
                }
            }
            if let Some(maximum) = maximum {
                if number > maximum {
                    return Some(format!("Form field is too large: {key}"));
                }
            }
            None
        }
        Field::Boolean { .. } => {
            if !matches!(value, FormValue::Bool(_)) {
                return Some(format!("Expected boolean for form field: {key}"));
            }
            None
        }
        Field::Multiselect { options, required, min_items, max_items, custom, .. } => {
            let FormValue::Strings(items) = value else {
                return Some(format!("Expected string array for form field: {key}"));
            };
            if required.unwrap_or(false) && items.is_empty() {
                return Some(format!("Missing required form field: {key}"));
            }
            if let Some(min) = min_items {
                if (items.len() as u64) < *min {
                    return Some(format!("Too few selections for form field: {key}"));
                }
            }
            if let Some(max) = max_items {
                if (items.len() as u64) > *max {
                    return Some(format!("Too many selections for form field: {key}"));
                }
            }
            if !custom.unwrap_or(false)
                && items.iter().any(|item| !options.iter().any(|option| option.value == *item))
            {
                return Some(format!("Invalid option for form field: {key}"));
            }
            None
        }
        Field::External { .. } => None,
    }
}

fn is_email(value: &str) -> bool {
    // /^[^\s@]+@[^\s@]+\.[^\s@]+$/
    let mut parts = value.split('@');
    let Some(local) = parts.next() else { return false };
    let Some(domain) = parts.next() else { return false };
    if parts.next().is_some() {
        return false;
    }
    !local.is_empty()
        && !local.contains(char::is_whitespace)
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !domain.contains(char::is_whitespace)
}

fn is_uri(value: &str) -> bool {
    // URL.canParse — requires `scheme:` with an ALPHA-first scheme.
    let Some(colon) = value.find(':') else { return false };
    let scheme = &value[..colon];
    !scheme.is_empty()
        && scheme.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && scheme.chars().all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.')
}

fn is_date(value: &str) -> bool {
    // /^\d{4}-\d{2}-\d{2}$/ + round-trip check.
    let bytes = value.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return false;
    }
    if !bytes
        .iter()
        .enumerate()
        .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit())
    {
        return false;
    }
    let year: i32 = value[0..4].parse().unwrap_or(0);
    let month: u32 = value[5..7].parse().unwrap_or(0);
    let day: u32 = value[8..10].parse().unwrap_or(0);
    (1..=12).contains(&month) && (1..=days_in_month(year, month)).contains(&day)
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}

fn is_date_time(value: &str) -> bool {
    // new Date(value) non-NaN — JS parses ISO 8601 dates/date-times
    // (optionally space-separated, fractional seconds, and Z/±HH:MM
    // offsets; a missing offset reads as local time, still valid).
    let (date, rest) = match value.split_once(['T', 't', ' ']) {
        Some((date, rest)) => (date, Some(rest)),
        None => (value, None),
    };
    if !is_date(date) {
        return false;
    }
    let Some(rest) = rest else { return true };
    // time: HH:mm[:ss[.fff]] followed by an optional offset.
    let bytes = rest.as_bytes();
    let mut index = 0;
    let take_digits = |index: &mut usize, count: usize| -> Option<u32> {
        if *index + count > bytes.len()
            || !bytes[*index..*index + count].iter().all(u8::is_ascii_digit)
        {
            return None;
        }
        let text = &rest[*index..*index + count];
        *index += count;
        text.parse().ok()
    };
    let Some(hour) = take_digits(&mut index, 2) else { return false };
    if index >= bytes.len() || bytes[index] != b':' {
        return false;
    }
    index += 1;
    let Some(minute) = take_digits(&mut index, 2) else { return false };
    let mut second = 0;
    let mut millis = 0;
    if index < bytes.len() && bytes[index] == b':' {
        index += 1;
        let Some(sec) = take_digits(&mut index, 2) else { return false };
        second = sec;
        if index < bytes.len() && bytes[index] == b'.' {
            index += 1;
            let start = index;
            while index < bytes.len() && bytes[index].is_ascii_digit() {
                index += 1;
            }
            if index == start {
                return false;
            }
            millis = rest[start..index].parse().unwrap_or(0);
        }
    }
    hour < 24 && minute < 60 && second < 60 && millis < 1000 && is_offset(&rest[index..])
}

/// Trailing UTC offset: empty (local), `Z`/`z`, `±HH:MM`, `±HHMM`, or `±HH`.
fn is_offset(rest: &str) -> bool {
    if rest.is_empty() || rest == "Z" || rest == "z" {
        return true;
    }
    let bytes = rest.as_bytes();
    if bytes[0] != b'+' && bytes[0] != b'-' {
        return false;
    }
    let digits = &bytes[1..];
    let is_digits = |slice: &[u8]| slice.iter().all(u8::is_ascii_digit);
    let (hour, minute): (u32, u32) = match digits.len() {
        2 => (rest[1..3].parse().unwrap_or(99), 0),
        4 => (rest[1..3].parse().unwrap_or(99), rest[3..5].parse().unwrap_or(99)),
        5 if digits[2] == b':' => {
            (rest[1..3].parse().unwrap_or(99), rest[4..6].parse().unwrap_or(99))
        }
        _ => return false,
    };
    let shape_ok = match digits.len() {
        2 => is_digits(digits),
        4 => is_digits(digits),
        5 => is_digits(&digits[..2]) && is_digits(&digits[3..]),
        _ => false,
    };
    shape_ok && hour < 24 && minute < 60
}

#[cfg(test)]
mod tests {
    #![allow(clippy::assert_is_empty)] // deliberate empty-state assertions

    use super::*;
    use oc_util::determinism::{SequentialIdGen, SystemClock};

    fn forms() -> Forms {
        Forms::new(Arc::new(SequentialIdGen::new()), Arc::new(SystemClock))
    }

    fn string_field(key: &str, required: bool) -> Field {
        Field::String {
            key: key.into(),
            title: None,
            description: None,
            required: Some(required),
            hidden: None,
            when: None,
            format: None,
            min_length: None,
            max_length: None,
            pattern: None,
            placeholder: None,
            default: None,
            options: None,
            custom: None,
        }
    }

    #[test]
    fn form_validates_absolute_uri_formats_without_restricting_schemes() {
        let fields = vec![Field::String {
            key: "uri".into(),
            title: None,
            description: None,
            required: None,
            hidden: None,
            when: None,
            format: Some(oc_schema::form::StringFormat::Uri),
            min_length: None,
            max_length: None,
            pattern: None,
            placeholder: None,
            default: None,
            options: None,
            custom: None,
        }];
        assert_eq!(
            validate_answer(
                &fields,
                &answer(&[("uri", FormValue::String("https://example.com/path".into()))])
            ),
            None
        );
        assert_eq!(
            validate_answer(
                &fields,
                &answer(&[("uri", FormValue::String("mailto:user@example.com".into()))])
            ),
            None
        );
        assert_eq!(
            validate_answer(
                &fields,
                &answer(&[("uri", FormValue::String("relative/path".into()))])
            ),
            Some("Expected URI for form field: uri".into())
        );
        assert_eq!(
            validate_answer(&fields, &answer(&[("uri", FormValue::String("://invalid".into()))])),
            Some("Expected URI for form field: uri".into())
        );
    }

    fn string_field_with_when(key: &str, required: bool, when: Vec<When>) -> Field {
        Field::String {
            key: key.into(),
            title: None,
            description: None,
            required: Some(required),
            hidden: None,
            when: Some(when),
            format: None,
            min_length: None,
            max_length: None,
            pattern: None,
            placeholder: None,
            default: None,
            options: None,
            custom: None,
        }
    }

    fn answer(entries: &[(&str, FormValue)]) -> Answer {
        entries.iter().map(|(key, value)| (key.to_string(), value.clone())).collect()
    }

    #[tokio::test]
    async fn form_returns_a_terminal_cancelled_state_from_ask() {
        let forms = forms();
        let asker = {
            let forms = forms.clone();
            tokio::spawn(async move {
                forms
                    .ask(NewForm {
                        session_id: "ses_test".into(),
                        title: "Test form".into(),
                        metadata: None,
                        fields: vec![string_field("name", true)],
                    })
                    .await
            })
        };
        tokio::task::yield_now().await;
        let pending = forms.list(None);
        assert_eq!(pending.len(), 1);
        forms.cancel(&pending[0].id, None).await.unwrap();
        assert_eq!(asker.await.unwrap().unwrap(), State::Cancelled { message: None });
        assert_eq!(forms.state(&pending[0].id).unwrap(), State::Cancelled { message: None });
    }

    #[tokio::test]
    async fn form_supports_the_global_elicitation_owner() {
        let forms = forms();
        let created = forms
            .create(NewForm {
                session_id: "global".into(),
                title: "MCP input".into(),
                metadata: None,
                fields: vec![string_field("name", false)],
            })
            .await
            .unwrap();
        assert_eq!(created.session_id, "global");
        assert_eq!(forms.list(Some("global")).len(), 1);
        assert!(forms.list(Some("other")).is_empty());
        forms
            .reply(&created.id, answer(&[("name", FormValue::String("Ava".into()))]))
            .await
            .unwrap();
        assert_eq!(
            forms.state(&created.id).unwrap(),
            State::Answered { answer: answer(&[("name", FormValue::String("Ava".into()))]) }
        );

        let external = forms
            .create(NewForm {
                session_id: "global".into(),
                title: "External setup".into(),
                metadata: None,
                fields: vec![Field::External {
                    key: "setup".into(),
                    url: "https://example.com/setup".into(),
                    title: None,
                    description: None,
                }],
            })
            .await
            .unwrap();
        forms.reply(&external.id, answer(&[("setup", FormValue::Bool(true))])).await.unwrap();
        assert_eq!(
            forms.state(&external.id).unwrap(),
            State::Answered { answer: answer(&[("setup", FormValue::Bool(true))]) }
        );
    }

    #[tokio::test]
    async fn form_gates_required_fields_and_rejects_inactive_answers_via_when() {
        let forms = forms();
        let created = forms
            .create(NewForm {
                session_id: "global".into(),
                title: "Conditional form".into(),
                metadata: None,
                fields: vec![
                    Field::Boolean {
                        key: "confirm".into(),
                        title: None,
                        description: None,
                        required: Some(true),
                        hidden: None,
                        when: None,
                        default: None,
                    },
                    string_field_with_when(
                        "reason",
                        true,
                        vec![When {
                            key: "confirm".into(),
                            op: WhenOp::Eq,
                            value: WhenValue::Bool(false),
                        }],
                    ),
                ],
            })
            .await
            .unwrap();
        let invalid = forms
            .reply(
                &created.id,
                answer(&[
                    ("confirm", FormValue::Bool(true)),
                    ("reason", FormValue::String("x".into())),
                ]),
            )
            .await
            .unwrap_err();
        assert!(invalid.to_string().ends_with("Form field is not active: reason"), "got {invalid}");
        let missing = forms
            .reply(&created.id, answer(&[("confirm", FormValue::Bool(false))]))
            .await
            .unwrap_err();
        assert!(
            missing.to_string().ends_with("Missing required form field: reason"),
            "got {missing}"
        );
        forms
            .reply(
                &created.id,
                answer(&[
                    ("confirm", FormValue::Bool(false)),
                    ("reason", FormValue::String("not ready".into())),
                ]),
            )
            .await
            .unwrap();
        assert!(matches!(forms.state(&created.id).unwrap(), State::Answered { .. }));
    }

    #[test]
    fn form_validates_fields_at_creation() {
        assert_eq!(validate_fields(&[]), Some("Form must have at least one field".to_string()));
        let duplicate = vec![string_field("a", false), string_field("a", false)];
        assert_eq!(validate_fields(&duplicate), Some("Duplicate form field key: a".to_string()));
        let forward = vec![string_field_with_when(
            "reason",
            false,
            vec![When { key: "confirm".into(), op: WhenOp::Eq, value: WhenValue::Bool(false) }],
        )];
        assert_eq!(
            validate_fields(&forward),
            Some(
                "Form field condition must reference an earlier field: reason -> confirm"
                    .to_string()
            )
        );
    }

    /// Mutable wall clock for deterministic TTL tests.
    struct MutableClock(parking_lot::Mutex<u64>);

    impl MutableClock {
        fn at(unix_ms: u64) -> Arc<Self> {
            Arc::new(Self(parking_lot::Mutex::new(unix_ms)))
        }
        fn advance(&self, ms: u64) {
            *self.0.lock() += ms;
        }
    }

    impl oc_util::determinism::Clock for MutableClock {
        fn now(&self) -> std::time::SystemTime {
            std::time::UNIX_EPOCH + std::time::Duration::from_millis(*self.0.lock())
        }
    }

    fn forms_with_clock(clock: Arc<MutableClock>) -> Forms {
        Forms::new(Arc::new(SequentialIdGen::new()), clock)
    }

    async fn settle_one(forms: &Forms) -> FormId {
        let created = forms
            .create(NewForm {
                session_id: "ses_ttl".into(),
                title: "TTL".into(),
                metadata: None,
                fields: vec![string_field("name", false)],
            })
            .await
            .unwrap();
        forms
            .reply(&created.id, answer(&[("name", FormValue::String("x".into()))]))
            .await
            .unwrap();
        created.id
    }

    #[tokio::test]
    async fn settled_forms_expire_after_retention_while_pending_forms_do_not() {
        let clock = MutableClock::at(1_000_000);
        let forms = forms_with_clock(Arc::clone(&clock));
        let settled = settle_one(&forms).await;
        let pending = forms
            .create(NewForm {
                session_id: "ses_ttl".into(),
                title: "Pending".into(),
                metadata: None,
                fields: vec![string_field("name", false)],
            })
            .await
            .unwrap();

        // Just before the retention window closes the settled entry survives.
        clock.advance(RETENTION_MS - 1_000);
        assert!(matches!(forms.state(&settled).unwrap(), State::Answered { .. }));

        // Past retention the settled entry is evicted; pending never expires.
        clock.advance(2_000);
        assert!(forms.get(&settled).is_err());
        assert!(matches!(forms.state(&settled), Err(NotFoundError(_))));
        assert!(forms.get(&pending.id).is_ok());
        assert_eq!(forms.list(None).len(), 1);
    }

    #[tokio::test]
    async fn cancelled_forms_expire_after_retention_too() {
        let clock = MutableClock::at(1_000_000);
        let forms = forms_with_clock(Arc::clone(&clock));
        let created = forms
            .create(NewForm {
                session_id: "ses_ttl".into(),
                title: "Cancel TTL".into(),
                metadata: None,
                fields: vec![string_field("name", false)],
            })
            .await
            .unwrap();
        forms.cancel(&created.id, None).await.unwrap();
        clock.advance(RETENTION_MS + 1);
        assert!(forms.state(&created.id).is_err());
    }

    #[test]
    fn date_time_format_validates_iso_shapes() {
        let field = Field::String {
            key: "when".into(),
            title: None,
            description: None,
            required: None,
            hidden: None,
            when: None,
            format: Some(oc_schema::form::StringFormat::DateTime),
            min_length: None,
            max_length: None,
            pattern: None,
            placeholder: None,
            default: None,
            options: None,
            custom: None,
        };
        let valid = [
            "2026-01-02T03:04:05Z",
            "2026-01-02T03:04:05.123Z",
            "2026-01-02t03:04:05z",
            "2026-01-02 03:04:05",
            "2026-01-02T03:04:05+02:00",
            "2026-01-02T03:04:05-0200",
            "2026-01-02",
            "2026-01-02T03:04:05",
        ];
        for value in valid {
            assert_eq!(
                validate_answer(std::slice::from_ref(&field), &answer(&[("when", FormValue::String(value.into()))])),
                None,
                "expected valid: {value}"
            );
        }
        let invalid = [
            "not a date",
            "2026-13-02T03:04:05Z",
            "2026-02-30",
            "2026-01-02T25:04:05Z",
            "2026-01-02T03:60:05Z",
            "2026-01-02T03:04:05+99:00",
            "2026-01-02T03",
            "2026-01-02T03:04:05.Z",
            "26-01-02",
        ];
        for value in invalid {
            assert_eq!(
                validate_answer(std::slice::from_ref(&field), &answer(&[("when", FormValue::String(value.into()))])),
                Some("Expected date-time for form field: when".to_string()),
                "expected invalid: {value}"
            );
        }
    }
}
