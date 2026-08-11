use crate::output::should_use_colors;
use dialoguer::{Confirm, Input, MultiSelect, Select, theme::ColorfulTheme, theme::SimpleTheme};
use eyre::Result;
use skim::prelude::*;
use std::io::{Cursor, IsTerminal, Write};

#[derive(Debug, PartialEq, Eq)]
enum ThemeKind {
    Colorful,
    Simple,
}

fn theme_kind() -> ThemeKind {
    if should_use_colors() {
        ThemeKind::Colorful
    } else {
        ThemeKind::Simple
    }
}

fn dialoguer_theme() -> Box<dyn dialoguer::theme::Theme> {
    match theme_kind() {
        ThemeKind::Colorful => Box::new(ColorfulTheme::default()),
        ThemeKind::Simple => Box::new(SimpleTheme),
    }
}

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
    if should_use_colors() {
        let mut stderr = std::io::stderr().lock();
        let _ = stderr.write_all(b"\x1b[0m\x1b[J");
        let _ = stderr.flush();
    }

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

    let theme = dialoguer_theme();
    let selection = Select::with_theme(theme.as_ref())
        .with_prompt(prompt)
        .items(items)
        .default(0)
        .interact_opt()
        .ok()
        .flatten()?;

    Some(items[selection].clone())
}

/// Describes what is being chosen so `select_item` can render each of the
/// three ways a pick can fail to happen as its own actionable error.
///
/// Returning one `Option` for all three let call sites collapse "nothing to
/// choose from", "cannot draw a picker" and "user dismissed the picker" into
/// one misleading message.
///
/// `noun` is pluralised as `{noun}s`; every candidate kind in this tree
/// pluralises regularly.
pub struct Choice {
    prompt: String,
    noun: String,
    argument: Option<String>,
    populate: Option<String>,
}

impl Choice {
    pub fn new(noun: &str) -> Self {
        Self {
            prompt: format!("Select {}", noun),
            noun: noun.to_string(),
            argument: None,
            populate: None,
        }
    }

    pub fn with_prompt(mut self, prompt: &str) -> Self {
        self.prompt = prompt.to_string();
        self
    }

    /// How the caller supplies the value when no picker can be drawn, e.g.
    /// `-H <host>`.
    pub fn resolved_by(mut self, argument: &str) -> Self {
        self.argument = Some(argument.to_string());
        self
    }

    /// Command that creates candidates when there are none, e.g.
    /// `auberge host add`.
    pub fn populated_by(mut self, command: &str) -> Self {
        self.populate = Some(command.to_string());
        self
    }

    fn no_candidates(&self) -> eyre::Report {
        match &self.populate {
            Some(command) => {
                eyre::eyre!("No {}s configured — run `{}`", self.noun, command)
            }
            None => eyre::eyre!("No {}s configured", self.noun),
        }
    }

    fn not_interactive(&self, candidates: usize) -> eyre::Report {
        match &self.argument {
            Some(argument) => eyre::eyre!(
                "{} {}s to choose from and stdin is not a terminal — pass {}",
                candidates,
                self.noun,
                argument
            ),
            None => eyre::eyre!(
                "{} {}s to choose from and stdin is not a terminal",
                candidates,
                self.noun
            ),
        }
    }

    fn aborted(&self) -> eyre::Report {
        eyre::eyre!("No {} selected", self.noun)
    }
}

