//! G-06 — the V1→V2 data-migration overlay
//! (`component/migration-overlay.tsx`): a NON-interactive top-right
//! widget. On app start a watcher sleeps 1 s, then polls
//! `GET /api/experimental/migration/v1` every 1 s; while the status is
//! `running` the overlay renders (`top=1`, `right=2`, raised
//! background, `SplitBorder` left rail in the info color, Spinner +
//! "label numerator/denominator"); `completed` removes it and stops
//! the poll; an `error` status (or transport failure) stops the poll
//! and shows the "Data migration failed" error toast (~10 s).

use crate::render::{CellColor, Frame};
use crate::theme::ThemeStyles;
use oc_client::types::ExperimentalMigrationV1StatusResponse as StatusResponse;

/// `Progress` (migration-overlay.tsx:9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationProgress {
    pub label: String,
    pub numerator: Option<i64>,
    pub denominator: Option<i64>,
}

/// The watcher's events (the spawned task → the run-loop pump → the
/// store).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationEvent {
    Running(MigrationProgress),
    Completed,
    Failed(String),
}

/// The poll loop (migration-overlay.tsx:18-38): 1 s initial sleep,
/// then `migration.v1.status` every 1 s. `running` → progress;
/// `required` keeps polling (no overlay — the ref clears the signal);
/// `completed` stops; `error` (or a transport failure) stops with the
/// failure. `fetch` and `delay` are injected so tests drive the loop
/// deterministically (no real timers).
pub async fn watch_migration<F, Fut, D, DFut>(
    mut fetch: F,
    mut delay: D,
    mut emit: impl FnMut(MigrationEvent),
) where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<StatusResponse, String>>,
    D: FnMut() -> DFut,
    DFut: std::future::Future<Output = ()>,
{
    delay().await; // the 1 s onMount sleep
    loop {
        match fetch().await {
            Err(error) => {
                emit(MigrationEvent::Failed(error));
                return;
            }
            Ok(status) => match status {
                StatusResponse::ExperimentalMigrationV1StatusResponseVariant1(variant) => {
                    use oc_client::types::ExperimentalMigrationV1StatusResponseVariant1Status as S;
                    match variant.status {
                        S::Completed => {
                            emit(MigrationEvent::Completed);
                            return;
                        }
                        // `required`: not yet running — keep polling.
                        S::Required => {}
                    }
                }
                StatusResponse::ExperimentalMigrationV1StatusResponseVariant2(variant) => {
                    emit(MigrationEvent::Running(MigrationProgress {
                        label: variant.progress.label,
                        numerator: variant.progress.numerator,
                        denominator: variant.progress.denominator,
                    }));
                }
                StatusResponse::ExperimentalMigrationV1StatusResponseVariant3(variant) => {
                    emit(MigrationEvent::Failed(variant.error));
                    return;
                }
            },
        }
        delay().await; // the 1 s poll interval
    }
}

/// The production watcher: the generated client against the live
/// server, real 1 s timers, events over an unbounded channel (the
/// run loop drains them synchronously).
#[must_use]
pub fn spawn_migration_watcher(
    client: oc_client::Client,
) -> (tokio::sync::mpsc::UnboundedReceiver<MigrationEvent>, tokio::task::JoinHandle<()>) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let task = tokio::spawn(async move {
        let client_for_fetch = client.clone();
        watch_migration(
            move || {
                let client = client_for_fetch.clone();
                async move { client.migration().status().await.map_err(|error| error.to_string()) }
            },
            || async { tokio::time::sleep(std::time::Duration::from_secs(1)).await },
            move |event| {
                let _ = tx.send(event);
            },
        )
        .await;
    });
    (rx, task)
}

/// `count` (migration-overlay.tsx:41-45): " n", " n/d" or "".
#[must_use]
pub fn progress_count(progress: &MigrationProgress) -> String {
    match (progress.numerator, progress.denominator) {
        (Some(numerator), Some(denominator)) => format!(" {numerator}/{denominator}"),
        (Some(numerator), None) => format!(" {numerator}"),
        _ => String::new(),
    }
}

/// `SPINNER_FRAMES` — the shared braille spinner (`component/spinner`,
/// interval 80 ms — the same table the app-state overlays use).
const SPINNER_FRAMES: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

fn spinner_frame(elapsed_ms: u64) -> char {
    SPINNER_FRAMES[((elapsed_ms / 80) % SPINNER_FRAMES.len() as u64) as usize]
}

