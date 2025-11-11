use {
    super::manifest_file::PostCommand,
    anyhow::{Context, Result},
    futures::TryFutureExt,
    std::path::PathBuf,
    tap::prelude::*,
    tracing::{debug, info, instrument},
    typed_path::Utf8TypedPath,
};

#[derive(PartialEq, Eq, Clone, Debug)]
pub enum ParsedPostCommand {
    Rename(PathBuf, String),
    Delete(PathBuf),
}

fn normalize_windows_shell_path(path: String) -> Result<PathBuf> {
    #[cfg(unix)]
    let path = path.replace(r#"\"#, "/");
    let path = snailquote::unescape(&path).context("unescaping")?;
    match Utf8TypedPath::derive(&path) {
        Utf8TypedPath::Unix(utf8_path) => utf8_path
            .with_platform_encoding_checked()
            .context("converting to platform encoding"),
        Utf8TypedPath::Windows(utf8_path) => utf8_path
            .with_platform_encoding_checked()
            .context("converting to platform encoding"),
    }
    .context("normalizing platform encoding")
    .map(|encoded| encoded.normalize())
    .map(PathBuf::from)
}

impl ParsedPostCommand {
    fn parse(command: &str) -> Result<Self> {
        futures_executor::block_on(async {
            use yash_syntax::{input::Memory, source::Source};
            let input = Box::new(Memory::new(command));
            use {std::num::NonZeroU64, yash_syntax::parser::lex::Lexer};
            let line = NonZeroU64::new(1).expect("not 0");
            let mut lexer = Lexer::new(input, line, Source::Unknown.into());
            use yash_syntax::{alias::EmptyGlossary, parser::Parser};
            let mut parser = Parser::new(&mut lexer, &EmptyGlossary);

            let next = async move |parser: &mut Parser, context: &'static str| {
                parser
                    .take_token_auto(&[])
                    .await
                    .map_err(|e| anyhow::anyhow!("{e:?}"))
                    .context(context)
                    .map(|e| e.to_string())
                    .inspect(|token| debug!(%token))
            };
            let assert_eq = async |a: String, b: &str| {
                if a == b { Ok(()) } else { Err(anyhow::anyhow!("[{a} != {b}]")) }
            };
            next(&mut parser, "cmd.exe")
                .and_then(|a| assert_eq(a, "cmd.exe"))
                .await?;

            next(&mut parser, "slash_c")
                .and_then(|a| assert_eq(a, "/C"))
                .await?;
            let command = next(&mut parser, "command").await?;
            let command = match command.as_str() {
                "del" => {
                    let from = next(&mut parser, "from")
                        .await
                        .and_then(normalize_windows_shell_path)?;
                    Ok(ParsedPostCommand::Delete(from))
                }
                "ren" => {
                    let from = next(&mut parser, "from")
                        .await
                        .and_then(normalize_windows_shell_path)?;
                    let to = next(&mut parser, "to")
                        .await
                        .and_then(|s| snailquote::unescape(&s).context("unescaping"))?;

                    Ok(ParsedPostCommand::Rename(from, to))
                }
                other => Err(anyhow::anyhow!("bad command: [{other}]")),
            }?;
            debug!(?command);

            Ok(command)
        })
    }
}

#[instrument(skip_all)]
pub fn handle_post_commands(post_commands: Vec<PostCommand>) -> Result<()> {
    post_commands.into_iter().try_for_each(|c| {
        ParsedPostCommand::parse(&c.value)
            .and_then(|command| {
                match &command {
                    ParsedPostCommand::Rename(from, new_file_name) => {
                        let (from, to) = (&from, from.with_file_name(new_file_name));
                        std::fs::rename(from, &to).with_context(|| format!("renaming [{from:?}] -> [{to:?}]"))
                    }
                    ParsedPostCommand::Delete(_path_buf) => {
                        info!("skipping {command:?}");
                        Ok(())
                        // std::fs::remove_file(path_buf).with_context(|| format!("removing [{path_buf:?}]"))
                    }
                }
                .tap_ok(|_| info!("executed succesfully: {command:?}"))
            })
            .with_context(|| format!("when handling [{c:#?}]"))
            .or_else(|error| {
                tracing::warn!("{error:?}\n\ncould not execute command, please report this incident and fix it up by trying to run it manually");
                Ok(())
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test_log::test]
    fn test_example_1() -> Result<()> {
        assert_eq!(
            ParsedPostCommand::Delete(PathBuf::from("%DESTINATION%/Fallout - Meshes.bsa")),
            ParsedPostCommand::parse("cmd.exe /C del \"%DESTINATION%\\Fallout - Meshes.bsa\"").unwrap()
        );
        Ok(())
    }

    #[test_log::test]
    fn test_example_2() -> Result<()> {
        assert_eq!(
            ParsedPostCommand::Rename(PathBuf::from("%DESTINATION%/New Fallout - Meshes.bsa"), String::from("Fallout - Meshes.bsa")),
            ParsedPostCommand::parse("cmd.exe /C ren \"%DESTINATION%\\New Fallout - Meshes.bsa\" \"Fallout - Meshes.bsa\"").unwrap()
        );
        Ok(())
    }

    #[test_log::test]
    fn test_example_3() -> Result<()> {
        assert_eq!(
            ParsedPostCommand::Delete(PathBuf::from("%DESTINATION%/Fallout - Textures.bsa")),
            ParsedPostCommand::parse("cmd.exe /C del \"%DESTINATION%\\Fallout - Textures.bsa\"").unwrap()
        );
        Ok(())
    }

    #[test_log::test]
    fn test_example_4() -> Result<()> {
        assert_eq!(
            ParsedPostCommand::Rename(
                PathBuf::from("%DESTINATION%/New Fallout - Textures.bsa"),
                String::from("Fallout - Textures.bsa")
            ),
            ParsedPostCommand::parse("cmd.exe /C ren \"%DESTINATION%\\New Fallout - Textures.bsa\" \"Fallout - Textures.bsa\"").unwrap()
        );
        Ok(())
    }

    #[test_log::test]
    fn test_example_5() -> Result<()> {
        assert_eq!(
            ParsedPostCommand::Delete(PathBuf::from("%DESTINATION%/Fallout - Textures2.bsa")),
            ParsedPostCommand::parse("cmd.exe /C del \"%DESTINATION%\\Fallout - Textures2.bsa\"").unwrap()
        );
        Ok(())
    }

    #[test_log::test]
    fn test_example_6() -> Result<()> {
        assert_eq!(
            ParsedPostCommand::Delete(PathBuf::from("%DESTINATION%/Fallout - Sound.bsa")),
            ParsedPostCommand::parse("cmd.exe /C del \"%DESTINATION%\\Fallout - Sound.bsa\"").unwrap()
        );
        Ok(())
    }
}
