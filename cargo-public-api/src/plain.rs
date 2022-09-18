use std::io::{Result, Write};

use public_api::{diff::PublicItemsDiff, tokens::Token, PublicItem};
use yansi::{Color, Paint};

use crate::Args;

pub struct Plain;

impl Plain {
    pub fn print_items(w: &mut dyn Write, args: &Args, items: Vec<PublicItem>) -> Result<()> {
        for item in items {
            if args.color.active() {
                writeln!(w, "{}", color_item(&item))?;
            } else {
                writeln!(w, "{}", item)?;
            }
        }

        Ok(())
    }

    pub fn print_diff(w: &mut dyn Write, args: &Args, diff: &PublicItemsDiff) -> Result<()> {
        let use_color = args.color.active();

        print_items_with_header(
            w,
            "Removed items from the public API\n\
             =================================",
            &diff.removed,
            |w, item| {
                if use_color {
                    writeln!(w, "-{}", color_item(item))
                } else {
                    writeln!(w, "-{}", item)
                }
            },
        )?;

        print_items_with_header(
            w,
            "Changed items in the public API\n\
             ===============================",
            &diff.changed,
            |w, changed_item| {
                if use_color {
                    let old_tokens: Vec<&Token> = changed_item.old.tokens().collect();
                    let new_tokens: Vec<&Token> = changed_item.new.tokens().collect();
                    let diff_slice = diff::slice(old_tokens.as_slice(), new_tokens.as_slice());
                    writeln!(
                        w,
                        "-{}\n+{}",
                        color_item_with_diff(&diff_slice, true),
                        color_item_with_diff(&diff_slice, false),
                    )
                } else {
                    writeln!(w, "-{}\n+{}", changed_item.old, changed_item.new)
                }
            },
        )?;

        print_items_with_header(
            w,
            "Added items to the public API\n\
             =============================",
            &diff.added,
            |w, item| {
                if use_color {
                    writeln!(w, "+{}", color_item(item))
                } else {
                    writeln!(w, "+{}", item)
                }
            },
        )?;

        Ok(())
    }
}

fn color_item(item: &public_api::PublicItem) -> String {
    color_token_stream(item.tokens(), None)
}

fn color_token_stream<'a>(tokens: impl Iterator<Item = &'a Token>, bg: Option<Color>) -> String {
    tokens
        .map(|t| color_item_token(t, bg))
        .collect::<Vec<_>>()
        .join("")
}

/// Color the given Token to render it with a nice syntax highlighting. The
/// theme is inspired by dark+ in VS Code and uses the default colors from the
/// terminal to always provide a readable and consistent color scheme.
/// An extra color can be provided to be used as background color.
fn color_item_token<'a>(token: &'a Token, bg: Option<Color>) -> String {
    let style = |color: Color, text: &'a str| {
        let mut paint = Paint::new(text).fg(color);
        if let Some(bg) = bg {
            paint = paint.bg(bg);
        }
        paint.to_string()
    };
    #[allow(clippy::match_same_arms)]
    match token {
        Token::Symbol(text) => style(Color::Default, text),
        Token::Qualifier(text) => style(Color::Blue, text),
        Token::Kind(text) => style(Color::Blue, text),
        Token::Whitespace => style(Color::Default, " "),
        Token::Identifier(text) => style(Color::Cyan, text),
        Token::Annotation(text) => style(Color::Default, text),
        Token::Self_(text) => style(Color::Blue, text),
        Token::Function(text) => style(Color::Yellow, text),
        Token::Lifetime(text) => style(Color::Blue, text),
        Token::Keyword(text) => style(Color::Blue, text),
        Token::Generic(text) => style(Color::Green, text),
        Token::Primitive(text) => style(Color::Green, text),
        Token::Type(text) => style(Color::Green, text),
    }
}

/// Returns a styled string similar to `color_item_token`, but where whole tokens are highlighted if
/// they contain a difference.
fn color_item_with_diff(diff_slice: &[diff::Result<&&Token>], is_old_item: bool) -> String {
    diff_slice
        .iter()
        .filter_map(|diff_result| match diff_result {
            diff::Result::Left(&token) => is_old_item.then(|| {
                Paint::new(token.text())
                    .fg(Color::Fixed(9))
                    .bg(Color::Fixed(52))
                    .bold()
                    .to_string()
            }),
            diff::Result::Both(&token, _) => Some(color_item_token(token, None)),
            diff::Result::Right(&token) => (!is_old_item).then(|| {
                Paint::new(token.text())
                    .fg(Color::Fixed(10))
                    .bg(Color::Fixed(22))
                    .bold()
                    .to_string()
            }),
        })
        .collect::<Vec<_>>()
        .join("")
}

pub fn print_items_with_header<T>(
    w: &mut dyn Write,
    header: &str,
    items: &[T],
    print_fn: impl Fn(&mut dyn Write, &T) -> Result<()>,
) -> Result<()> {
    writeln!(w, "{}", header)?;
    if items.is_empty() {
        writeln!(w, "(none)")?;
    } else {
        for item in items {
            print_fn(w, item)?;
        }
    }
    writeln!(w)
}
