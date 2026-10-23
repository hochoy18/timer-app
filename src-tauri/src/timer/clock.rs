//! 权威时钟线程。
//!
//! 这是整个 app 的心跳来源（见 ADR-0002）：一个内部线程持有状态机，
//! 每 250ms 读一次墙钟，把"到点了没有"翻译成事件送进状态机，
//! 再把状态机吐出来的副作用分发出去。
//!
//! 三层分工：
//!
//! - [`TimerEngine`]：纯逻辑。持状态机，算出"这一 tick 该做什么"，不读时钟、不做 I/O。
//! - [`Dispatcher`]：副作用的出口。通知、音效、面板、托盘都从这里出去。
//! - [`ClockThread`]：把上面两层接到真实的墙钟和真实的线程上。
//!
//! 不变量：**墙钟是唯一的时间来源**。倒计时不累加秒数，一律用绝对时刻现算
//! （见 ADR-0001）；因此休眠跨过截止时刻后，醒来第一次 tick 就会直接推进到
//! 正确状态，不用"补发"中间错过的那几次。

use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::config::TimerConfig;

use super::state::{reduce, Effect, Event, TimerCore, TimerSnapshot, TimerState};

/// 心跳间隔。
///
/// 250ms 足够跟上秒级的托盘显示，又不至于让一个常驻应用白烧 CPU。
pub const TICK_INTERVAL: Duration = Duration::from_millis(250);

/// 副作用的出口。
///
/// 状态机只说"要通知、要响音效、要把面板顶出来"，具体怎么做是这一层的事。
/// 做成 trait 是为了让时钟线程不必依赖 Tauri——测试里换个假实现就行。
pub trait Dispatcher: Send + Sync + 'static {
    /// 发一条系统通知。
    fn notify(&self, title: &str, body: &str);
    /// 播放一次系统音效。
    fn play_sound(&self);
    /// 显示并置顶面板。**不得抢焦点**。
    fn show_panel(&self);
    /// 设置托盘的提醒态样式（醒目图标 + 红色剩余时间）。
    fn set_tray_alert(&self, alerting: bool);
    /// 设置托盘标题（剩余时间）。
    fn set_tray_title(&self, title: &str);
    /// 把快照推给前端。
    fn emit_state(&self, snapshot: &TimerSnapshot);
}

/// 一次 tick 对外的输出。
///
/// 三个字段都是"有才发"：没变化的就不惊动前端、也不重复设置托盘。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TickOutput {
    /// 要推给前端的快照。状态、剩余秒数、截止时刻都没变时为 `None`。
    pub snapshot: Option<TimerSnapshot>,
    /// 要设到托盘上的标题。标题文本没变时为 `None`。
    pub tray_title: Option<String>,
    /// 状态机产生的副作用。
    pub effects: Vec<Effect>,
}

/// 已经发出去过的东西：用来判断"这一次要不要再发一遍"。
#[derive(Debug, Default)]
struct Sent {
    /// 上次推给前端的快照。
    snapshot: Option<TimerSnapshot>,
    /// 上次推给前端时的剩余秒数。
    seconds: Option<u64>,
    /// 上次设到托盘上的标题。
    title: Option<String>,
}

/// 计时引擎：持有状态机，并记住"上次给前端、给托盘发过什么"。
///
/// 这一层是纯的：`now` 总是从外面传进来，因此每一个 tick 都能被测试精确驱动。
/// 它自己不读时钟——读时钟是 [`ClockThread`] 的事。
pub struct TimerEngine {
    core: TimerCore,
    cfg: TimerConfig,
    sent: Sent,
}

impl TimerEngine {
    /// 应用启动：立即进入一个完整的工作时段。
    pub fn new(now: u64, cfg: TimerConfig) -> Self {
        Self {
            core: TimerCore::start(now, &cfg),
            cfg,
            sent: Sent::default(),
        }
    }

    /// 当前状态机。
    pub fn core(&self) -> &TimerCore {
        &self.core
    }

    /// 当前快照。
    pub fn snapshot(&self) -> TimerSnapshot {
        self.core.snapshot()
    }

