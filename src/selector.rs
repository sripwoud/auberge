use dialoguer::{Select, theme::ColorfulTheme};
use eyre::Result;
use skim::prelude::*;
use std::env;
use std::io::{Cursor, IsTerminal};

pub fn has_skim_support() -> bool {
    std::io::stdin().is_terminal() && std::io::stderr().is_terminal()
}

pub fn select_with_skim(items: &[String], prompt: &str) -> Option<String> {
    if items.is_empty() {
        return None;
    }

    let prompt_str = format!("{}> ", prompt);

    let mut builder = SkimOptionsBuilder::default();
    builder
        .prompt(Some(&prompt_str))
        .height(Some("40%"))
        .multi(false)
        .reverse(true);

    if env::var("NO_COLOR").is_err() {
        builder.color(Some(
            "fg:regular,bg:regular,current:reverse,pointer:magenta",
        ));
    }

    let options = builder.build().ok()?;

    let input = items.join("\n");
    let item_reader = SkimItemReader::default();
    let items = item_reader.of_bufread(Cursor::new(input));

    let output = Skim::run_with(&options, Some(items))?;

    if output.is_abort {
        return None;
    }

    output
        .selected_items
        .first()
        .map(|item| item.output().to_string())
}

pub fn select_with_dialoguer(items: &[String], prompt: &str) -> Option<String> {
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

pub fn select(items: &[String], prompt: &str) -> Option<String> {
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
