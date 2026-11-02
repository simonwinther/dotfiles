//! Reuse matcher scratch space and parsed queries within each search worker.
use nucleo_matcher::{
    Matcher, Utf32Str,
    pattern::{AtomKind, CaseMatching, Normalization, Pattern},
};
use std::cell::RefCell;

struct State {
    query: String,
    exact: bool,
    pattern: Pattern,
    matcher: Matcher,
    buffer: Vec<char>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            query: String::new(),
            exact: false,
            pattern: Pattern::new(
                "",
                CaseMatching::Ignore,
                Normalization::Smart,
                AtomKind::Fuzzy,
            ),
            matcher: Matcher::new(nucleo_matcher::Config::DEFAULT),
            buffer: vec![],
        }
    }
}
thread_local! { static STATE: RefCell<State> = RefCell::default(); }

pub fn score(query: &str, text: &str, exact: bool) -> Option<u32> {
    STATE.with_borrow_mut(|state| {
        if state.query != query || state.exact != exact {
            state.query.clear();
            state.query.push_str(query);
            state.exact = exact;
            state.pattern = Pattern::new(
                query,
                CaseMatching::Ignore,
                Normalization::Smart,
                if exact {
                    AtomKind::Substring
                } else {
                    AtomKind::Fuzzy
                },
            );
        }
        state
            .pattern
            .score(Utf32Str::new(text, &mut state.buffer), &mut state.matcher)
    })
}

#[allow(dead_code)]
pub fn indices(query: &str, text: &str, exact: bool) -> Option<(i32, Vec<i32>)> {
    score(query, text, exact)?;
    STATE.with_borrow_mut(|state| {
        let mut indices = vec![];
        let score = state.pattern.indices(
            Utf32Str::new(text, &mut state.buffer),
            &mut state.matcher,
            &mut indices,
        )?;
        indices.sort_unstable();
        indices.dedup();
        Some((
            score as i32,
            indices.into_iter().map(|index| index as i32).collect(),
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn abbreviations_rank_word_boundaries_and_keep_literal_characters() {
        assert!(score("bluec", "Bluetooth controls", false).is_some());
        assert!(score("bf", "Firefox Browser", false).is_none());
        assert!(score("fb", "Firefox Browser", true).is_none());
        assert_eq!(
            indices("$HOME;", "Literal $HOME; name", true).unwrap().1,
            vec![8, 9, 10, 11, 12, 13]
        );
        assert!(score("cafe", "Café", false).is_some());
    }
}
