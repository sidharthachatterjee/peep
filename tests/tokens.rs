//! Raw colours and sizes belong to `src/theme`. Everything else, components and views
//! alike, asks the theme. This test fails when a raw value is written anywhere else.

use std::{fs, path::Path};

/// Functions that build a colour or a length from a number.
const RAW_FUNCTIONS: &[&str] = &["rgb", "rgba", "hsla", "black", "white", "px", "rems"];

/// Styling methods that end in a step of GPUI's built-in scale, like `gap_2` or `p_4`.
const SCALED: &[&str] = &[
    "p", "px", "py", "pt", "pr", "pb", "pl", "m", "mx", "my", "mt", "mr", "mb", "ml", "gap",
    "gap_x", "gap_y", "w", "h", "size", "min_w", "min_h", "max_w", "max_h", "top", "right",
    "bottom", "left", "inset", "border", "border_t", "border_r", "border_b", "border_l",
];

/// Styling methods that end in a named step, like `rounded_md` or `text_sm`.
const NAMED: &[&str] = &["rounded", "text", "shadow"];
const NAMED_STEPS: &[&str] = &[
    "2xs", "xs", "sm", "base", "md", "lg", "xl", "2xl", "3xl", "full",
];

fn is_raw(name: &str, is_method: bool) -> bool {
    if !is_method {
        return RAW_FUNCTIONS.contains(&name);
    }
    let Some((prefix, step)) = name.rsplit_once('_') else {
        return false;
    };
    let numbered = step.starts_with(|c: char| c.is_ascii_digit()) && step != "0";
    (SCALED.contains(&prefix) && numbered)
        || (NAMED.contains(&prefix) && NAMED_STEPS.contains(&step))
}

/// The calls on one line that name a raw value.
fn raw_values(line: &str) -> Vec<&str> {
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let mut found = Vec::new();
    let mut rest = line;
    let mut before = ' ';
    while let Some(start) = rest.find(is_word) {
        before = rest[..start].chars().next_back().unwrap_or(before);
        let word = &rest[start..];
        let end = word.find(|c| !is_word(c)).unwrap_or(word.len());
        let (name, after) = word.split_at(end);
        if after.starts_with('(') && is_raw(name, before == '.') {
            found.push(name);
        }
        before = name.chars().next_back().unwrap_or(before);
        rest = after;
    }
    if line.contains("FontWeight::") {
        found.push("FontWeight");
    }
    found
}

fn check(directory: &Path, violations: &mut Vec<String>) {
    for entry in fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            if !path.ends_with("src/theme") {
                check(&path, violations);
            }
        } else if path.extension().is_some_and(|extension| extension == "rs")
            && !path.ends_with("src/theme.rs")
        {
            let source = fs::read_to_string(&path).unwrap();
            for (number, line) in source.lines().enumerate() {
                if line.trim_start().starts_with("//") {
                    continue;
                }
                for name in raw_values(line) {
                    violations.push(format!("{}:{}: `{name}`", path.display(), number + 1));
                }
            }
        }
    }
}

#[test]
fn only_the_theme_names_raw_colours_and_sizes() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut violations = Vec::new();
    check(&root.join("src"), &mut violations);
    check(&root.join("examples"), &mut violations);
    assert!(
        violations.is_empty(),
        "use a value from `cx.theme()` instead of a raw one:\n{}",
        violations.join("\n")
    );
}

#[test]
fn the_check_recognises_raw_values() {
    assert_eq!(raw_values("div().bg(rgb(0xff0000)).p_4()"), ["rgb", "p_4"]);
    assert_eq!(
        raw_values("this.w(px(12.)).rounded_md().text_sm()"),
        ["px", "rounded_md", "text_sm"]
    );
    assert_eq!(
        raw_values("div().gap(gpui::rems(1.)).shadow_lg()"),
        ["rems", "shadow_lg"]
    );
    assert!(raw_values("div().px(theme.spacing.md).inset_0().size_full().flex_1()").is_empty());
    assert!(raw_values("this.bg(transparent_black()).rounded(theme.radius.md)").is_empty());
}
