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
//!   ([`Runtime::run_effects`]): notifications landing while it runs
//!   (e.g. a memo recomputing underneath it) coalesce into the next pump.
//! * Effects do not run inside `set`; they run when the owner pumps
//!   [`Runtime::run_effects`], which is the coalescing point: N mutations
//!   between two pumps produce at most one effect run each.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

type NodeId = usize;

struct Node {
    observers: Vec<NodeId>,
    sources: Vec<NodeId>,
    memo: bool,
    alive: bool,
    /// Memo nodes: a source changed, recompute on next read.
    dirty: Cell<bool>,
}

struct Graph {
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
}

impl Default for Graph {
    /// Pump generations start at 1 so the all-zero `last_pump` table
    /// never suppresses the first notifications.
    fn default() -> Self {
        Self {
            nodes: Vec::new(),
            free: Vec::new(),
            queue: Vec::new(),
            queued: Vec::new(),
            observing: None,
            pump: Cell::new(1),
            in_pump: Cell::new(false),
            last_pump: Vec::new(),
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
            id
        } else {
            self.nodes.push(node);
            self.queued.push(false);
            self.last_pump.push(0);
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
    /// dirty; effects are enqueued once.
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
            } else if !self.queued[observer]
                // An effect runs at most once per pump: notifications that
                // land while it is running (a memo recomputing underneath
                // it) coalesce into the next pump.
                && (!self.in_pump.get() || self.last_pump[observer] != self.pump.get())
            {
                self.queued[observer] = true;
                self.queue.push(observer);
            }
        }
    }
}

// Effect bodies live in a thread-local slab keyed by node id so the graph
// stays data-only while effect closures capture arbitrary state.
type EffectSlot = Option<Rc<dyn Fn()>>;
thread_local! {
    static EFFECTS: RefCell<Vec<EffectSlot>> = const { RefCell::new(Vec::new()) };
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

    /// Run every enqueued effect (draining the queue) until the graph is
    /// quiet. An effect runs at most once per pump: notifications that
    /// land while it is running (e.g. a memo recomputing underneath it)
    /// coalesce into the next pump; a hard iteration cap guards
    /// pathological feedback loops.
    pub fn run_effects(&self) {
        {
            let graph = self.graph.borrow_mut();
            graph.pump.set(graph.pump.get() + 1);
            graph.in_pump.set(true);
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
                // run coalesce into the next pump (once-per-pump).
                self.graph.borrow_mut().last_pump[id] = pump;
                self.run_effect_node(id);
            }
        }
        self.graph.borrow_mut().in_pump.set(false);
    }

    fn run_effect_node(&self, id: NodeId) {
        let effect = EFFECTS.with(|effects| effects.borrow().get(id).cloned().flatten());
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

    fn store_effect(id: NodeId, f: Rc<dyn Fn()>) {
        EFFECTS.with(|effects| {
            let mut effects = effects.borrow_mut();
            if effects.len() <= id {
                effects.resize(id + 1, None);
            }
            effects[id] = Some(f);
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
        Runtime::store_effect(id, Rc::clone(&f));
        let handle = EffectHandle { graph, id };
        rt.run_effect_node(id);
        handle
    }
}

/// Registration handle for an [`Effect`]; dropping it unregisters the
/// effect (its body stops running).
pub struct EffectHandle {
    graph: Weak<RefCell<Graph>>,
    id: NodeId,
}

impl Drop for EffectHandle {
    fn drop(&mut self) {
        if let Some(graph) = self.graph.upgrade() {
            graph.borrow_mut().drop_node(self.id);
        }
        EFFECTS.with(|effects| {
            let mut effects = effects.borrow_mut();
            if effects.len() > self.id {
                effects[self.id] = None;
            }
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
}
