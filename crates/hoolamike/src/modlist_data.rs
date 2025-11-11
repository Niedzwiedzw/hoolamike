use {
    crate::{helpers::human_readable_size, modlist_json::Modlist},
    itertools::Itertools,
    std::collections::BTreeMap,
    tabled::{
        Tabled,
        settings::{Color, Rotate, Style, object::Columns},
    },
    tap::prelude::*,
};

#[derive(Tabled)]
pub struct ModlistSummary {
    pub author: String,
    pub total_mods: usize,
    pub total_directives: usize,
    pub unique_directive_kinds: String,
    // pub unique_authors: usize,
    pub sources: String,
    pub name: String,
    // pub unique_headers: String,
    pub website: String,
    pub total_download_size: String,
    pub description: String,
    pub directive_examples: String,
}

fn summarize_value_count<'a, I: std::fmt::Display + Ord + Clone + Eq>(items: impl Iterator<Item = I> + 'a) -> String {
    items
        .fold(BTreeMap::new(), |acc, directive| {
            acc.tap_mut(move |acc| {
                *acc.entry(directive).or_insert(0) += 1;
            })
        })
        .iter()
        .map(|(k, v)| format!("{k}: {v}"))
        .join("\n")
}
impl ModlistSummary {
    pub fn print(&self) -> String {
        tabled::Table::new([self])
            .with(Style::modern())
            .with(Rotate::Left)
            .modify(Columns::single(0), Color::FG_GREEN)
            .to_string()
    }

    pub fn new(
        Modlist {
            archives,
            author,
            description,
            directives,
            name,
            website,
            is_nsfw: _,
            game_type: _,
            image: _,
            readme: _,
            version: _,
            wabbajack_version: _,
        }: &Modlist,
    ) -> Self {
        Self {
            directive_examples: directives
                .iter()
                .unique_by(|d| d.directive_kind())
                .map(|directive| {
                    (
                        directive.directive_kind(),
                        serde_json::to_string_pretty(&directive).expect("serliaizing directive"),
                    )
                })
                .map(|(kind, directive)| format!("{kind}:\n{directive}"))
                .join("\n\n"),
            author: author.clone(),
            sources: archives
                .iter()
                .map(|archive| archive.state.kind().to_string())
                .pipe(summarize_value_count),
            total_mods: archives.len(),
            // unique_authors: archives
            //     .iter()
            //     .filter_map(|archive| archive.state.author.as_ref())
            //     .unique()
            //     .count(),
            total_directives: directives.len(),
            unique_directive_kinds: directives
                .iter()
                .map(|d| d.directive_kind())
                .pipe(summarize_value_count),
            name: name.clone(),
            // unique_headers: archives
            //     .iter()
            //     .flat_map(|a| {
            //         a.state
            //             .headers
            //             .iter()
            //             .flat_map(|m| m.iter().map(|h| h.as_str()))
            //     })
            //     .unique()
            //     .join(",\n"),
            website: website.clone(),
            total_download_size: archives
                .iter()
                .map(|a| a.descriptor.size)
                .sum::<u64>()
                .pipe(human_readable_size),
            description: description.clone(),
        }
    }
}
