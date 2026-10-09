//! `opencode stats` (handlers/stats.ts) — the usage dashboard. The
//! GitHub-style activity calendar is a REGISTERED simplification (a single
//! summary line instead of the painted week grid); everything else renders
//! v2's shape: heading, metrics, COST & TOKENS, MODELS, TOOL RELIABILITY.

use serde_json::Value;

use crate::cli::ServerParams;
use crate::handlers::connect;
use crate::util::error_message;

#[allow(clippy::struct_excessive_bools)]
pub struct Flags {
    pub days: Option<u32>,
    pub year: Option<u32>,
    pub all: bool,
    pub project: Option<String>,
    pub models: bool,
    pub tools: bool,
    pub cost: bool,
    pub full: bool,
    pub limit: u32,
    pub json: bool,
}

#[allow(clippy::cast_possible_wrap)]
pub async fn run(flags: &Flags, server: &ServerParams) -> i32 {
    let Flags { days, year, all, project, models, tools, cost, full, limit, json } = flags;
    if usize::from(days.is_some()) + usize::from(year.is_some()) + usize::from(*all) > 1 {
        eprintln!("Error: --days, --year, and --all cannot be combined");
        return 1;
    }
    let resolved = match connect(server, None).await {
        Ok(resolved) => resolved,
        Err(code) => return code,
    };
    let client = resolved.endpoint.client();

    let now_ms = crate::util::now_ms() as i64;
    let (from, to, label) = stats_range(*days, *year, *all, now_ms);
    let project_id = if project.as_deref() == Some(".") {
        let directory =
            std::env::current_dir().map(|path| path.display().to_string()).unwrap_or_default();
        match client
            .location()
            .get(&oc_client::types::LocationGetQuery {
                location: Some(oc_client::types::QueryLocation { directory: Some(directory) }),
            })
            .await
        {
            Ok(location) => Some(location.project.id),
            Err(error) => {
                eprintln!("{}", error_message(&error.to_string()));
                return 1;
            }
        }
    } else {
        project.clone()
    };
    let tools_mode = if *json || *tools || *full {
        "detail"
    } else if *models || *cost {
        "none"
    } else {
        "summary"
    };
    let stats = match client
        .session()
        .stats(&oc_client::types::ExperimentalSessionStatsQuery {
            from: from.map(|from| from.to_string()),
            to: Some(to.to_string()),
            project: project_id,
            timezone: Some(local_timezone()),
            tools: Some(match tools_mode {
                "detail" => oc_client::types::ExperimentalSessionStatsQueryTools::Detail,
                "none" => oc_client::types::ExperimentalSessionStatsQueryTools::None,
                _ => oc_client::types::ExperimentalSessionStatsQueryTools::Summary,
            }),
        })
        .await
    {
        Ok(stats) => stats,
        Err(error) => {
            eprintln!("{}", error_message(&error.to_string()));
            return 1;
        }
    };
    let stats = serde_json::to_value(&stats).expect("stats serializes");
    if *json {
        println!("{}", serde_json::to_string_pretty(&stats).expect("serializes"));
        return 0;
    }
    let scope = if project.is_none() {
        "all projects"
    } else if project.as_deref() == Some(".") {
        "current project"
    } else {
        "selected project"
    };
    print!(
        "{}",
        render(&stats, &label, scope, *models || *full, *tools || *full, *cost || *full, *limit)
    );
    0
}

fn local_timezone() -> String {
    std::env::var("TZ").unwrap_or_else(|_| "UTC".to_owned())
}

