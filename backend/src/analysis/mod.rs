//! 验证分析:全部为纯函数,输入遥测镜像与冻结配置,输出发现。
//! 不计算杀菌设定值,不输出任何控制动作。
pub mod checks;
pub mod passage;

pub use checks::*;
pub use passage::*;
