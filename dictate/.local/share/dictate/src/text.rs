use crate::config::Config;
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    sync::LazyLock,
};

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dictionary {
    #[serde(default)]
    pub terms: Vec<String>,
    #[serde(default)]
    pub replacements: BTreeMap<String, String>,
}
pub struct Processor {
    pub dictionary: Dictionary,
    corrections: Phrases,
    mode: String,
    cleanup: bool,
}

#[derive(Default)]
struct Node {
    children: BTreeMap<char, usize>,
    target: Option<String>,
}
/// Literal, case-insensitive phrase matching without compiling Unicode regexes.
/// Each replacement is applied once; the longest valid phrase at a position wins.
struct Phrases {
    nodes: Vec<Node>,
    unicode_spaces: bool,
}
fn word(c: char) -> bool {
    c.is_alphanumeric()
        || c == '_'
        || matches!(c,'\u{0300}'..='\u{036f}'|'\u{1ab0}'..='\u{1aff}'|'\u{1dc0}'..='\u{1dff}'|'\u{20d0}'..='\u{20ff}'|'\u{fe20}'..='\u{fe2f}')
}
impl Phrases {
    fn new<'a>(pairs: impl IntoIterator<Item = (&'a str, &'a str)>, unicode_spaces: bool) -> Self {
        let mut result = Self {
            nodes: vec![Node::default()],
            unicode_spaces,
        };
        for (source, target) in pairs {
            let key = source
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .to_lowercase();
            let mut index = 0;
            for c in key.chars() {
                let child = if let Some(&child) = result.nodes[index].children.get(&c) {
                    child
                } else {
                    let child = result.nodes.len();
                    result.nodes.push(Node::default());
                    result.nodes[index].children.insert(c, child);
                    child
                };
                index = child;
            }
            result.nodes[index].target = Some(target.into());
        }
        result
    }
    fn space(&self, c: char) -> bool {
        if self.unicode_spaces {
            c.is_whitespace() && c != '\n' && c != '\r'
        } else {
            matches!(c, ' ' | '\t')
        }
    }
    fn replace(&self, source: &str) -> String {
        if self.nodes[0].children.is_empty() {
            return source.into();
        }
        let mut output = String::with_capacity(source.len());
        let mut consumed = 0;
        for (start, _) in source.char_indices() {
            if start < consumed || source[..start].chars().next_back().is_some_and(word) {
                continue;
            }
            let mut input = source[start..].char_indices().peekable();
            let mut index = 0;
            let mut best = None;
            'phrase: while let Some((offset, c)) = input.next() {
                let mut end = start + offset + c.len_utf8();
                let c = if self.space(c) {
                    while let Some(&(offset, next)) = input.peek() {
                        if !self.space(next) {
                            break;
                        }
                        input.next();
                        end = start + offset + next.len_utf8();
                    }
                    ' '
                } else {
                    c
                };
                for folded in c.to_lowercase() {
                    let Some(&child) = self.nodes[index].children.get(&folded) else {
                        break 'phrase;
                    };
                    index = child;
                }
                if let Some(target) = &self.nodes[index].target
                    && !source[end..].chars().next().is_some_and(word)
                {
                    best = Some((end, target));
                }
            }
            if let Some((end, target)) = best {
                output.push_str(&source[consumed..start]);
                output.push_str(target);
                consumed = end;
            }
        }
        output.push_str(&source[consumed..]);
        output
    }
}

const COMMANDS: &[(&str, &str)] = &[
    ("exclamation point", "!"),
    ("exclamation mark", "!"),
    ("new paragraph", "\n\n"),
    ("question mark", "?"),
    ("nyt afsnit", "\n\n"),
    ("full stop", "."),
    ("new line", "\n"),
    ("ny linje", "\n"),
    ("spørgsmålstegn", "?"),
    ("udråbstegn", "!"),
    ("semicolon", ";"),
    ("semikolon", ";"),
    ("comma", ","),
    ("period", "."),
    ("colon", ":"),
    ("komma", ","),
    ("punktum", "."),
    ("kolon", ":"),
];
static SPOKEN: LazyLock<Phrases> = LazyLock::new(|| Phrases::new(COMMANDS.iter().copied(), true));
static EXPLICIT: LazyLock<Phrases> = LazyLock::new(|| {
    let pairs: Vec<_> = COMMANDS
        .iter()
        .flat_map(|(phrase, target)| {
            ["punctuation", "tegn"]
                .into_iter()
                .map(move |prefix| (format!("{prefix} {phrase}"), *target))
        })
        .collect();
    Phrases::new(
        pairs
            .iter()
            .map(|(source, target)| (source.as_str(), *target)),
        true,
    )
});
static CONTRACTIONS: LazyLock<Phrases> = LazyLock::new(|| {
    Phrases::new(
        [
            ("i'm", "I'm"),
            ("i've", "I've"),
            ("i'll", "I'll"),
            ("i'd", "I'd"),
            ("i’m", "I’m"),
            ("i’ve", "I’ve"),
            ("i’ll", "I’ll"),
            ("i’d", "I’d"),
        ],
        false,
    )
});

