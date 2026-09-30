#[derive(Default)]
pub struct Selection {
    pub index: usize,
    pub len: usize,
}

impl Selection {
    pub fn reset(&mut self, len: usize) {
        self.len = len;
        self.index = 0;
    }
    pub fn move_by(&mut self, delta: isize) {
        self.index = self
            .index
            .saturating_add_signed(delta)
            .min(self.len.saturating_sub(1));
    }
    pub fn selected(&self) -> Option<usize> {
        (self.len > 0).then_some(self.index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_stays_inside_results() {
        let mut selection = Selection::default();
        selection.reset(3);
        assert_eq!(selection.selected(), Some(0));
        selection.move_by(-1);
        assert_eq!(selection.selected(), Some(0));
        selection.move_by(20);
        assert_eq!(selection.selected(), Some(2));
        selection.move_by(-1);
        assert_eq!(selection.selected(), Some(1));
    }

    #[test]
    fn new_query_cannot_keep_an_old_selection() {
        let mut selection = Selection::default();
        selection.reset(10);
        selection.move_by(8);
        selection.reset(2);
        assert_eq!(selection.selected(), Some(0));
        selection.reset(0);
        selection.move_by(1);
        assert_eq!(selection.selected(), None);
    }
}
