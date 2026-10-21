// Upstream-mirror lint posture: the merman port keeps the TypeScript
// arithmetic shape (JS Number semantics on an i64 grid, the same cast
// sites and long single-responsibility functions), so the pedantic
// numeric/style lints that would rewrite that shape are quieted here.
#![allow(
    clippy::cast_possible_wrap,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_lossless,
    clippy::doc_markdown,
    clippy::must_use_candidate,
    clippy::too_many_lines,
    clippy::too_many_arguments,
    clippy::needless_range_loop,
    clippy::items_after_statements,
    clippy::explicit_iter_loop,
    clippy::for_kv_map,
    clippy::map_unwrap_or,
    clippy::manual_strip,
    clippy::range_plus_one,
    clippy::option_option,
    clippy::implicit_hasher,
    clippy::match_same_arms
)]

//! `core/spatial.ts` — the claim/conflict index (notes + labels + route
//! collision policy).

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::render::mermaid::core::geometry::{orthogonal_path_points, Bounds, Point};

/// `SpatialRole`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SpatialRole {
    Body,
    Boundary,
    Terminal,
    Route,
    Label,
}

/// `SpatialSpan`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpatialSpan {
    pub y: i64,
    pub from_x: i64,
    pub to_x: i64,
}

/// `SpatialClaim`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpatialClaim {
    pub id: String,
    pub owner: String,
    pub role: SpatialRole,
    pub spans: Vec<SpatialSpan>,
}

/// `SpatialContact`.
#[derive(Debug, Clone)]
pub struct SpatialContact {
    pub owner: String,
    pub points: Vec<Point>,
}

/// `SpatialClearance` — uniform or per-axis.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Clearance {
    pub x: i64,
    pub y: i64,
}

/// `SpatialCollisionPolicy` — clearance per role (absent = 0).
#[derive(Debug, Clone, Default)]
pub struct CollisionPolicy {
    pub contacts: Vec<SpatialContact>,
    pub clearance: Option<Clearance>,
    pub clearance_by_role: HashMap<SpatialRole, Clearance>,
}

fn normalized_span(y: i64, from_x: i64, to_x: i64) -> SpatialSpan {
    SpatialSpan { y, from_x: from_x.min(to_x), to_x: from_x.max(to_x) }
}

/// `spatialRectSpans`.
#[must_use]
pub fn spatial_rect_spans(bounds: &Bounds) -> Vec<SpatialSpan> {
    assert!(bounds.width > 0 && bounds.height > 0, "Spatial bounds must have positive dimensions");
    (0..bounds.height)
        .map(|offset| {
            normalized_span(bounds.top + offset, bounds.left, bounds.left + bounds.width - 1)
        })
        .collect()
}

/// `spatialPathSpans` — the orthogonal polyline's cells, merged into
/// per-row spans.
#[must_use]
pub fn spatial_path_spans(points: &[Point]) -> Vec<SpatialSpan> {
    for window in points.windows(2) {
        assert!(
            window[0].x == window[1].x || window[0].y == window[1].y,
            "Spatial paths must be orthogonal"
        );
    }
    let mut cells: BTreeMap<i64, BTreeSet<i64>> = BTreeMap::new();
    let mut add = |point: Point| {
        cells.entry(point.y).or_default().insert(point.x);
    };
    if points.len() == 1 {
        add(points[0]);
    }
    for point in orthogonal_path_points(points) {
        add(point);
    }

    let mut spans = Vec::new();
    for (y, xs) in cells {
        let mut start = None;
        let mut end = None;
        for x in xs {
            match (start, end) {
                (None, _) => start = Some(x),
                (Some(_), Some(previous)) if x == previous + 1 => end = Some(x),
                (Some(start_value), Some(end_value)) => {
                    spans.push(normalized_span(y, start_value, end_value));
                    start = Some(x);
                    end = None;
                }
                (Some(start_value), None) => {
                    spans.push(normalized_span(y, start_value, start_value));
                    start = Some(x);
                }
            }
        }
        if let Some(start_value) = start {
            spans.push(normalized_span(y, start_value, end.unwrap_or(start_value)));
        }
    }
    spans
}

/// `spatialRectClaim`.
#[must_use]
pub fn spatial_rect_claim(
    id: impl Into<String>,
    owner: impl Into<String>,
    role: SpatialRole,
    bounds: &Bounds,
) -> SpatialClaim {
    SpatialClaim { id: id.into(), owner: owner.into(), role, spans: spatial_rect_spans(bounds) }
}

/// `spatialPathClaim`.
#[must_use]
pub fn spatial_path_claim(
    id: impl Into<String>,
    owner: impl Into<String>,
    role: SpatialRole,
    points: &[Point],
) -> SpatialClaim {
    SpatialClaim { id: id.into(), owner: owner.into(), role, spans: spatial_path_spans(points) }
}

