//! Minimal reactive primitives for the TUI store: signals, memos and
//! effects over interior-mutability cells plus explicit subscription
//! edges — a SolidJS-shaped micrograph with **no external dependencies**.
//!
//! Design (single-threaded, UI-thread owned):
//!
//! * [`Runtime`] owns the subscription graph: every node (signal / memo /
//!   effect) registers its observers, and every observer records its
//!   sources. Handles (`Signal`/`Memo`/`EffectHandle`) hold the values in
//!   `RefCell`s and keep the runtime alive through `Rc`.
//! * Reads inside an active observer (`Effect::new`'s body, `Memo::get`)
//!   automatically record dependencies. A `Signal::set` marks downstream
//!   memos dirty (transitively) and enqueues downstream effects — memo
//!   evaluation stays lazy, and an effect runs **at most once per pump**
//!   ([`Runtime::run_effects`]).
//! * Effects do not run inside `set`; they run when the owner pumps
//!   [`Runtime::run_effects`], which is the coalescing point: N mutations
//!   between two pumps produce at most one effect run each.
//! * **Never-dropped notifications** (gate-9 finding 4): a notification
//!   landing while its effect has already run this pump sets a *pending
//!   marker* — the effect re-runs on the NEXT pump instead of losing the
//!   notification. Within one pump an effect still runs at most once;
//!   across pumps nothing is dropped.
//! * Effect bodies live in a thread-local slab keyed **per graph**
//!   (`(graph id, node id)` — gate-9 finding 3): two runtimes on one
//!   thread allocate independent graphs with globally-unique ids, so
//!   their effect slots can never collide or cross-drop.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicU64, Ordering};

type NodeId = usize;
/// EFFECTS key: graph ids come from a global allocator, so two live
/// runtimes on one thread never share a key even when their node ids
/// coincide — and a freed graph's memory being reused cannot resurrect
/// a stale slot.
type EffectKey = (u64, NodeId);

/// Global graph-id allocator (graph ids are never reused).
static NEXT_GRAPH_ID: AtomicU64 = AtomicU64::new(1);

struct Node {
    observers: Vec<NodeId>,
    sources: Vec<NodeId>,
    memo: bool,
    alive: bool,
    /// Memo nodes: a source changed, recompute on next read.
    dirty: Cell<bool>,
}

struct Graph {
    /// Globally unique id (see [`EffectKey`]).
    id: u64,
    nodes: Vec<Node>,
    free: Vec<NodeId>,
    queue: Vec<NodeId>,
    queued: Vec<bool>,
    observing: Option<NodeId>,
    /// Current pump generation (see [`Runtime::run_effects`]).
    pump: Cell<u64>,
    /// True only while a pump is mid-flight (once-per-pump coalescing
    /// applies then; between pumps everything may enqueue again).
    in_pump: Cell<bool>,
    /// Per effect node: the pump it last ran in.
    last_pump: Vec<u64>,
    /// Per effect node: notified after it ran this pump — re-runs next
    /// pump (never dropped; gate-9 finding 4).
    pending: Vec<bool>,
}

impl Default for Graph {
    /// Pump generations start at 1 so the all-zero `last_pump` table
    /// never suppresses the first notifications.
    fn default() -> Self {
        Self {
            id: NEXT_GRAPH_ID.fetch_add(1, Ordering::Relaxed),
            nodes: Vec::new(),
            free: Vec::new(),
            queue: Vec::new(),
            queued: Vec::new(),
            observing: None,
            pump: Cell::new(1),
            in_pump: Cell::new(false),
            last_pump: Vec::new(),
            pending: Vec::new(),
        }
    }
}

impl Graph {
    fn register(&mut self, memo: bool) -> NodeId {
        let node = Node {
            observers: Vec::new(),
            sources: Vec::new(),
            memo,
            alive: true,
            dirty: Cell::new(false),
        };
        if let Some(id) = self.free.pop() {
            self.nodes[id] = node;
            // A recycled id carries no scheduling state from its past life.
            self.queued[id] = false;
            self.pending[id] = false;
            id
        } else {
            self.nodes.push(node);
            self.queued.push(false);
            self.last_pump.push(0);
            self.pending.push(false);
            self.nodes.len() - 1
        }
    }

    fn drop_node(&mut self, id: NodeId) {
        let node = &mut self.nodes[id];
        node.alive = false;
        let observers = std::mem::take(&mut node.observers);
        let sources = std::mem::take(&mut node.sources);
        for observer in observers {
            self.nodes[observer].sources.retain(|&source| source != id);
        }
        for source in sources {
            self.nodes[source].observers.retain(|&observer| observer != id);
        }
        self.free.push(id);
    }

