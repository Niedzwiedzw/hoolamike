use {
    crate::{
        compression::{zip::ZipArchive, ProcessArchive},
        config_file::{DownloadersConfig, FixupConfig, GameConfig, HoolamikeConfig, InstallationConfig, NexusConfig},
        gui::helpers::{BoldText, MaybeRelativeTo},
        modlist_json::{GameFileSourceState, GameName, Modlist},
        post_install_fixup::common::Resolution,
        wabbajack_file::WabbajackFile,
        Cli,
    },
    anyhow::{Context, Result},
    futures::{FutureExt, TryFutureExt},
    iced::{
        alignment::{Horizontal, Vertical},
        border,
        widget::{button, center_x, checkbox, container, image::Handle as ImageHandle, scrollable, text, text_input, Column, Row, Stack},
        Alignment,
        Color,
        Element,
        Length,
        Padding,
        Task,
        Theme,
    },
    image::{DynamicImage, GenericImage, GenericImageView},
    itertools::Itertools,
    normalize_path::NormalizePath,
    std::{
        collections::BTreeSet,
        convert::identity,
        future::ready,
        io::{BufRead, Read, Seek},
        iter::{empty, once},
        ops::Not,
        path::{Path, PathBuf},
    },
    tap::prelude::*,
};

mod helpers {
    use {
        extension_traits::extension,
        iced::{font::Weight, Font},
        normalize_path::NormalizePath,
        std::path::{Path, PathBuf},
        tap::Pipe,
    };

    #[extension(pub trait BoldText)]
    impl<'a> iced::widget::Text<'a> {
        fn bold(self) -> Self {
            self.font(Font {
                weight: Weight::Bold,
                ..Font::DEFAULT
            })
        }
    }

    #[extension(pub trait MaybeRelativeTo)]
    impl<P: AsRef<Path>> P {
        fn maybe_relative_to<Parent: AsRef<Path>>(&self, parent: Parent) -> PathBuf {
            match self
                .as_ref()
                .normalize()
                .strip_prefix(parent.as_ref().normalize())
            {
                Ok(relative) => relative.to_owned(),
                Err(_) => self.as_ref().normalize().to_owned(),
            }
        }
        fn maybe_relative_to_exists<Parent: AsRef<Path>>(&self, parent: Parent) -> PathBuf {
            match (self.as_ref().canonicalize(), parent.as_ref().canonicalize()) {
                (Ok(this), Ok(parent)) => this
                    .maybe_relative_to(parent)
                    .pipe(|p| if p.exists() { p } else { this }),
                _ => self.as_ref().to_owned(),
            }
        }
    }
}

const TITLE: &str = concat!(clap::crate_name!(), " ", clap::crate_version!());

mod embedded_terminal;

/// GUESTTIMATED
const FONT_SIZE: f32 = 10.;

#[derive(Clone, Debug)]
enum FinalMessage {
    Save,
    SaveAndRun,
}
#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
enum Message {
    Final(FinalMessage),
    TryUpdateConfig(Result<HoolamikeConfig>),
    SelectWabbajackFile(PathBuf),
    ImageLoaded(Result<ImageHandle>),
    ToggleTTW(bool),
    ToggleTexconv(bool),
}

type AppMessage = Option<Message>;

struct State {
    output_command: Option<String>,
    error: Option<anyhow::Error>,
    config_path: PathBuf,
    config: HoolamikeConfig,
    loaded_modlist_json: Option<WabbajackFile>,
    required_games: BTreeSet<GameName>,
    theme: Theme,
    loaded_image: Option<ImageHandle>,
    project_root: PathBuf,
}

fn read_image<R: BufRead + Seek>(bytes: R) -> Result<ImageHandle> {
    image::ImageReader::new(bytes)
        .with_guessed_format()
        .context("bad image format")
        .and_then(|image| image.decode().context("decoding image"))
        .map(|image| image.to_rgba8())
        // lowering the contrast because it's a background image
        .map(DynamicImage::from)
        .tap_ok_mut(|image| {
            image
                .pixels()
                .collect_vec()
                .into_iter()
                .for_each(|(x, y, pixel)| image.put_pixel(x, y, pixel.tap_mut(|p| p.0[3] /= 10)))
        })
        .map(|i| i.to_rgba8())
        .map(|image| ImageHandle::from_rgba(image.width(), image.height(), image.into_raw()))
}