/// Paint the overlay above every other surface (`zIndex` `10_000`): a
/// raised box at `top=1, right=2` with a left `SplitBorder` rail in
/// the info color, padding 2/2/1/1, one row of Spinner + label +
/// counters (migration-overlay.tsx:47-70).
pub fn overlay_migration(
    frame: &mut Frame,
    styles: &ThemeStyles,
    progress: &MigrationProgress,
    elapsed_ms: u64,
) {
    let width = frame.width;
    if width < 6 || frame.height < 4 {
        return;
    }
    let spinner = spinner_frame(elapsed_ms);
    let content = format!("{spinner} {}{}", progress.label, progress_count(progress));
    // border(1) + paddingLeft(2) + content + paddingRight(2).
    let box_width = content.chars().count() + 1 + 2 + 2;
    let left = width.saturating_sub(box_width + 2); // right=2
    let top = 1; // top=1
    let rows = top + 3; // paddingTop(1) + row + paddingBottom(1)
    let raised = styles.background_raised;
    let info = CellColor::from(styles.info);
    let fg_base: CellColor = styles.text.fg.unwrap_or(ratatui::style::Color::Reset).into();
    for row in top..rows.min(frame.height) {
        for col in left..(left + box_width).min(width) {
            if let Some(cell) = frame.cells.get_mut(row).and_then(|line| line.get_mut(col)) {
                cell.bg = CellColor::from(raised);
                cell.fg = fg_base;
                cell.ch = ' ';
                cell.attrs = 0;
            }
        }
        // The SplitBorder left rail in the info color.
        if let Some(cell) = frame.cells.get_mut(row).and_then(|line| line.get_mut(left)) {
            cell.ch = '▎';
            cell.fg = info;
        }
    }
    // The content row (inside the paddings, after the rail).
    let content_col = left + 1 + 2;
    for (offset, ch) in content.chars().enumerate() {
        if let Some(cell) =
            frame.cells.get_mut(top + 1).and_then(|line| line.get_mut(content_col + offset))
        {
            cell.ch = ch;
            cell.fg = info;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reactive::Runtime;
    use crate::render::Cell;
    use crate::store::Store;
    use oc_client::types::{
        ExperimentalMigrationV1StatusResponseVariant1 as V1,
        ExperimentalMigrationV1StatusResponseVariant1Status as V1Status,
        ExperimentalMigrationV1StatusResponseVariant2 as V2,
        ExperimentalMigrationV1StatusResponseVariant2Progress as V2Progress,
        ExperimentalMigrationV1StatusResponseVariant2Status as V2Status,
        ExperimentalMigrationV1StatusResponseVariant3 as V3,
        ExperimentalMigrationV1StatusResponseVariant3Status as V3Status,
    };

    fn running(label: &str, numerator: Option<i64>, denominator: Option<i64>) -> StatusResponse {
        StatusResponse::ExperimentalMigrationV1StatusResponseVariant2(V2 {
            progress: V2Progress { label: label.to_owned(), numerator, denominator },
            status: V2Status::Running,
        })
    }

    fn required() -> StatusResponse {
        StatusResponse::ExperimentalMigrationV1StatusResponseVariant1(V1 {
            status: V1Status::Required,
        })
    }

    fn completed() -> StatusResponse {
        StatusResponse::ExperimentalMigrationV1StatusResponseVariant1(V1 {
            status: V1Status::Completed,
        })
    }

    fn errored() -> StatusResponse {
        StatusResponse::ExperimentalMigrationV1StatusResponseVariant3(V3 {
            error: "boom".to_owned(),
            status: V3Status::Error,
        })
    }

    /// A scripted fetch source + instant delays: the poll loop driven
    /// deterministically (no real timers).
    #[derive(Clone)]
    struct Script {
        responses: std::rc::Rc<std::cell::RefCell<Vec<Result<StatusResponse, String>>>>,
        calls: std::rc::Rc<std::cell::Cell<usize>>,
        delays: std::rc::Rc<std::cell::Cell<usize>>,
    }

    impl Script {
        fn new(responses: Vec<Result<StatusResponse, String>>) -> Self {
            Self {
                responses: std::rc::Rc::new(std::cell::RefCell::new(responses)),
                calls: std::rc::Rc::new(std::cell::Cell::new(0)),
                delays: std::rc::Rc::new(std::cell::Cell::new(0)),
            }
        }
    }

    async fn drive(script: &Script) -> Vec<MigrationEvent> {
        let events = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let events_sink = events.clone();
        let fetch_script = script.clone();
        let delay_script = script.clone();
        watch_migration(
            move || {
                let script = fetch_script.clone();
                async move {
                    script.calls.set(script.calls.get() + 1);
                    script.responses.borrow_mut().remove(0)
                }
            },
            move || {
                let script = delay_script.clone();
                async move {
                    script.delays.set(script.delays.get() + 1);
                }
            },
            move |event| events_sink.borrow_mut().push(event),
        )
        .await;
        let events = events.borrow().clone();
        events
    }

    #[tokio::test]
    async fn watcher_running_then_completed() {
        let script = Script::new(vec![
            Ok(required()),
            Ok(running("Migrating sessions", Some(2), Some(5))),
            Ok(completed()),
            Ok(completed()), // must never be fetched
        ]);
        let events = drive(&script).await;
        assert_eq!(
            events,
            vec![
                MigrationEvent::Running(MigrationProgress {
                    label: "Migrating sessions".to_owned(),
                    numerator: Some(2),
                    denominator: Some(5),
                }),
                MigrationEvent::Completed,
            ],
            "required stays silent; running reports counters; completed stops"
        );
        assert_eq!(script.calls.get(), 3, "completed exits before the 4th poll");
        assert_eq!(script.delays.get(), 3, "initial sleep + one interval per silent/running poll");
    }

    #[tokio::test]
    async fn watcher_error_status_and_transport_failure() {
        let script = Script::new(vec![Ok(errored())]);
        assert_eq!(
            drive(&script).await,
            vec![MigrationEvent::Failed("boom".to_owned())],
            "the error status surfaces the failure"
        );
        let script = Script::new(vec![Err("network down".to_owned())]);
        assert_eq!(
            drive(&script).await,
            vec![MigrationEvent::Failed("network down".to_owned())],
            "a transport failure surfaces the failure"
        );
    }

    #[tokio::test]
    async fn watcher_sleeps_before_the_first_poll() {
        let script = Script::new(vec![Ok(completed())]);
        let _ = drive(&script).await;
        assert_eq!(
            script.delays.get(),
            1,
            "the onMount 1 s sleep ran (interval sleep skipped by exit)"
        );
    }

    /// `count` (migration-overlay.tsx:41-45).
    #[test]
    fn progress_count_format() {
        let make = |numerator, denominator| MigrationProgress {
            label: "l".into(),
            numerator,
            denominator,
        };
        assert_eq!(progress_count(&make(None, None)), "");
        assert_eq!(progress_count(&make(Some(3), None)), " 3");
        assert_eq!(progress_count(&make(Some(3), Some(7))), " 3/7");
    }

    fn frame_with(width: usize, height: usize) -> Frame {
        Frame {
            cells: vec![
                vec![Cell { bg: CellColor::Rgb(60, 60, 60), ..Cell::default() }; width];
                height
            ],
            width,
            height,
        }
    }

    /// The overlay paints top-right with the counters and clears with
    /// the store state (completed → None).
    #[test]
    fn overlay_paints_top_right_with_counters_and_clears() {
        let styles = crate::theme::ThemeState::v2_dark().styles();
        let store = Store::new(Runtime::new());
        assert!(store.migration_progress().is_none(), "no overlay before running");

        let progress = MigrationProgress {
            label: "Migrating sessions".to_owned(),
            numerator: Some(2),
            denominator: Some(5),
        };
        store.set_migration_progress(Some(progress.clone()));
        let mut frame = frame_with(60, 12);
        if let Some(progress) = store.migration_progress() {
            overlay_migration(&mut frame, &styles, &progress, store.clock());
        }
        let row = frame.text_grid()[2].clone();
        assert!(row.contains("Migrating sessions 2/5"), "content row: {row}");
        assert!(row.ends_with("  "), "right=2 margin");
        let spinner = spinner_frame(0);
        assert!(
            row.contains(&format!("{spinner} Migrating sessions 2/5")),
            "spinner + label + counters: {row}"
        );
        assert!(!frame.text_grid()[4].contains("Migrating"), "3 rows tall only");

        // Completed → the store clears → nothing paints.
        store.set_migration_progress(None);
        let mut frame = frame_with(60, 12);
        if let Some(progress) = store.migration_progress() {
            overlay_migration(&mut frame, &styles, &progress, store.clock());
        }
        assert!(!frame.text_grid().join("\n").contains("Migrating sessions"));
    }
}
