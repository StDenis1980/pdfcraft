//! A Hunspell word list, read to tell a real word from a misread one (Tinta Office fork).
//!
//! Only what the Ukrainian dictionary of LibreOffice (dict_uk) uses is read: single-character
//! flags and suffix rules. A word is known when it is a stem of the `.dic` file or a stem with
//! one of its suffix rules applied.

use std::collections::{HashMap, HashSet};

/// The most stems and suffix rules read, so a damaged file cannot take all memory.
const MAX_STEMS: usize = 2_000_000;
const MAX_RULES: usize = 100_000;

/// One suffix rule: strip `strip` from the end of a stem whose end matches `condition`, add `add`.
#[derive(Debug)]
struct Rule {
    flag: char,
    strip: String,
    condition: Vec<Class>,
}

/// One character of a rule's condition: any character, one of a set, or none of a set.
#[derive(Debug)]
enum Class {
    Any,
    In(Vec<char>),
    NotIn(Vec<char>),
}

impl Class {
    fn matches(&self, c: char) -> bool {
        match self {
            Class::Any => true,
            Class::In(set) => set.contains(&c),
            Class::NotIn(set) => !set.contains(&c),
        }
    }
}

/// The words of a Hunspell dictionary.
#[derive(Debug, Default)]
pub struct Lexicon {
    /// Stem → its flags.
    stems: HashMap<String, Vec<char>>,
    /// Added suffix → the rules that add it.
    rules: HashMap<String, Vec<Rule>>,
    /// The longest added suffix, in characters.
    longest: usize,
}

impl Lexicon {
    /// Read a dictionary from the text of its `.aff` and `.dic` files.
    pub fn parse(aff: &str, dic: &str) -> Lexicon {
        let mut lex = Lexicon::default();
        let mut count = 0usize;
        for line in aff.lines() {
            let mut parts = line.split_whitespace();
            if parts.next() != Some("SFX") {
                continue;
            }
            let fields: Vec<&str> = parts.collect();
            // The header of a rule set ("SFX f Y n") has three fields, a rule five or more.
            let [flag, strip, add, condition, ..] = fields.as_slice() else { continue };
            let Some(flag) = single(flag) else { continue };
            if count >= MAX_RULES {
                break;
            }
            count += 1;
            let strip = if *strip == "0" { String::new() } else { (*strip).to_owned() };
            // Continuation flags after a slash are not followed.
            let add = add.split('/').next().unwrap_or_default();
            let add = if add == "0" { String::new() } else { add.to_owned() };
            lex.longest = lex.longest.max(add.chars().count());
            lex.rules.entry(add).or_default().push(Rule { flag, strip, condition: condition_classes(condition) });
        }
        for line in dic.lines().skip(1) {
            if lex.stems.len() >= MAX_STEMS {
                break;
            }
            let (word, flags) = line.split_once('/').unwrap_or((line, ""));
            let word = word.trim();
            if !word.chars().next().is_some_and(char::is_alphabetic) {
                continue;
            }
            lex.stems.entry(word.to_owned()).or_default().extend(flags.trim().chars());
        }
        lex
    }

    /// How many stems were read.
    pub fn len(&self) -> usize {
        self.stems.len()
    }

    pub fn is_empty(&self) -> bool {
        self.stems.is_empty()
    }

    /// Whether `word` is a word of the dictionary, as written or with its first letter or all its
    /// letters in lower case (a word starting a sentence, a heading in capitals).
    pub fn knows(&self, word: &str) -> bool {
        let word = word.replace(['’', 'ʼ'], "'");
        if self.form(&word) {
            return true;
        }
        let lower = word.to_lowercase();
        if lower != word && self.form(&lower) {
            return true;
        }
        // A heading in capitals of a proper noun ("КИЇВ" for "Київ").
        if word.chars().any(char::is_lowercase) {
            return false;
        }
        let mut chars = lower.chars();
        let capital: String = chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default();
        capital != word && self.form(&capital)
    }

