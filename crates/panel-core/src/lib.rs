use panel_model::PageId;
use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NavigationState {
    pages: Vec<PageId>,
    selected: usize,
}

impl NavigationState {
    pub fn new(pages: Vec<PageId>, initial_page: &PageId) -> Result<Self, NavigationError> {
        if pages.is_empty() {
            return Err(NavigationError::NoPages);
        }
        let selected = pages
            .iter()
            .position(|page| page == initial_page)
            .ok_or_else(|| NavigationError::UnknownInitialPage(initial_page.as_str().into()))?;
        Ok(Self { pages, selected })
    }

    pub fn current(&self) -> &PageId {
        &self.pages[self.selected]
    }

    pub fn next_page(&mut self) -> &PageId {
        self.selected = (self.selected + 1) % self.pages.len();
        self.current()
    }

    pub fn previous_page(&mut self) -> &PageId {
        self.selected = self.selected.checked_sub(1).unwrap_or(self.pages.len() - 1);
        self.current()
    }

    pub fn select(&mut self, page: &PageId) -> Result<(), NavigationError> {
        self.selected = self
            .pages
            .iter()
            .position(|candidate| candidate == page)
            .ok_or_else(|| NavigationError::UnknownPage(page.as_str().into()))?;
        Ok(())
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum NavigationError {
    #[error("at least one page is required")]
    NoPages,
    #[error("initial page does not exist: {0}")]
    UnknownInitialPage(String),
    #[error("page does not exist: {0}")]
    UnknownPage(String),
}

pub fn checked_counter_delta(current: u64, previous: u64) -> Option<u64> {
    current.checked_sub(previous)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(value: &str) -> PageId {
        PageId::new(value).unwrap()
    }

    #[test]
    fn rejects_unknown_initial_page() {
        assert_eq!(
            NavigationState::new(vec![id("overview")], &id("missing")),
            Err(NavigationError::UnknownInitialPage("missing".into()))
        );
    }

    #[test]
    fn wraps_navigation_in_both_directions() {
        let mut state = NavigationState::new(
            vec![id("overview"), id("lyrics"), id("local")],
            &id("overview"),
        )
        .unwrap();
        assert_eq!(state.previous_page().as_str(), "local");
        assert_eq!(state.next_page().as_str(), "overview");
    }

    #[test]
    fn treats_decreasing_counter_as_reset() {
        assert_eq!(checked_counter_delta(5, 10), None);
        assert_eq!(checked_counter_delta(15, 10), Some(5));
    }
}
