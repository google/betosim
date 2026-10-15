// Copyright 2024 The Android Open Source Project
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

/// A vector of tuples representing wildcard patterns.
/// Each tuple contains a prefix string and a suffix string.
pub struct PatternVec {
    patterns: Vec<(String, String)>,
}

impl PatternVec {
    /// Creates a new PatternVec from a string containing semicolon (;) or comma (,)
    /// separated wildcard patterns.
    pub fn new(pattern_list: impl Into<String>) -> PatternVec {
        let pattern_list = pattern_list.into();
        let patterns = if pattern_list.trim().is_empty() {
            Vec::new()
        } else {
            pattern_list
                .split([';', ','])
                // Splits a string at the first occurrence of '*', returning a tuple
                // containing the prefix (before *) and suffix (after *).
                // If no '*' is found, returns the entire string as prefix.
                .map(|s| match s.find('*') {
                    Some(i) => (String::from(&s[..i]), String::from(&s[i + 1..])),
                    None => (String::from(s), String::new()),
                })
                .collect()
        };
        PatternVec { patterns }
    }

    /// Checks if a given string matches any of the patterns in the PatternVec.
    /// A match occurs if the string starts with a pattern's prefix and ends with its suffix.
    pub fn matches(&self, s: &str) -> bool {
        self.patterns.iter().any(|(prefix, suffix)| s.starts_with(prefix) && s.ends_with(suffix))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! tuple_str {
        ($a:expr, $b:expr) => {
            (String::from($a), String::from($b))
        };
    }

    #[test]
    fn test_new_empty_string() {
        let pattern_vec = PatternVec::new("");
        assert_eq!(pattern_vec.patterns.len(), 0);
    }

    #[test]
    fn test_new_single_pattern() {
        let pattern_vec = PatternVec::new("*.example.com");
        assert_eq!(pattern_vec.patterns.len(), 1);
        assert_eq!(pattern_vec.patterns[0], tuple_str!("", ".example.com"));
    }

    #[test]
    fn test_new_multiple_patterns() {
        let pattern_vec = PatternVec::new("*.example.com;*.org");
        assert_eq!(pattern_vec.patterns.len(), 2);
        assert_eq!(pattern_vec.patterns[0], tuple_str!("", ".example.com"));
        assert_eq!(pattern_vec.patterns[1], tuple_str!("", ".org"));
    }

    #[test]
    fn test_matches_exact_match() {
        let pattern_vec = PatternVec::new("example.com");
        assert!(pattern_vec.matches("example.com"));
    }

    #[test]
    fn test_matches_prefix_match() {
        let pattern_vec = PatternVec::new("*.google.com");
        assert!(pattern_vec.matches("foo.google.com"));
    }

    #[test]
    fn test_matches_suffix_match() {
        let pattern_vec = PatternVec::new("*.com");
        assert!(pattern_vec.matches("example.com"));
    }

    #[test]
    fn test_matches_no_match() {
        let pattern_vec = PatternVec::new("*.google.com");
        assert!(!pattern_vec.matches("example.org"));
    }

    #[test]
    fn test_matches_multiple_patterns() {
        let pattern_vec = PatternVec::new("*.example.com;*.org");
        assert!(pattern_vec.matches("some.example.com"));
        assert!(pattern_vec.matches("another.org"));
    }

    #[test]
    fn test_matches_middle_wildcard() {
        let pattern_vec = PatternVec::new("some*.com");
        assert!(pattern_vec.matches("somemiddle.com"));
        assert!(pattern_vec.matches("some.middle.com"));
        assert!(pattern_vec.matches("some.middle.example.com"));
    }
}
