//! Power control for the bench CMU. Interface only: switching power is done outside
//! this crate by the Jetson bench service or the XIAO supervisor. Nothing here touches
//! GPIO; [`MockPower`] exists for tests.

use crate::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerState {
    On,
    Off,
    Unknown,
}

pub trait PowerControl {
    fn state(&mut self) -> Result<PowerState>;
    fn set(&mut self, on: bool) -> Result<()>;
}

/// Records requested transitions; never touches hardware.
#[derive(Debug, Default)]
pub struct MockPower {
    pub on: Option<bool>,
    pub history: Vec<bool>,
}

impl PowerControl for MockPower {
    fn state(&mut self) -> Result<PowerState> {
        Ok(match self.on {
            Some(true) => PowerState::On,
            Some(false) => PowerState::Off,
            None => PowerState::Unknown,
        })
    }

    fn set(&mut self, on: bool) -> Result<()> {
        self.on = Some(on);
        self.history.push(on);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_records_transitions() {
        let mut p = MockPower::default();
        assert_eq!(p.state().unwrap(), PowerState::Unknown);
        p.set(true).unwrap();
        p.set(false).unwrap();
        assert_eq!(p.state().unwrap(), PowerState::Off);
        assert_eq!(p.history, [true, false]);
    }
}