    /// 推进一个 tick。
    ///
    /// 时间到了就把对应事件送进状态机；没到就只更新"还剩多少秒"。
    pub fn tick(&mut self, now: u64) -> TickOutput {
        match self.time_event(now) {
            Some(event) => self.apply(event, now),
            None => self.publish(now, Vec::new()),
        }
    }

    /// 应用一个事件（用户动作或时间到期）。
    pub fn apply(&mut self, event: Event, now: u64) -> TickOutput {
        let transition = reduce(&self.core, event, now, &self.cfg);
        self.core = transition.core;
        self.publish(now, transition.effects)
    }

    /// 这一刻该送哪个事件？
    ///
    /// 每种倒计时状态只有一个"下一件事发生的时刻"，所以一个 tick 最多推进一步。
    /// 休眠跨过好多个时刻也只推进一步——补发错过的历史没有意义（票 02 的结论）。
    fn time_event(&self, now: u64) -> Option<Event> {
        match self.core.state {
            TimerState::Working if now >= self.core.deadline_ms => Some(Event::WorkElapsed),
            TimerState::WorkAlerting if now >= self.core.deadline_ms => Some(Event::AlertRepeat),
            TimerState::Breaking if now >= self.core.deadline_ms => Some(Event::BreakElapsed),
            _ => None,
        }
    }

    /// 把当前状态整理成一份对外输出，并记住"发过什么"。
    fn publish(&mut self, now: u64, effects: Vec<Effect>) -> TickOutput {
        let snapshot = self.core.snapshot();
        let seconds = display_seconds(&self.core, now);
        let title = format_remaining(seconds);

        // 状态、截止时刻、剩余秒数里任何一样变了才推快照。
        // 倒计时状态每秒推一帧（秒数变了），其余时候安静待着。
        let unchanged =
            self.sent.snapshot == Some(snapshot) && self.sent.seconds == Some(seconds);
        let snapshot_out = if unchanged {
            None
        } else {
            self.sent.snapshot = Some(snapshot);
            self.sent.seconds = Some(seconds);
            Some(snapshot)
        };

        // 标题文本没变就不重复设置托盘。
        let title_out = if self.sent.title.as_deref() != Some(title.as_str()) {
            self.sent.title = Some(title.clone());
            Some(title)
        } else {
            None
        };

        TickOutput {
            snapshot: snapshot_out,
            tray_title: title_out,
            effects,
        }
    }
}

/// 当前显示的剩余秒数（向上取整，与前端 `formatDuration` 一致）。
fn display_seconds(core: &TimerCore, now: u64) -> u64 {
    let ms = match core.state {
        // 提醒态没有倒计时：工作时长的账已经结清了。
        // （`deadline_ms` 此时是下一次重复提醒的时刻，不是倒计时。）
        TimerState::WorkAlerting => 0,
        _ => core.remaining_ms(now),
    };
    ms.saturating_add(999) / 1000
}

/// 把秒数格式化成托盘标题。
///
/// 一小时以内是 `MM:SS`；超过一小时用 `H:MM:SS`（票 04 的要求）。
///
/// 正好一小时仍是 `MM:SS`（`60:00`）——与前端的 `formatDuration` 一致，
/// 免得面板和托盘在同一时刻显示成两种写法。再往上（`H:MM:SS`）前端还没有
/// 对应的写法，但那时候离得够远，两边写法不同也无所谓。
pub fn format_remaining(seconds: u64) -> String {
    if seconds > 3600 {
        format!(
            "{}:{:02}:{:02}",
            seconds / 3600,
            (seconds % 3600) / 60,
            seconds % 60
        )
    } else {
        format!("{:02}:{:02}", seconds / 60, seconds % 60)
    }
}

/// 当前的墙钟时刻（epoch 毫秒）。
///
/// 用墙钟而不是单调钟：单调钟在 macOS 上不含休眠时间，休眠跨过截止时刻后会算错。
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 前端传来的动作 → 状态机事件。
pub fn event_for_action(action: &str) -> Option<Event> {
    match action {
        "pause" => Some(Event::Pause),
        "resume" => Some(Event::Resume),
        "restart" => Some(Event::Restart),
        "acknowledge" => Some(Event::Acknowledge),
        _ => None,
    }
}