async fn download_image(url: url::Url) -> Result<ImageHandle> {
    const MAX_IMAGE_SIZE: u64 = 20 * 1024 * 1024;
    reqwest::get(url.to_string())
        .map(|r| r.context("performing request"))
        .and_then(|request| {
            request
                .content_length()
                .context("no content length")
                .and_then(|content_length| {
                    (content_length < MAX_IMAGE_SIZE)
                        .then_some(request)
                        .with_context(|| format!("image too large for download (it is {content_length} bytes, max is {MAX_IMAGE_SIZE} bytes)"))
                })
                .pipe(ready)
        })
        .and_then(|request| request.bytes().map(|r| r.context("fetching bytes")))
        .and_then(|bytes| read_image(std::io::Cursor::new(bytes)).pipe(ready))
        .await
        .with_context(|| format!("fetching image at [{url}]"))
}

fn load_image_from_zip(wabbajack_file: PathBuf, path: PathBuf) -> Result<ImageHandle> {
    ZipArchive::new(&wabbajack_file)
        .with_context(|| format!("reading wabbajack file contents at [{wabbajack_file:?}]"))
        .and_then(|mut archive| archive.get_handle(&path))
        .and_then(|mut handle| {
            Vec::new().pipe(|mut buf| {
                handle
                    .read_to_end(&mut buf)
                    .context("extracting image")
                    .map(|_| buf)
            })
        })
        .map(std::io::Cursor::new)
        .and_then(read_image)
}

mod ttw {
    use {
        crate::{config_file::ExtrasConfig, extensions::tale_of_two_wastelands_installer::ExtensionConfig},
        std::{collections::BTreeMap, path::PathBuf},
        tap::prelude::*,
    };

    pub fn default_extension_config() -> ExtensionConfig {
        ExtensionConfig {
            path_to_ttw_mpi_file: PathBuf::from("FIXME"),
            variables: BTreeMap::new().tap_mut(|b| {
                b.insert("USERPROFILE".to_string(), "FIXME".to_string());
                b.insert("DESTINATION".into(), "./mods/[NoDelete] TTW".into());
            }),
        }
    }

    pub fn default_extras() -> ExtrasConfig {
        ExtrasConfig {
            tale_of_two_wastelands: Some(default_extension_config()),
            ..Default::default()
        }
    }
}

mod texconv {
    use {
        crate::{config_file::ExtrasConfig, extensions::texconv_wine::ExtensionConfig},
        std::path::PathBuf,
    };

    pub fn default_extension_config() -> ExtensionConfig {
        ExtensionConfig {
            wine_path: PathBuf::from("wine"),
            texconv_path: PathBuf::from("FIXME"),
        }
    }

    pub fn default_extras() -> ExtrasConfig {
        ExtrasConfig {
            texconv_wine: Some(default_extension_config()),
            ..Default::default()
        }
    }
}