    fn subscribe(&mut self, observer: NodeId, source: NodeId) {
        if !self.nodes[source].observers.contains(&observer) {
            self.nodes[source].observers.push(observer);
        }
        if !self.nodes[observer].sources.contains(&source) {
            self.nodes[observer].sources.push(source);
        }
    }

    /// Mark everything downstream of `id` as needing a re-run. Memos turn
    /// dirty; effects are enqueued once — and an effect that already ran
    /// this pump is marked *pending* so the notification survives into
    /// the next pump (never dropped — gate-9 finding 4).
    fn notify(&mut self, id: NodeId) {
        let observers = self.nodes[id].observers.clone();
        for observer in observers {
            if !self.nodes[observer].alive {
                continue;
            }
            if self.nodes[observer].memo {
                // Value recomputation is lazy (on next read); keep walking
                // so effects observing the memo are scheduled.
                self.nodes[observer].dirty.set(true);
                self.notify(observer);
            } else if self.queued[observer] {
                // Already scheduled this pump — coalesced.
            } else if self.in_pump.get() && self.last_pump[observer] == self.pump.get() {
                // Ran earlier THIS pump: at most once per pump, but the
                // notification is never dropped — re-run next pump.
                self.pending[observer] = true;
            } else {
                self.queued[observer] = true;
                self.queue.push(observer);
            }
        }
    }
}

// Effect bodies live in a thread-local slab keyed by (graph id, node id)
// so the graph stays data-only while effect closures capture arbitrary
// state — and two runtimes on one thread never cross (finding 3).
type EffectSlot = Option<Rc<dyn Fn()>>;
thread_local! {
    static EFFECTS: RefCell<HashMap<EffectKey, EffectSlot>> = RefCell::new(HashMap::new());
}

/// Shared graph handle.
type GraphRef = Weak<RefCell<Graph>>;

/// Owner of the reactive graph. Clones share one graph.
#[derive(Clone, Default)]
pub struct Runtime {
    graph: Rc<RefCell<Graph>>,
}

impl Runtime {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn register(&self, memo: bool) -> (GraphRef, NodeId) {
        let id = self.graph.borrow_mut().register(memo);
        (Rc::downgrade(&self.graph), id)
    }

    fn graph_id(&self) -> u64 {
        self.graph.borrow().id
    }

    /// Run every enqueued effect (draining the queue) until the graph is
    /// quiet. An effect runs at most once per pump: notifications that
    /// land while it is running (e.g. a memo recomputing underneath it)
    /// set the pending marker and re-run on the NEXT pump — never
    /// dropped; a hard iteration cap guards pathological feedback loops.
    pub fn run_effects(&self) {
        {
            let mut graph = self.graph.borrow_mut();
            graph.pump.set(graph.pump.get() + 1);
            graph.in_pump.set(true);
            // Carry pending notifications into this pump (finding 4).
            for id in 0..graph.pending.len() {
                if graph.pending[id] {
                    graph.pending[id] = false;
                    if !graph.queued[id] && graph.nodes[id].alive && !graph.nodes[id].memo {
                        graph.queued[id] = true;
                        graph.queue.push(id);
                    }
                }
            }
        }
        for _ in 0..64 {
            let batch: Vec<NodeId> = {
                let mut graph = self.graph.borrow_mut();
                if graph.queue.is_empty() {
                    break;
                }
                let batch = std::mem::take(&mut graph.queue);
                for id in &batch {
                    graph.queued[*id] = false;
                }
                batch
            };
            let pump = self.graph.borrow().pump.get();
            for id in batch {
                // Mark before running so notifications landing inside the
                // run pending-mark rather than re-enqueue
                // (once-per-pump, never dropped).
                self.graph.borrow_mut().last_pump[id] = pump;
                self.run_effect_node(id);
            }
        }
        self.graph.borrow_mut().in_pump.set(false);
    }

    fn run_effect_node(&self, id: NodeId) {
        let key = (self.graph_id(), id);
        let effect = EFFECTS.with(|effects| effects.borrow().get(&key).cloned().flatten());
        let Some(effect) = effect else { return };
        if !self.graph.borrow().nodes[id].alive {
            return;
        }
        // Detach sources; the run re-collects them.
        {
            let mut graph = self.graph.borrow_mut();
            let sources = graph.nodes[id].sources.clone();
            for source in sources {
                graph.nodes[source].observers.retain(|&observer| observer != id);
            }
            graph.nodes[id].sources.clear();
        }
        self.with_observing(id, effect.as_ref());
    }