/// `statsRange`: local-midnight bucketing.
fn stats_range(
    days: Option<u32>,
    year: Option<u32>,
    all: bool,
    now_ms: i64,
) -> (Option<i64>, i64, String) {
    let to = now_ms + 1;
    if all {
        return (None, to, "all time".to_owned());
    }
    if let Some(days) = days {
        let today_start = now_ms - now_ms.rem_euclid(86_400_000);
        let from = today_start - i64::from(days.saturating_sub(1)) * 86_400_000;
        let label =
            if days == 0 || days == 1 { "today".to_owned() } else { format!("last {days} days") };
        return (Some(from), to, label);
    }
    let year = year.unwrap_or_else(|| {
        let seconds = now_ms.div_euclid(1000);
        let days = seconds.div_euclid(86_400);
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        u32::try_from(if m <= 2 { y + 1 } else { y }).unwrap_or(2026)
    });
    let from = unix_ms_from_civil(i64::from(year), 1, 1);
    let label = if from <= now_ms && now_ms < unix_ms_from_civil(i64::from(year) + 1, 1, 1) {
        format!("{year} so far")
    } else {
        year.to_string()
    };
    let to =
        if label.ends_with("so far") { to } else { unix_ms_from_civil(i64::from(year) + 1, 1, 1) };
    (Some(from), to, label)
}

fn unix_ms_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    (era * 146_097 + doe - 719_468) * 86_400_000
}

fn field<'a>(value: &'a Value, key: &str) -> &'a Value {
    value.get(key).unwrap_or(&Value::Null)
}

fn i64_of(value: &Value) -> i64 {
    value.as_i64().unwrap_or(0)
}

fn f64_of(value: &Value) -> f64 {
    value.as_f64().unwrap_or(0.0)
}

#[allow(clippy::cast_possible_truncation)]
fn format_number(value: f64) -> String {
    if value >= 1_000_000_000.0 {
        format!("{}b", trim_decimal(value / 1_000_000_000.0))
    } else if value >= 1_000_000.0 {
        format!("{}m", trim_decimal(value / 1_000_000.0))
    } else if value >= 1_000.0 {
        format!("{}k", trim_decimal(value / 1_000.0))
    } else {
        format!("{}", value.round() as i64)
    }
}

fn trim_decimal(value: f64) -> String {
    let text = format!("{value:.1}");
    text.trim_end_matches(".0").to_owned()
}

fn format_percent(value: f64) -> String {
    format!("{value:.prec$}%", value = value, prec = if value >= 10.0 { 1 } else { 2 })
}

#[allow(clippy::cast_possible_truncation)]
fn format_duration(value: f64) -> String {
    if value < 1000.0 {
        format!("{}ms", value.round() as i64)
    } else {
        format!("{}s", trim_decimal(value / 1000.0))
    }
}

#[allow(clippy::cast_precision_loss)]
fn metric_count(value: i64, noun: &str) -> String {
    metric_count_f64(value as f64, noun)
}

#[allow(clippy::cast_possible_truncation, clippy::float_cmp)]
fn metric_count_f64(value: f64, noun: &str) -> String {
    format!("{} {noun}{}", format_number(value), if value == 1.0 { "" } else { "s" })
}

fn token_total(tokens: &Value) -> f64 {
    f64_of(field(tokens, "input"))
        + f64_of(field(tokens, "output"))
        + f64_of(field(tokens, "reasoning"))
        + f64_of(field(field(tokens, "cache"), "read"))
        + f64_of(field(field(tokens, "cache"), "write"))
}

fn table_row(label: &str, second: &str, third: &str, fourth: &str) -> String {
    let truncated: String = label.chars().take(34).collect();
    format!("{truncated:<34}{second:>10}{third:>12}{fourth:>12}")
}

