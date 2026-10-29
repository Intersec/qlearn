//! The writes to the store, numbered, for whoever waits on them.
//!
//! Two parties write: the learner in the browser and an agent in a
//! terminal. The server is the only writer of the store, so it learns of
//! every write when it makes it, and each party learns of the other's here.
//!
//! The clock is the one of tokio, so a test moves it forward instead of
//! waiting half an hour.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tokio::sync::Notify;
use tokio::time::Instant;

use crate::model::{Author, Batch, Detail, Event, EventKind};

/// How many events are kept. A party that falls further behind reads
/// everything again.
const KEPT: usize = 512;

/// How long a wait is held before it answers empty. Short enough that a
/// proxy or a browser never gives up on it first.
pub const HOLD: Duration = Duration::from_secs(30);

/// How long an agent stays present after a wait that got its answer. A
/// `qlearn wait` that runs out of time returns, and the agent starts the
/// next one on its next turn.
const LISTENS_FOR: Duration = Duration::from_secs(30);

/// How long an agent stays present after it hung up on a wait. Its
/// `qlearn wait` stopped: only one that the agent starts again at once is
/// not leaving.
const HUNG_UP_FOR: Duration = Duration::from_secs(5);

/// How long a request that a wait handed over keeps the agent present with
/// no word of it. A lesson takes minutes to write with no wait open. Past
/// this, the agent is taken for gone.
const WORKS_FOR: Duration = Duration::from_secs(30 * 60);

/// How long no page may wait before the page counts as closed. A reload
/// opens a new wait within a second.
const PAGE_GONE_AFTER: Duration = Duration::from_secs(3);

pub struct Events {
    /// Drawn at the start. A new server numbers its events from 1 again,
    /// so an `after` of the last one may name events of this one.
    server: String,
    log: Mutex<Log>,
    notify: Notify,
    /// The server stops. A page always holds a wait open, and a stop must
    /// not sit behind it.
    closed: AtomicBool,
    agent: Mutex<Presence>,
    pages: Mutex<Pages>,
}

impl Default for Events {
    fn default() -> Self {
        let mut bytes = [0u8; 8];
        getrandom::fill(&mut bytes).expect("the system has no randomness");
        Self {
            server: hex::encode(bytes),
            log: Mutex::default(),
            notify: Notify::default(),
            closed: AtomicBool::default(),
            agent: Mutex::default(),
            pages: Mutex::default(),
        }
    }
}

/// The pages of the course that hold a wait open.
#[derive(Default)]
struct Pages {
    open: usize,
    ended: Option<Instant>,
    /// The `changes` of the presence that the last answer to a page held.
    saw: u64,
}

/// Held while a page waits. A guard, because the learner closes the tab.
pub struct PageWaits<'a>(&'a Events);

impl PageWaits<'_> {
    /// As [`Events::after`], and at once when the presence changed since the
    /// last answer to a page.
    ///
    /// A request answers the page and the wait of the agent together. So
    /// the page is often between two waits when the agent starts to work,
    /// and the wake of that change finds no wait of the page.
    pub async fn after(&self, after: Option<u64>, wait: Duration) -> Batch {
        self.0.answer(after, wait, true).await
    }
}

impl Drop for PageWaits<'_> {
    fn drop(&mut self) {
        let mut pages = self.0.pages.lock().expect("the page state is poisoned");
        pages.open = pages.open.saturating_sub(1);
        pages.ended = Some(Instant::now());
    }
}

#[derive(Default)]
struct Presence {
    open: usize,
    ended: Option<Instant>,
    /// How long the agent is still present after `ended`.
    grace: Duration,
    /// The agent wrote since its last wait opened.
    wrote: bool,
    /// When a wait handed over a request that the agent has not finished.
    working: Option<Instant>,
    /// That work holds a Next request: the agent writes a page, where a
    /// broken diagram only needs a fix.
    next: bool,
    /// What the agent says it does. Its next wait clears it.
    status: Option<String>,
    /// An agent waited once on this server.
    seen: bool,
    /// The changes that the page shows and that no event carries.
    changes: u64,
}