impl Processor {
    pub fn load(c: &Config) -> Result<Self> {
        let dictionary = if c.get("DICTIONARY_FILE").is_empty() {
            Dictionary::default()
        } else {
            let path = c.path("DICTIONARY_FILE");
            match fs::read_to_string(&path) {
                Ok(text) => toml::from_str(&text)
                    .with_context(|| format!("Invalid dictionary {}", path.display()))?,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Dictionary::default(),
                Err(e) => return Err(e.into()),
            }
        };
        for term in &dictionary.terms {
            ensure!(
                !term.trim().is_empty() && !term.contains(['<', '>', '\r', '\n']),
                "Dictionary terms must be nonempty single lines without < or >"
            );
        }
        let mut seen = BTreeSet::new();
        for source in dictionary.replacements.keys() {
            ensure!(
                !source.trim().is_empty() && !source.contains(['\r', '\n']),
                "Invalid dictionary replacement phrase"
            );
            ensure!(
                seen.insert(
                    source
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ")
                        .to_lowercase()
                ),
                "Duplicate dictionary replacement: {source}"
            );
        }
        let corrections = Phrases::new(
            dictionary
                .replacements
                .iter()
                .map(|(source, target)| (source.as_str(), target.as_str())),
            false,
        );
        // Build tiny static tries before capture, with no work deferred to the first delta.
        match c.get("PUNCTUATION_MODE") {
            "spoken" => {
                LazyLock::force(&SPOKEN);
            }
            "explicit" => {
                LazyLock::force(&EXPLICIT);
            }
            _ => (),
        }
        if c.yes("TEXT_CLEANUP") {
            LazyLock::force(&CONTRACTIONS);
        }
        Ok(Self {
            dictionary,
            corrections,
            mode: c.get("PUNCTUATION_MODE").into(),
            cleanup: c.yes("TEXT_CLEANUP"),
        })
    }
    pub fn preview(&self, text: &str) -> String {
        punctuation(text, &self.mode)
    }
    pub fn process(&self, text: &str) -> String {
        let mut text = self.preview(text);
        if self.cleanup {
            let mut collapsed = String::with_capacity(text.len());
            let mut spacing = false;
            for c in text.chars() {
                if matches!(c, ' ' | '\t') {
                    if !spacing {
                        collapsed.push(' ');
                    }
                    spacing = true;
                } else {
                    collapsed.push(c);
                    spacing = false;
                }
            }
            text = CONTRACTIONS.replace(&collapsed);
        }
        self.corrections.replace(&text).trim().to_owned()
    }
}
pub fn punctuation(text: &str, mode: &str) -> String {
    if mode == "off" {
        return text.into();
    }
    let text = if mode == "explicit" {
        EXPLICIT.replace(text)
    } else {
        SPOKEN.replace(text)
    };
    let mut output = String::with_capacity(text.len());
    let mut after_break = false;
    for c in text.chars() {
        if matches!(c, ',' | '.' | ':' | ';' | '!' | '?' | '\n') {
            let trim = |tail: char| {
                if c == '\n' {
                    matches!(tail, ' ' | '\t')
                } else {
                    tail.is_whitespace() && tail != '\n' && tail != '\r'
                }
            };
            output.truncate(output.trim_end_matches(trim).len());
        }
        if after_break && matches!(c, ' ' | '\t') {
            continue;
        }
        output.push(c);
        after_break = c == '\n';
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scientific_literal_words() {
        assert_eq!(
            punctuation(
                "training period punctuation comma colon tegn ny linje next",
                "explicit"
            ),
            "training period, colon\nnext"
        );
    }
    #[test]
    fn bilingual_commands() {
        assert_eq!(
            punctuation("hej komma world new paragraph next punktum", "spoken"),
            "hej, world\n\nnext."
        );
    }
    #[test]
    fn one_pass_corrections() {
        let p = Processor {
            dictionary: Dictionary::default(),
            corrections: Phrases::new([("pie torch", "PyTorch"), ("PyTorch", "other")], false),
            mode: "off".into(),
            cleanup: true,
        };
        assert_eq!(
            p.process("  i'm using pie torch with 1e-4\n\ni dag"),
            "I'm using PyTorch with 1e-4\n\ni dag"
        );
    }
    #[test]
    fn longest_valid_phrase_with_word_boundaries() {
        let phrases = Phrases::new(
            [("pie torch", "PyTorch"), ("torch", "Torch"), ("pie", "Pie")],
            false,
        );
        assert_eq!(
            phrases.replace("pie torch, pie torchlight, apie torch; pie\ttorch"),
            "PyTorch, Pie torchlight, apie Torch; PyTorch"
        );
        assert_eq!(phrases.replace("pie\ntorch"), "Pie\nTorch");
    }
    #[test]
    fn unicode_case_and_punctuation_boundaries() {
        let phrases = Phrases::new([("ørsted", "Ørsted"), ("c++", "C++")], false);
        assert_eq!(
            phrases.replace("ØRSTED and c++ then ac++ and c++17"),
            "Ørsted and C++ then ac++ and c++17"
        );
        assert_eq!(
            punctuation("acommab periodical spørgsmålstegn", "spoken"),
            "acommab periodical?"
        );
    }
}
