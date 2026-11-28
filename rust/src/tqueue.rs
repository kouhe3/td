//! `td::TQueue` (tddb/td/db/TQueue.h): the `TQueueImpl` queue semantics plus
//! `TQueueMemoryStorage`, without the binlog and without an actor.
//!
//! The shape follows the C++ closely, including the parts that look odd until
//! you see why they are there (docs/facts.md F199/F200):
//!
//!   * `get_size` subtracts one when the **last** event's data is empty, so an
//!     event whose payload was dropped keeps its slot without counting;
//!   * `clear(q, 0)` therefore returns `size - 1` events: the last one keeps its
//!     `log_event_id`, its data is cleared and the storage is asked to rewrite it;
//!   * `pop` rewrites rather than removes when the event is the last one
//!     (`event.next() == tail_id`), which is how a drained queue keeps a tail;
//!   * `get` returns `get_size`, not the number of events it handed back, and it
//!     sweeps expired/empty events from the front before counting;
//!   * `push` only honours `hint_new_id` while the tail is still empty.
//!
//! Not modelled: `TQueueBinlog` (binlog frames 2314/2315 and `rewrite`), and
//! `close(Promise)` which needs an actor. `run_gc`'s 50 ms budget and
//! `(++counter & 128)` slice are modelled but never fire on the corpus.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::time::Instant;

pub const MAX_ID: i32 = 2_000_000_000;
const MAX_EVENT_LENGTH: usize = 65536 * 8;
const MAX_QUEUE_EVENTS: usize = 100_000;
const MAX_TOTAL_EVENT_LENGTH: usize = 1 << 27;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub struct EventId(i32);

impl EventId {
    /// `EventId::from_int32`: 0 is a *valid* id that is also `empty()`.
    pub fn from_int32(id: i32) -> Result<EventId, String> {
        if !Self::is_valid_id(id) {
            return Err("Invalid ID".to_string());
        }
        Ok(EventId(id))
    }

    /// Only used where the C++ builds an id it has already validated.
    fn assume_valid(id: i32) -> EventId {
        assert!(Self::is_valid_id(id));
        EventId(id)
    }

    pub fn is_valid(&self) -> bool {
        !self.empty() && Self::is_valid_id(self.0)
    }

    pub fn value(&self) -> i32 {
        self.0
    }

    pub fn next(&self) -> Result<EventId, String> {
        Self::from_int32(self.0 + 1)
    }

    /// `narrow_cast_safe<int32>(id_ + offset)` then `from_int32`.
    pub fn advance(&self, offset: usize) -> Result<EventId, String> {
        if offset > i32::MAX as usize || (self.0 as u64) + (offset as u64) > i32::MAX as u64 {
            return Err("Can't cast to int32".to_string());
        }
        Self::from_int32(self.0 + offset as i32)
    }

    pub fn empty(&self) -> bool {
        self.0 == 0
    }

    fn is_valid_id(id: i32) -> bool {
        0 <= id && id < MAX_ID
    }
}

impl std::fmt::Display for EventId {
    /// `operator<<(StringBuilder &, EventId)`
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "EventId{{{}}}", self.0)
    }
}

#[derive(Clone, Default, Debug)]
pub struct RawEvent {
    pub log_event_id: u64,
    pub event_id: EventId,
    pub expires_at: i32,
    pub data: Vec<u8>,
    pub extra: i64,
}

#[derive(Clone, Debug)]
pub struct Event {
    pub id: EventId,
    pub expires_at: i32,
    pub data: Vec<u8>,
    pub extra: i64,
}

pub trait StorageCallback {
    fn push(&mut self, queue_id: i64, event: &RawEvent) -> u64;
    fn pop(&mut self, log_event_id: u64);
    fn pop_batch(&mut self, log_event_ids: Vec<u64>) {
        for id in log_event_ids {
            self.pop(id);
        }
    }
    fn close(&mut self);
}

/// `TQueueMemoryStorage`: log event ids from 1 upward, reusable when the caller
/// supplies one (which is what a rewrite does).
#[derive(Default)]
pub struct TQueueMemoryStorage {
    next_log_event_id: u64,
    pub events: BTreeMap<u64, (i64, RawEvent)>,
}

impl TQueueMemoryStorage {
    pub fn new() -> Self {
        TQueueMemoryStorage {
            next_log_event_id: 1,
            events: BTreeMap::new(),
        }
    }

