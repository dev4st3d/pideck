#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct AppState {
    count: i32,
}

impl AppState {
    pub fn count(&self) -> i32 {
        self.count
    }

    pub fn increment(&mut self) {
        self.count += 1;
    }

    pub fn reset(&mut self) {
        self.count = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::AppState;

    #[test]
    fn counter_can_increment_and_reset() {
        let mut state = AppState::default();
        state.increment();
        state.increment();
        assert_eq!(state.count(), 2);
        state.reset();
        assert_eq!(state.count(), 0);
    }
}
