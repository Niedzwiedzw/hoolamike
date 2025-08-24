use {
    crate::{
        config_file::{DownloadersConfig, FixupConfig, GameConfig, HoolamikeConfig, InstallationConfig, NexusConfig},
        gui::{
            fixup,
            helpers::{BoldText, MaybeRelativeTo},
            texconv,
            ttw,
            AppMessage,
            FinalMessage,
            Message,
            TITLE,
        },
        modlist_json::Modlist,
        post_install_fixup::common::Resolution,
    },
    anyhow::Context,
    clipboard_rs::Clipboard,
    iced::{
        alignment::{Horizontal, Vertical},
        border,
        widget::{button, center_x, checkbox, container, scrollable, text, text_input, tooltip, Column, Row, Stack},
        Alignment,
        Color,
        Element,
        Length,
        Padding,
    },
    itertools::Itertools,
    normalize_path::NormalizePath,
    std::{
        convert::identity,
        iter::{empty, once},
        ops::Not,
        path::{Path, PathBuf},
    },
    tap::prelude::*,
};

/// GUESTTIMATED
pub const FONT_SIZE: f32 = 10.;

impl super::State {
    pub fn view(&self) -> Element<'_, AppMessage> {
        self.pipe(
            |Self {
                 output_command,
                 error,
                 config_path: _,
                 config,
                 theme: _,
                 loaded_modlist_json,
                 loaded_image,
                 required_games,
                 project_root,
             }| {
                let config_editor = config.pipe(
                    |HoolamikeConfig {
                         downloaders:
                             DownloadersConfig {
                                 downloads_directory,
                                 nexus: NexusConfig { api_key },
                             },
                         installation:
                             InstallationConfig {
                                 wabbajack_file_path,
                                 installation_path,
                             },
                         games,
                         fixup,
                         extras,
                     }| {
                        let config = config.clone();
                        enum PromptMode {
                            File,
                            Directory,
                        }

                        fn table_entry_alignment<'a, Message>(
                            tooltip_content: String,
                            title: String,
                            middle: impl Into<Element<'a, Message>>,
                            last: impl Into<Element<'a, Message>>,
                        ) -> Element<'a, Message>
                        where
                            Message: 'a,
                        {
                            Row::with_children([
                                text(title)
                                    .bold()
                                    .width(Length::Fixed(" wabbajack file path ".len() as f32 * FONT_SIZE))
                                    .pipe(move |el| {
                                        tooltip(
                                            el,
                                            container(text(tooltip_content))
                                                .padding(10)
                                                .style(container::rounded_box),
                                            tooltip::Position::Bottom,
                                        )
                                    })
                                    .conv::<Element<_, _, _>>(),
                                // TODO: add manual edit with validation
                                container(middle)
                                    .width(Length::Fill)
                                    .align_x(Horizontal::Left)
                                    .conv(),
                                last.conv(),
                            ])
                            .align_y(Vertical::Center)
                            .spacing(20)
                            .into()
                        }
                        fn section<'text, 'a, M: 'a>(title: &'text str) -> Element<'a, M> {
                            container(text(title.to_uppercase()).bold())
                                .padding(Padding {
                                    top: 40.,
                                    bottom: 20.,
                                    ..Default::default()
                                })
                                .width(Length::Fill)
                                .align_x(Horizontal::Center)
                                .into()
                        }
                        fn path_entry<'a>(tooltip_content: &str, name: &str, current: &Path, mode: PromptMode) -> Element<'a, Option<PathBuf>> {
                            let name = name.to_string();
                            button(text("Browse..."))
                                .on_press_with({
                                    cloned![name];
                                    let current = current.to_owned();
                                    move || {
                                        fn first_sane_path(p: &Path) -> Option<PathBuf> {
                                            p.normalize().pipe(|p| {
                                                std::iter::successors(Some(p.clone()), |p| p.parent().map(|p| p.to_owned())).find_map(|p| p.canonicalize().ok())
                                            })
                                        }

                                        rfd::FileDialog::new()
                                            .pipe(|dialog| match mode {
                                                PromptMode::File => match current.parent() {
                                                    Some(parent) => {
                                                        dialog.set_directory(first_sane_path(parent).unwrap_or_else(|| std::env::current_dir().unwrap()))
                                                    }
                                                    None => dialog.set_directory(std::env::current_dir().expect("to have cwd")),
                                                },
                                                PromptMode::Directory => {
                                                    dialog.set_directory(first_sane_path(&current).unwrap_or_else(|| std::env::current_dir().unwrap()))
                                                }
                                            })
                                            .set_title(name.clone())
                                            .pipe(|d| match mode {
                                                PromptMode::File => d.pick_file(),
                                                PromptMode::Directory => d.pick_folder(),
                                            })
                                    }
                                })
                                .pipe(move |button| {
                                    table_entry_alignment(tooltip_content.to_string(), name.to_string(), text(current.display().to_string()), button)
                                })
                        }

                        fn text_input_entry<'a>(tooltip_content: &str, placeholder: &str, name: &str, current: &str) -> Element<'a, String> {
                            table_entry_alignment(
                                tooltip_content.into(),
                                name.to_string(),
                                text_input(placeholder, current).on_input(identity),
                                text(""),
                            )
                        }

                        fn text_input_entry_password<'a>(tooltip_content: &str, placeholder: &str, name: &str, current: &str) -> Element<'a, String> {
                            table_entry_alignment(
                                tooltip_content.into(),
                                name.to_string(),
                                text_input(placeholder, current)
                                    .secure(true)
                                    .on_input(identity),
                                text(""),
                            )
                        }

                        // false positive
                        #[allow(clippy::needless_borrows_for_generic_args)]
                        {
                            fn non_fallible(m: Option<HoolamikeConfig>) -> AppMessage {
                                m.map(Ok).map(Message::TryUpdateConfig)
                            }
                            Column::with_children(
                                empty()
                                    // INSTALLATION
                                    .chain([
                                        section("installation"),
                                        path_entry(
                                            "Path to the .wabbajack file, you probably wanna place it in root directory",
                                            "wabbajack file path",
                                            wabbajack_file_path,
                                            PromptMode::File,
                                        )
                                        .map({
                                            cloned![project_root];
                                            move |p| {
                                                p.map(|p| p.maybe_relative_to_exists(&project_root))
                                                    .map(Message::SelectWabbajackFile)
                                            }
                                        }),
                                        path_entry(
                                            "Installation path - this is where .wabbajack files will be extracted. Default is fine.",
                                            "installation path",
                                            installation_path,
                                            PromptMode::Directory,
                                        )
                                        .map({
                                            cloned![config];
                                            move |p| {
                                                p.map(|p| {
                                                    config
                                                        .clone()
                                                        .tap_mut(|c| c.installation.installation_path = p.maybe_relative_to(&project_root))
                                                })
                                            }
                                        })
                                        .map(non_fallible),
                                    ])
                                    // DOWNLOADS
                                    .chain([
                                        section("downloads"),
                                        path_entry(
                                            "Downloads from Nexus and also copied game assets will be put here.",
                                            "downloads directory",
                                            downloads_directory,
                                            PromptMode::Directory,
                                        )
                                        .map({
                                            cloned![config];
                                            move |p| {
                                                p.map(|p| {
                                                    config
                                                        .clone()
                                                        .tap_mut(|c| c.downloaders.downloads_directory = p.maybe_relative_to(&project_root))
                                                })
                                            }
                                        })
                                        .map(non_fallible),
                                        text_input_entry_password(
                                            "Your Nexus api key for premium downloads.  You can also use nxm handler for non-premium accounts - read \
                                             `hoolamike handle-nxm --help` for details",
                                            "<optional>",
                                            "nexus api key",
                                            &api_key.clone().unwrap_or_default(),
                                        )
                                        .map({
                                            cloned![config];
                                            move |api_key| {
                                                config.clone().tap_mut(|config| {
                                                    config.downloaders.nexus.api_key = match api_key.is_empty() {
                                                        true => None,
                                                        false => Some(api_key),
                                                    }
                                                })
                                            }
                                        })
                                        .map(Some)
                                        .map(non_fallible),
                                    ])
                                    .chain(
                                        games
                                            .iter()
                                            .map(|(game_name, GameConfig { root_directory })| {
                                                path_entry(
                                                    &format!("Game directory for {game_name}."),
                                                    &game_name.to_string(),
                                                    root_directory,
                                                    PromptMode::Directory,
                                                )
                                                .map({
                                                    cloned![config];
                                                    move |p| {
                                                        p.map(|p| {
                                                            config
                                                                .clone()
                                                                .tap_mut(|c| c.games[game_name].root_directory = p)
                                                        })
                                                    }
                                                })
                                                .map(non_fallible)
                                            }),
                                    )
                                    // GAME DIRECTORIES
                                    .chain(section("game directories").pipe(once))
                                    .chain(
                                        required_games
                                            .iter()
                                            .filter(|r| games.contains_key(*r).not())
                                            .map(|game_name| {
                                                path_entry(
                                                    &format!("You still have to set up game directory for {game_name}."),
                                                    &game_name.to_string(),
                                                    Path::new("FIXME"),
                                                    PromptMode::Directory,
                                                )
                                                .map({
                                                    cloned![config];
                                                    move |p| {
                                                        p.map(|p| {
                                                            config.clone().tap_mut(|c| {
                                                                c.games
                                                                    .insert(game_name.clone(), GameConfig { root_directory: p });
                                                            })
                                                        })
                                                    }
                                                })
                                                .map(non_fallible)
                                                .pipe(|entry| {
                                                    container(entry)
                                                        .style(|theme| {
                                                            iced::widget::container::Style::default()
                                                                .border(border::color(theme.extended_palette().warning.strong.color).width(4))
                                                        })
                                                        .align_y(Vertical::Center)
                                                        .padding(20)
                                                        .conv::<Element<_>>()
                                                })
                                            }),
                                    )
                                    .chain(
                                        // FIXUP
                                        empty()
                                            .chain(section("fixup").pipe(once))
                                            .chain(
                                                checkbox("Use fixup (used in some bethesda game modlists)", config.fixup.is_some())
                                                    .on_toggle(|t| t.pipe(Message::ToggleFixup).pipe(Some))
                                                    .conv::<Element<_>>()
                                                    .pipe(once),
                                            )
                                            .chain(
                                                fixup
                                                    .as_ref()
                                                    .map(|FixupConfig { game_resolution }| {
                                                        text_input_entry(
                                                            "Game resolution which will be automatically applied for Bethesda games. Format is '1280x800'",
                                                            "game resolution",
                                                            "game resolution",
                                                            game_resolution.to_string().as_str(),
                                                        )
                                                        .map({
                                                            cloned![config];
                                                            move |resolution| {
                                                                resolution
                                                                    .parse::<Resolution>()
                                                                    .context("bad resolution value")
                                                                    .map({
                                                                        cloned![config];
                                                                        move |resolution: Resolution| {
                                                                            config.clone().tap_mut(|c| {
                                                                                c.fixup
                                                                                    .get_or_insert_with(fixup::default_fixup)
                                                                                    .game_resolution = resolution;
                                                                            })
                                                                        }
                                                                    })
                                                                    .pipe(Message::TryUpdateConfig)
                                                                    .pipe(Some)
                                                            }
                                                        })
                                                        .pipe(once)
                                                    })
                                                    .into_iter()
                                                    .flatten(),
                                            ),
                                    )
                                    .chain(
                                        // TEXCONV WINE
                                        empty()
                                            .chain(section("texconv (via wine)").pipe(once))
                                            .chain(
                                                checkbox(
                                                    "use texconv to recompress textures (faster, wine only)",
                                                    config
                                                        .extras
                                                        .as_ref()
                                                        .and_then(|e| e.texconv_wine.as_ref())
                                                        .is_some(),
                                                )
                                                .on_toggle(|t| t.pipe(Message::ToggleTexconv).pipe(Some))
                                                .conv::<Element<_>>()
                                                .pipe(once),
                                            )
                                            .chain(
                                                config
                                                    .extras
                                                    .as_ref()
                                                    .and_then(|e| e.texconv_wine.as_ref())
                                                    .is_some()
                                                    .then(|| {
                                                        use crate::extensions::texconv_wine::ExtensionConfig;

                                                        extras
                                                            .as_ref()
                                                            .and_then(|e| e.texconv_wine.as_ref())
                                                            .pipe(|e| {
                                                                e.cloned()
                                                                    .unwrap_or_else(texconv::default_extension_config)
                                                                    .pipe(|ExtensionConfig { wine_path, texconv_path }| {
                                                                        empty()
                                                                            .chain(
                                                                                path_entry(
                                                                                    "Path to the wine binary, you can probably leave it as the default value \
                                                                                     ('wine')",
                                                                                    "path to wine binary",
                                                                                    &wine_path,
                                                                                    PromptMode::File,
                                                                                )
                                                                                .map({
                                                                                    cloned![config];
                                                                                    move |p| {
                                                                                        p.map(|p| {
                                                                                            config.clone().tap_mut(|c| {
                                                                                                c.extras
                                                                                                    .get_or_insert_with(texconv::default_extras)
                                                                                                    .texconv_wine
                                                                                                    .get_or_insert_with(texconv::default_extension_config)
                                                                                                    .wine_path = p.maybe_relative_to_exists(project_root)
                                                                                            })
                                                                                        })
                                                                                    }
                                                                                })
                                                                                .map(non_fallible)
                                                                                .pipe(once),
                                                                            )
                                                                            .chain(
                                                                                path_entry(
                                                                                    "Path to texconv.exe, you should download it from official source",
                                                                                    " path to texconv.exe",
                                                                                    &texconv_path,
                                                                                    PromptMode::File,
                                                                                )
                                                                                .map({
                                                                                    cloned![config];
                                                                                    move |p| {
                                                                                        p.map(|p| {
                                                                                            config.clone().tap_mut(|c| {
                                                                                                c.extras
                                                                                                    .get_or_insert_with(texconv::default_extras)
                                                                                                    .texconv_wine
                                                                                                    .get_or_insert_with(texconv::default_extension_config)
                                                                                                    .texconv_path = p.maybe_relative_to_exists(&project_root)
                                                                                            })
                                                                                        })
                                                                                    }
                                                                                })
                                                                                .map(non_fallible)
                                                                                .pipe(once),
                                                                            )
                                                                            .collect_vec()
                                                                    })
                                                            })
                                                    })
                                                    .into_iter()
                                                    .flatten(),
                                            ),
                                    )
                                    // TALE OF TWO WASTELANDS
                                    .chain(
                                        empty()
                                            .chain(section("tale of two wastelands").pipe(once))
                                            .chain(
                                                checkbox(
                                                    "install tale of two wastelands",
                                                    config
                                                        .extras
                                                        .as_ref()
                                                        .and_then(|e| e.tale_of_two_wastelands.as_ref())
                                                        .is_some(),
                                                )
                                                .on_toggle(|t| t.pipe(Message::ToggleTTW).pipe(Some))
                                                .conv::<Element<_>>()
                                                .pipe(once),
                                            )
                                            .chain(
                                                config
                                                    .extras
                                                    .as_ref()
                                                    .and_then(|e| e.tale_of_two_wastelands.as_ref())
                                                    .is_some()
                                                    .then(|| {
                                                        use crate::extensions::tale_of_two_wastelands_installer::ExtensionConfig;

                                                        extras
                                                            .as_ref()
                                                            .and_then(|e| e.tale_of_two_wastelands.as_ref())
                                                            .pipe(|e| {
                                                                e.cloned()
                                                                    .unwrap_or_else(ttw::default_extension_config)
                                                                    .pipe(
                                                                        |ExtensionConfig {
                                                                             path_to_ttw_mpi_file,
                                                                             variables,
                                                                         }| {
                                                                            empty()
                                                                                .chain(
                                                                                    path_entry(
                                                                                        "Path to Tale of Two Wastelands installer (.MPI file)",
                                                                                        "TTW MPI file",
                                                                                        &path_to_ttw_mpi_file,
                                                                                        PromptMode::File,
                                                                                    )
                                                                                    .map({
                                                                                        cloned![config];
                                                                                        move |p| {
                                                                                            p.map(|p| {
                                                                                                config.clone().tap_mut(|c| {
                                                                                                    c.extras
                                                                                                        .get_or_insert_with(ttw::default_extras)
                                                                                                        .tale_of_two_wastelands
                                                                                                        .get_or_insert_with(ttw::default_extension_config)
                                                                                                        .path_to_ttw_mpi_file =
                                                                                                        p.maybe_relative_to_exists(&project_root)
                                                                                                })
                                                                                            })
                                                                                        }
                                                                                    })
                                                                                    .map(non_fallible)
                                                                                    .pipe(once),
                                                                                )
                                                                                .chain(variables.clone().into_iter().map(|(name, value)| {
                                                                                    path_entry(
                                                                                        &format!(
                                                                                            "'{name}' is a required parameter for the installer, consult \
                                                                                             installation guide to see what should the value be"
                                                                                        ),
                                                                                        &name,
                                                                                        Path::new(value.as_str()),
                                                                                        PromptMode::Directory,
                                                                                    )
                                                                                    .map({
                                                                                        cloned![config];
                                                                                        move |p| {
                                                                                            p.map(|p| {
                                                                                                config.clone().tap_mut(|c| {
                                                                                                    c.extras
                                                                                                        .get_or_insert_with(ttw::default_extras)
                                                                                                        .tale_of_two_wastelands
                                                                                                        .get_or_insert_with(ttw::default_extension_config)
                                                                                                        .variables
                                                                                                        .insert(name.clone(), p.display().to_string());
                                                                                                })
                                                                                            })
                                                                                        }
                                                                                    })
                                                                                    .map(non_fallible)
                                                                                }))
                                                                                .collect_vec()
                                                                        },
                                                                    )
                                                            })
                                                    })
                                                    .into_iter()
                                                    .flatten(),
                                            ),
                                    )
                                    .chain(section("run installation").pipe(once))
                                    .chain(match output_command {
                                        Some(output_command) => Row::with_children([
                                            text_input("", output_command)
                                                .width(Length::Fill)
                                                .conv::<Element<_>>(),
                                            button("COPY COMMAND")
                                                .on_press_with(|| {
                                                    clipboard_rs::ClipboardContext::new()
                                                        .map_err(|e| anyhow::anyhow!("{e:?}"))
                                                        .context("instantiating clipboard")
                                                        .and_then(|clipboard| {
                                                            clipboard
                                                                .set_text(output_command.clone())
                                                                .map_err(|e| anyhow::anyhow!("{e:?}"))
                                                                .context("copying to clipboard")
                                                        })
                                                        .pipe(|res| {
                                                            if let Err(e) = res {
                                                                tracing::error!("could not copy to clipboard:\n{e:?}")
                                                            }
                                                        })
                                                })
                                                .conv::<Element<()>>(),
                                        ])
                                        .spacing(20)
                                        .padding(20)
                                        .conv::<Element<_>>()
                                        .map::<AppMessage>(|_| None)
                                        .pipe(once),
                                        None => Row::with_children([
                                            button("SAVE")
                                                .on_press_with(|| FinalMessage::Save)
                                                .conv::<Element<_>>(),
                                            button("SAVE AND RUN")
                                                .on_press_with(|| FinalMessage::SaveAndRun)
                                                .into(),
                                        ])
                                        .spacing(20)
                                        .padding(20)
                                        .pipe(|r| {
                                            container(r)
                                                .width(Length::Fill)
                                                .align_x(Horizontal::Center)
                                                .align_y(Vertical::Center)
                                        })
                                        .conv::<Element<_>>()
                                        .map(|m| Some(Message::Final(m)))
                                        .pipe(once),
                                    }),
                            )
                            .align_x(Horizontal::Center)
                            .padding(20)
                            .spacing(15)
                        }
                    },
                );
                let main_content = Column::with_children([
                    loaded_modlist_json
                        .as_ref()
                        .map(|f| &f.modlist)
                        .map(
                            |Modlist {
                                 name,
                                 version,
                                 image: _,
                                 game_type,
                                 author,
                                 ..
                             }| {
                                text(format!("[{game_type}]: \"{name}\" by {author} (v{version})"))
                                    .bold()
                                    .conv::<Element<_>>()
                            },
                        )
                        .into(),
                    scrollable(config_editor)
                        .height(Length::FillPortion(3))
                        .conv::<Element<_, _, _>>(),
                    scrollable(center_x(
                        text(error.as_ref().map(|e| format!("{e:?}")).unwrap_or_default()).color(Color::from_rgb(1., 0.5, 0.)),
                    ))
                    .height(Length::FillPortion(1))
                    .conv(),
                ])
                .spacing(20);
                Stack::with_children(
                    std::iter::empty()
                        .chain(
                            loaded_image
                                .as_ref()
                                .map(iced::widget::image)
                                .map(|e| e.conv::<Element<_>>()),
                        )
                        .chain(
                            container(
                                Column::with_children([
                                    text(TITLE)
                                        .bold()
                                        .width(Length::Fill)
                                        .align_x(Alignment::Center)
                                        .conv::<Element<_, _, _>>(),
                                    text_input("", &format!("{}", project_root.display()))
                                        .width(Length::Fill)
                                        .align_x(Alignment::Center)
                                        .conv::<Element<_, _, _>>()
                                        .map::<AppMessage>(|_: String| None)
                                        .pipe(|root| {
                                            Row::with_children([text("ROOT:").conv::<Element<_>>(), root])
                                                .align_y(Vertical::Center)
                                                .spacing(20)
                                        })
                                        .into(),
                                    main_content.into(),
                                ])
                                // .pipe(|c| {
                                // // DEBUG STUFF
                                //     #[cfg(not(debug_assertions))]
                                //     {
                                //         c
                                //     }
                                //     #[cfg(debug_assertions)]
                                //     {
                                //         use iced::widget::text_editor;
                                //         c.extend([text_editor(
                                //             text_editor::Content::with_text(
                                //                 serde_json::to_string_pretty(&self)
                                //                     .unwrap()
                                //                     .pipe(Box::new)
                                //                     .pipe(Box::leak::<'static>),
                                //             )
                                //             .pipe(Box::new)
                                //             .pipe(Box::leak::<'static>),
                                //         )
                                //         .conv::<Element<_>>()
                                //         .map::<AppMessage>(|_: text_editor::Action| None)])
                                //     }
                                // })
                                .spacing(20),
                            )
                            .padding(30)
                            .width(Length::Fill)
                            .conv::<Element<_>>()
                            .pipe(once),
                        ),
                )
                .height(Length::Fill)
            },
        )
        .into()
    }
}