    /// Whether `word` is a stem or a stem with one of its suffixes.
    fn form(&self, word: &str) -> bool {
        if self.stems.contains_key(word) {
            return true;
        }
        let chars: Vec<char> = word.chars().collect();
        for n in 0..=self.longest.min(chars.len()) {
            let Some(split) = chars.len().checked_sub(n) else { continue };
            let (head, tail): (String, String) = (chars.iter().take(split).collect(), chars.iter().skip(split).collect());
            let Some(rules) = self.rules.get(&tail) else { continue };
            for rule in rules {
                let stem = format!("{head}{}", rule.strip);
                if stem.is_empty() || !self.stems.get(&stem).is_some_and(|flags| flags.contains(&rule.flag)) {
                    continue;
                }
                let end: Vec<char> = stem.chars().rev().take(rule.condition.len()).collect();
                if end.len() == rule.condition.len() && rule.condition.iter().rev().zip(&end).all(|(class, c)| class.matches(*c)) {
                    return true;
                }
            }
        }
        false
    }
}

/// The one character of `s`, if it has exactly one.
fn single(s: &str) -> Option<char> {
    let mut chars = s.chars();
    let c = chars.next()?;
    chars.next().is_none().then_some(c)
}

/// A Hunspell condition (`.`, `[^жшщ]ів`, `[аиуео]р`) as one class per character.
fn condition_classes(condition: &str) -> Vec<Class> {
    if condition == "." {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut chars = condition.chars();
    while let Some(c) = chars.next() {
        out.push(match c {
            '.' => Class::Any,
            '[' => {
                let mut set: Vec<char> = chars.by_ref().take_while(|c| *c != ']').collect();
                if set.first() == Some(&'^') {
                    set.remove(0);
                    Class::NotIn(set)
                } else {
                    Class::In(set)
                }
            }
            c => Class::In(vec![c]),
        });
    }
    out
}

/// Letters the Cyrillic model confuses in Ukrainian, each with the letters it is read for.
fn confusable(c: char) -> &'static [char] {
    match c {
        'и' | 'і' | 'ї' => &['і', 'и', 'ї'],
        'И' | 'І' | 'Ї' => &['І', 'И', 'Ї'],
        'е' | 'є' => &['є', 'е'],
        'Е' | 'Є' => &['Є', 'Е'],
        _ => &[],
    }
}

/// The most letters changed in one word.
const MAX_CHANGES: usize = 3;
/// The most spellings tried for one word.
const MAX_TRIES: usize = 4096;

/// `word` corrected to a word of `lex` by changing the letters the Cyrillic model confuses in
/// Ukrainian (и/і/ї, е/є) and putting back an "і" it left out, the fewest changes first; `None`
/// when it is known already, has no single best correction, or is not a Cyrillic word.
/// Punctuation around the word is kept.
pub fn correct(word: &str, lex: &Lexicon) -> Option<String> {
    let start = word.find(|c: char| c.is_alphabetic())?;
    let end = word.rfind(|c: char| c.is_alphabetic()).and_then(|i| Some(i + word.get(i..)?.chars().next()?.len_utf8()))?;
    let (before, core, after) = (word.get(..start)?, word.get(start..end)?, word.get(end..)?);
    let letters = core.chars().filter(|c| c.is_alphabetic()).count();
    if letters < 3 || !core.chars().all(|c| matches!(c, '\u{0400}'..='\u{04FF}' | '\'' | '’' | 'ʼ' | '-')) || lex.knows(core) {
        return None;
    }
    let chars: Vec<char> = core.chars().collect();
    let places: Vec<usize> = chars.iter().enumerate().filter(|(_, c)| !confusable(**c).is_empty()).map(|(i, _)| i).collect();
    let mut tries = 0usize;
    for changes in 1..=MAX_CHANGES {
        let mut found: HashSet<String> = HashSet::new();
        // Letters swapped, `changes` of them.
        for set in subsets(places.len(), changes) {
            let mut spellings = vec![chars.clone()];
            for i in set.iter().filter_map(|i| places.get(*i)) {
                spellings = spellings
                    .into_iter()
                    .flat_map(|s| {
                        let here = s.get(*i).copied();
                        confusable(here.unwrap_or(' ')).iter().filter(move |c| Some(**c) != here).map(move |c| {
                            let mut t = s.clone();
                            if let Some(slot) = t.get_mut(*i) {
                                *slot = *c;
                            }
                            t
                        })
                    })
                    .collect();
            }
            for s in spellings {
                tries += 1;
                let s: String = s.into_iter().collect();
                if lex.knows(&s) {
                    found.insert(s);
                }
            }
            if tries > MAX_TRIES {
                return None;
            }
        }
        // An "і" left out, with the swaps of one change fewer.
        if found.is_empty() {
            for set in subsets(places.len(), changes - 1) {
                for at in 1..chars.len() {
                    let mut s = chars.clone();
                    for i in set.iter().filter_map(|i| places.get(*i)) {
                        if let Some(slot) = s.get_mut(*i) {
                            *slot = confusable(*slot).first().copied().unwrap_or(*slot);
                        }
                    }
                    let upper = s.iter().all(|c| !c.is_lowercase());
                    s.insert(at, if upper { 'І' } else { 'і' });
                    tries += 1;
                    let s: String = s.into_iter().collect();
                    if lex.knows(&s) {
                        found.insert(s);
                    }
                }
                if tries > MAX_TRIES {
                    return None;
                }
            }
        }
        match found.len() {
            0 => continue,
            1 => return found.into_iter().next().map(|s| format!("{before}{s}{after}")),
            _ => return None,
        }
    }
    None
}