    fn with_observing<R>(&self, observer: NodeId, f: &dyn Fn() -> R) -> R {
        let previous = self.graph.borrow_mut().observing.replace(observer);
        let result = f();
        self.graph.borrow_mut().observing = previous;
        result
    }

    fn track(&self, observer: NodeId, source: NodeId) {
        let mut graph = self.graph.borrow_mut();
        if graph.nodes[observer].alive && graph.nodes[source].alive {
            graph.subscribe(observer, source);
        }
    }

    fn notify_source(&self, source: NodeId) {
        self.graph.borrow_mut().notify(source);
    }

    fn store_effect(&self, id: NodeId, f: Rc<dyn Fn()>) {
        let key = (self.graph_id(), id);
        EFFECTS.with(|effects| {
            effects.borrow_mut().insert(key, Some(f));
        });
    }
}

/// A writable reactive cell. Cheap clones share the same cell; the node
/// detaches when the last handle drops.
pub struct Signal<T> {
    inner: std::rc::Rc<SignalInner<T>>,
}

struct SignalInner<T> {
    graph: Weak<RefCell<Graph>>,
    rt: Runtime,
    id: NodeId,
    value: RefCell<T>,
}

impl<T> Clone for Signal<T> {
    fn clone(&self) -> Self {
        Self { inner: std::rc::Rc::clone(&self.inner) }
    }
}

impl<T: PartialEq + Clone> Signal<T> {
    pub fn new(rt: &Runtime, value: T) -> Self {
        let (graph, id) = rt.register(false);
        Self {
            inner: std::rc::Rc::new(SignalInner {
                graph,
                rt: rt.clone(),
                id,
                value: RefCell::new(value),
            }),
        }
    }

    /// Current value; records a dependency when called inside an observer.
    #[must_use]
    pub fn get(&self) -> T {
        let inner = &self.inner;
        if let Some(graph) = inner.graph.upgrade() {
            let observing = graph.borrow().observing;
            if let Some(observer) = observing {
                inner.rt.track(observer, inner.id);
            }
        }
        inner.value.borrow().clone()
    }

    /// Write `value`; notifies observers when it differs.
    pub fn set(&self, value: T) {
        let inner = &self.inner;
        if *inner.value.borrow() == value {
            return;
        }
        *inner.value.borrow_mut() = value;
        inner.rt.notify_source(inner.id);
    }

    /// Mutate in place through a closure; always notifies (in-place
    /// mutation cannot be compared, unlike [`Signal::set`]).
    pub fn update(&self, f: impl FnOnce(&mut T)) {
        let inner = &self.inner;
        f(&mut inner.value.borrow_mut());
        inner.rt.notify_source(inner.id);
    }
}

impl<T> Drop for SignalInner<T> {
    fn drop(&mut self) {
        if let Some(graph) = self.graph.upgrade() {
            graph.borrow_mut().drop_node(self.id);
        }
    }
}

/// A derived reactive value, recomputed lazily when dirty. Cheap clones
/// share the computed cell.
pub struct Memo<T> {
    inner: std::rc::Rc<MemoInner<T>>,
}

struct MemoInner<T> {
    graph: Weak<RefCell<Graph>>,
    rt: Runtime,
    id: NodeId,
    value: RefCell<Option<T>>,
    compute: RefCell<Rc<dyn Fn() -> T>>,
}

impl<T> Clone for Memo<T> {
    fn clone(&self) -> Self {
        Self { inner: std::rc::Rc::clone(&self.inner) }
    }
}

impl<T: PartialEq + Clone> Memo<T> {
    pub fn new(rt: &Runtime, compute: impl Fn() -> T + 'static) -> Self {
        let (graph, id) = rt.register(true);
        let memo = Self {
            inner: std::rc::Rc::new(MemoInner {
                graph,
                rt: rt.clone(),
                id,
                value: RefCell::new(None),
                compute: RefCell::new(Rc::new(compute)),
            }),
        };
        // Compute immediately (dirty by construction).
        if let Some(graph) = memo.inner.graph.upgrade() {
            graph.borrow_mut().nodes[id].dirty.set(true);
        }
        memo.recompute();
        memo
    }

