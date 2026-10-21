//! Component `vision-data` (1.1.2): the owner's vision, kept as written.
//!
//! The prompt that started a project describes where it should be after a
//! long time, not what to build first. Kept verbatim in `.harna/vision.md`,
//! it is what every later session plans toward and what assumptions are
//! measured against; it is never generated and never rewritten by harna.

/// A short line about the vision for briefings: its length and first
/// sentence, or None when there is none.
pub fn summary(text: &str) -> Option<String> {
    let body: String = text
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join(" ");
    let words = body.split_whitespace().count();
    if words == 0 {
        return None;
    }
    let first: String = first_sentence(body.trim()).chars().take(120).collect();
    Some(format!("{words} words — \"{first}…\""))
}

/// Up to the first `.`, `!` or `?` that ends a sentence: followed by the
/// end, or by a space and a capital (review of 1.1.2: "e.g. a CRM" cut the
/// first sentence at "e").
fn first_sentence(text: &str) -> &str {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    for (k, &(i, c)) in chars.iter().enumerate() {
        if !matches!(c, '.' | '!' | '?') {
            continue;
        }
        let spaced = chars.get(k + 1).is_none_or(|(_, c)| c.is_whitespace());
        let next = chars[k + 1..]
            .iter()
            .map(|(_, c)| *c)
            .find(|c| !c.is_whitespace());
        if spaced && next.is_none_or(char::is_uppercase) {
            return text[..i].trim();
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_vision_is_summed_up_by_its_length_and_first_sentence() {
        let v = "# Fieldlog\n\nI run a small company. We repair boilers.\n";
        assert_eq!(summary(v).unwrap(), "8 words — \"I run a small company…\"");
        assert!(summary("# only a title\n").is_none());
        assert_eq!(
            summary("A tool for crews, e.g. plumbers. Then more.").unwrap(),
            "8 words — \"A tool for crews, e.g. plumbers…\""
        );
    }
}
