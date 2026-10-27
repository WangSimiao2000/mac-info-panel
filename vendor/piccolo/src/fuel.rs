/// A checked counter for conservatively weighted executor work.
///
/// VM opcodes, callbacks, sequences, and executor steps have fixed weights. A
/// caller must reserve a complete weight with [`Fuel::try_consume`] before the
/// corresponding operation mutates observable state. Variable work inside an
/// already funded callback or opcode is bounded separately by the embedder's
/// wall-time and memory limits.
#[derive(Debug, Clone)]
pub struct Fuel {
    fuel: i32,
    interrupted: bool,
}

impl Fuel {
    pub fn empty() -> Self {
        Self::with(0)
    }

    pub fn with(fuel: i32) -> Self {
        Self {
            fuel: fuel.max(0),
            interrupted: false,
        }
    }

    /// Refills fuel up to a given maximum and also clears the fuel interrupt flag.
    ///
    /// This is a convenience method that is intended to be called outside of a call to
    /// `Executor::step` in some loop. It does the operations that all loops that would *re-use* a
    /// fuel container from one tick to another would need to do.
    ///
    /// It credits the running thread with fuel while also preventing available fuel from growing to
    /// an unbounded level, and also clears the interrupt flag since we are now on the "outside" of
    /// whatever lua code is being run and this is where we would want to interrupt to.
    pub fn refill(&mut self, fuel: i32, max_fuel: i32) {
        self.fuel = self.fuel.saturating_add(fuel).min(max_fuel.max(0)).max(0);
        self.interrupted = false;
    }

    /// Add to or subtract from the current remaining fuel.
    pub fn adjust(&mut self, fuel: i32) {
        self.fuel = self.fuel.saturating_add(fuel).max(0);
    }

    /// Returns whether `fuel` can be consumed without going negative.
    pub fn can_consume(&self, fuel: i32) -> bool {
        fuel >= 0 && !self.interrupted && self.fuel >= fuel
    }

    /// Atomically consumes `fuel` only when the complete amount is available.
    /// Returns false without changing the balance when interrupted or underfunded.
    pub fn try_consume(&mut self, fuel: i32) -> bool {
        if !self.can_consume(fuel) {
            return false;
        }
        self.fuel -= fuel;
        true
    }

    /// Subtract from the current remaining fuel without allowing debt.
    ///
    /// New execution paths should use [`Fuel::try_consume`] before performing
    /// observable work. This compatibility method saturates at zero.
    pub fn consume(&mut self, fuel: i32) {
        if fuel > 0 {
            self.fuel = self.fuel.saturating_sub(fuel).max(0);
        }
    }

    pub fn remaining(&self) -> i32 {
        self.fuel
    }

    pub fn set_remaining(&mut self, fuel: i32) {
        self.fuel = fuel.max(0);
    }

    /// Marks that the calling `Executor` should immediately stop executing, without actually
    /// consuming any fuel.
    ///
    /// This is useful in situations where you need to interrupt a running `Executor` because it
    /// may be waiting on an external event that has not yet occurred, but still want to record the
    /// correct fuel usage.
    pub fn interrupt(&mut self) {
        self.interrupted = true;
    }

    pub fn is_interrupted(&self) -> bool {
        self.interrupted
    }

    pub fn clear_interrupt(&mut self) {
        self.interrupted = false;
    }

    /// Returns true if we have positive fuel remaining *and* we have not been interrupted.
    pub fn should_continue(&self) -> bool {
        self.fuel > 0 && !self.interrupted
    }
}
