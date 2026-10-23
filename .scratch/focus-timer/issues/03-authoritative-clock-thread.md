# 03 — Rust 权威时钟线程

Status: resolved

## 目标

启动一个 Rust 内部线程作为唯一时钟源：持有 `TimerCore`，周期性校验时间戳，驱动状态机推进，并向托盘推送剩余时间。

## 验收

- [x] `src-tauri/src/timer/clock.rs` 提供 `ClockThread`，启动后独立运行
- [x] 每 250ms 读取一次 `SystemTime::now()`，与 `deadline_ms` 比较，构造相应 `Event` 送进状态机
- [x] 状态机返回的副作用被分发执行（通知、音效、面板显示等）
- [x] 每次 tick 把最新 `TimerSnapshot`（见 [`spec.md` §5.3](../spec.md)）emit 给前端，**仅在状态或剩余秒数发生变化时**
- [x] 托盘标题每秒刷新一次（`MM:SS`），非秒级变化时不重复设置
- [x] 线程在应用退出时干净停止（置标志位 + `join`，不留下游离线程）—— 注意 `Drop` 单独不够，见 Comments
- [x] 系统休眠跨越目标时刻后，唤醒时状态机立即推进到正确状态（不等到下一个 250ms tick 之后的额外延迟）
- [x] 27 个用例全绿 —— `cargo test` 52 passed / 0 failed（其中 25 个来自票 02），见 Comments

## 背景

- 这是整个 app 的心跳来源；前端只做插值渲染，见 [ADR-0002](../../../../docs/adr/0002-authoritative-clock-in-rust.md)
- 依赖 02 提供的 `TimerCore` 与 `reduce`（**02 的字段已从 `target_ms` + `next_repeat_ms` 合并为单个 `deadline_ms`**，见该票 Comments）
- 睡眠语义见 [`spec.md` §4.3](../spec.md)

## Comments

### 2026-10-23 — 27 个用例 + 跑起来看一眼

`cargo test` 52 passed / 0 failed（票 02 的 25 个 + 本票 27 个），`npm run typecheck` 无输出。另外用 `FOCUS_TIMER_SCALE=0.01` 把 60 分钟压成 36 秒真跑了一遍：`emit_state` 收到 36 帧 `Working`（每秒一帧，不是每 250ms 一帧），到点后自动转成 `WorkAlerting { deadline_ms: None }` 并弹出面板。截屏工具在这台机器上被系统挡了，所以是临时在 `emit_state` 里加 `eprintln!` 看的，看完撤掉。

### 三层，而不是一个线程

`clock.rs` 里是三层，可测性来自这个切法：

| 层 | 是什么 | 测试怎么碰它 |
|---|---|---|
| `TimerEngine` | 纯逻辑。`tick(now)` / `apply(event, now)` 收一个 `now`，吐出 `TickOutput { snapshot, tray_title, effects }` | 直接喂假时间，逐毫秒驱动，27 个用例里 19 个在这一层 |
| `Dispatcher` | 副作用的出口（通知/音效/面板/托盘/emit）。trait | 测试换成 `FakeDispatcher`，按顺序录下每一次调用 |
| `ClockThread` | 把上面两层接到真实墙钟和真实线程上 | 起真线程跑（5ms 心跳），断言"线程自己会 tick"，8 个用例 |

`TimerEngine` 不读时钟——这是它能被精确驱动的原因，也是 02 那条"纯函数"约束在这里的延续。

### 代码评审查出来的：`Drop` 在真 app 里根本不会执行

验收第六条写的是「`Drop` 里置标志位 + `join`」。照字面写完，测试也是绿的——**但真 app 里这条路一次都不会走到**。

成环的是这么一条引用链：

```
ClockThread（在 Tauri 状态表里）
  └─ Arc<dyn Dispatcher> → TauriDispatcher { app: AppHandle }
                              └─ AppHandle.manager: Arc<AppManager>
                                    └─ AppManager.state: Arc<StateManager>   ← 回到起点
```

`Arc` 只在计数归零时才析构，而这条环上的计数永远降不下来：`ClockThread` 排在状态表里，状态表排在 `AppHandle` 里，`AppHandle` 又排在 `ClockThread` 里。**`Drop` 永远等不到。** 单元测试里没有这个环（`FakeDispatcher` 不持有 `AppHandle`），所以测试通过并不能说明退出时线程会收干净——这正是它隐蔽的地方。