/// The sets of `k` of the numbers below `n`, in order.
fn subsets(n: usize, k: usize) -> Vec<Vec<usize>> {
    if k == 0 {
        return vec![Vec::new()];
    }
    if k > n {
        return Vec::new();
    }
    let mut out = Vec::new();
    for first in 0..n {
        for mut rest in subsets(n - first - 1, k - 1) {
            for r in rest.iter_mut() {
                *r += first + 1;
            }
            rest.insert(0, first);
            out.push(rest);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A few Ukrainian stems with suffix rules shaped as dict_uk's.
    fn lexicon() -> Lexicon {
        let aff = "SET UTF-8\nSFX a Y 3\nSFX a 0 а .\nSFX a 0 ом [^жшщ]\nSFX a ь і ь\nSFX b Y 1\nSFX b ий а [^ц]ий\n";
        let dic = "6\nсертифікат/a\nвідповідно\nреєстр/a\nКиїв/a\nжиттєвий/b\nвін\n";
        Lexicon::parse(aff, dic)
    }

    #[test]
    fn words_are_known_with_their_suffixes() {
        let lex = lexicon();
        assert_eq!(lex.len(), 6);
        for w in ["сертифікат", "сертифіката", "сертифікатом", "реєстром", "життєва", "Відповідно", "Київ", "КИЇВ"]
        {
            assert!(lex.knows(w), "{w}");
        }
        for w in ["реєстру", "сертифікатів", "київ", "життєвий’"] {
            assert!(!lex.knows(w), "{w}");
        }
        assert!(!lex.knows("життєвца"), "the condition holds the stem back");
    }

    #[test]
    fn misread_letters_are_put_right_by_the_dictionary() {
        let lex = lexicon();
        assert_eq!(correct("видповидно", &lex).as_deref(), Some("відповідно"));
        assert_eq!(correct("ресстр", &lex), None, "с is not a misread є");
        assert_eq!(correct("реестр,", &lex).as_deref(), Some("реєстр,"), "punctuation is kept");
        assert_eq!(correct("сертифкатом", &lex).as_deref(), Some("сертифікатом"), "a dropped і is put back");
        assert_eq!(correct("Киив", &lex).as_deref(), Some("Київ"));
        assert_eq!(correct("КИІВ", &lex).as_deref(), Some("КИЇВ"));
        assert_eq!(correct("життевий", &lex).as_deref(), Some("життєвий"));
        assert_eq!(correct("він", &lex), None, "known");
        assert_eq!(correct("вин", &lex).as_deref(), Some("він"));
        assert_eq!(correct("hello", &lex), None, "not Cyrillic");
        assert_eq!(correct("ви", &lex), None, "too short to judge");
        assert_eq!(correct("", &lex), None);
        assert_eq!(correct("...", &lex), None);
    }

    #[test]
    fn a_word_with_two_equal_corrections_is_left_alone() {
        let lex = Lexicon::parse("", "2\nбік\nбик\n");
        assert_eq!(correct("бїк", &lex), None);
    }

    #[test]
    fn damaged_files_read_as_far_as_they_go() {
        let lex = Lexicon::parse("SFX\nSFX a\nSFX ab 0 x .\nSFX a 0 [ .\n", "x\n/\n\nслово/\nслово/a/b\n");
        assert!(lex.knows("слово"));
        assert!(lex.knows("слово["), "a rule from an odd line still applies");
        assert_eq!(subsets(3, 2), [vec![0, 1], vec![0, 2], vec![1, 2]]);
        assert!(subsets(1, 2).is_empty());
    }
}