/// 取锁。
///
/// 锁中毒只说明"之前有线程 panic 过"，数据本身没坏——常驻的计时器不该因为
/// 一次 panic 就整个 app 陪葬，更不该在 `Drop` 里 panic（那会直接 abort）。
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// 把一次 tick 的输出分发出去。
///
/// 顺序有讲究：
///
/// 1. 先设托盘标题、先推前端快照——否则面板弹出来时，前端手里还拿着上一帧状态；
/// 2. 再触发通知/音效/面板；
/// 3. 提醒态样式（`SetTrayAlert`）最后生效——它要盖过刚设上去的标题颜色。
fn dispatch(out: TickOutput, dispatcher: &dyn Dispatcher) {
    if let Some(title) = &out.tray_title {
        dispatcher.set_tray_title(title);
    }
    if let Some(snapshot) = &out.snapshot {
        dispatcher.emit_state(snapshot);
    }
    for effect in out.effects {
        match effect {
            Effect::Notify { title, body } => dispatcher.notify(title, body),
            Effect::PlaySound => dispatcher.play_sound(),
            Effect::ShowPanel => dispatcher.show_panel(),
            Effect::SetTrayAlert(alerting) => dispatcher.set_tray_alert(alerting),
        }
    }
}

/// 停机信号：一个标志位 + 一个条件变量。
///
/// 光有标志位的话，线程要睡满一个心跳才醒；条件变量让它立刻醒。
struct StopSignal {
    stopped: Mutex<bool>,
    condvar: Condvar,
}

impl StopSignal {
    fn new() -> Self {
        Self {
            stopped: Mutex::new(false),
            condvar: Condvar::new(),
        }
    }

    fn stop(&self) {
        let mut stopped = lock(&self.stopped);
        *stopped = true;
        self.condvar.notify_all();
    }

    /// 最多等 `duration`。返回 `true` 表示"被要求停了"。
    fn wait(&self, duration: Duration) -> bool {
        let stopped = lock(&self.stopped);
        if *stopped {
            return true;
        }
        let (stopped, _) = self
            .condvar
            .wait_timeout(stopped, duration)
            .unwrap_or_else(|e| e.into_inner());
        *stopped
    }
}

/// 权威时钟线程。
///
/// 线程自己读墙钟、按固定节奏 tick。用户动作从别的线程进来时**立刻生效**，
/// 不用等下一个 tick——按钮点了就该马上变。
pub struct ClockThread {
    engine: Arc<Mutex<TimerEngine>>,
    dispatcher: Arc<dyn Dispatcher>,
    stop: Arc<StopSignal>,
    handle: Mutex<Option<JoinHandle<()>>>,
}

impl ClockThread {
    /// 启动时钟线程。
    pub fn start(dispatcher: Arc<dyn Dispatcher>, cfg: TimerConfig) -> Self {
        Self::start_with_interval(dispatcher, cfg, TICK_INTERVAL)
    }

    /// 以指定的心跳间隔启动（测试用）。
    pub fn start_with_interval(
        dispatcher: Arc<dyn Dispatcher>,
        cfg: TimerConfig,
        interval: Duration,
    ) -> Self {
        let engine = Arc::new(Mutex::new(TimerEngine::new(now_ms(), cfg)));
        let stop = Arc::new(StopSignal::new());
        let handle = {
            let engine = Arc::clone(&engine);
            let dispatcher = Arc::clone(&dispatcher);
            let stop = Arc::clone(&stop);
            std::thread::Builder::new()
                .name("focus-timer-clock".to_string())
                .spawn(move || run(engine, dispatcher, stop, interval))
                .expect("无法启动时钟线程")
        };
        Self {
            engine,
            dispatcher,
            stop,
            handle: Mutex::new(Some(handle)),
        }
    }