修法：加一个幂等的 `ClockThread::stop()`，在 `lib.rs` 的 `app.run(...)` 里挂 `RunEvent::Exit` 显式调一次。`Drop` 保留（测试和非 Tauri 场景仍然靠它）。

`stop()` 里等线程结束用了**有上限的等待**（轮询 `JoinHandle::is_finished()`，最多一个心跳），不是直接 `join`：分发副作用要经 Tauri 的事件循环，万一那一刻它正好在等主线程，而调用 `stop()` 的自己就是主线程（`RunEvent` 回调跑在事件循环线程上），`join` 就会和主线程互锁。**退出时宁可放过一个线程，也不能把 app 卡在关不掉的状态。** 超时放弃时 `JoinHandle` 还留在槽里，之后 `Drop` 仍会 join。

验证到什么程度：`the_clock_thread_can_be_stopped_without_being_dropped` 锁住了停机语义，`cargo build` 与真跑 8 秒确认 `run()` 改造后 app 照常启动。**但端到端的"点退出 → 线程收干净"没跑过**——托盘菜单（04）还没做，现在没有优雅退出的入口。这条留到 04 做完托盘菜单后补一次真实验证。

### 与验收清单的偏离 / 追加

**一、多加了 `timer_action` 命令（[`spec.md` §5.3](../spec.md) 的前端 → Rust 通道）。** 清单里没提，但没有它，06 的按钮点了没有回声，而验收又要求"状态或剩余秒数变化时才 emit"——用户点一下暂停，如果动作只能等下一个 tick 才被应用，UI 会迟 250ms。命令做的事就一件：`event_for_action("pause") → Event::Pause → clock.apply(event)`，立刻生效。顺带加了 `event_for_action`（前端动作字符串 → 状态机事件的映射）和 `app.manage(clock)`。

**二、emit 规则按本票走，与 [`spec.md` §5.3](../spec.md) / ADR-0002 的措辞不一致——需要拍板。**

- 本票验收：「**仅在状态或剩余秒数发生变化时**」
- spec §5.3：「**仅状态变化时触发**」
- ADR-0002：「**只有状态发生变化时才 emit 给前端**」

实现跟着本票走：状态或显示秒数变了就推（每秒一帧）。但要记一笔——前端的 `useNow` 本来就自己按 250ms 插值渲染（[`spec.md` §5.3](../spec.md)："前端每 250ms 用本地 `setInterval` 做插值渲染"），`deadlineMs` 一帧就够它算出每一帧的剩余时间。**改成"仅状态变化"在功能上不会少任何东西**，只省掉每秒一条 IPC 消息。三处措辞留两处不一致的账，票 04 接托盘时可以顺手定。

**三、`format_remaining` 的边界定成 `> 3600` 而不是 `> 60`。** 一小时以内 `MM:SS`；超过一小时 `H:MM:SS`（04 的要求）。**正好一小时仍写 `60:00`**——和前端 `formatDuration` 一致，免得同一时刻面板写 `1:00:00`、托盘写 `60:00`。再往上前端没有 `H:MM:SS` 的写法，但那时离得够远，两边写得不一样也无所谓。

**四、通知、音效、托盘标题的真实实现不在本票。** `Dispatcher` 的六个方法里，现在只有 `show_panel`（`window.show()`，不抢焦点）和 `emit_state` 是真的；`notify` / `play_sound` / `set_tray_alert` / `set_tray_title` 都是留好的空出口，实现分别在 05 和 04。本票只保证**"状态机说了要做什么"这件事被分发出去**——`the_thread_dispatches_the_effects_of_a_state_change` 锁住的是这一层。

**五、`WORK_ALERTING` 的托盘标题定成 `00:00`，不是"距下一次重复提醒还有多久"。** [`spec.md` §5.4](../spec.md) 只说「标题：剩余时间 `MM:SS`」，没说提醒态怎么办；§5.3 又明确 alerting 没有倒计时（`deadlineMs: null`）。两种读法都通：`00:00`，或者显示下一次重复提醒的倒计时（但那个时刻在语义上不是"剩余工作时间"）。选了前者，因为托盘标题在四个状态下都该是同一件事——**这一轮还剩多少工作时间**，而它在提醒态确实已经归零；换句话说 `00:00` 是"账已结清"，而 `09:47` 会让人以为还有时间可以继续工作。04 接托盘配色时按这个前提来。