#[allow(clippy::too_many_arguments)]
fn render(
    stats: &Value,
    label: &str,
    scope: &str,
    models: bool,
    tools: bool,
    cost: bool,
    limit: u32,
) -> String {
    let mut lines: Vec<String> = Vec::new();
    let details = models || tools || cost;
    let empty = i64_of(field(stats, "sessions")) == 0
        && i64_of(field(stats, "prompts")) == 0
        && i64_of(field(stats, "steps")) == 0;
    let tokens = field(stats, "tokens");
    let total_tokens = token_total(tokens);
    let tools_mode = field(field(stats, "tools"), "mode").as_str().unwrap_or("none");
    let tool_totals =
        if tools_mode == "none" { None } else { Some(field(field(stats, "tools"), "totals")) };
    #[allow(clippy::cast_precision_loss)]
    let tool_rate = tool_totals.and_then(|totals| {
        let terminal = i64_of(field(totals, "succeeded")) + i64_of(field(totals, "failed"));
        if terminal == 0 {
            None
        } else {
            Some((i64_of(field(totals, "succeeded")) as f64 / terminal as f64) * 100.0)
        }
    });
    let tool_summary = match (tool_totals, tool_rate) {
        (None, _) => "tool stats unavailable".to_owned(),
        (Some(_), None) => "no tool calls".to_owned(),
        (Some(_), Some(rate)) => format!("{} tool success", format_percent(rate)),
    };
    let subagents = i64_of(field(stats, "subagents"));
    let session_line = if subagents > 0 {
        format!(
            "{} · {}",
            metric_count(i64_of(field(stats, "sessions")), "session"),
            metric_count(subagents, "subagent")
        )
    } else {
        metric_count(i64_of(field(stats, "sessions")), "session")
    };

    if details {
        lines.push(format!("{label} · {scope}"));
    } else if empty {
        lines.push(format!("opencode stats · {label} · {scope}"));
        lines.push(String::new());
        lines.push("no activity in this range".to_owned());
        lines.push(String::new());
        lines.push("opencode.ai".to_owned());
    } else {
        lines.push(format!("opencode stats · {label} · {scope}"));
        lines.push(String::new());
        // REGISTERED simplification: activity summary replaces the painted
        // calendar grid (activeDays + streak).
        lines.push(format!(
            "activity · {} · best streak {} day{}",
            metric_count(i64_of(field(stats, "activeDays")), "active day"),
            i64_of(field(stats, "streak")),
            if i64_of(field(stats, "streak")) == 1 { "" } else { "s" }
        ));
        lines.push(String::new());
        lines.push(session_line);
        lines.push(format!(
            "{} · {} · {}",
            metric_count(i64_of(field(stats, "prompts")), "prompt"),
            metric_count(i64_of(field(stats, "steps")), "step"),
            metric_count_f64(total_tokens, "token")
        ));
        lines.push(format!(
            "{tool_summary} · {} · best streak {} day{}",
            metric_count(i64_of(field(stats, "activeDays")), "active day"),
            i64_of(field(stats, "streak")),
            if i64_of(field(stats, "streak")) == 1 { "" } else { "s" }
        ));
        lines.push(String::new());
        lines.push("opencode.ai".to_owned());
    }

    if cost {
        lines.push(String::new());
        lines.extend(render_cost(stats));
    }
    if models {
        lines.push(String::new());
        lines.extend(render_models(stats, limit));
    }
    if tools {
        lines.push(String::new());
        lines.extend(render_tools(stats, limit));
    }
    let mut output = lines.join("\n");
    output.push('\n');
    output
}

fn render_cost(stats: &Value) -> Vec<String> {
    let tokens = field(stats, "tokens");
    let cache = field(tokens, "cache");
    let input = f64_of(field(tokens, "input"))
        + f64_of(field(cache, "read"))
        + f64_of(field(cache, "write"));
    let cached = if input == 0.0 { 0.0 } else { f64_of(field(cache, "read")) / input * 100.0 };
    vec![
        "COST & TOKENS".to_owned(),
        row("cost", &format!("${:.2}", f64_of(field(stats, "cost")))),
        row("input", &format_number(f64_of(field(tokens, "input")))),
        row("output", &format_number(f64_of(field(tokens, "output")))),
        row("reasoning", &format_number(f64_of(field(tokens, "reasoning")))),
        row("cache read", &format_number(f64_of(field(cache, "read")))),
        row("cache write", &format_number(f64_of(field(cache, "write")))),
        row("cached input", &format_percent(cached)),
    ]
}

