use crate::services::bichon::api::MailBox;
use std::collections::HashSet;

const FALLBACK_NAMES: &[&str] = &[
    "spam",
    "junk",
    "junk email",
    "junk mail",
    "trash",
    "deleted items",
    "deleted messages",
    "bin",
];

pub fn is_excluded(mailbox: &MailBox, operator_extras: &HashSet<String>) -> bool {
    if mailbox.attributes.iter().any(|a| {
        let kind = a.kind().trim_start_matches('\\').to_ascii_lowercase();
        kind == "junk" || kind == "trash"
    }) {
        return true;
    }

    let leaf = mailbox
        .name
        .rsplit_once(['/', '.'])
        .map(|(_, tail)| tail)
        .unwrap_or(mailbox.name.as_str());
    if FALLBACK_NAMES.contains(&leaf.to_ascii_lowercase().as_str()) {
        return true;
    }

    operator_extras.contains(&mailbox.name)
}

#[cfg(test)]
mod tests {
    use super::is_excluded;
    use crate::services::bichon::api::{MailBox, MailboxAttribute};
    use std::collections::HashSet;

    fn mailbox(name: &str, attrs: &[&str]) -> MailBox {
        MailBox {
            name: name.to_string(),
            attributes: attrs
                .iter()
                .map(|k| MailboxAttribute::Raw((*k).to_string()))
                .collect(),
        }
    }

    #[test]
    fn excludes_special_use_junk_and_trash() {
        let extras = HashSet::new();
        assert!(is_excluded(&mailbox("Papierkorb", &["\\Trash"]), &extras));
        assert!(is_excluded(&mailbox("Pourriels", &["Junk"]), &extras));
    }

    #[test]
    fn excludes_by_fallback_leaf_names_when_special_use_missing() {
        let extras = HashSet::new();
        assert!(is_excluded(&mailbox("INBOX/Deleted Items", &[]), &extras));
        assert!(is_excluded(&mailbox("Archive.Spam", &[]), &extras));
        assert!(is_excluded(&mailbox("Junk Mail", &[]), &extras));
    }

    #[test]
    fn does_not_exclude_non_english_without_special_use_or_fallback_name() {
        let extras = HashSet::new();
        assert!(!is_excluded(&mailbox("Papierkorb", &[]), &extras));
        assert!(!is_excluded(&mailbox("Pourriels", &[]), &extras));
    }

    #[test]
    fn excludes_operator_extra_folders() {
        let extras = ["Newsletters".to_string(), "Receipts/2019".to_string()]
            .into_iter()
            .collect();
        assert!(is_excluded(&mailbox("Newsletters", &[]), &extras));
        assert!(is_excluded(&mailbox("Receipts/2019", &[]), &extras));
        assert!(!is_excluded(&mailbox("Receipts/2020", &[]), &extras));
    }
}