/// Picks one of `items`, or fails with the reason no pick happened.
///
/// A lone candidate is auto-selected without a TTY: the choice is not a choice.
pub fn select_item<T, F>(items: &[T], display_fn: F, choice: Choice) -> Result<T>
where
    T: Clone,
    F: Fn(&T) -> String,
{
    if items.is_empty() {
        return Err(choice.no_candidates());
    }

    if !has_skim_support() {
        if let [only] = items {
            return Ok(only.clone());
        }
        return Err(choice.not_interactive(items.len()));
    }

    let display_items: Vec<String> = items.iter().map(&display_fn).collect();
    let selected = select_with_skim(&display_items, &choice.prompt)
        .or_else(|| select_with_dialoguer(&display_items, &choice.prompt))
        .ok_or_else(|| choice.aborted())?;

    display_items
        .iter()
        .position(|d| d == &selected)
        .map(|i| items[i].clone())
        .ok_or_else(|| choice.aborted())
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

    if should_use_colors() {
        let mut stderr = std::io::stderr().lock();
        let _ = stderr.write_all(b"\x1b[0m\x1b[J");
        let _ = stderr.flush();
    }

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

    let theme = dialoguer_theme();
    let selections = MultiSelect::with_theme(theme.as_ref())
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

    let theme = dialoguer_theme();
    Confirm::with_theme(theme.as_ref())
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

    let theme = dialoguer_theme();
    let typed: String = Input::with_theme(theme.as_ref())
        .with_prompt(prompt_msg)
        .allow_empty(true)
        .interact_text()?;

    Ok(typed.trim() == expected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::{TEST_LOCK, set_no_color};

    fn hosts() -> Vec<String> {
        vec!["auberge".to_string(), "hermes".to_string()]
    }

    #[test]
    fn select_item_reports_nothing_configured_for_an_empty_list() {
        // The bug behind #468: an empty candidate list used to yield the same
        // "No host selected" as a dismissed picker, so `sync music`'s bogus
        // group filter looked like the user had declined to choose.
        let empty: Vec<String> = vec![];
        let err = select_item(
            &empty,
            |s: &String| s.clone(),
            Choice::new("host").populated_by("auberge host add"),
        )
        .unwrap_err();

        assert_eq!(
            err.to_string(),
            "No hosts configured — run `auberge host add`"
        );
    }

    #[test]
    fn select_item_omits_the_populate_hint_when_none_is_given() {
        let empty: Vec<String> = vec![];
        let err = select_item(&empty, |s: &String| s.clone(), Choice::new("playbook")).unwrap_err();

        assert_eq!(err.to_string(), "No playbooks configured");
    }

    #[test]
    fn select_item_names_the_argument_when_it_cannot_prompt() {
        // `cargo test` runs without a TTY, so this is the scripted path: two
        // candidates, no picker possible, and the error must name the flag
        // that resolves it.
        let err = select_item(
            &hosts(),
            |s: &String| s.clone(),
            Choice::new("host").resolved_by("-H <host>"),
        )
        .unwrap_err();

        assert_eq!(
            err.to_string(),
            "2 hosts to choose from and stdin is not a terminal — pass -H <host>"
        );
    }

    #[test]
    fn select_item_still_reports_the_count_without_an_argument_hint() {
        let err = select_item(&hosts(), |s: &String| s.clone(), Choice::new("host")).unwrap_err();

        assert_eq!(
            err.to_string(),
            "2 hosts to choose from and stdin is not a terminal"
        );
    }

    #[test]
    fn select_item_auto_selects_a_lone_candidate_without_a_tty() {
        // Deliberate, and matches `backup verify`'s sole_configured_host: one
        // candidate is not a choice, so scripts need no flag.
        let only = vec!["auberge".to_string()];
        let selected = select_item(
            &only,
            |s: &String| s.clone(),
            Choice::new("host").resolved_by("-H <host>"),
        )
        .unwrap();

        assert_eq!(selected, "auberge");
    }

    #[test]
    fn choice_defaults_its_prompt_from_the_noun() {
        assert_eq!(Choice::new("subdomain").prompt, "Select subdomain");
        assert_eq!(
            Choice::new("subdomain").with_prompt("Pick one").prompt,
            "Pick one"
        );
    }

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

    #[test]
    fn theme_kind_is_simple_when_no_color_flag_set() {
        let _guard = TEST_LOCK.lock().unwrap();
        set_no_color(true);
        assert_eq!(theme_kind(), ThemeKind::Simple);
        set_no_color(false);
    }

    #[test]
    fn dialoguer_theme_does_not_panic_in_either_branch() {
        let _guard = TEST_LOCK.lock().unwrap();
        set_no_color(true);
        let _simple = dialoguer_theme();
        set_no_color(false);
        let _maybe_colorful = dialoguer_theme();
    }
}