fn row(label: &str, value: &str) -> String {
    format!("  {label:<20}{value}")
}

#[allow(clippy::cast_precision_loss)]
fn render_models(stats: &Value, limit: u32) -> Vec<String> {
    let models = field(stats, "models").as_array().cloned().unwrap_or_default();
    if models.is_empty() {
        return vec!["MODELS".to_owned(), "  no model usage".to_owned()];
    }
    let selected: Vec<&Value> = models.iter().take(limit as usize).collect();
    let more = models.len() - selected.len();
    let mut lines = vec!["MODELS".to_owned(), table_row("model", "tokens", "steps", "cost")];
    for item in &selected {
        let model = field(item, "model");
        let reference = format!(
            "{}/{}{}",
            field(model, "providerID").as_str().unwrap_or_default(),
            field(model, "id").as_str().unwrap_or_default(),
            field(model, "variant")
                .as_str()
                .map(|variant| format!("#{variant}"))
                .unwrap_or_default()
        );
        lines.push(table_row(
            &reference,
            &format_number(token_total(field(item, "tokens"))),
            &format_number(i64_of(field(item, "steps")) as f64),
            &format!("${:.2}", f64_of(field(item, "cost"))),
        ));
    }
    if more > 0 {
        lines.push(String::new());
        lines.push(format!("+{more} more model{}", if more == 1 { "" } else { "s" }));
    }
    lines
}

#[allow(clippy::cast_precision_loss)]
fn render_tools(stats: &Value, limit: u32) -> Vec<String> {
    let tools = field(stats, "tools");
    if field(tools, "mode").as_str() != Some("detail") {
        return vec!["TOOL RELIABILITY".to_owned(), "  tool details unavailable".to_owned()];
    }
    let usage = field(tools, "usage").as_array().cloned().unwrap_or_default();
    if usage.is_empty() {
        return vec!["TOOL RELIABILITY".to_owned(), "  no tool calls".to_owned()];
    }
    let selected: Vec<&Value> = usage.iter().take(limit as usize).collect();
    let more = usage.len() - selected.len();
    let totals = field(tools, "totals");
    let mut lines = vec!["TOOL RELIABILITY".to_owned(), table_row("tool", "calls", "error", "p50")];
    for tool in &selected {
        let terminal = i64_of(field(tool, "succeeded")) + i64_of(field(tool, "failed"));
        let error = if terminal == 0 {
            "-".to_owned()
        } else {
            format_percent(i64_of(field(tool, "failed")) as f64 / terminal as f64 * 100.0)
        };
        let p50 =
            field(tool, "durationP50").as_f64().map_or_else(|| "-".to_owned(), format_duration);
        lines.push(table_row(
            field(tool, "name").as_str().unwrap_or_default(),
            &format_number(i64_of(field(tool, "calls")) as f64),
            &error,
            &p50,
        ));
    }
    lines.push(String::new());
    lines.push(format!(
        "{} finished calls · {} unfinished",
        format_number(
            (i64_of(field(totals, "succeeded")) + i64_of(field(totals, "failed"))) as f64
        ),
        format_number(i64_of(field(totals, "unfinished")) as f64)
    ));
    if more > 0 {
        lines.push(format!("+{more} more tool{}", if more == 1 { "" } else { "s" }));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_labels() {
        let now = 1_700_000_000_000_i64;
        let (from, _to, label) = stats_range(Some(1), None, false, now);
        assert_eq!(label, "today");
        assert!(from.is_some());
        let (_from, _to, label) = stats_range(None, None, true, now);
        assert_eq!(label, "all time");
        let (_from, _to, label) = stats_range(Some(7), None, false, now);
        assert_eq!(label, "last 7 days");
    }

    #[test]
    fn number_formatting() {
        assert_eq!(format_number(1500.0), "1.5k");
        assert_eq!(format_number(2_000_000.0), "2m");
        assert_eq!(format_number(999.0), "999");
    }
}