**六、`format_remaining` 与前端 `formatDuration` 是两份格式化器，会漂。** 见上一条第三点的边界分歧：Rust 侧写 `1:00:01`，前端写 `60:01`。今天走不到（工作时长 60 分钟正好卡在边界内侧），但**一旦时长可配（比如 04 让用户自定义工作时段）就会露出来**，两边的显示会不一样。真要收敛，只有一条路：把 `formatDuration` 的规则改成 `> 3600` 才用 `H:MM:SS`，两边就此一致。

### 三条设计上值得记一笔的

- **墙钟只用 `SystemTime`，`Instant` 只用来排心跳。** 验收最后一条（休眠跨越截止时刻）靠的就是这个：macOS 上 `Instant` 不含休眠时间，休眠期间单调钟几乎不走，用它算剩余时间会把 8 小时的觉算成 0。`now` 一律在**醒来之后**才读——休眠期间读到的墙钟和醒来后读到的不是一回事。
- **分发在锁内完成。** "点暂停"和"恰好到点"撞在一起时，两个线程抢同一把锁，谁先拿到谁先分发，发出的顺序必然与状态变化的顺序一致。早先是先改状态、出了锁再分发，能构造出"前端先收到新的、再收到旧的"这种序列。
- **心跳睡过头就重新起算，不补发。** 唤醒后 `next = now + interval`，而不是把错过的几个 tick 一口气补齐——补发只会让休眠回来之后先卡几帧旧画面。

### 代码评审提了、但本票**不改**的几条判断题

留着是因为它们不是错，是取舍。写下来是为了下一个人别把它们当成漏掉的：

- **"该发哪个事件"写了两次**：`state.rs::reduce` 里的 `now >= deadline_ms` 守卫，和 `clock.rs::time_event` 里的状态→事件对照表。加一个状态要改两个文件。没合并是因为分工清楚：`reduce` 管"这个事件在这个状态下合不合法"，`time_event` 管"此刻该试哪个事件"——后者必须先回答"此刻是什么状态"才能决定试谁，而这是纯查询，混进 `reduce` 会让纯函数变成"猜"。真要收敛，接口得重设计成 `reduce(core, now, cfg)` 自己算出该走哪条边。
- **加一个 `Effect` 变体要改五处**：`state.rs`（产出）、`Dispatcher` trait、`dispatch()` 的 match、`TauriDispatcher`、`FakeDispatcher`。trait 接缝的固有代价，值这个价——不接缝就没法在测试里断言副作用。
- **`event_for_action`（前端动作字符串 → `Event`）住在 `clock.rs`**，严格说它属于 IPC 层、该在 `lib.rs`。没搬是因为搬过去就要给 `lib.rs` 开一个 `#[cfg(test)]` 模块为一个纯函数，代价大于收益。它映射的是 `state.rs` 的 `Event`、被 `ClockThread::apply` 用，放哪儿都说得通。
- **`dispatch()` 里"`SetTrayAlert` 最后生效"这句保证，落在 `state.rs` 的副作用顺序上**——谁在 `reduce` 里把 `SetTrayAlert` 往前挪，这句注释就悄悄过期了。真正由 `dispatch` 自己保证的只有"标题先于所有副作用"，这一半不依赖任何人。
- **`action` 传的是 `String`**，未知动作在 `lib.rs` 里被静默丢掉（`event_for_action` 返回 `None`）。前端 `src/types.ts` 有 `TimerAction` 联合类型挡着，Rust 侧没有对应物。给 Rust 加一个 `serde` 枚举能拿到同样的类型安全，但那属于票 05/06 打磨 IPC 时的活，现在加是提前。

### 2026-10-23（事后）— 上面第二、第六条已拍板

**第二条（emit 规则）**：用户定了**保留本票的实现**，改 spec 与 ADR-0002 的措辞。理由是前端用 `Date.now()`、Rust 用 `SystemTime::now()`，两个时钟可能差几秒；每秒一帧相当于持续对时，改成仅状态变化就没有纠偏机制了。spec §5.3、ADR-0002 已改，顺带修掉了 §5.3 里残留的 `targetMs` 旧名。

**第六条（两份格式化器）**：用户定了**两边统一成 `> 3600`**，前端 `formatDuration` 的阈值从 `> 60` 改成 `> 3600`，与 `format_remaining` 逐字一致。spec §5.4 补了一句"改一处就要改另一处"。

跨语言对拍：用 Rust 侧已有的断言（`clock.rs` 里的 `59→00:59`、`60→01:00`、`3600→60:00`、`3601→1:00:01`、`3661→1:01:01`）跑前端 `formatDuration`，五项全对。