    fn recompute(&self) {
        let inner = &self.inner;
        // Detach sources; the compute re-collects them.
        if let Some(graph) = inner.graph.upgrade() {
            let mut graph = graph.borrow_mut();
            let sources = graph.nodes[inner.id].sources.clone();
            for source in sources {
                graph.nodes[source].observers.retain(|&observer| observer != inner.id);
            }
            graph.nodes[inner.id].sources.clear();
        }
        let compute = inner.compute.borrow().clone();
        let next = inner.rt.with_observing(inner.id, &*compute);
        let changed = {
            let mut value = inner.value.borrow_mut();
            let changed = value.as_ref() != Some(&next);
            *value = Some(next);
            changed
        };
        if let Some(graph) = inner.graph.upgrade() {
            graph.borrow_mut().nodes[inner.id].dirty.set(false);
        }
        if changed {
            inner.rt.notify_source(inner.id);
        }
    }

    #[must_use]
    pub fn get(&self) -> T {
        let inner = &self.inner;
        let dirty =
            inner.graph.upgrade().is_some_and(|graph| graph.borrow().nodes[inner.id].dirty.get());
        if let Some(graph) = inner.graph.upgrade() {
            let observing = graph.borrow().observing;
            if let Some(observer) = observing {
                inner.rt.track(observer, inner.id);
            }
        }
        if dirty {
            self.recompute();
        }
        inner.value.borrow().clone().expect("memo computed")
    }
}

impl<T> Drop for MemoInner<T> {
    fn drop(&mut self) {
        if let Some(graph) = self.graph.upgrade() {
            graph.borrow_mut().drop_node(self.id);
        }
    }
}

/// Effect constructor namespace (mirrors solid's `createEffect` shape).
pub struct Effect;

impl Effect {
    /// Register `f`; runs once immediately, then once per pump while any
    /// recorded dependency changed. Dropping the handle unregisters it.
    /// (Named `new` to mirror solid's `createEffect`; the handle — not a
    /// guard object — is the registration.)
    #[allow(clippy::new_ret_no_self)]
    pub fn new(rt: &Runtime, f: impl Fn() + 'static) -> EffectHandle {
        let (graph, id) = rt.register(false);
        let f: Rc<dyn Fn()> = Rc::new(f);
        rt.store_effect(id, Rc::clone(&f));
        let handle = EffectHandle { graph, graph_id: rt.graph_id(), id };
        rt.run_effect_node(id);
        handle
    }
}

/// Registration handle for an [`Effect`]; dropping it unregisters the
/// effect (its body stops running).
pub struct EffectHandle {
    graph: Weak<RefCell<Graph>>,
    /// The graph's globally-unique id — survives the graph itself so the
    /// EFFECTS slot is reclaimed even if the graph is gone first.
    graph_id: u64,
    id: NodeId,
}

