use crate::digest::{get_all_streams, get_id, read_input, ReadOutput, StreamError};
use crate::{Cli, Mode};
use core::panic;
use open;
use std::process::Command;

#[derive(Debug)]
pub struct Stream {
    id: String,
    mode: Mode,
    mpvargs: Option<Vec<String>>,
    default_mpv_args: Option<Vec<String>>,
    persistent: Option<Vec<String>>,
}

impl Stream {
    pub fn from(args: Cli) -> Result<Self, StreamError> {
        let input = read_input(args.target);

        let id = match input {
            None => panic!("this shouldn't happen lol"),
            Some(ReadOutput::Id(id)) => Ok(id),
            Some(ReadOutput::Channel(channel)) => {
                if args.all {
                    Ok(get_all_streams(channel))
                } else {
                    get_id(channel)
                }
            }
        }?;

        let default_mpv_args = if !args.debug {
            Some(
                vec![
                    // "--ytdl-raw-options=downloader-args=\"http_persistent:0\"",
                    "--ytdl-raw-options=extractor-args=youtube:player_client=default",
                    "--msg-level=all=fatal,statusline=status",
                ]
                .into_iter()
                .map(std::borrow::ToOwned::to_owned)
                .collect(),
            )
        } else {
            None
        };

        let persistent = if args.persistent {
            Some(
                vec!["--ontop", "--keepaspect-window=yes"]
                    .into_iter()
                    .map(std::string::ToString::to_string)
                    .collect(),
            )
        } else {
            None
        };

        Ok(Stream {
            id,
            mode: if let Some(mode) = args.mode.mode {
                mode
            } else if args.mode.embed {
                Mode::Embed
            } else if args.mode.full {
                Mode::Full
            } else {
                Mode::Mpv
            },
            mpvargs: args.mpvargs,
            default_mpv_args,
            persistent,
        })
    }

    fn info(&self) {
        println!(
            "\n----------------------------------------------------------------------\n\
            STREAM LINK: {url}\n\
            ----------------------------------------------------------------------\n\
            EMBED LINK: {embed}\n\
            ----------------------------------------------------------------------\n\
            CHAT LINK: {id}\n\
            ----------------------------------------------------------------------",
            url = self.link(),
            embed = self.embed(),
            id = self.chat(),
        )
    }

    pub fn chat(&self) -> String {
        format!(
            "https://www.youtube.com/live_chat?is_popout=1&v={}",
            self.id
        )
    }

    pub fn embed(&self) -> String {
        format!("https://www.youtube.com/embed/{}", self.id)
    }

    pub fn link(&self) -> String {
        format!("https://www.youtube.com/watch?v={}", self.id)
    }

    /// Consume Stream object and attempt to play live stream/video
    pub fn play(mut self) -> anyhow::Result<()> {
        self.info();

        let url: String = if self.mode == Mode::Embed {
            self.embed()
        } else {
            self.link()
        };

        if self.mode == Mode::Mpv {
            let mut argvec: Vec<String> = vec!["/C".into(), "mpv".into(), url];

            if let Some(v) = &mut self.default_mpv_args {
                argvec.append(v);
            }

            if let Some(v) = &mut self.mpvargs {
                argvec.append(v);
            }

            if let Some(v) = &mut self.persistent {
                argvec.append(v);
            }

            Command::new("cmd").args(argvec).spawn()?;
        } else {
            open::that(url)?;
        };

        Ok(())
    }
}
