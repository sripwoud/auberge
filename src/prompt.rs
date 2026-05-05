use dialoguer::{Confirm, Input, MultiSelect, Select, theme::ColorfulTheme};
use eyre::Result;
use skim::prelude::*;
use std::io::{Cursor, IsTerminal, Write};

fn has_skim_support() -> bool {
    std::io::stdin().is_terminal() && std::io::stderr().is_terminal()
}

fn select_with_skim(items: &[String], prompt: &str) -> Option<String> {
    if items.is_empty() {
        return None;
    }

    let prompt_str = format!("{}> ", prompt);

    let options = SkimOptionsBuilder::default()
        .prompt(Some(&prompt_str))
        .height(Some("40%"))
        .multi(false)
        .reverse(true)
        .build()
        .ok()?;

    let input = items.join("\n");
    let item_reader = SkimItemReader::default();
    let items = item_reader.of_bufread(Cursor::new(input));

    let output = Skim::run_with(&options, Some(items))?;

    // Skim leaves terminal background color set after exit.
    // \x1b[0m resets all SGR attributes (fixes colored bands on subsequent lines).
    // \x1b[J clears from cursor to end of screen (removes phantom blank lines).
    let mut stderr = std::io::stderr().lock();
    let _ = stderr.write_all(b"\x1b[0m\x1b[J");
    let _ = stderr.flush();

    if output.is_abort {
        return None;
    }

    output
        .selected_items
        .first()
        .map(|item| item.output().to_string())
}

fn select_with_dialoguer(items: &[String], prompt: &str) -> Option<String> {
    if items.is_empty() {
        return None;
    }

    let selection = Select::with_theme(&ColorfulTheme::default())
        .with_prompt(prompt)
        .items(items)
        .default(0)
        .interact_opt()
        .ok()
        .flatten()?;

    Some(items[selection].clone())
}

fn select(items: &[String], prompt: &str) -> Option<String> {
    if items.is_empty() {
        return None;
    }

    if !has_skim_support() {
        if items.len() == 1 {
            return Some(items[0].clone());
        }
        return None;
    }

    select_with_skim(items, prompt).or_else(|| select_with_dialoguer(items, prompt))
}

pub fn select_item<T, F>(items: &[T], display_fn: F, prompt: &str) -> Result<Option<T>>
where
    T: Clone,
    F: Fn(&T) -> String,
{
    if items.is_empty() {
        return Ok(None);
    }

    let display_items: Vec<String> = items.iter().map(&display_fn).collect();
    let selected_display = select(&display_items, prompt);

    match selected_display {
        Some(display) => {
            let idx = display_items.iter().position(|d| d == &display);
            Ok(idx.map(|i| items[i].clone()))
        }
        None => Ok(None),
    }
}

fn select_multi_with_skim(items: &[String], prompt: &str) -> Option<Vec<String>> {
    if items.is_empty() {
        return None;
    }

    let prompt_str = format!("{}> ", prompt);

    let options = SkimOptionsBuilder::default()
        .prompt(Some(&prompt_str))
        .height(Some("40%"))
        .multi(true)
        .reverse(true)
        .build()
        .ok()?;

    let input = items.join("\n");
    let item_reader = SkimItemReader::default();
    let items = item_reader.of_bufread(Cursor::new(input));

    let output = Skim::run_with(&options, Some(items))?;

    let mut stderr = std::io::stderr().lock();
    let _ = stderr.write_all(b"\x1b[0m\x1b[J");
    let _ = stderr.flush();

    if output.is_abort {
        return None;
    }

    let selected: Vec<String> = output
        .selected_items
        .iter()
        .map(|item| item.output().to_string())
        .collect();

    if selected.is_empty() {
        None
    } else {
        Some(selected)
    }
}

fn select_multi_with_dialoguer(items: &[String], prompt: &str) -> Option<Vec<String>> {
    if items.is_empty() {
        return None;
    }

    let selections = MultiSelect::with_theme(&ColorfulTheme::default())
        .with_prompt(prompt)
        .items(items)
        .interact_opt()
        .ok()
        .flatten()?;

    if selections.is_empty() {
        return None;
    }

    Some(selections.iter().map(|&i| items[i].clone()).collect())
}

pub fn select_multi(items: &[String], prompt: &str) -> Option<Vec<String>> {
    if items.is_empty() {
        return None;
    }

    if !has_skim_support() {
        if items.len() == 1 {
            return Some(vec![items[0].clone()]);
        }
        return None;
    }

    if let Some(result) = select_multi_with_skim(items, prompt) {
        return Some(result);
    }

    select_multi_with_dialoguer(items, prompt)
}

pub fn confirm(msg: &str, yes_flag: bool) -> bool {
    if yes_flag {
        return true;
    }

    if !std::io::stdin().is_terminal() {
        return false;
    }

    Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt(msg)
        .default(false)
        .interact()
        .unwrap_or(false)
}

/// Severe confirmation: the user must type `expected` exactly to proceed.
/// Use for irreversible / production-impacting actions.
///
/// Honors `yes_flag` (skip prompt, proceed) and non-TTY stdin (refuse, return
/// `Ok(false)` so callers can bail with an actionable message instead of
/// hanging on a prompt that nobody can answer).
pub fn confirm_typed(prompt_msg: &str, expected: &str, yes_flag: bool) -> Result<bool> {
    if yes_flag {
        return Ok(true);
    }

    if !std::io::stdin().is_terminal() {
        return Ok(false);
    }

    let typed: String = Input::with_theme(&ColorfulTheme::default())
        .with_prompt(prompt_msg)
        .allow_empty(true)
        .interact_text()?;

    Ok(typed.trim() == expected)
}

#[cfg(test)]
mod tests {
    use super::{confirm, confirm_typed};

    #[test]
    fn confirm_short_circuits_to_true_when_yes_flag_set() {
        assert!(confirm("anything", true));
    }

    #[test]
    fn confirm_returns_false_in_non_tty_without_yes_flag() {
        // `cargo test` runs with non-TTY stdin, so the is_terminal() guard
        // takes effect.  This is the path that prevents `dns set-all` and
        // `dns delete` from hanging in CI when --yes is omitted.
        assert!(!confirm("anything", false));
    }

    #[test]
    fn confirm_typed_short_circuits_to_true_when_yes_flag_set() {
        // --yes must bypass the typed-confirmation gate so CI can run without
        // a TTY attached.  Expected value is irrelevant on this path.
        assert!(confirm_typed("type the name", "freshrss", true).unwrap());
    }

    #[test]
    fn confirm_typed_returns_false_in_non_tty_without_yes_flag() {
        // Without --yes and without a TTY, severe confirmation cannot be
        // satisfied — callers should treat this as cancellation and surface
        // an actionable error rather than dispatching the destructive op.
        assert!(!confirm_typed("type the name", "freshrss", false).unwrap());
    }
}
