use {
    console::{Style, style},
    similar::{ChangeTag, TextDiff},
    std::fmt,
    tap::prelude::*,
};

struct Line(Option<usize>);

impl fmt::Display for Line {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.0 {
            None => write!(f, "    "),
            Some(idx) => write!(f, "{:<4}", idx + 1),
        }
    }
}

#[derive(Clone, Copy)]
pub struct PrettyDiff<'a> {
    pub old: &'a str,
    pub new: &'a str,
}

impl<'a> PrettyDiff<'a> {
    pub fn new(old: &'a str, new: &'a str) -> Self {
        Self { old, new }
    }
    pub fn is_empty(&self) -> bool {
        self.old.eq(self.new)
    }
}

impl std::fmt::Display for PrettyDiff<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        (*self).pipe(|Self { old, new }| {
            TextDiff::from_lines(old, new).pipe_ref(|diff| {
                diff.grouped_ops(3)
                    .iter()
                    .enumerate()
                    .flat_map(|(idx, group)| {
                        group
                            .iter()
                            .flat_map(|op| diff.iter_inline_changes(op))
                            .map(move |a| (idx, a))
                    })
                    .try_for_each(|(idx, change)| {
                        if idx > 0 {
                            writeln!(f, "{:-^1$}", "-", 80)?;
                        }
                        let (sign, s) = match change.tag() {
                            ChangeTag::Delete => ("-", Style::new().red()),
                            ChangeTag::Insert => ("+", Style::new().green()),
                            ChangeTag::Equal => (" ", Style::new().dim()),
                        };
                        write!(
                            f,
                            "{}{} |{}",
                            style(Line(change.old_index())).dim(),
                            style(Line(change.new_index())).dim(),
                            s.apply_to(sign).bold(),
                        )?;
                        for (emphasized, value) in change.iter_strings_lossy() {
                            if emphasized {
                                write!(f, "{}", s.apply_to(value).underlined().on_black())?;
                            } else {
                                write!(f, "{}", s.apply_to(value))?;
                            }
                        }
                        if change.missing_newline() {
                            writeln!(f)?;
                        }
                        Ok(())
                    })
            })
        })
    }
}
