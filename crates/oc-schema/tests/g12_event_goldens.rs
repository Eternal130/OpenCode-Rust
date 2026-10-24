//! G-12 golden parity fixtures — the 7 raw-JSON event types from the
//! v2 recon (topic-core-recon.md) now carried by typed oc-schema models.
//! Each golden mirrors the canonical ref schema field names, casing and
//! nullability (`packages/schema/src/{pty,persistent-pty,filesystem,reference}.ts`,
//! v2.0.24): camelCase `exitCode` / `sessionID` / `ptyID` /
//! `foregroundProcess` (nullable), `size {cols, rows}`, `output
//! {head, tail}`, `exitCode` omitted while running.

use oc_schema::event::EphemeralEnvelope;
use oc_schema::filesystem::{Change, Changed, FilesystemEvent};
use oc_schema::id::EventId;
use oc_schema::misc_events::Empty;
use oc_schema::persistent_pty::{
    Added, Info as PptyInfo, Output, PersistentPtyEvent, Removed, Size,
};
use oc_schema::pty::{Created, Deleted, Exited, Info, PtyEvent, Status, Updated};
use oc_schema::reference::ReferenceEvent;
use serde_json::json;

fn envelope<D>(id: &str, created: u64, data: D) -> EphemeralEnvelope<D> {
    EphemeralEnvelope {
        id: EventId::parse(id).unwrap(),
        created,
        metadata: None,
        location: None,
        data,
    }
}

fn running_info() -> Info {
    Info {
        id: "pty_1".into(),
        title: "terminal".into(),
        command: "/bin/sh".into(),
        args: vec!["-l".into()],
        cwd: "/repo".into(),
        status: Status::Running,
        pid: 4242,
        exit_code: None,
    }
}

#[test]
fn golden_pty_created() {
    let event = PtyEvent::Created(envelope("evt_1", 1, Created { info: running_info() }));
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(
        json,
        json!({
            "id": "evt_1",
            "created": 1,
            "type": "pty.created",
            "data": {
                "info": {
                    "id": "pty_1",
                    "title": "terminal",
                    "command": "/bin/sh",
                    "args": ["-l"],
                    "cwd": "/repo",
                    "status": "running",
                    "pid": 4242
                }
            }
        }),
        "running info must omit exitCode"
    );
    assert_eq!(serde_json::from_value::<PtyEvent>(json.clone()).unwrap(), event);
}

#[test]
fn golden_pty_updated() {
    let mut info = running_info();
    info.status = Status::Exited;
    info.exit_code = Some(127);
    let event = PtyEvent::Updated(envelope("evt_2", 2, Updated { info }));
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "pty.updated");
    assert_eq!(json["data"]["info"]["exitCode"], 127);
    assert_eq!(json["data"]["info"]["status"], "exited");
    assert_eq!(serde_json::from_value::<PtyEvent>(json).unwrap(), event);
}

#[test]
fn golden_pty_exited_and_deleted() {
    let exited =
        PtyEvent::Exited(envelope("evt_3", 3, Exited { id: "pty_1".into(), exit_code: 3 }));
    assert_eq!(
        serde_json::to_value(&exited).unwrap(),
        json!({
            "id": "evt_3", "created": 3, "type": "pty.exited",
            "data": { "id": "pty_1", "exitCode": 3 }
        })
    );
    let deleted = PtyEvent::Deleted(envelope("evt_4", 4, Deleted { id: "pty_1".into() }));
    assert_eq!(
        serde_json::to_value(&deleted).unwrap(),
        json!({
            "id": "evt_4", "created": 4, "type": "pty.deleted",
            "data": { "id": "pty_1" }
        })
    );
    assert_eq!(
        serde_json::from_value::<PtyEvent>(serde_json::to_value(&deleted).unwrap()).unwrap(),
        deleted
    );
}

#[test]
fn golden_filesystem_changed() {
    for (kind, wire) in
        [(Change::Add, "add"), (Change::Change, "change"), (Change::Unlink, "unlink")]
    {
        let event = FilesystemEvent::Changed(envelope(
            "evt_5",
            5,
            Changed { file: "/repo/opencode.json".into(), event: kind },
        ));
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "filesystem.changed");
        assert_eq!(json["data"]["file"], "/repo/opencode.json");
        assert_eq!(json["data"]["event"], wire);
        assert_eq!(serde_json::from_value::<FilesystemEvent>(json).unwrap(), event);
    }
}

#[test]
fn golden_persistent_pty_added() {
    let event = PersistentPtyEvent::Added(envelope(
        "evt_6",
        6,
        Added {
            session_id: oc_schema::id::SessionId::parse("ses_1").unwrap(),
            terminal: PptyInfo {
                base: running_info(),
                session_id: oc_schema::id::SessionId::parse("ses_1").unwrap(),
                foreground_process: None,
                size: Size { cols: 80, rows: 24 },
                output: Output { head: 12, tail: 34 },
            },
        },
    ));
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(
        json,
        json!({
            "id": "evt_6",
            "created": 6,
            "type": "persistent-pty.added",
            "data": {
                "sessionID": "ses_1",
                "terminal": {
                    "id": "pty_1",
                    "title": "terminal",
                    "command": "/bin/sh",
                    "args": ["-l"],
                    "cwd": "/repo",
                    "status": "running",
                    "pid": 4242,
                    "sessionID": "ses_1",
                    "foregroundProcess": null,
                    "size": { "cols": 80, "rows": 24 },
                    "output": { "head": 12, "tail": 34 }
                }
            }
        }),
        "foregroundProcess must serialize as null, exitCode must be absent"
    );
    assert_eq!(serde_json::from_value::<PersistentPtyEvent>(json).unwrap(), event);
}

#[test]
fn golden_persistent_pty_removed() {
    let event = PersistentPtyEvent::Removed(envelope(
        "evt_7",
        7,
        Removed {
            session_id: oc_schema::id::SessionId::parse("ses_1").unwrap(),
            pty_id: "pty_1".into(),
        },
    ));
    assert_eq!(
        serde_json::to_value(&event).unwrap(),
        json!({
            "id": "evt_7", "created": 7, "type": "persistent-pty.removed",
            "data": { "sessionID": "ses_1", "ptyID": "pty_1" }
        })
    );
}

#[test]
fn golden_reference_updated() {
    let event = ReferenceEvent::Updated(envelope("evt_8", 8, Empty {}));
    assert_eq!(
        serde_json::to_value(&event).unwrap(),
        json!({
            "id": "evt_8", "created": 8, "type": "reference.updated", "data": {}
        })
    );
}