fn same_claim(left: &SpatialClaim, right: &SpatialClaim) -> bool {
    left.id == right.id
        && left.owner == right.owner
        && left.role == right.role
        && left.spans == right.spans
}

fn point_is_contact(point: Point, existing: &SpatialClaim, contacts: &[SpatialContact]) -> bool {
    contacts
        .iter()
        .any(|contact| contact.owner == existing.owner && contact.points.contains(&point))
}

fn roles_may_overlap(moving: &SpatialClaim, existing: &SpatialClaim) -> bool {
    if moving.owner == existing.owner {
        return true;
    }
    moving.role == SpatialRole::Route && existing.role == SpatialRole::Route
}

fn inflate_span(span: &SpatialSpan, clearance: Clearance) -> SpatialSpan {
    SpatialSpan { y: span.y, from_x: span.from_x - clearance.x, to_x: span.to_x + clearance.x }
}

/// `SpatialIndex` — immutable overlay of claims.
#[derive(Debug, Clone, Default)]
pub struct SpatialIndex {
    claims: Vec<SpatialClaim>,
}

impl SpatialIndex {
    #[must_use]
    pub fn empty() -> Self {
        Self { claims: Vec::new() }
    }

    #[must_use]
    pub fn claims(&self) -> &[SpatialClaim] {
        &self.claims
    }

    fn from_claims(claims: Vec<SpatialClaim>) -> Self {
        let mut claims = claims;
        for claim in &mut claims {
            claim.spans.sort_by(|left, right| {
                left.y
                    .cmp(&right.y)
                    .then(left.from_x.cmp(&right.from_x))
                    .then(left.to_x.cmp(&right.to_x))
            });
        }
        claims.sort_by(|left, right| left.id.cmp(&right.id));
        Self { claims }
    }

    #[must_use]
    pub fn add(&self, claims: Vec<SpatialClaim>) -> Self {
        self.overlay(&Self::from_claims(claims))
    }

    #[must_use]
    pub fn overlay(&self, other: &SpatialIndex) -> SpatialIndex {
        let mut claims: HashMap<String, SpatialClaim> =
            self.claims.iter().cloned().map(|claim| (claim.id.clone(), claim)).collect();
        for claim in &other.claims {
            if let Some(existing) = claims.get(&claim.id) {
                assert!(same_claim(existing, claim), "Conflicting spatial claim id: {}", claim.id);
            }
            claims.insert(claim.id.clone(), claim.clone());
        }
        let mut merged: Vec<SpatialClaim> = claims.into_values().collect();
        merged.sort_by(|left, right| left.id.cmp(&right.id));
        SpatialIndex { claims: merged }
    }

    /// `conflicts(moving, policy)` — the moving claim's collision
    /// points (only emptiness matters to the callers, matching
    /// upstream's `conflicts(...).length === 0` uses).
    pub fn conflicts(&self, moving: &SpatialClaim, policy: &CollisionPolicy) -> Vec<Point> {
        let mut conflicts = Vec::new();

        for existing in &self.claims {
            if roles_may_overlap(moving, existing) {
                continue;
            }
            let clearance = policy.clearance.unwrap_or(
                *policy.clearance_by_role.get(&existing.role).unwrap_or(&Clearance::default()),
            );
            for moving_span in &moving.spans {
                for dy in -clearance.y..=clearance.y {
                    let inflated = inflate_span(
                        &SpatialSpan { y: moving_span.y + dy, ..*moving_span },
                        clearance,
                    );
                    for existing_span in &existing.spans {
                        if inflated.y != existing_span.y {
                            continue;
                        }
                        let from_x = inflated.from_x.max(existing_span.from_x);
                        let to_x = inflated.to_x.min(existing_span.to_x);
                        for x in from_x..=to_x {
                            let point = Point { x, y: inflated.y };
                            let moving_occupies_point = moving.spans.iter().any(|span| {
                                span.y == point.y && point.x >= span.from_x && point.x <= span.to_x
                            });
                            if !(moving_occupies_point
                                && point_is_contact(point, existing, &policy.contacts))
                            {
                                conflicts.push(point);
                            }
                        }
                    }
                }
            }
        }
        conflicts
    }

    /// `isFree`.
    pub fn is_free(&self, claim: &SpatialClaim, policy: &CollisionPolicy) -> bool {
        self.conflicts(claim, policy).is_empty()
    }

    /// `firstFit`.
    #[must_use]
    pub fn first_fit<'a, T>(
        &'a self,
        candidates: &'a [T],
        claim_of: impl Fn(&T) -> SpatialClaim,
        policy: &CollisionPolicy,
    ) -> Option<&'a T> {
        candidates.iter().find(|candidate| self.is_free(&claim_of(candidate), policy))
    }
}
