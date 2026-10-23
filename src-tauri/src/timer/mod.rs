pub mod clock;
pub mod state;

/// 测试用的公共脚手架：两个测试模块都要"一分钟"和"规格里的三个时长"。
#[cfg(test)]
pub(crate) mod test_support {
    use crate::config::TimerConfig;

    /// 一分钟（毫秒）。
    pub const MIN: u64 = 60_000;

    /// 规格里的三个时长：工作 60 分钟 / 休息 15 分钟 / 重复提醒 10 分钟。
    pub fn cfg() -> TimerConfig {
        TimerConfig {
            work_ms: 60 * MIN,
            break_ms: 15 * MIN,
            repeat_ms: 10 * MIN,
        }
    }
}