impl Drop for EffectHandle {
    fn drop(&mut self) {
        if let Some(graph) = self.graph.upgrade() {
            graph.borrow_mut().drop_node(self.id);
        }
        EFFECTS.with(|effects| {
            effects.borrow_mut().remove(&(self.graph_id, self.id));
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signals_notify_effects_coalesced_per_pump() {
        let rt = Runtime::new();
        let signal = Signal::new(&rt, 1i32);
        let watched = signal.clone();
        let runs = Rc::new(Cell::new(0));
        let runs_inner = Rc::clone(&runs);
        let _effect = Effect::new(&rt, move || {
            let _ = watched.get();
            runs_inner.set(runs_inner.get() + 1);
        });
        assert_eq!(runs.get(), 1, "effect runs once at registration");
        signal.set(2);
        signal.set(3);
        assert_eq!(runs.get(), 1, "no run before the pump");
        rt.run_effects();
        assert_eq!(runs.get(), 2, "coalesced into one run");
        rt.run_effects();
        assert_eq!(runs.get(), 2);
    }

    #[test]
    fn memos_recompute_lazily_and_propagate() {
        let rt = Runtime::new();
        let source = Signal::new(&rt, 2i32);
        let watched = source.clone();
        let doubled = Memo::new(&rt, move || watched.get() * 2);
        assert_eq!(doubled.get(), 4);
        let seen = Rc::new(RefCell::new(Vec::new()));
        let seen_inner = Rc::clone(&seen);
        let memo = doubled.clone();
        let _effect = Effect::new(&rt, move || seen_inner.borrow_mut().push(memo.get()));
        assert_eq!(*seen.borrow(), vec![4]);
        source.set(5);
        rt.run_effects();
        assert_eq!(*seen.borrow(), vec![4, 10]);
    }

    #[test]
    fn equal_sets_do_not_notify() {
        let rt = Runtime::new();
        let signal = Signal::new(&rt, "a".to_owned());
        let watched = signal.clone();
        let runs = Rc::new(Cell::new(0));
        let runs_inner = Rc::clone(&runs);
        let _effect = Effect::new(&rt, move || {
            let _ = watched.get();
            runs_inner.set(runs_inner.get() + 1);
        });
        signal.set("a".to_owned());
        rt.run_effects();
        assert_eq!(runs.get(), 1);
    }

    #[test]
    fn dropping_a_handle_detaches() {
        let rt = Runtime::new();
        let signal = Signal::new(&rt, 1i32);
        let watched = signal.clone();
        let runs = Rc::new(Cell::new(0));
        let runs_inner = Rc::clone(&runs);
        let effect = Effect::new(&rt, move || {
            let _ = watched.get();
            runs_inner.set(runs_inner.get() + 1);
        });
        drop(effect);
        signal.set(9);
        rt.run_effects();
        assert_eq!(runs.get(), 1, "dropped effect never runs again");
    }

    /// Gate-9 finding 3: two runtimes on ONE thread. The old slab was
    /// keyed by node id alone, so runtime B's effect (node 0) overwrote
    /// runtime A's slot and dropping A's handle zeroed B's closure.
    /// Graph-keyed slots keep the graphs independent.
    #[test]
    fn two_runtimes_on_one_thread_do_not_cross_effects() {
        let rt_a = Runtime::new();
        let rt_b = Runtime::new();
        let signal_a = Signal::new(&rt_a, 1i32);
        let signal_b = Signal::new(&rt_b, 1i32);
        let runs_a = Rc::new(Cell::new(0));
        let runs_b = Rc::new(Cell::new(0));
        let watched_a = signal_a.clone();
        let inner_a = Rc::clone(&runs_a);
        let effect_a = Effect::new(&rt_a, move || {
            let _ = watched_a.get();
            inner_a.set(inner_a.get() + 1);
        });
        let watched_b = signal_b.clone();
        let inner_b = Rc::clone(&runs_b);
        let effect_b = Effect::new(&rt_b, move || {
            let _ = watched_b.get();
            inner_b.set(inner_b.get() + 1);
        });
        assert_eq!((runs_a.get(), runs_b.get()), (1, 1), "both register-time runs");
        signal_a.set(2);
        signal_b.set(2);
        rt_a.run_effects();
        rt_b.run_effects();
        assert_eq!((runs_a.get(), runs_b.get()), (2, 2), "both pumps fire their own effect");
        // Dropping A's handle must not touch B's slot (the old bug: both
        // lived at node id 0 of one thread-local slab).
        drop(effect_a);
        signal_b.set(3);
        rt_b.run_effects();
        assert_eq!(runs_b.get(), 3, "B's effect survives A's teardown");
        assert_eq!(runs_a.get(), 2);
        drop(effect_b);
    }

    /// Gate-9 finding 4: a notification landing AFTER its effect ran this
    /// pump is deferred (pending marker), never dropped. F reads `s`;
    /// E runs after F in queue order and writes `s` — F must re-run on
    /// the NEXT pump (the old semantics lost F's re-run entirely).
    #[test]
    fn notification_after_running_repeats_on_the_next_pump() {
        let rt = Runtime::new();
        let s = Signal::new(&rt, 0i32);
        let t = Signal::new(&rt, 0i32);
        let armed = Rc::new(Cell::new(false));
        let f_runs = Rc::new(Cell::new(0));
        // F: queued first (reads s only).
        let watched = s.clone();
        let f_counter = Rc::clone(&f_runs);
        let _f = Effect::new(&rt, move || {
            let _ = watched.get();
            f_counter.set(f_counter.get() + 1);
        });
        // E: queued second (reads t; writes s when armed).
        let s_write = s.clone();
        let t_watched = t.clone();
        let armed_flag = Rc::clone(&armed);
        let _e = Effect::new(&rt, move || {
            let _ = t_watched.get();
            if armed_flag.get() {
                armed_flag.set(false);
                s_write.set(99);
            }
        });
        assert_eq!(f_runs.get(), 1, "registration run");
        armed.set(true);
        s.set(1); // → F enqueued
        t.set(1); // → E enqueued after F
        rt.run_effects();
        assert_eq!(f_runs.get(), 2, "F ran once this pump (E's write deferred)");
        rt.run_effects();
        assert_eq!(f_runs.get(), 3, "the deferred notification re-ran F next pump");
        rt.run_effects();
        assert_eq!(f_runs.get(), 3, "nothing pending — quiet");
    }
}
