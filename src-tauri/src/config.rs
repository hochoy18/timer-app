//! 时长配置。
//!
//! 时长在产品上是**写死的**（用户明确决定：先写死 60 / 15 / 10）。
//! 但开发期需要一个加速开关来快速验证整个轮次——那不属于用户可见配置，
//! 因此只在 debug 构建里读环境变量。

/// 开发期加速开关的环境变量名。
///
/// 这是一纸给开发者的契约：`FOCUS_TIMER_SCALE=0.01 npm run tauri dev`。
/// 改名字等于改契约——用它的脚本会静默失效（退回到真实时长，60 分钟一轮）。
pub const SCALE_ENV_VAR: &str = "FOCUS_TIMER_SCALE";

/// 三个时长的配置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimerConfig {
    /// 工作时段时长。
    pub work_ms: u64,
    /// 休息时段时长。
    pub break_ms: u64,
    /// 待确认提醒的重复间隔。
    pub repeat_ms: u64,
}

impl std::fmt::Display for TimerConfig {
    /// 把三个时长说成一句人能读的话。
    ///
    /// "工作时段 60 分钟 / 休息时段 15 分钟 / 重复提醒 10 分钟"——这句话在
    /// **两种构建**里都会打在 stderr 上（见 `lib.rs` 的 `announce_durations`）。
    /// 不说的话，应用看起来和平常一模一样，只有托盘上的数字变短了。
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "工作时段 {} / 休息时段 {} / 重复提醒 {}",
            human_ms(self.work_ms),
            human_ms(self.break_ms),
            human_ms(self.repeat_ms)
        )
    }
}

/// 毫秒 → 一句人能读的话。
///
/// 不到一秒就说毫秒：说"0 秒"比什么都不说更让人困惑。
/// 到了分钟就说分钟——"工作时段 3600 秒"没人能一眼看出那是**一小时**，
/// 而打包版启动时打印的就是这句话（见 `lib.rs` 里的 `announce_durations`）：
/// 那行字是"这个包用的是真实时长"的凭据，得一眼看得懂。
///
/// 全部用整数算：浮点秒数会把"30.5 秒"这种小数漏进给人看的字里。
fn human_ms(ms: u64) -> String {
    let secs = ms / 1000;
    if secs == 0 {
        return format!("{ms} 毫秒");
    }
    if secs < 60 {
        return format!("{secs} 秒");
    }
    let mins = secs / 60;
    let rest = secs % 60;
    if rest == 0 {
        format!("{mins} 分钟")
    } else {
        format!("{mins} 分 {rest} 秒")
    }
}

/// 缩放后的时长上限。
///
/// 取 `u64::MAX / 4`：离 `u64::MAX` 足够远，`now + 时长` 永远不会溢出。
/// 手滑填出来一个巨大的倍率，不该让应用 panic。
const MAX_PHASE_MS: u64 = u64::MAX / 4;

impl Default for TimerConfig {
    fn default() -> Self {
        Self {
            work_ms: 60 * 60 * 1000,
            break_ms: 15 * 60 * 1000,
            repeat_ms: 10 * 60 * 1000,
        }
    }
}

impl TimerConfig {
    /// 按倍率缩放三个时长。
    ///
    /// 倍率不是"有限且大于 0"的数（`inf`、`NaN`、0、负数）时**原样返回**：
    /// 一个猜错的倍率不该让计时器变得不可用。
    ///
    /// 缩放后至少留 1ms：0 长的时段会立刻到期，状态机会在原地打转。
    ///
    /// 缩放后至多 `MAX_PHASE_MS`：倍率大到离谱时（`1e13`）`as u64` 会饱和成
    /// `u64::MAX`，状态机里的 `now + work_ms` 随即溢出——debug 构建直接 panic。
    /// 封顶之后，"巨大倍率"的后果只是这一轮永远走不到头，而不是应用崩掉。
    pub fn scaled(&self, scale: f64) -> Self {
        if !(scale.is_finite() && scale > 0.0) {
            return *self;
        }
        let f = |ms: u64| (((ms as f64) * scale).round() as u64).clamp(1, MAX_PHASE_MS);
        Self {
            work_ms: f(self.work_ms),
            break_ms: f(self.break_ms),
            repeat_ms: f(self.repeat_ms),
        }
    }