impl State {
    fn update(&mut self, message: AppMessage) -> iced::Task<AppMessage> {
        message
            .and_then(|message| match message {
                Message::TryUpdateConfig(hoolamike_config) => match hoolamike_config {
                    Ok(config) => {
                        self.config = config;
                        None
                    }
                    Err(error) => {
                        self.error = Some(error);
                        None
                    }
                },
                Message::SelectWabbajackFile(path_buf) => WabbajackFile::load_modlist_json(&path_buf).pipe(|res| match res {
                    Ok(file) => {
                        let image_url = file.modlist.image.clone();
                        self.required_games = file
                            .modlist
                            .archives
                            .iter()
                            .filter_map(|a| match &a.state {
                                crate::modlist_json::State::GameFileSource(GameFileSourceState { game, .. }) => Some(game),
                                _ => None,
                            })
                            .collect::<BTreeSet<_>>()
                            .into_iter()
                            .cloned()
                            .collect::<BTreeSet<_>>();
                        self.loaded_modlist_json = Some(file);
                        self.config.installation.wabbajack_file_path = path_buf.clone();

                        Task::perform(
                            match image_url
                                .parse::<url::Url>()
                                .with_context(|| format!("bad image url: {image_url}"))
                            {
                                Ok(url) => download_image(url).boxed(),
                                Err(reason) => {
                                    tracing::debug!("not a url?: {reason:?}");
                                    load_image_from_zip(path_buf, image_url.into())
                                        .pipe(ready)
                                        .boxed()
                                }
                            },
                            |image| Some(Message::ImageLoaded(image)),
                        )
                        .pipe(Some)
                    }
                    Err(reason) => {
                        self.error = Some(reason);
                        None
                    }
                }),
                Message::ImageLoaded(handle) => match handle {
                    Ok(handle) => {
                        self.loaded_image = Some(handle);
                        None
                    }
                    Err(error) => {
                        self.error = Some(error);
                        None
                    }
                },
                Message::ToggleTexconv(to) => {
                    match to {
                        true => {
                            self.config
                                .extras
                                .get_or_insert_with(texconv::default_extras)
                                .texconv_wine
                                .get_or_insert_with(texconv::default_extension_config);
                        }
                        false => {
                            if let Some(extras) = self.config.extras.as_mut() {
                                extras.texconv_wine.take();
                            }
                        }
                    };
                    None
                }
                Message::ToggleTTW(to) => {
                    match to {
                        true => {
                            self.config
                                .extras
                                .get_or_insert_with(ttw::default_extras)
                                .tale_of_two_wastelands
                                .get_or_insert_with(ttw::default_extension_config);
                        }
                        false => {
                            if let Some(extras) = self.config.extras.as_mut() {
                                extras.tale_of_two_wastelands.take();
                            }
                        }
                    }

                    None
                }
                Message::Final(m) => {
                    let write_config = |config: &HoolamikeConfig, config_path: &Path| {
                        config
                            .write()
                            .and_then(|contents| std::fs::write(config_path, contents).with_context(|| format!("writing to [{}]", self.config_path.display())))
                            .context("writing config")
                    };
                    match m {
                        FinalMessage::Save => match write_config(&self.config, &self.config_path) {
                            Ok(()) => {
                                self.error.take();
                                None
                            }
                            Err(error) => {
                                self.error = Some(error);
                                None
                            }
                        },
                        FinalMessage::SaveAndRun => match write_config(&self.config, &self.config_path) {
                            Ok(()) => {
                                self.error.take();
                                self.output_command = Some(format!(
                                    "cd {project_root} && {current_exe} install",
                                    project_root = self.project_root.display(),
                                    current_exe = std::env::current_exe().unwrap().display()
                                ));
                                None
                            }
                            Err(error) => {
                                self.error = Some(error);
                                None
                            }
                        },
                    }
                }
            })
            .unwrap_or_default()
    }

