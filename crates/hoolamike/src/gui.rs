use {
    crate::{
        compression::{zip::ZipArchive, ProcessArchive},
        config_file::{HoolamikeConfig, CONFIG_FILE_NAME},
        modlist_json::{GameFileSourceState, GameName},
        wabbajack_file::WabbajackFile,
        Cli,
    },
    anyhow::{anyhow, Context, Result},
    futures::{FutureExt, TryFutureExt},
    iced::{widget::image::Handle as ImageHandle, Task, Theme},
    image::{DynamicImage, GenericImage, GenericImageView},
    itertools::Itertools,
    serde::Serialize,
    std::{
        collections::BTreeSet,
        future::ready,
        io::{BufRead, Read, Seek},
        path::{Path, PathBuf},
    },
    tap::prelude::*,
    tracing::{error, info},
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
    ToggleFixup(bool),
}

type AppMessage = Option<Message>;

#[derive(Serialize)]
struct State {
    output_command: Option<String>,
    #[serde(skip_serializing)]
    error: Option<anyhow::Error>,
    config_path: PathBuf,
    config: HoolamikeConfig,
    #[serde(skip_serializing)]
    loaded_modlist_json: Option<WabbajackFile>,
    required_games: BTreeSet<GameName>,
    #[serde(skip_serializing)]
    theme: Theme,
    #[serde(skip_serializing)]
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

mod fixup {
    use crate::config_file::FixupConfig;

    pub fn default_fixup() -> FixupConfig {
        FixupConfig::default()
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

mod view;

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
                Message::ToggleFixup(to) => {
                    match to {
                        true => {
                            self.config.fixup.get_or_insert_with(fixup::default_fixup);
                        }
                        false => {
                            self.config.fixup.take();
                        }
                    }

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
                            .write_with_gui_message()
                            .and_then(|contents| {
                                std::fs::write(config_path, &contents)
                                    .with_context(|| format!("writing to [{}]", self.config_path.display()))
                                    .tap_ok(|_| info!("saved to {config_path:?}\n{contents}"))
                            })
                            .context("writing config")
                            .tap_err(|e| error!("{e:?}"))
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
        HoolamikeConfig::read(&hoolamike_config)
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
                        .canonicalize()
                        .expect("checked above")
                        .to_owned(),
                    config_path,
                }
                .pipe(|state| Task::done(Some(Message::SelectWabbajackFile(state.config.installation.wabbajack_file_path.clone()))).pipe(|task| (state, task)))
            })
            .unwrap_or_else(|error| {
                rfd::FileDialog::new()
                    .set_directory(std::env::current_exe().unwrap().parent().unwrap())
                    .set_file_name(CONFIG_FILE_NAME)
                    .add_filter("Hoolamike config", &[CONFIG_FILE_NAME.split_once(".").unwrap().1])
                    .set_title("IGNORE OVERWRITE WARNING, Root installation location (parent for hoolamike.yaml)")
                    .save_file()
                    .unwrap_or_else(|| std::process::exit(1))
                    .pipe(|hoolamike_config| {
                        let config = HoolamikeConfig::read(&hoolamike_config)
                            .map(|(_, c)| c)
                            .tap_err(|e| error!("bad config at [{}]\n{e:?}", hoolamike_config.display()));
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
        // .resizable(false)
        .run()
        .map_err(|e| anyhow!("{e:?}"))
        .context("running gui")
}