    pub fn replay(&self, queue: &mut TQueueImpl) {
        for (log_event_id, (queue_id, event)) in &self.events {
            let mut copy = event.clone();
            copy.log_event_id = *log_event_id;
            let added = queue.do_push(*queue_id, copy);
            assert!(added, "TQueueMemoryStorage::replay: failed to add an event");
        }
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

impl StorageCallback for TQueueMemoryStorage {
    fn push(&mut self, queue_id: i64, event: &RawEvent) -> u64 {
        let log_event_id = if event.log_event_id == 0 {
            let id = self.next_log_event_id;
            self.next_log_event_id += 1;
            id
        } else {
            event.log_event_id
        };
        self.events.insert(log_event_id, (queue_id, event.clone()));
        log_event_id
    }

    fn pop(&mut self, log_event_id: u64) {
        self.events.remove(&log_event_id);
    }

    fn close(&mut self) {
        self.events.clear();
    }
}

#[derive(Default)]
struct Queue {
    tail_id: EventId,
    events: BTreeMap<i32, RawEvent>,
    total_event_length: usize,
    gc_at: i32,
}

pub struct TQueueImpl {
    queues: HashMap<i64, Queue>,
    queue_gc_at: BTreeSet<(i32, i64)>,
    callback: Option<Box<dyn StorageCallback>>,
}

fn get_queue_head(queue: &Queue) -> EventId {
    match queue.events.keys().next() {
        Some(id) => EventId::assume_valid(*id),
        None => queue.tail_id,
    }
}

fn get_size(queue: &Queue) -> usize {
    if queue.events.is_empty() {
        return 0;
    }
    let last_is_empty = queue
        .events
        .values()
        .next_back()
        .map(|event| event.data.is_empty())
        .unwrap_or(false);
    queue.events.len() - usize::from(last_is_empty)
}

fn remove_event(queue: &mut Queue, key: i32) {
    if let Some(event) = queue.events.remove(&key) {
        queue.total_event_length -= event.data.len();
    }
}

fn schedule_queue_gc(
    queue_gc_at: &mut BTreeSet<(i32, i64)>,
    queue_id: i64,
    queue: &mut Queue,
    gc_at: i32,
) {
    if queue.gc_at != 0 {
        let removed = queue_gc_at.remove(&(queue.gc_at, queue_id));
        assert!(removed, "TQueue: gc slot was missing");
    }
    queue.gc_at = gc_at;
    if queue.gc_at != 0 {
        let inserted = queue_gc_at.insert((gc_at, queue_id));
        assert!(inserted, "TQueue: gc slot was already taken");
    }
}

/// `TQueueImpl::pop`: the last event is rewritten, not removed.
fn pop(
    queue: &mut Queue,
    callback: &mut Option<Box<dyn StorageCallback>>,
    queue_id: i64,
    key: i32,
    tail_id: EventId,
) {
    let Some(event) = queue.events.get(&key) else {
        return;
    };
    if callback.is_none() || event.log_event_id == 0 {
        remove_event(queue, key);
        return;
    }
    let is_last = event.event_id.next().ok() == Some(tail_id);
    if is_last {
        if !event.data.is_empty() {
            let mut copy = queue.events.get_mut(&key).unwrap().clone();
            // the stored event loses its payload but keeps its slot and its id
            let stored = queue.events.get_mut(&key).unwrap();
            queue.total_event_length -= stored.data.len();
            stored.data.clear();
            copy.data.clear();
            callback.as_mut().unwrap().push(queue_id, &copy);
        }
    } else {
        let log_event_id = event.log_event_id;
        callback.as_mut().unwrap().pop(log_event_id);
        remove_event(queue, key);
    }
}

impl TQueueImpl {
    pub fn create() -> TQueueImpl {
        TQueueImpl {
            queues: HashMap::new(),
            queue_gc_at: BTreeSet::new(),
            callback: None,
        }
    }

    pub fn set_callback(&mut self, callback: Box<dyn StorageCallback>) {
        self.callback = Some(callback);
    }

    pub fn extract_callback(&mut self) -> Option<Box<dyn StorageCallback>> {
        self.callback.take()
    }

    pub fn do_push(&mut self, queue_id: i64, raw_event: RawEvent) -> bool {
        assert!(raw_event.event_id.is_valid(), "CHECK(raw_event.event_id.is_valid())");
        if raw_event.data.len() > MAX_EVENT_LENGTH || queue_id == 0 {
            return false;
        }
        let queue = self.queues.entry(queue_id).or_default();
        if queue.events.len() >= MAX_QUEUE_EVENTS
            || queue.total_event_length > MAX_TOTAL_EVENT_LENGTH - raw_event.data.len()
            || raw_event.expires_at <= 0
        {
            return false;
        }
        let event_id = raw_event.event_id;
        if event_id < queue.tail_id {
            return false;
        }

        if let Some((&last_key, last)) = queue.events.iter().next_back() {
            if last.data.is_empty() {
                let log_event_id = last.log_event_id;
                if let Some(callback) = self.callback.as_mut() {
                    if log_event_id != 0 {
                        callback.pop(log_event_id);
                    }
                }
                queue.events.remove(&last_key);
            }
        }
        if queue.events.is_empty() && !raw_event.data.is_empty() {
            schedule_queue_gc(&mut self.queue_gc_at, queue_id, queue, raw_event.expires_at);
        }

        let mut raw_event = raw_event;
        if raw_event.log_event_id == 0 {
            if let Some(callback) = self.callback.as_mut() {
                raw_event.log_event_id = callback.push(queue_id, &raw_event);
            }
        }
        queue.tail_id = event_id.next().expect("tail id overflow");
        queue.total_event_length += raw_event.data.len();
        queue.events.insert(event_id.value(), raw_event);
        true
    }

    pub fn push(
        &mut self,
        queue_id: i64,
        data: Vec<u8>,
        expires_at: i32,
        extra: i64,
        hint_new_id: EventId,
    ) -> Result<EventId, String> {
        if data.is_empty() {
            return Err("Data is empty".to_string());
        }
        if data.len() > MAX_EVENT_LENGTH {
            return Err("Data is too big".to_string());
        }
        if queue_id == 0 {
            return Err("Queue identifier is invalid".to_string());
        }

        let queue = self.queues.entry(queue_id).or_default();
        if queue.events.len() >= MAX_QUEUE_EVENTS {
            return Err("Queue is full".to_string());
        }
        if queue.total_event_length > MAX_TOTAL_EVENT_LENGTH - data.len() {
            return Err("Queue size is too big".to_string());
        }
        if expires_at <= 0 {
            return Err("Failed to add already expired event".to_string());
        }

        let event_id = loop {
            if queue.tail_id.empty() {
                if hint_new_id.empty() {
                    // `Random::fast(2 * max(MAX_QUEUE_EVENTS, 1000000) + 1, MAX_ID / 2)`
                    queue.tail_id = EventId::assume_valid(random_tail_id());
                } else {
                    queue.tail_id = hint_new_id;
                }
            }
            let candidate = queue.tail_id;
            if candidate.next().is_ok() {
                break candidate;
            }
            let keys: Vec<i32> = queue.events.keys().copied().collect();
            for key in keys {
                pop(queue, &mut self.callback, queue_id, key, queue.tail_id);
            }
            queue.tail_id = EventId::default();
            assert!(hint_new_id.next().is_ok(), "CHECK(hint_new_id.next().is_ok())");
        };

        let raw_event = RawEvent {
            log_event_id: 0,
            event_id,
            expires_at,
            data,
            extra,
        };
        let added = self.do_push(queue_id, raw_event);
        assert!(added, "CHECK(is_added)");
        Ok(event_id)
    }

    pub fn forget(&mut self, queue_id: i64, event_id: EventId) {
        let Some(queue) = self.queues.get_mut(&queue_id) else {
            return;
        };
        if !queue.events.contains_key(&event_id.value()) {
            return;
        }
        let tail_id = queue.tail_id;
        pop(queue, &mut self.callback, queue_id, event_id.value(), tail_id);
    }

    pub fn clear(&mut self, queue_id: i64, keep_count: usize) -> BTreeMap<EventId, RawEvent> {
        let Some(queue) = self.queues.get_mut(&queue_id) else {
            return BTreeMap::new();
        };
        let size = get_size(queue);
        if size <= keep_count {
            return BTreeMap::new();
        }
        let n = queue.events.len();
        let mut delete_count = n - keep_count;
        if keep_count == 0 {
            delete_count = n - 1;
            let last_key = *queue.events.keys().next_back().unwrap();
            let event = queue.events.get(&last_key).unwrap();
            let has_log_id = event.log_event_id != 0;
            if self.callback.is_none() || !has_log_id {
                delete_count = n;
            } else if !event.data.is_empty() {
                let mut copy = queue.events.get(&last_key).unwrap().clone();
                let stored = queue.events.get_mut(&last_key).unwrap();
                queue.total_event_length -= stored.data.len();
                stored.data.clear();
                copy.data.clear();
                self.callback.as_mut().unwrap().push(queue_id, &copy);
            }
        }

        if let Some(callback) = self.callback.as_mut() {
            let mut deleted_log_event_ids = Vec::with_capacity(delete_count);
            for (_, event) in queue.events.iter().take(delete_count) {
                if event.log_event_id != 0 {
                    deleted_log_event_ids.push(event.log_event_id);
                }
            }
            callback.pop_batch(deleted_log_event_ids);
        }

        let keys: Vec<i32> = queue.events.keys().copied().take(delete_count).collect();
        let mut deleted_events = BTreeMap::new();
        for key in keys {
            let event = queue.events.remove(&key).unwrap();
            queue.total_event_length -= event.data.len();
            deleted_events.insert(EventId::assume_valid(key), event);
        }
        deleted_events
    }

    pub fn get_head(&self, queue_id: i64) -> EventId {
        match self.queues.get(&queue_id) {
            Some(queue) => get_queue_head(queue),
            None => EventId::default(),
        }
    }

    pub fn get_tail(&self, queue_id: i64) -> EventId {
        match self.queues.get(&queue_id) {
            Some(queue) => queue.tail_id,
            None => EventId::default(),
        }
    }

    pub fn get_size(&self, queue_id: i64) -> usize {
        match self.queues.get(&queue_id) {
            Some(queue) => get_size(queue),
            None => 0,
        }
    }

    pub fn get(
        &mut self,
        queue_id: i64,
        from_id: EventId,
        forget_previous: bool,
        unix_time_now: i32,
        max_events: usize,
        result_events: &mut Vec<Event>,
    ) -> Result<usize, String> {
        result_events.clear();
        let Some(queue) = self.queues.get_mut(&queue_id) else {
            return Ok(0);
        };
        if from_id.value() > queue.tail_id.value() + 10 {
            return Err("Specified from_id is in the future".to_string());
        }
        if from_id.value() < get_queue_head(queue).value() - MAX_QUEUE_EVENTS as i32 {
            return Err("Specified from_id is in the past".to_string());
        }

        if forget_previous {
            let keys: Vec<i32> = queue.events.range(..from_id.value()).map(|(k, _)| *k).collect();
            let tail_id = queue.tail_id;
            for key in keys {
                pop(queue, &mut self.callback, queue_id, key, tail_id);
            }
        }

        let keys: Vec<i32> = queue.events.range(from_id.value()..).map(|(k, _)| *k).collect();
        let tail_id = queue.tail_id;
        for key in keys {
            let Some(event) = queue.events.get(&key) else {
                continue;
            };
            if event.expires_at < unix_time_now || event.data.is_empty() {
                pop(queue, &mut self.callback, queue_id, key, tail_id);
            } else {
                if result_events.len() == max_events {
                    break;
                }
                result_events.push(Event {
                    id: event.event_id,
                    expires_at: event.expires_at,
                    data: event.data.clone(),
                    extra: event.extra,
                });
            }
        }

        Ok(get_size(queue))
    }

    pub fn run_gc(&mut self, unix_time_now: i32) -> (i64, bool) {
        let mut deleted_events: i64 = 0;
        let max_finish_time = Instant::now() + std::time::Duration::from_millis(50);
        let mut counter: i64 = 0;
        while let Some(&(gc_at, queue_id)) = self.queue_gc_at.iter().next() {
            if gc_at >= unix_time_now {
                break;
            }
            let queue = self.queues.entry(queue_id).or_default();
            assert_eq!(queue.gc_at, gc_at, "TQueue: gc slot mismatch");
            let mut new_gc_at: i32 = 0;

            if !queue.events.is_empty() {
                let size_before = get_size(queue);
                let keys: Vec<i32> = queue.events.keys().copied().collect();
                let tail_id = queue.tail_id;
                for key in keys {
                    let Some(event) = queue.events.get(&key) else {
                        continue;
                    };
                    counter += 1;
                    if (counter & 128) == 0 && Instant::now() >= max_finish_time {
                        if new_gc_at == 0 {
                            new_gc_at = event.expires_at;
                        }
                        break;
                    }
                    if event.expires_at < unix_time_now || event.data.is_empty() {
                        pop(queue, &mut self.callback, queue_id, key, tail_id);
                    } else {
                        if new_gc_at != 0 {
                            break;
                        }
                        new_gc_at = event.expires_at;
                    }
                }
                let size_after = get_size(queue);
                assert!(size_after <= size_before, "TQueue: size grew during gc");
                deleted_events += (size_before - size_after) as i64;
            }
            schedule_queue_gc(&mut self.queue_gc_at, queue_id, queue, new_gc_at);
            if Instant::now() >= max_finish_time {
                return (deleted_events, false);
            }
        }
        (deleted_events, true)
    }
}

/// `Random::fast(2000001, MAX_ID / 2)` -- the interval is the contract, the value
/// is not (docs/facts.md F199: no injectable source, so the corpus always hints).
fn random_tail_id() -> i32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64 + d.as_secs())
        .unwrap_or(0);
    const LO: i32 = 2_000_001;
    const HI: i32 = MAX_ID / 2;
    LO + (nanos % (HI - LO) as u64) as i32
}