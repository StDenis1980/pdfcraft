//! A Hunspell word list, read to tell a real word from a misread one (Tinta Office fork).
//!
//! Only what the Ukrainian and Spanish dictionaries of LibreOffice use is read: single-character
//! flags, suffix and prefix rules, and suffixes that allow a second suffix (continuation flags).
//! A word is known when it is a stem of the `.dic` file, or a stem with up to two suffixes and a
//! prefix applied as its flags allow.

use std::collections::{HashMap, HashSet};

/// The most stems and suffix rules read, so a damaged file cannot take all memory.
const MAX_STEMS: usize = 2_000_000;
const MAX_RULES: usize = 100_000;

/// One affix rule: strip `strip` from the end (a suffix) or the start (a prefix) of a stem whose
/// end or start matches `condition`, and add the affix; a suffix may then take a second suffix of
/// one of the flags `then`.
#[derive(Debug)]
struct Rule {
    flag: char,
    strip: String,
    condition: Vec<Class>,
    then: Vec<char>,
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
    /// Added prefix → the rules that add it.
    prefixes: HashMap<String, Vec<Rule>>,
    /// The longest added prefix, in characters.
    longest_prefix: usize,
}

impl Lexicon {
    /// Read a dictionary from the text of its `.aff` and `.dic` files.
    pub fn parse(aff: &str, dic: &str) -> Lexicon {
        let mut lex = Lexicon::default();
        let mut count = 0usize;
        for line in aff.lines() {
            let mut parts = line.split_whitespace();
            let prefix = match parts.next() {
                Some("SFX") => false,
                Some("PFX") => true,
                _ => continue,
            };
            let fields: Vec<&str> = parts.collect();
            // The header of a rule set ("SFX f Y n") has three fields, a rule five or more.
            let [flag, strip, add, condition, ..] = fields.as_slice() else { continue };
            let Some(flag) = single(flag) else { continue };
            if count >= MAX_RULES {
                break;
            }
            count += 1;
            let strip = if *strip == "0" { String::new() } else { (*strip).to_owned() };
            let (add, then) = add.split_once('/').unwrap_or((add, ""));
            let add = if add == "0" { String::new() } else { add.to_owned() };
            let rule = Rule { flag, strip, condition: condition_classes(condition), then: then.chars().collect() };
            if prefix {
                lex.longest_prefix = lex.longest_prefix.max(add.chars().count());
                lex.prefixes.entry(add).or_default().push(rule);
            } else {
                lex.longest = lex.longest.max(add.chars().count());
                lex.rules.entry(add).or_default().push(rule);
            }
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

    /// Whether `word` is a stem with its affixes: up to two suffixes, under a prefix.
    fn form(&self, word: &str) -> bool {
        if self.suffixed(word, None, true) {
            return true;
        }
        let chars: Vec<char> = word.chars().collect();
        for n in 1..=self.longest_prefix.min(chars.len()) {
            let (head, rest): (String, String) = (chars.iter().take(n).collect(), chars.iter().skip(n).collect());
            for rule in self.prefixes.get(&head).into_iter().flatten() {
                let stem = format!("{}{rest}", rule.strip);
                let start: Vec<char> = stem.chars().take(rule.condition.len()).collect();
                if start.len() == rule.condition.len()
                    && rule.condition.iter().zip(&start).all(|(class, c)| class.matches(*c))
                    && self.suffixed(&stem, Some(rule.flag), true)
                {
                    return true;
                }
            }
        }
        false
    }

    /// Whether `word` is a stem with the flag `with` (when given), or such a stem with a suffix
    /// (and, when `twice`, a second suffix its first allows).
    fn suffixed(&self, word: &str, with: Option<char>, twice: bool) -> bool {
        let has = |stem: &str, flag: Option<char>| {
            self.stems.get(stem).is_some_and(|flags| with.is_none_or(|w| flags.contains(&w)) && flag.is_none_or(|f| flags.contains(&f)))
        };
        if has(word, None) {
            return true;
        }
        self.suffixes_of(word).any(|(base, rule)| {
            has(&base, Some(rule.flag))
                || twice && self.suffixes_of(&base).any(|(stem, inner)| inner.then.contains(&rule.flag) && has(&stem, Some(inner.flag)))
        })
    }

    /// Each way `word` ends in a suffix rule's addition over a base the rule's condition allows:
    /// the base (with the rule's strip put back) and the rule.
    fn suffixes_of<'a>(&'a self, word: &str) -> impl Iterator<Item = (String, &'a Rule)> + 'a {
        let chars: Vec<char> = word.chars().collect();
        (0..=self.longest.min(chars.len())).flat_map(move |n| {
            let split = chars.len().saturating_sub(n);
            let (head, tail): (String, String) = (chars.iter().take(split).collect(), chars.iter().skip(split).collect());
            self.rules.get(&tail).into_iter().flatten().filter_map(move |rule| {
                let stem = format!("{head}{}", rule.strip);
                let end: Vec<char> = stem.chars().rev().take(rule.condition.len()).collect();
                (!stem.is_empty() && end.len() == rule.condition.len() && rule.condition.iter().rev().zip(&end).all(|(class, c)| class.matches(*c)))
                    .then_some((stem, rule))
            })
        })
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
fn confusable_uk(c: char) -> &'static [char] {
    match c {
        'и' | 'і' | 'ї' => &['і', 'и', 'ї'],
        'И' | 'І' | 'Ї' => &['І', 'И', 'Ї'],
        'е' | 'є' => &['є', 'е'],
        'Е' | 'Є' => &['Є', 'Е'],
        _ => &[],
    }
}

/// Letters the Latin model confuses in small print: a thin "i" and "l".
fn confusable_latin(c: char) -> &'static [char] {
    match c {
        'i' | 'l' => &['i', 'l'],
        _ => &[],
    }
}

/// What a recognition model gets wrong in a language: the letters it reads for one another, the
/// letters it leaves out (lower case; upper case in a word in capitals), and the letters of a
/// word it judges.
struct Confusions {
    swap: fn(char) -> &'static [char],
    dropped: &'static [char],
    /// When given, a letter is only put back beside one of these: the Latin model loses a thin
    /// letter that runs into a thin neighbour ("disponiblidad", "caidades"), never one between
    /// round letters ("Residence" is no "Residencie").
    dropped_beside: Option<&'static [char]>,
    letter: fn(char) -> bool,
}

/// An apostrophe or a hyphen inside a word.
fn apostrophe(c: char) -> bool {
    matches!(c, '\'' | '’' | 'ʼ' | '-')
}

/// The [`Confusions`] of the model reading `language`, for the languages with a word list.
fn confusions(language: &str) -> Option<Confusions> {
    match language {
        "uk" => Some(Confusions {
            swap: confusable_uk,
            dropped: &['і'],
            dropped_beside: None,
            letter: |c| matches!(c, '\u{0400}'..='\u{04FF}') || apostrophe(c),
        }),
        "es" => Some(Confusions {
            swap: confusable_latin,
            dropped: &['i', 'l'],
            dropped_beside: Some(&['i', 'l', 'í', 'I']),
            letter: |c| (c.is_alphabetic() && !matches!(c, '\u{0400}'..='\u{04FF}')) || apostrophe(c),
        }),
        _ => None,
    }
}

/// The most letters changed in one word.
const MAX_CHANGES: usize = 3;
/// The most spellings tried for one word.
const MAX_TRIES: usize = 4096;

/// `word`, read in `language`, corrected to a word of `lex` by changing the letters the model
/// confuses (Ukrainian и/і/ї and е/є, Spanish i/l) and putting back one it left out (Ukrainian
/// "і", Spanish "i" or "l"), the fewest changes first; `None` when it is known already, has no
/// single best correction, is not written in the language's letters, or the language has no
/// [`Confusions`]. Punctuation around the word is kept.
pub fn correct(word: &str, lex: &Lexicon, language: &str) -> Option<String> {
    let model = confusions(language)?;
    let confusable = model.swap;
    let start = word.find(|c: char| c.is_alphabetic())?;
    let end = word.rfind(|c: char| c.is_alphabetic()).and_then(|i| Some(i + word.get(i..)?.chars().next()?.len_utf8()))?;
    let (before, core, after) = (word.get(..start)?, word.get(start..end)?, word.get(end..)?);
    let letters = core.chars().filter(|c| c.is_alphabetic()).count();
    if letters < 3 || !core.chars().all(model.letter) || lex.knows(core) {
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
        // A letter left out, with the swaps of one change fewer.
        if found.is_empty() {
            for set in subsets(places.len(), changes - 1) {
                for (at, dropped) in (1..chars.len()).flat_map(|at| model.dropped.iter().map(move |d| (at, *d))) {
                    let beside = |c: Option<&char>| c.is_some_and(|c| model.dropped_beside.is_none_or(|b| b.contains(c)));
                    if model.dropped_beside.is_some() && !beside(chars.get(at.wrapping_sub(1))) && !beside(chars.get(at)) {
                        continue;
                    }
                    let mut s = chars.clone();
                    for i in set.iter().filter_map(|i| places.get(*i)) {
                        if let Some(slot) = s.get_mut(*i) {
                            *slot = confusable(*slot).first().copied().unwrap_or(*slot);
                        }
                    }
                    let upper = s.iter().all(|c| !c.is_lowercase());
                    s.insert(at, if upper { dropped.to_uppercase().next().unwrap_or(dropped) } else { dropped });
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
        assert_eq!(correct("видповидно", &lex, "uk").as_deref(), Some("відповідно"));
        assert_eq!(correct("ресстр", &lex, "uk"), None, "с is not a misread є");
        assert_eq!(correct("реестр,", &lex, "uk").as_deref(), Some("реєстр,"), "punctuation is kept");
        assert_eq!(correct("сертифкатом", &lex, "uk").as_deref(), Some("сертифікатом"), "a dropped і is put back");
        assert_eq!(correct("Киив", &lex, "uk").as_deref(), Some("Київ"));
        assert_eq!(correct("КИІВ", &lex, "uk").as_deref(), Some("КИЇВ"));
        assert_eq!(correct("життевий", &lex, "uk").as_deref(), Some("життєвий"));
        assert_eq!(correct("він", &lex, "uk"), None, "known");
        assert_eq!(correct("вин", &lex, "uk").as_deref(), Some("він"));
        assert_eq!(correct("hello", &lex, "uk"), None, "not Cyrillic");
        assert_eq!(correct("ви", &lex, "uk"), None, "too short to judge");
        assert_eq!(correct("", &lex, "uk"), None);
        assert_eq!(correct("...", &lex, "uk"), None);
    }

    #[test]
    fn a_word_with_two_equal_corrections_is_left_alone() {
        let lex = Lexicon::parse("", "2\nбік\nбик\n");
        assert_eq!(correct("бїк", &lex, "uk"), None);
    }

    #[test]
    fn damaged_files_read_as_far_as_they_go() {
        let lex = Lexicon::parse("SFX\nSFX a\nSFX ab 0 x .\nSFX a 0 [ .\n", "x\n/\n\nслово/\nслово/a/b\n");
        assert!(lex.knows("слово"));
        assert!(lex.knows("слово["), "a rule from an odd line still applies");
        assert_eq!(subsets(3, 2), [vec![0, 1], vec![0, 2], vec![1, 2]]);
        assert!(subsets(1, 2).is_empty());
    }

    /// A few Spanish stems with rules shaped as the LibreOffice es_ES dictionary's: a plural,
    /// a suffix that takes the plural after it, and a prefix.
    fn spanish() -> Lexicon {
        let aff = "SET UTF-8\nFLAG UTF-8\nSFX S Y 2\nSFX S 0 s [aeiou]\nSFX S 0 es [^aeiou]\nSFX R Y 2\nSFX R ar ación ar\nSFX R ar ador/S ar\nPFX a Y 1\nPFX a 0 re .\n";
        let dic = "6\ninformar/Ra\nprecio/S\nincluido/S\ncalidad/S\ncambio/S\nresidencie\n";
        Lexicon::parse(aff, dic)
    }

    #[test]
    fn prefixes_and_a_second_suffix_are_read() {
        let lex = spanish();
        assert!(lex.knows("información"), "a suffix that strips");
        assert!(lex.knows("informadores"), "the plural its continuation flag allows");
        assert!(!lex.knows("informaciónes"), "no plural without the flag");
        assert!(lex.knows("Información"));
        assert!(lex.knows("reinformar"), "a prefix");
        assert!(lex.knows("reinformación"), "a prefix with a suffix");
        assert!(!lex.knows("informares"), "no plural on the stem itself");
        assert!(!lex.knows("precioación"), "the stem has no R");
        assert!(!lex.knows("reprecio"), "the stem has no prefix flag");
    }

    #[test]
    fn spanish_i_and_l_misread_in_small_print_are_corrected() {
        let lex = spanish();
        assert_eq!(correct("Preclo", &lex, "es").as_deref(), Some("Precio"));
        assert_eq!(correct("Incluldos", &lex, "es").as_deref(), Some("Incluidos"));
        assert_eq!(correct("camblos:", &lex, "es").as_deref(), Some("cambios:"));
        assert_eq!(correct("caidades", &lex, "es").as_deref(), Some("calidades"), "an l lost beside an i");
        assert_eq!(correct("Residence", &lex, "es"), None, "no letter put back between round ones");
        assert_eq!(correct("precio", &lex, "es"), None, "known");
        assert_eq!(correct("Preclo", &lex, "ca"), None, "no word list for the language");
        assert_eq!(correct("видповидно", &lex, "es"), None, "not Latin");
    }
}