    /// 按环境变量的**原始值**建配置（纯函数，不碰进程环境）。
    ///
    /// 值不是数字（`abc`）、不是正数（`0`、`-1`）、或者压根没有值——一律当作
    /// "没有倍率"，用真实时长。
    pub fn from_scale_value(raw: Option<&str>) -> Self {
        let default = Self::default();
        match raw.and_then(|r| r.trim().parse::<f64>().ok()) {
            Some(scale) => default.scaled(scale),
            None => default,
        }
    }

    /// 读取开发期加速开关；release 构建恒为真实时长。
    ///
    /// 环境变量 `FOCUS_TIMER_SCALE` 是一个倍率，例如 `0.01` 把 60 分钟变成 36 秒。
    /// 非数字或非正数的值一律忽略。
    ///
    /// **release 构建完全不看这个变量**——不是"看了但不用"，是那段代码根本
    /// 没被编进去（`#[cfg(debug_assertions)]`）。这样打包版不可能因为谁在
    /// 启动时手滑带了个环境变量，就跑出 36 秒一轮的怪事。
    pub fn from_env_or_default() -> Self {
        #[cfg(debug_assertions)]
        {
            Self::from_scale_value(std::env::var(SCALE_ENV_VAR).ok().as_deref())
        }
        #[cfg(not(debug_assertions))]
        {
            Self::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_spec() {
        let c = TimerConfig::default();
        assert_eq!(c.work_ms, 60 * 60 * 1000);
        assert_eq!(c.break_ms, 15 * 60 * 1000);
        assert_eq!(c.repeat_ms, 10 * 60 * 1000);
    }

    #[test]
    fn scaling_shortens_all_three_durations() {
        let scaled = TimerConfig::default().scaled(0.01);
        assert_eq!(scaled.work_ms, 36_000);
        assert_eq!(scaled.break_ms, 9_000);
        assert_eq!(scaled.repeat_ms, 6_000);
    }

    #[test]
    fn a_scale_of_one_changes_nothing() {
        assert_eq!(TimerConfig::default().scaled(1.0), TimerConfig::default());
    }

    #[test]
    fn the_scale_covers_work_break_and_repeat() {
        // 三个时长是一起缩的——漏掉哪一个，那一段就会按真实时长跑，
        // 半分钟加速的验证就白搭了。
        let scaled = TimerConfig::default().scaled(0.5);
        assert_eq!(scaled.work_ms, 30 * 60 * 1000);
        assert_eq!(scaled.break_ms, 7 * 60 * 1000 + 30 * 1000);
        assert_eq!(scaled.repeat_ms, 5 * 60 * 1000);
    }

    #[test]
    fn an_unusable_scale_leaves_the_durations_alone() {
        // `NaN` 算出来的时长是 0，`inf` 算出来是 u64 的顶，0 和负数则根本
        // 不该被当成倍率。四种都当作没有倍率。
        for scale in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.0, -1.0] {
            assert_eq!(
                TimerConfig::default().scaled(scale),
                TimerConfig::default(),
                "倍率 {scale} 不该改动时长"
            );
        }
    }

    #[test]
    fn a_phase_is_never_shorter_than_a_millisecond() {
        // 0 长的时段会立刻到期，状态机会在原地打转。
        let scaled = TimerConfig::default().scaled(1e-12);
        assert_eq!(scaled.work_ms, 1);
        assert_eq!(scaled.break_ms, 1);
        assert_eq!(scaled.repeat_ms, 1);
    }

    #[test]
    fn an_enormous_scale_is_capped_instead_of_overflowing() {
        // 巨大倍率会让 `as u64` 饱和成 u64::MAX，状态机里的
        // `now + work_ms` 随即溢出——debug 构建直接 panic。
        // 封顶之后，巨大倍率的后果只是"这一轮永远走不到头"。
        let scaled = TimerConfig::default().scaled(1e13);
        assert_eq!(scaled.work_ms, MAX_PHASE_MS);
        assert_eq!(scaled.break_ms, MAX_PHASE_MS);
        assert_eq!(scaled.repeat_ms, MAX_PHASE_MS);

        // 封顶值离 u64::MAX 足够远：加上当前的时刻也不会溢出。
        let now = 1_700_000_000_000_u64;
        assert!(now.checked_add(scaled.work_ms).is_some());
    }

    #[test]
    fn the_config_describes_itself_in_human_terms() {
        // 开发期加速时终端上会打出这一句：数字得是人能一眼看懂的。
        assert_eq!(
            TimerConfig::default().scaled(0.01).to_string(),
            "工作时段 36 秒 / 休息时段 9 秒 / 重复提醒 6 秒"
        );
    }

    #[test]
    fn the_real_durations_are_spoken_in_minutes() {
        // 打包版启动时打的是这一句（见 `lib.rs` 的 `announce_durations`）。
        // "3600 秒"没人能一眼看出那是**一小时**——这行字是"这个包用的是真实
        // 时长"的凭据（票 10 的验收），得一眼看得懂。
        assert_eq!(
            TimerConfig::default().to_string(),
            "工作时段 60 分钟 / 休息时段 15 分钟 / 重复提醒 10 分钟"
        );
    }

    #[test]
    fn a_mixed_length_is_spoken_in_both_units() {
        // 不是整分钟的时长（产品上不存在，但格式化器不该把它说成"0 分钟"）。
        assert_eq!(human_ms(90_000), "1 分 30 秒");
    }

    #[test]
    fn a_sub_second_length_is_spoken_in_milliseconds() {
        // 不到一秒就说毫秒：说"0 秒"比什么都不说更让人困惑。
        assert_eq!(human_ms(999), "999 毫秒");
    }

    #[test]
    fn fractions_of_a_second_are_dropped() {
        // 计时器看的是秒，毫秒那一截不进话里——不然会印出"30.5 秒"。
        assert_eq!(human_ms(90_500), "1 分 30 秒");
    }

    // ── 环境变量的值 ────────────────────────────────────────────────────

    #[test]
    fn a_scale_value_scales_all_three_durations() {
        let c = TimerConfig::from_scale_value(Some("0.01"));
        assert_eq!(c.work_ms, 36_000);
        assert_eq!(c.break_ms, 9_000);
        assert_eq!(c.repeat_ms, 6_000);
    }

    #[test]
    fn no_scale_value_means_the_real_durations() {
        assert_eq!(TimerConfig::from_scale_value(None), TimerConfig::default());
    }

    #[test]
    fn a_scale_value_that_is_not_a_number_is_ignored() {
        for raw in ["", "abc", "0.01x", "1,2", "分钟"] {
            assert_eq!(
                TimerConfig::from_scale_value(Some(raw)),
                TimerConfig::default(),
                "值 {raw:?} 不该被当成倍率"
            );
        }
    }

    #[test]
    fn a_scale_value_that_is_not_positive_is_ignored() {
        for raw in ["0", "0.0", "-1", "-0.5"] {
            assert_eq!(
                TimerConfig::from_scale_value(Some(raw)),
                TimerConfig::default(),
                "值 {raw:?} 不该被当成倍率"
            );
        }
    }

    #[test]
    fn a_scale_value_with_spaces_around_it_still_works() {
        // 抄环境变量时最容易带上空格。
        let c = TimerConfig::from_scale_value(Some("  0.01\n"));
        assert_eq!(c.work_ms, 36_000);
    }

    #[test]
    fn release_builds_ignore_the_env_var() {
        // release 构建里 from_env_or_default 必须是恒定的真实时长。
        // 这个测试只在 debug 下有意义，所以用 cfg 包起来。
        #[cfg(not(debug_assertions))]
        {
            let c = TimerConfig::from_env_or_default();
            assert_eq!(c, TimerConfig::default());
        }
    }
}