impl Presence {
    fn listens(&self) -> bool {
        self.open > 0 || self.ended.is_some_and(|at| at.elapsed() < self.grace) || self.works()
    }

    fn works(&self) -> bool {
        self.working.is_some_and(|at| at.elapsed() < WORKS_FOR)
    }

    fn prepares(&self) -> bool {
        self.next && self.works()
    }
}

/// Held while an agent waits. A guard, because the agent can hang up.
pub struct AgentWaits<'a> {
    events: &'a Events,
    answered: bool,
}

impl AgentWaits<'_> {
    /// The wait got its answer. A guard dropped before this hung up.
    ///
    /// A request in the answer is handed over: the agent works on it until
    /// its next wait.
    pub fn answered(&mut self, batch: &Batch) {
        self.answered = true;
        if !batch.events.iter().any(|event| event.kind.is_request()) {
            return;
        }
        {
            let mut agent = self
                .events
                .agent
                .lock()
                .expect("the agent state is poisoned");
            agent.working = Some(Instant::now());
            agent.next = batch
                .events
                .iter()
                .any(|event| event.kind == EventKind::Next);
            agent.changes += 1;
        }
        // The page shows that the agent works, and its wait would hold the
        // news for up to 30 seconds.
        self.events.notify.notify_waiters();
    }
}

impl Drop for AgentWaits<'_> {
    fn drop(&mut self) {
        let mut agent = self
            .events
            .agent
            .lock()
            .expect("the agent state is poisoned");
        agent.open = agent.open.saturating_sub(1);
        agent.ended = Some(Instant::now());
        agent.grace = if self.answered {
            LISTENS_FOR
        } else {
            HUNG_UP_FOR
        };
        // Not counted in `changes`: the page shows the agent present until
        // the grace runs out, and a later wait times that end by itself.
        let hung_up = !self.answered && agent.open == 0;
        drop(agent);
        if hung_up {
            // The waits of the pages answer when the grace runs out, and
            // they must learn of it now to count from here.
            self.events.notify.notify_waiters();
        }
    }
}

#[derive(Default)]
struct Log {
    /// The number of the last event, 0 before the first.
    last: u64,
    kept: VecDeque<Event>,
}

impl Events {
    /// The id of this server, which every batch carries.
    pub fn server(&self) -> &str {
        &self.server
    }

