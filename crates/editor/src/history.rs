//! Undo and redo: a stack of whole editing states. A state is small (at most 128 × 128 tile words plus the units),
//! so a copy per step is simpler than recording each change and costs a few megabytes at most.

use crate::doc::State;

/// How many steps undo keeps.
pub const DEPTH: usize = 200;

#[derive(Clone, Debug, Default)]
pub struct History {
    undo: Vec<State>,
    redo: Vec<State>,
}

impl History {
    /// Records `before`, the state as it was before an edit. Redo steps are dropped.
    pub fn push(&mut self, before: State) {
        if self.undo.last() == Some(&before) {
            return;
        }
        self.undo.push(before);
        if self.undo.len() > DEPTH {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// The state to go back to, given the current one (kept for redo).
    pub fn undo(&mut self, current: &State) -> Option<State> {
        let s = self.undo.pop()?;
        self.redo.push(current.clone());
        Some(s)
    }

    pub fn redo(&mut self, current: &State) -> Option<State> {
        let s = self.redo.pop()?;
        self.undo.push(current.clone());
        Some(s)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(d: &str) -> State {
        State {
            tiles: vec![0; 4],
            units: Vec::new(),
            description: d.into(),
            owners: [3; 16],
            sides: [0; 16],
            gold: [0; 16],
            lumber: [0; 16],
            oil: [0; 16],
            ai: None,
        }
    }

    #[test]
    fn undo_and_redo_walk_the_steps() {
        let mut h = History::default();
        let (a, b, c) = (state("a"), state("b"), state("c"));
        h.push(a.clone());
        h.push(b.clone());
        // Current is c.
        assert_eq!(h.undo(&c), Some(b.clone()));
        assert_eq!(h.undo(&b), Some(a.clone()));
        assert_eq!(h.undo(&a), None);
        assert_eq!(h.redo(&a), Some(b.clone()));
        assert_eq!(h.redo(&b), Some(c.clone()));
        assert!(!h.can_redo());
        // A new edit drops the redo steps.
        h.undo(&c);
        h.push(b.clone());
        assert!(!h.can_redo());
    }
}