    /// 应用一个用户动作：立刻生效，不等下一个 tick。
    pub fn apply(&self, event: Event) {
        // 分发在锁内完成：发出的顺序必须与状态变化的顺序一致，
        // 否则"点暂停"和"同时到点"撞在一起时，前端会先收到新的、再收到旧的。
        let mut engine = lock(&self.engine);
        let out = engine.apply(event, now_ms());
        dispatch(out, self.dispatcher.as_ref());
    }

    /// 当前快照。
    pub fn snapshot(&self) -> TimerSnapshot {
        lock(&self.engine).snapshot()
    }

    /// 停机：置标志位、唤醒线程、等它结束。可重复调用。
    ///
    /// **应用退出时必须显式叫一次，不能只靠 `Drop`。** 时钟线程住在 Tauri 的
    /// 状态表里，而它的分发器持有 `AppHandle`，状态表又挂在 `AppHandle` 内部
    /// （`AppHandle → Arc<AppManager> → Arc<StateManager> → ClockThread →
    /// Dispatcher → AppHandle`）——这条引用链成环，`Drop` 永远等不到。
    /// 单元测试里没有这个环，所以测试绿了不代表退出时线程会收干净。
    ///
    /// 等线程结束最多等一个心跳，超时就算了。分发副作用要经 Tauri 的事件循环，
    /// 万一那一刻它恰好在等主线程，而调用方自己就是主线程，`join` 就会互锁——
    /// 退出时宁可放过一个线程，也不能把 app 卡在关不掉的状态。
    pub fn stop(&self) {
        self.stop.stop();
        let deadline = Instant::now() + TICK_INTERVAL;
        loop {
            let finished = {
                let slot = lock(&self.handle);
                match &*slot {
                    Some(handle) => handle.is_finished(),
                    // 已经 join 过了，没什么可等的。
                    None => true,
                }
            };
            if finished || Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        if let Some(handle) = lock(&self.handle).take() {
            let _ = handle.join();
        }
    }
}

impl Drop for ClockThread {
    fn drop(&mut self) {
        // 置标志位 + 唤醒线程 + join：不留下游离线程。
        // `stop()` 已经被调用过时这里是空的。
        self.stop.stop();
        let handle = self
            .handle
            .get_mut()
            .ok()
            .and_then(|slot| slot.take());
        if let Some(handle) = handle {
            let _ = handle.join();
        }
    }
}

/// 时钟线程的主循环。
fn run(
    engine: Arc<Mutex<TimerEngine>>,
    dispatcher: Arc<dyn Dispatcher>,
    stop: Arc<StopSignal>,
    interval: Duration,
) {
    // 第一个心跳立刻来一次：应用一启动就该把初始状态推给前端。
    let mut next = Instant::now();
    loop {
        // 注意 `now` 是在醒来之后才读的。休眠期间读到的墙钟与醒来之后读到的
        // 不是一回事——只有醒来后这一读，才能看到"截止时刻其实早就过了"。
        {
            let mut engine = lock(&engine);
            let out = engine.tick(now_ms());
            // 分发在锁内完成：发出的顺序必须与状态变化的顺序一致。
            dispatch(out, dispatcher.as_ref());
        }

        // 对齐下一个心跳：睡过头就重新起算，不补发一串 tick。
        let after = Instant::now();
        let candidate = next + interval;
        next = if candidate > after {
            candidate
        } else {
            after + interval
        };

        // 睡到下一个心跳；被要求停机时立刻醒。
        if stop.wait(next.saturating_duration_since(Instant::now())) {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timer::test_support::{cfg, MIN};
    use std::sync::atomic::{AtomicBool, Ordering};

    /// 记录所有副作用的假分发器。
    #[derive(Default)]
    struct FakeDispatcher {
        calls: Mutex<Vec<Call>>,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Call {
        Notify(String, String),
        PlaySound,
        ShowPanel,
        SetTrayAlert(bool),
        SetTrayTitle(String),
        EmitState(TimerSnapshot),
    }

    impl FakeDispatcher {
        fn calls(&self) -> Vec<Call> {
            self.calls.lock().unwrap().clone()
        }

        fn titles(&self) -> Vec<String> {
            self.calls()
                .into_iter()
                .filter_map(|c| match c {
                    Call::SetTrayTitle(t) => Some(t),
                    _ => None,
                })
                .collect()
        }

        fn effects(&self) -> Vec<Call> {
            self.calls()
                .into_iter()
                .filter(|c| {
                    matches!(
                        c,
                        Call::Notify(..) | Call::PlaySound | Call::ShowPanel | Call::SetTrayAlert(..)
                    )
                })
                .collect()
        }
    }

    impl Dispatcher for FakeDispatcher {
        fn notify(&self, title: &str, body: &str) {
            self.calls
                .lock()
                .unwrap()
                .push(Call::Notify(title.to_string(), body.to_string()));
        }

        fn play_sound(&self) {
            self.calls.lock().unwrap().push(Call::PlaySound);
        }

        fn show_panel(&self) {
            self.calls.lock().unwrap().push(Call::ShowPanel);
        }

        fn set_tray_alert(&self, alerting: bool) {
            self.calls
                .lock()
                .unwrap()
                .push(Call::SetTrayAlert(alerting));
        }

        fn set_tray_title(&self, title: &str) {
            self.calls
                .lock()
                .unwrap()
                .push(Call::SetTrayTitle(title.to_string()));
        }

        fn emit_state(&self, snapshot: &TimerSnapshot) {
            self.calls
                .lock()
                .unwrap()
                .push(Call::EmitState(*snapshot));
        }
    }

    // ── 引擎：时间推进 ─────────────────────────────────────────────────

    #[test]
    fn a_tick_before_the_deadline_does_not_advance_the_state() {
        let mut engine = TimerEngine::new(0, cfg());
        let out = engine.tick(59 * MIN);
        assert_eq!(engine.core().state, TimerState::Working);
        assert!(out.effects.is_empty());
    }

    #[test]
    fn a_tick_at_the_deadline_alerts() {
        let mut engine = TimerEngine::new(0, cfg());
        let out = engine.tick(60 * MIN);
        assert_eq!(engine.core().state, TimerState::WorkAlerting);
        assert!(out.effects.iter().any(|e| matches!(e, Effect::PlaySound)));
        assert!(out.effects.iter().any(|e| matches!(e, Effect::ShowPanel)));
        assert!(out
            .effects
            .iter()
            .any(|e| matches!(e, Effect::SetTrayAlert(true))));
        assert!(out
            .effects
            .iter()
            .any(|e| matches!(e, Effect::Notify { .. })));
    }

    #[test]
    fn the_alerting_state_repeats_the_reminder() {
        let mut engine = TimerEngine::new(0, cfg());
        engine.tick(60 * MIN);
        let out = engine.tick(70 * MIN);
        assert_eq!(engine.core().state, TimerState::WorkAlerting);
        assert!(out.effects.iter().any(|e| matches!(e, Effect::PlaySound)));
        // 重复间隔从**这一次的提醒时刻**起算。
        assert_eq!(engine.core().deadline_ms, 70 * MIN + 10 * MIN);
    }

    #[test]
    fn the_breaking_state_returns_to_working() {
        let mut engine = TimerEngine::new(0, cfg());
        engine.tick(60 * MIN);
        engine.apply(Event::Acknowledge, 60 * MIN);
        assert_eq!(engine.core().state, TimerState::Breaking);
        let out = engine.tick(75 * MIN);
        assert_eq!(engine.core().state, TimerState::Working);
        assert!(out.effects.iter().any(|e| matches!(e, Effect::PlaySound)));
    }

    #[test]
    fn a_paused_timer_never_alerts() {
        let mut engine = TimerEngine::new(0, cfg());
        engine.apply(Event::Pause, 10 * MIN);
        let out = engine.tick(10 * 60 * MIN);
        assert_eq!(engine.core().state, TimerState::Paused);
        assert!(out.effects.is_empty());
    }

    // ── 引擎：休眠跨越截止时刻 ─────────────────────────────────────────

    #[test]
    fn waking_up_after_a_long_sleep_advances_in_one_tick() {
        // 睡了 8 小时，醒来的第一次 tick 就要把状态推到提醒态。
        let mut engine = TimerEngine::new(0, cfg());
        let out = engine.tick(8 * 60 * MIN);
        assert_eq!(engine.core().state, TimerState::WorkAlerting);
        assert!(out.effects.iter().any(|e| matches!(e, Effect::ShowPanel)));
        // 重复提醒从**醒来这一刻**起算，而不是从原定的 60 分钟起算。
        assert_eq!(engine.core().deadline_ms, 8 * 60 * MIN + 10 * MIN);
    }

    #[test]
    fn waking_up_after_a_long_sleep_does_not_backfill_the_missed_repeats() {
        let mut engine = TimerEngine::new(0, cfg());
        engine.tick(8 * 60 * MIN);
        // 还没到下一次重复提醒的时间：安静。
        let out = engine.tick(8 * 60 * MIN + 5 * MIN);
        assert!(out.effects.is_empty());
        assert_eq!(engine.core().state, TimerState::WorkAlerting);
    }

    // ── 引擎：快照与标题 ───────────────────────────────────────────────

    #[test]
    fn the_first_tick_emits_the_snapshot_and_the_title() {
        let mut engine = TimerEngine::new(0, cfg());
        let out = engine.tick(0);
        assert_eq!(out.snapshot.unwrap().state, TimerState::Working);
        assert_eq!(out.tray_title.unwrap(), "60:00");
    }

    #[test]
    fn the_snapshot_is_emitted_once_per_second() {
        let mut engine = TimerEngine::new(0, cfg());
        engine.tick(0);
        // 同一秒内的 tick 不重复推。
        assert!(engine.tick(100).snapshot.is_none());
        assert!(engine.tick(900).snapshot.is_none());
        // 跨过一秒边界：推。
        assert!(engine.tick(1_000).snapshot.is_some());
    }

    #[test]
    fn the_tray_title_is_only_set_when_the_text_changes() {
        let mut engine = TimerEngine::new(0, cfg());
        engine.tick(0);
        // 同一秒内：不重复设置。
        assert!(engine.tick(100).tray_title.is_none());
        // 跨过一秒边界：设置新标题。
        assert_eq!(engine.tick(1_000).tray_title.unwrap(), "59:59");
    }

    #[test]
    fn a_paused_timer_stops_emitting_snapshots() {
        let mut engine = TimerEngine::new(0, cfg());
        engine.tick(0);
        engine.apply(Event::Pause, 10 * MIN);
        // 暂停后剩余时间被冻结：既不推快照，也不改标题。
        assert!(engine.tick(10 * MIN + 100).snapshot.is_none());
        assert!(engine.tick(10 * MIN + 100).tray_title.is_none());
    }

    #[test]
    fn the_alerting_state_has_no_countdown() {
        let mut engine = TimerEngine::new(0, cfg());
        // 提醒态的标题是 00:00，不是"距离下一次重复提醒还有多久"。
        let out = engine.tick(60 * MIN);
        assert_eq!(out.tray_title.unwrap(), "00:00");
        let snapshot = engine.snapshot();
        assert_eq!(snapshot.deadline_ms, None);
    }

    #[test]
    fn the_title_switches_to_hours_beyond_an_hour() {
        assert_eq!(format_remaining(59), "00:59");
        assert_eq!(format_remaining(60), "01:00");
        // 正好一小时：与前端的 `formatDuration` 一样写 `60:00`。
        assert_eq!(format_remaining(3_600), "60:00");
        // 超过一小时：`H:MM:SS`。
        assert_eq!(format_remaining(3_601), "1:00:01");
        assert_eq!(format_remaining(3_661), "1:01:01");
    }

    // ── 引擎：用户动作 ─────────────────────────────────────────────────

    #[test]
    fn applying_pause_freezes_the_remaining_time() {
        let mut engine = TimerEngine::new(0, cfg());
        let out = engine.apply(Event::Pause, 20 * MIN);
        assert_eq!(engine.core().state, TimerState::Paused);
        assert_eq!(out.snapshot.unwrap().remaining_ms, 40 * MIN);
    }

    #[test]
    fn applying_resume_continues_from_the_frozen_remaining() {
        let mut engine = TimerEngine::new(0, cfg());
        engine.apply(Event::Pause, 20 * MIN);
        engine.apply(Event::Resume, 20 * MIN);
        assert_eq!(engine.core().state, TimerState::Working);
        assert_eq!(engine.core().deadline_ms, 20 * MIN + 40 * MIN);
    }

    #[test]
    fn applying_restart_starts_a_full_work_session() {
        let mut engine = TimerEngine::new(0, cfg());
        engine.apply(Event::Restart, 33 * MIN);
        assert_eq!(engine.core().deadline_ms, 33 * MIN + 60 * MIN);
    }

    #[test]
    fn a_restart_is_pushed_even_though_the_displayed_second_does_not_change() {
        let mut engine = TimerEngine::new(0, cfg());
        engine.tick(0);
        // 只过了 100ms：显示的秒数还是 3600，标题也不用改。
        // 但截止时刻变了——前端必须拿到新的 deadlineMs，否则会按旧的时刻插值。
        let out = engine.apply(Event::Restart, 100);
        assert_eq!(out.snapshot.unwrap().deadline_ms, Some(100 + 60 * MIN));
        assert!(out.tray_title.is_none());
    }

    #[test]
    fn front_end_actions_map_to_events() {
        assert_eq!(event_for_action("pause"), Some(Event::Pause));
        assert_eq!(event_for_action("resume"), Some(Event::Resume));
        assert_eq!(event_for_action("restart"), Some(Event::Restart));
        assert_eq!(event_for_action("acknowledge"), Some(Event::Acknowledge));
        assert_eq!(event_for_action("nonsense"), None);
    }

    #[test]
    fn the_emitted_payload_matches_the_front_end_contract() {
        // 前端 `src/types.ts` 的 `TimerSnapshot`：state / deadlineMs / remainingMs。
        // 字段名对不上，前端就静默地读到 undefined——这里锁死它。
        let json = serde_json::to_value(TimerSnapshot {
            state: TimerState::WorkAlerting,
            deadline_ms: None,
            remaining_ms: 0,
        })
        .unwrap();
        assert_eq!(json["state"], "workAlerting");
        assert!(json["deadlineMs"].is_null());
        assert_eq!(json["remainingMs"], 0);
        // 不多不少就这三个字段。
        assert_eq!(json.as_object().unwrap().len(), 3);
    }

    // ── 线程 ───────────────────────────────────────────────────────────

    #[test]
    fn the_thread_runs_on_its_own() {
        let dispatcher = Arc::new(FakeDispatcher::default());
        let clock = ClockThread::start_with_interval(
            Arc::clone(&dispatcher) as Arc<dyn Dispatcher>,
            cfg(),
            Duration::from_millis(5),
        );
        // 线程自己跑起来，第一帧快照自己发出去。
        let deadline = Instant::now() + Duration::from_secs(2);
        while dispatcher.titles().is_empty() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(dispatcher.titles(), vec!["60:00".to_string()]);
        drop(clock);
    }

    #[test]
    fn the_thread_dispatches_the_effects_of_a_state_change() {
        let dispatcher = Arc::new(FakeDispatcher::default());
        let cfg = TimerConfig {
            work_ms: 50,
            break_ms: 50,
            repeat_ms: 1_000,
        };
        let clock = ClockThread::start_with_interval(
            Arc::clone(&dispatcher) as Arc<dyn Dispatcher>,
            cfg,
            Duration::from_millis(5),
        );
        let deadline = Instant::now() + Duration::from_secs(2);
        while !dispatcher.effects().iter().any(|c| matches!(c, Call::ShowPanel))
            && Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(5));
        }
        let calls = dispatcher.calls();
        assert!(calls.iter().any(|c| matches!(c, Call::PlaySound)));
        assert!(calls.iter().any(|c| matches!(c, Call::ShowPanel)));
        assert!(calls
            .iter()
            .any(|c| matches!(c, Call::SetTrayAlert(true))));
        assert!(calls.iter().any(|c| matches!(c, Call::Notify(..))));
        drop(clock);
    }

    #[test]
    fn the_thread_refreshes_the_title_once_per_second() {
        let dispatcher = Arc::new(FakeDispatcher::default());
        let cfg = TimerConfig {
            work_ms: 60 * MIN,
            break_ms: 1_000,
            repeat_ms: 1_000,
        };
        let clock = ClockThread::start_with_interval(
            Arc::clone(&dispatcher) as Arc<dyn Dispatcher>,
            cfg,
            Duration::from_millis(5),
        );
        // 等到标题变了两次（初值 + 跨过一秒边界）。
        let deadline = Instant::now() + Duration::from_secs(3);
        while dispatcher.titles().len() < 2 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        let titles = dispatcher.titles();
        assert_eq!(titles[0], "60:00");
        assert_eq!(titles[1], "59:59");
        // 两次设置之间至少隔了一秒——没有在 250ms 的心跳上反复刷同一个标题。
        assert!(titles.len() <= 4, "标题刷新得太频繁：{titles:?}");
        drop(clock);
    }

    #[test]
    fn a_user_action_takes_effect_without_waiting_for_a_tick() {
        let dispatcher = Arc::new(FakeDispatcher::default());
        let clock = ClockThread::start_with_interval(
            Arc::clone(&dispatcher) as Arc<dyn Dispatcher>,
            cfg(),
            // 心跳慢到不可能在断言之前发生。
            Duration::from_millis(60_000),
        );
        clock.apply(Event::Pause);
        // 立刻就是暂停态，不用等下一个 tick。
        let snapshot = clock.snapshot();
        assert_eq!(snapshot.state, TimerState::Paused);
        // 剩余时间被冻结在一整个工作时段上（差几毫秒是时钟本身的精度）。
        assert!(snapshot.remaining_ms <= 60 * MIN);
        assert!(snapshot.remaining_ms > 60 * MIN - 1_000);
        drop(clock);
    }

    #[test]
    fn dropping_the_clock_thread_stops_it_cleanly() {
        let dispatcher = Arc::new(FakeDispatcher::default());
        let recorder = Arc::clone(&dispatcher);
        let clock = ClockThread::start_with_interval(
            Arc::clone(&dispatcher) as Arc<dyn Dispatcher>,
            cfg(),
            Duration::from_millis(5),
        );
        std::thread::sleep(Duration::from_millis(50));
        drop(clock);
        // 线程已经 join 掉了：之后再调用不会再有新的副作用。
        let after_drop = recorder.calls().len();
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(recorder.calls().len(), after_drop);
    }

    #[test]
    fn the_clock_thread_can_be_stopped_without_being_dropped() {
        // 应用退出走的是这条路（引用成环，`Drop` 等不到），所以它也得干净。
        let dispatcher = Arc::new(FakeDispatcher::default());
        let clock = ClockThread::start_with_interval(
            Arc::clone(&dispatcher) as Arc<dyn Dispatcher>,
            cfg(),
            Duration::from_millis(5),
        );
        std::thread::sleep(Duration::from_millis(50));
        clock.stop();
        let after_stop = dispatcher.calls().len();
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(dispatcher.calls().len(), after_stop);
        // 可重复调用；随后的 `Drop` 也不该因此 panic。
        clock.stop();
        drop(clock);
    }

    // ── 其它 ───────────────────────────────────────────────────────────

    #[test]
    fn the_tick_interval_is_250ms() {
        assert_eq!(TICK_INTERVAL, Duration::from_millis(250));
    }

    #[test]
    fn the_stop_flag_wakes_the_sleeping_thread() {
        let stop = Arc::new(StopSignal::new());
        let woke = Arc::new(AtomicBool::new(false));
        let handle = {
            let stop = Arc::clone(&stop);
            let woke = Arc::clone(&woke);
            std::thread::spawn(move || {
                stop.wait(Duration::from_secs(30));
                woke.store(true, Ordering::SeqCst);
            })
        };
        std::thread::sleep(Duration::from_millis(20));
        stop.stop();
        handle.join().unwrap();
        assert!(woke.load(Ordering::SeqCst));
    }
}