    /// Mark an agent as waiting until the guard is dropped.
    ///
    /// A wait that opens ends the work of the agent and clears its status:
    /// the agent goes back to waiting once it is done, so this says so with
    /// no command.
    pub fn agent_waits(&self) -> AgentWaits<'_> {
        let (ended, arrived, worked) = {
            let mut agent = self.agent.lock().expect("the agent state is poisoned");
            let arrived = !agent.listens();
            agent.open += 1;
            agent.seen = true;
            let worked = agent.working.take().is_some();
            agent.next = false;
            agent.status = None;
            if arrived || worked {
                agent.changes += 1;
            }
            (std::mem::take(&mut agent.wrote), arrived, worked)
        };
        if ended {
            self.push(EventKind::Idle, Author::Agent, None, None);
        } else if arrived || worked {
            // No event, but the page shows whether an agent is present and
            // works, and its wait would hold the news for up to 30 seconds.
            self.notify.notify_waiters();
        }
        AgentWaits {
            events: self,
            answered: false,
        }
    }

    /// Mark a page as waiting until the guard is dropped.
    pub fn page_waits(&self) -> PageWaits<'_> {
        self.pages.lock().expect("the page state is poisoned").open += 1;
        PageWaits(self)
    }

    /// Whether a page of the course is open. None before the first one.
    pub fn page_open(&self) -> Option<bool> {
        let pages = self.pages.lock().expect("the page state is poisoned");
        if pages.open > 0 {
            return Some(true);
        }
        pages.ended.map(|at| at.elapsed() < PAGE_GONE_AFTER)
    }

    pub fn agent_listens(&self) -> bool {
        self.agent
            .lock()
            .expect("the agent state is poisoned")
            .listens()
    }

    pub fn agent_works(&self) -> bool {
        self.agent
            .lock()
            .expect("the agent state is poisoned")
            .works()
    }

    /// An agent was present, and is present no more.
    pub fn agent_left(&self) -> bool {
        let agent = self.agent.lock().expect("the agent state is poisoned");
        agent.seen && !agent.listens()
    }

    /// How long until an agent with no wait open is gone, when the grace
    /// of its last wait or its work keeps it.
    fn agent_gone_in(&self) -> Option<Duration> {
        let agent = self.agent.lock().expect("the agent state is poisoned");
        if agent.open > 0 {
            return None;
        }
        let grace = agent.ended.map(|at| at + agent.grace);
        let work = agent.working.map(|at| at + WORKS_FOR);
        Some(grace.max(work)?.saturating_duration_since(Instant::now()))
            .filter(|left| !left.is_zero())
    }

    /// Say what the agent does, until its next wait. An empty text clears
    /// it. Answers the status as the page shows it.
    pub fn set_status(&self, text: &str) -> Option<String> {
        let text = text.trim();
        let status = (!text.is_empty()).then(|| text.to_owned());
        self.agent
            .lock()
            .expect("the agent state is poisoned")
            .status = status.clone();
        self.push(EventKind::Status, Author::Agent, None, None);
        status
    }

    pub fn push(&self, kind: EventKind, author: Author, page: Option<&str>, id: Option<&str>) {
        self.record(kind, author, page, id, None);
    }

    /// Push a request of the learner, with what the agent needs to answer
    /// it. Answers the event as the log holds it.
    pub fn request(
        &self,
        kind: EventKind,
        page: Option<&str>,
        id: Option<&str>,
        detail: Detail,
    ) -> Event {
        debug_assert!(kind.is_request(), "{kind:?} is not a request");
        self.record(kind, Author::Learner, page, id, Some(detail))
    }

    fn record(
        &self,
        kind: EventKind,
        author: Author,
        page: Option<&str>,
        id: Option<&str>,
        detail: Option<Detail>,
    ) -> Event {
        if author == Author::Agent && kind != EventKind::Idle {
            self.agent
                .lock()
                .expect("the agent state is poisoned")
                .wrote = true;
        }
        let event = {
            let mut log = self.log.lock().expect("the event log is poisoned");
            log.last += 1;
            let event = Event {
                seq: log.last,
                kind,
                author,
                page: page.map(str::to_owned),
                id: id.map(str::to_owned),
                detail,
            };
            log.kept.push_back(event.clone());
            if log.kept.len() > KEPT {
                log.kept.pop_front();
            }
            event
        };
        self.notify.notify_waiters();
        event
    }

    /// Answer every wait now, and every later one at once.
    pub fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    /// The events after `after`, as soon as there is one, or none once
    /// `wait` has passed. With no `after`, the number to start from, at once.
    pub async fn after(&self, after: Option<u64>, wait: Duration) -> Batch {
        self.answer(after, wait, false).await
    }

    async fn answer(&self, after: Option<u64>, wait: Duration, page: bool) -> Batch {
        let (batch, changes) = self.hold(after, wait, page).await;
        if page {
            self.pages.lock().expect("the page state is poisoned").saw = changes;
        }
        batch
    }

    /// The batch, and the `changes` of the presence that it holds.
    async fn hold(&self, after: Option<u64>, wait: Duration, page: bool) -> (Batch, u64) {
        let Some(after) = after else {
            return self.since(None);
        };

        let notified = self.notify.notified();
        tokio::pin!(notified);
        // Registered before the log is read, so a push between the read and
        // the wait still wakes this one.
        notified.as_mut().enable();

        let (batch, changes) = self.since(Some(after));
        // One count for all the pages, as no wait names its tab. A tab that
        // comes back after another one heard the change holds until the
        // next write, for up to 30 seconds.
        let missed = page && changes != self.pages.lock().expect("the page state is poisoned").saw;
        if !batch.events.is_empty() || batch.reset || missed || self.closed.load(Ordering::SeqCst) {
            return (batch, changes);
        }
        // The page shows the agent as gone once its grace or its work runs
        // out, with no event to wake it then.
        let wait = self.agent_gone_in().map_or(wait, |gone| wait.min(gone));
        let _ = tokio::time::timeout(wait, notified).await;

        self.since(Some(after))
    }

    fn since(&self, after: Option<u64>) -> (Batch, u64) {
        let (agent, working, preparing, left, status, changes) = {
            let presence = self.agent.lock().expect("the agent state is poisoned");
            let listens = presence.listens();
            (
                listens,
                presence.works(),
                presence.prepares(),
                presence.seen && !listens,
                presence.status.clone(),
                presence.changes,
            )
        };
        let log = self.log.lock().expect("the event log is poisoned");
        let mut batch = Batch {
            events: Vec::new(),
            next: log.last,
            reset: false,
            agent,
            working,
            preparing,
            left,
            status,
            server: self.server.clone(),
        };

        let Some(after) = after else {
            return (batch, changes);
        };
        let first = log.kept.front().map_or(log.last + 1, |e| e.seq);
        // Ahead of this server: it restarted. Behind what is kept: the
        // events fell out of memory.
        if after > log.last || after + 1 < first {
            batch.reset = true;
            return (batch, changes);
        }
        batch.events = log.kept.iter().filter(|e| e.seq > after).cloned().collect();
        (batch, changes)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    /// Long enough for a spawned wait to reach its sleep.
    const SHORT: Duration = Duration::from_millis(20);

    fn waiting(events: &Arc<Events>) -> tokio::task::JoinHandle<Batch> {
        let events = events.clone();
        tokio::spawn(async move { events.after(Some(0), HOLD).await })
    }

    /// One wait of the agent that answered with what was pushed.
    async fn hand_over(events: &Events) {
        let mut waits = events.agent_waits();
        let batch = events.after(Some(0), SHORT).await;
        waits.answered(&batch);
    }

    #[tokio::test(start_paused = true)]
    async fn a_page_counts_as_closed_once_no_wait_holds_it() {
        let events = Events::default();
        assert_eq!(events.page_open(), None, "no page yet");

        let first = events.page_waits();
        let second = events.page_waits();
        drop(first);
        assert_eq!(events.page_open(), Some(true));

        drop(second);
        assert_eq!(events.page_open(), Some(true), "a reload is not a close");
        tokio::time::advance(PAGE_GONE_AFTER).await;
        assert_eq!(events.page_open(), Some(false));
    }

    #[tokio::test]
    async fn no_number_answers_at_once_with_the_one_to_start_from() {
        let events = Events::default();
        events.push(EventKind::Next, Author::Learner, Some("1"), None);

        let batch = events.after(None, HOLD).await;

        assert_eq!(batch.events, []);
        assert_eq!(batch.next, 1);
        assert!(!batch.reset);
    }

    #[tokio::test]
    async fn the_events_after_a_number_come_back_at_once() {
        let events = Events::default();
        events.push(EventKind::Next, Author::Learner, Some("1"), None);
        events.push(EventKind::Page, Author::Agent, Some("2"), None);

        let batch = events.after(Some(1), HOLD).await;

        assert_eq!(
            batch.events,
            [Event {
                seq: 2,
                kind: EventKind::Page,
                author: Author::Agent,
                page: Some("2".to_owned()),
                id: None,
                detail: None,
            }]
        );
        assert_eq!(batch.next, 2);
    }

    #[tokio::test(start_paused = true)]
    async fn a_wait_is_held_for_30_seconds_then_ends_empty() {
        let events = Events::default();
        let started = Instant::now();

        let batch = events.after(Some(0), HOLD).await;

        assert_eq!(started.elapsed(), Duration::from_secs(30));
        assert_eq!(batch.events, []);
        assert_eq!(batch.next, 0);
        assert!(!batch.reset);
    }

    #[tokio::test(start_paused = true)]
    async fn a_push_wakes_a_wait() {
        let events = Arc::new(Events::default());
        let page = waiting(&events);
        tokio::time::sleep(SHORT).await;
        let started = Instant::now();

        events.push(EventKind::Page, Author::Agent, Some("1"), None);
        let batch = page.await.unwrap();

        assert_eq!(started.elapsed(), Duration::ZERO, "the push ended the wait");
        assert_eq!(batch.events.len(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn a_closed_log_answers_every_wait_at_once() {
        let events = Arc::new(Events::default());
        let page = waiting(&events);
        tokio::time::sleep(SHORT).await;
        let started = Instant::now();

        events.close();
        page.await.unwrap();
        events.after(Some(0), HOLD).await;

        assert_eq!(
            started.elapsed(),
            Duration::ZERO,
            "a stop waits for no wait, and a wait after the stop answers too"
        );
    }

    #[tokio::test]
    async fn a_number_this_server_never_gave_reads_everything_again() {
        let events = Events::default();
        events.push(EventKind::Page, Author::Agent, Some("1"), None);

        let batch = events.after(Some(7), HOLD).await;

        assert!(batch.reset);
        assert_eq!(batch.next, 1, "the number to read from again");
    }

    #[tokio::test]
    async fn a_number_older_than_what_is_kept_reads_everything_again() {
        let events = Events::default();
        for _ in 0..KEPT + 2 {
            events.push(EventKind::Page, Author::Agent, Some("1"), None);
        }

        assert!(events.after(Some(1), HOLD).await.reset);
        assert!(!events.after(Some(2), HOLD).await.reset);
    }

    #[tokio::test(start_paused = true)]
    async fn an_agent_is_present_while_it_waits_and_a_moment_after() {
        let events = Events::default();
        assert!(!events.agent_listens());

        hand_over(&events).await;
        assert!(events.agent_listens(), "it reads before it waits again");

        tokio::time::advance(LISTENS_FOR).await;
        assert!(!events.agent_listens());
        assert!(events.agent_left());
    }

    #[tokio::test(start_paused = true)]
    async fn an_agent_that_hangs_up_leaves_sooner_than_one_that_was_answered() {
        let events = Events::default();
        drop(events.agent_waits());
        assert!(events.agent_listens(), "a wait started again at once");
        tokio::time::advance(HUNG_UP_FOR).await;
        assert!(!events.agent_listens(), "its `qlearn wait` stopped");

        let events = Events::default();
        hand_over(&events).await;
        tokio::time::advance(HUNG_UP_FOR).await;
        assert!(events.agent_listens(), "it starts the next wait");
    }

    #[tokio::test(start_paused = true)]
    async fn the_page_hears_that_the_agent_hung_up_once_its_grace_runs_out() {
        let events = Arc::new(Events::default());
        let agent = events.agent_waits();
        let page = waiting(&events);
        tokio::time::sleep(SHORT).await;

        drop(agent);
        let woken = page.await.unwrap();
        assert!(woken.agent, "the grace still runs");

        let started = Instant::now();
        let gone = events.after(Some(0), HOLD).await;
        assert_eq!(
            started.elapsed(),
            HUNG_UP_FOR,
            "the wait ends with the grace"
        );
        assert!(gone.left);
    }

    #[tokio::test(start_paused = true)]
    async fn an_agent_that_was_present_and_is_gone_has_left() {
        let events = Events::default();
        assert!(!events.after(None, HOLD).await.left, "no agent came yet");

        drop(events.agent_waits());
        assert!(!events.after(None, HOLD).await.left);

        tokio::time::advance(HUNG_UP_FOR).await;
        assert!(events.after(None, HOLD).await.left);

        drop(events.agent_waits());
        assert!(!events.after(None, HOLD).await.left, "it came back");
    }

    #[tokio::test(start_paused = true)]
    async fn an_agent_that_arrives_wakes_the_page() {
        let events = Arc::new(Events::default());
        let page = waiting(&events);
        tokio::time::sleep(SHORT).await;
        let started = Instant::now();

        let _waits = events.agent_waits();
        let batch = page.await.unwrap();

        assert_eq!(started.elapsed(), Duration::ZERO);
        assert!(batch.agent);
    }

    #[tokio::test(start_paused = true)]
    async fn a_request_handed_over_keeps_the_agent_working_until_it_waits_again() {
        let events = Events::default();
        events.push(EventKind::Next, Author::Learner, Some("1"), None);
        hand_over(&events).await;

        tokio::time::advance(WORKS_FOR - Duration::from_secs(1)).await;
        let batch = events.after(None, HOLD).await;
        assert!(batch.working, "a long lesson is not leaving");
        assert!(batch.agent);

        let waits = events.agent_waits();
        assert!(!events.agent_works(), "a wait ends the work");
        drop(waits);
    }

    /// The page says that the agent prepares the next page only when it
    /// does: a broken diagram is work on the page of the learner.
    #[tokio::test(start_paused = true)]
    async fn only_a_next_request_handed_over_prepares_a_page() {
        let events = Events::default();
        events.push(EventKind::Broken, Author::Learner, Some("1"), None);
        hand_over(&events).await;
        let batch = events.after(None, HOLD).await;
        assert!(batch.working);
        assert!(!batch.preparing, "the agent fixes a diagram");

        events.push(EventKind::Next, Author::Learner, Some("1"), None);
        let mut waits = events.agent_waits();
        let batch = events.after(Some(1), SHORT).await;
        waits.answered(&batch);
        let batch = events.after(None, HOLD).await;
        assert!(batch.working);
        assert!(batch.preparing);

        drop(waits);
        let waits = events.agent_waits();
        let batch = events.after(None, HOLD).await;
        assert!(!batch.preparing, "a wait ends the work");
        drop(waits);

        events.push(EventKind::Next, Author::Learner, Some("1"), None);
        hand_over(&events).await;
        tokio::time::advance(WORKS_FOR).await;
        assert!(
            !events.after(None, HOLD).await.preparing,
            "the agent is gone"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn an_agent_that_works_for_30_minutes_with_no_word_is_gone() {
        let events = Events::default();
        events.push(EventKind::Ask, Author::Learner, Some("1"), Some("t-1"));
        hand_over(&events).await;

        tokio::time::advance(WORKS_FOR).await;
        let batch = events.after(None, HOLD).await;

        assert!(!batch.working);
        assert!(!batch.agent);
        assert!(batch.left);
    }

    #[tokio::test(start_paused = true)]
    async fn a_wait_that_hands_over_no_request_leaves_the_agent_idle() {
        let events = Events::default();
        events.push(EventKind::Page, Author::Agent, Some("1"), None);
        hand_over(&events).await;

        assert!(!events.agent_works());
        tokio::time::advance(LISTENS_FOR).await;
        assert!(!events.agent_listens());
    }

    #[tokio::test(start_paused = true)]
    async fn a_request_that_no_wait_handed_over_keeps_no_agent_present() {
        let events = Events::default();
        events.push(EventKind::Next, Author::Learner, Some("1"), None);

        assert!(!events.agent_works());
        assert!(!events.agent_listens());
    }

    #[tokio::test(start_paused = true)]
    async fn the_page_hears_at_once_that_the_agent_works() {
        let events = Arc::new(Events::default());
        // Present already, so the wait of the agent does not wake the page
        // as an arrival before it hands the request over.
        drop(events.agent_waits());
        events.push(EventKind::Next, Author::Learner, Some("1"), None);
        let page = {
            let events = events.clone();
            tokio::spawn(async move { events.after(Some(1), HOLD).await })
        };
        tokio::time::sleep(SHORT).await;
        let started = Instant::now();

        hand_over(&events).await;
        let batch = page.await.unwrap();

        assert_eq!(started.elapsed(), Duration::ZERO);
        assert!(batch.working);
    }

    #[tokio::test(start_paused = true)]
    async fn a_page_between_two_waits_hears_at_once_that_the_agent_works() {
        let events = Events::default();
        drop(events.agent_waits());
        events.push(EventKind::Next, Author::Learner, Some("1"), None);
        let page = events.page_waits();
        let before = page.after(Some(0), HOLD).await;
        drop(page);
        assert!(!before.working, "the request answers the page first");

        hand_over(&events).await;
        let started = Instant::now();
        let batch = events.page_waits().after(Some(before.next), HOLD).await;

        assert_eq!(started.elapsed(), Duration::ZERO);
        assert!(batch.working);

        let page = events.page_waits();
        let started = Instant::now();
        page.after(Some(batch.next), SHORT).await;
        assert_eq!(started.elapsed(), SHORT, "the page heard it once");
    }

    #[tokio::test(start_paused = true)]
    async fn two_tabs_share_one_count_so_the_late_one_hears_the_change_with_the_next_write() {
        // The pinned limit: the wire names no tab, so the server keeps one
        // count for all of them.
        let events = Arc::new(Events::default());
        let first = events.page_waits().after(Some(0), SHORT).await;
        assert!(!first.agent);
        let waiting = {
            let events = events.clone();
            tokio::spawn(async move { events.page_waits().after(Some(0), HOLD).await })
        };
        tokio::time::sleep(SHORT).await;

        let _agent = events.agent_waits();
        assert!(waiting.await.unwrap().agent, "the tab that waits hears it");

        let late = {
            let events = events.clone();
            tokio::spawn(async move { events.page_waits().after(Some(0), HOLD).await })
        };
        tokio::time::sleep(SHORT).await;
        assert!(!late.is_finished(), "the other tab answered first");
        let started = Instant::now();
        events.push(EventKind::Status, Author::Agent, None, None);
        let batch = late.await.unwrap();
        assert_eq!(started.elapsed(), Duration::ZERO);
        assert!(batch.agent);
    }

    #[tokio::test(start_paused = true)]
    async fn a_page_between_two_waits_hears_no_change_when_the_agent_hangs_up() {
        let events = Events::default();
        let agent = events.agent_waits();
        let before = events.page_waits().after(Some(0), HOLD).await;
        assert!(before.agent);

        drop(agent);
        let started = Instant::now();
        let batch = events.page_waits().after(Some(before.next), HOLD).await;

        assert_eq!(
            started.elapsed(),
            HUNG_UP_FOR,
            "the page shows the same until the grace runs out"
        );
        assert!(batch.left);
    }

    #[tokio::test(start_paused = true)]
    async fn the_page_hears_at_once_that_a_wait_with_no_write_ends_the_work() {
        let events = Arc::new(Events::default());
        events.push(EventKind::Chosen, Author::Learner, Some("1"), Some("q-1"));
        hand_over(&events).await;
        let page = {
            let events = events.clone();
            tokio::spawn(async move { events.after(Some(1), HOLD).await })
        };
        tokio::time::sleep(SHORT).await;
        let started = Instant::now();

        let _waits = events.agent_waits();
        let batch = page.await.unwrap();

        assert_eq!(started.elapsed(), Duration::ZERO);
        assert!(!batch.working);
    }

    #[tokio::test(start_paused = true)]
    async fn the_page_hears_that_the_agent_is_gone_once_its_work_runs_out() {
        let events = Events::default();
        events.push(EventKind::Next, Author::Learner, Some("1"), None);
        hand_over(&events).await;
        tokio::time::advance(WORKS_FOR - Duration::from_secs(1)).await;

        let started = Instant::now();
        let batch = events.after(Some(1), HOLD).await;

        assert_eq!(
            started.elapsed(),
            Duration::from_secs(1),
            "the wait ends with the work"
        );
        assert!(!batch.working);
        assert!(batch.left);
    }

    #[tokio::test]
    async fn the_next_wait_of_the_agent_clears_its_status() {
        let events = Events::default();
        drop(events.agent_waits());
        events.set_status("Writing lesson 4: the code tour");

        let batch = events.after(Some(0), HOLD).await;
        assert_eq!(
            batch.status.as_deref(),
            Some("Writing lesson 4: the code tour")
        );
        assert_eq!(batch.events[0].kind, EventKind::Status);

        drop(events.agent_waits());
        let batch = events.after(Some(1), HOLD).await;
        assert_eq!(batch.status, None);
        assert_eq!(
            batch.events[0].kind,
            EventKind::Idle,
            "the page learns of it"
        );
    }

    #[tokio::test]
    async fn an_empty_status_clears_the_one_before() {
        let events = Events::default();
        assert_eq!(
            events.set_status("  Writing lesson 4  ").as_deref(),
            Some("Writing lesson 4"),
            "a line of the top bar has no margin"
        );

        assert_eq!(events.set_status(" \n"), None);

        let batch = events.after(Some(0), HOLD).await;
        assert_eq!(batch.status, None);
        assert_eq!(batch.events.len(), 2, "the page learns of each one");
    }

    #[tokio::test]
    async fn a_request_keeps_its_detail_in_the_log() {
        let events = Events::default();
        let detail = Detail::Next {
            left: crate::model::PageName {
                id: "1".to_owned(),
                title: "Two leaders".to_owned(),
            },
        };

        let event = events.request(EventKind::Next, Some("1"), None, detail.clone());

        assert_eq!(event.author, Author::Learner);
        assert_eq!(event.detail, Some(detail));
        assert_eq!(events.after(Some(0), HOLD).await.events, [event]);
    }

    #[tokio::test]
    async fn a_wait_after_the_agent_wrote_says_it_is_idle() {
        let events = Events::default();
        drop(events.agent_waits());
        assert_eq!(
            events.after(None, HOLD).await.next,
            0,
            "nothing written yet"
        );

        events.push(EventKind::Page, Author::Agent, Some("1"), None);
        events.push(EventKind::Next, Author::Learner, Some("1"), None);
        drop(events.agent_waits());
        drop(events.agent_waits());

        let batch = events.after(Some(2), HOLD).await;
        assert_eq!(batch.events.len(), 1, "one idle, not one per wait");
        assert_eq!(batch.events[0].kind, EventKind::Idle);
        assert_eq!(batch.events[0].page, None);
    }

    /// A new server numbers its events from 1 again, so the id is what
    /// tells a party that its `after` counts the events of another one.
    #[tokio::test]
    async fn each_server_draws_an_id_that_every_batch_carries() {
        let old = Events::default();
        let new = Events::default();
        assert_ne!(old.server(), new.server());
        assert_eq!(old.server().len(), 16, "{}", old.server());

        new.push(EventKind::Page, Author::Agent, Some("1"), None);
        assert_eq!(new.after(None, HOLD).await.server, new.server());
        assert_eq!(new.after(Some(0), HOLD).await.server, new.server());
        assert_eq!(new.after(Some(9), HOLD).await.server, new.server());
    }

    /// A part of a quiz tells the other tabs, and is no work for the agent.
    #[tokio::test(start_paused = true)]
    async fn a_part_of_a_quiz_is_no_request() {
        let events = Events::default();
        assert!(!EventKind::Answered.is_request());

        events.push(EventKind::Answered, Author::Learner, Some("1"), Some("1"));
        hand_over(&events).await;

        assert!(!events.agent_works(), "the agent has nothing to do");
    }
}
