use std::time::Duration;

pub enum Progress {
    Idle,
    Ready,
    Waiting(Duration),
    Blocked,
}
