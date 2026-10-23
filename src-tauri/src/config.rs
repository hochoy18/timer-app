//! 时长配置。
//!
//! 时长在产品上是**写死的**（用户明确决定：先写死 60 / 15 / 10）。
//! 但开发期需要一个加速开关来快速验证整个轮次循环——那不属于用户可见配置，
//! 因此只在 debug 构建里读环境变量。

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
    pub fn scaled(&self, scale: f64) -> Self {
        let f = |ms: u64| ((ms as f64) * scale).round() as u64;
        Self {
            work_ms: f(self.work_ms),
            break_ms: f(self.break_ms),
            repeat_ms: f(self.repeat_ms),
        }
    }

    /// 读取开发期加速开关；release 构建恒为真实时长。
    ///
    /// 环境变量 `FOCUS_TIMER_SCALE` 是一个倍率，例如 `0.01` 把 60 分钟变成 36 秒。
    /// 非数字或非正数的值一律忽略。
    pub fn from_env_or_default() -> Self {
        #[cfg(debug_assertions)]
        {
            let default = Self::default();
            let scale = std::env::var("FOCUS_TIMER_SCALE")
                .ok()
                .and_then(|raw| raw.trim().parse::<f64>().ok())
                .filter(|s| *s > 0.0);
            match scale {
                Some(s) => default.scaled(s),
                None => default,
            }
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