    fn view(&self) -> Element<'_, AppMessage> {
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
                         fixup: FixupConfig { game_resolution },
                         extras,
                     }| {
                        let config = config.clone();
                        enum PromptMode {
                            File,
                            Directory,
                        }

                        fn table_entry_alignment<'a, Message>(
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
                                    .conv::<Element<_, _, _>>(),
                                // TODO: add manual edit with validation
                                container(middle)
                                    .width(Length::Fill)
                                    .align_x(Horizontal::Left)
                                    .conv(),
                                last.conv(),
                            ])
                            .spacing(20)
                            .into()
                        }
                        fn section<'text, 'a, M: 'a>(title: &'text str) -> Element<'a, M> {
                            container(text(title.to_uppercase()).bold())
                                .padding(Padding {
                                    top: 20.,
                                    bottom: 10.,
                                    ..Default::default()
                                })
                                .width(Length::Fill)
                                .align_x(Horizontal::Center)
                                .into()
                        }
                        fn path_entry<'a>(name: &str, current: &Path, mode: PromptMode) -> Element<'a, Option<PathBuf>> {
                            let name = name.to_string();
                            button(text("Browse..."))
                                .on_press_with({
                                    cloned![name];
                                    let current = current.to_owned();
                                    move || {
                                        fn first_sane_path(p: &Path) -> PathBuf {
                                            p.normalize().pipe(|p| {
                                                std::iter::successors(Some(p.clone()), |p| p.parent().map(|p| p.to_owned()))
                                                    .find_map(|p| p.canonicalize().ok())
                                                    .unwrap_or(p)
                                            })
                                        }

                                        rfd::FileDialog::new()
                                            .pipe(|dialog| match mode {
                                                PromptMode::File => match current.parent() {
                                                    Some(parent) => dialog.set_directory(first_sane_path(parent)),
                                                    None => dialog.set_directory(std::env::current_dir().expect("to have cwd")),
                                                },
                                                PromptMode::Directory => dialog.set_directory(first_sane_path(&current)),
                                            })
                                            .set_title(name.clone())
                                            .pipe(|d| match mode {
                                                PromptMode::File => d.pick_file(),
                                                PromptMode::Directory => d.pick_folder(),
                                            })
                                    }
                                })
                                .pipe(move |button| table_entry_alignment(name.to_string(), text(current.display().to_string()), button))
                        }

                        fn text_input_entry<'a>(placeholder: &str, name: &str, current: &str) -> Element<'a, String> {
                            table_entry_alignment(name.to_string(), text_input(placeholder, current).on_input(identity), text(""))
                        }

                        fn text_input_entry_password<'a>(placeholder: &str, name: &str, current: &str) -> Element<'a, String> {
                            table_entry_alignment(
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
                                        path_entry("wabbajack file path", wabbajack_file_path, PromptMode::File).map({
                                            cloned![project_root];
                                            move |p| {
                                                p.map(|p| p.maybe_relative_to_exists(&project_root))
                                                    .map(Message::SelectWabbajackFile)
                                            }
                                        }),
                                        path_entry("installation path", installation_path, PromptMode::Directory)
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
                                        path_entry("downloads directory", downloads_directory, PromptMode::Directory)
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
                                            "<optional, you can also use `hoolamike handle-nxm --help`>",
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
                                                path_entry(&game_name.to_string(), root_directory, PromptMode::Directory)
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
                                                path_entry(&game_name.to_string(), Path::new("FIXME"), PromptMode::Directory)
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
                                                            .padding(10)
                                                            .conv::<Element<_>>()
                                                    })
                                            }),
                                    )
                                    .chain(
                                        // FIXUP
                                        empty().chain(section("fixup").pipe(once)).chain(
                                            text_input_entry("game resolution", "game resolution", game_resolution.to_string().as_str())
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
                                                                        c.fixup.game_resolution = resolution;
                                                                    })
                                                                }
                                                            })
                                                            .pipe(Message::TryUpdateConfig)
                                                            .pipe(Some)
                                                    }
                                                })
                                                .pipe(once),
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
                                                                                path_entry("path to wine binary", &wine_path, PromptMode::File)
                                                                                    .map({
                                                                                        cloned![config];
                                                                                        move |p| {
                                                                                            p.map(|p| {
                                                                                                config.clone().tap_mut(|c| {
                                                                                                    c.extras
                                                                                                        .get_or_insert_with(texconv::default_extras)
                                                                                                        .texconv_wine
                                                                                                        .get_or_insert_with(texconv::default_extension_config)
                                                                                                        .wine_path = p
                                                                                                })
                                                                                            })
                                                                                        }
                                                                                    })
                                                                                    .map(non_fallible)
                                                                                    .pipe(once),
                                                                            )
                                                                            .chain(
                                                                                path_entry("path to texconv.exe", &texconv_path, PromptMode::File)
                                                                                    .map({
                                                                                        cloned![config];
                                                                                        move |p| {
                                                                                            p.map(|p| {
                                                                                                config.clone().tap_mut(|c| {
                                                                                                    c.extras
                                                                                                        .get_or_insert_with(texconv::default_extras)
                                                                                                        .texconv_wine
                                                                                                        .get_or_insert_with(texconv::default_extension_config)
                                                                                                        .texconv_path =
                                                                                                        p.maybe_relative_to_exists(&project_root)
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
                                                                                    path_entry("TTW MPI file", &path_to_ttw_mpi_file, PromptMode::File)
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
                                                                                    path_entry(&name, Path::new(value.as_str()), PromptMode::Directory)
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
                                            button("paste into terminal").conv::<Element<()>>(),
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
                                        .width(Length::Fill)
                                        .conv::<Element<_>>()
                                        .map(|m| Some(Message::Final(m)))
                                        .pipe(once),
                                    }),
                            )
                            .align_x(Horizontal::Center)
                            .spacing(5)
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
                        .height(Length::Fixed(APP_SIZE.1 / 4. * 3.))
                        .conv::<Element<_, _, _>>(),
                    scrollable(center_x(
                        text(error.as_ref().map(|e| format!("{e:?}")).unwrap_or_default()).color(Color::from_rgb(1., 0.5, 0.)),
                    ))
                    .conv(),
                ])
                .spacing(10);
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
                                .spacing(20),
                            )
                            .padding(20)
                            .conv::<Element<_>>()
                            .pipe(once),
                        ),
                )
            },
        )
        .into()
    }

    fn new(
        Cli {
            hoolamike_config,
            command: _,
            logging_mode: _,
            nxm_link_handler_port: _,
            nxm_link: _,
        }: Cli,
    ) -> (Self, Task<AppMessage>) {
        const DEFAULT_THEME: Theme = Theme::SolarizedDark;
        HoolamikeConfig::find(&hoolamike_config)
            .context("could not read config, default will be generated")
            .map(|(config_path, config)| {
                Self {
                    output_command: None,
                    theme: DEFAULT_THEME,
                    loaded_modlist_json: None,
                    error: None,
                    config,
                    loaded_image: None,
                    required_games: Default::default(),
                    project_root: config_path
                        .parent()
                        .expect("if this ever happens I'm installing windows")
                        .to_owned(),
                    config_path,
                }
                .pipe(|state| Task::done(Some(Message::SelectWabbajackFile(state.config.installation.wabbajack_file_path.clone()))).pipe(|task| (state, task)))
            })
            .unwrap_or_else(|error| {
                rfd::FileDialog::new()
                    .set_directory(std::env::current_exe().unwrap().parent().unwrap())
                    .set_file_name("hoolamike.yaml")
                    .add_filter("Hoolamike config", &["yaml"])
                    .set_title("IGNORE OVERWRITE WARNING, Root installation location (parent for hoolamike.yaml)")
                    .save_file()
                    .unwrap_or_else(|| std::process::exit(1))
                    .pipe(|hoolamike_config| {
                        let config = HoolamikeConfig::find(&hoolamike_config)
                            .map(|(_, c)| c)
                            .tap_err(|e| tracing::error!("bad config at [{}]\n{e:?}", hoolamike_config.display()));
                        let is_err = config.is_err();
                        Self {
                            output_command: None,
                            theme: DEFAULT_THEME,
                            loaded_modlist_json: None,
                            error: Some(error),
                            project_root: hoolamike_config
                                .parent()
                                .expect("if this ever happens I'm installing macos")
                                .to_owned(),
                            config: config.unwrap_or_default(),
                            config_path: hoolamike_config,
                            loaded_image: None,
                            required_games: Default::default(),
                        }
                        .pipe(|state| {
                            match is_err {
                                false => Task::done(Some(Message::SelectWabbajackFile(state.config.installation.wabbajack_file_path.clone()))),
                                true => Task::none(),
                            }
                            .pipe(|task| (state, task))
                        })
                    })
            })
            .tap(|(s, _)| std::env::set_current_dir(&s.project_root).expect("failed to set current working directory"))
    }
}

const APP_SIZE: (f32, f32) = (900., 640.);

pub fn run(cli: Cli) -> Result<()> {
    iced::application(move || State::new(cli.clone()), State::update, State::view)
        .theme(|s| s.theme.clone())
        .title(TITLE)
        .window_size(APP_SIZE)
        .resizable(false)
        .run()
        .map_err(|e| anyhow::anyhow!("{e:?}"))
        .context("running gui")
}
