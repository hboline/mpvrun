use crate::config::Config;
use crate::digest::{discover, read_input, ReadOutput, StreamError};
use crate::{Cli, Mode};
use core::panic;
use open;
use std::process::Command;

#[derive(Debug)]
pub struct Stream {
    id: String,
    mode: Mode,
    mpvargs: Option<Vec<String>>,
    mpv_options: Vec<String>,
    persistent: Option<Vec<String>>,
}

impl Stream {
    pub fn from(args: Cli, config: &Config) -> Result<Self, StreamError> {
        // Resolve once before discovery and use the same browser for playback.
        let cookie_browser = config.cookie_browser(args.use_cookies.as_ref());
        let input = read_input(args.target);

        let (id, upcoming) = match input {
            None => panic!("this shouldn't happen lol"),
            Some(ReadOutput::Id(id)) => (id, false),
            Some(ReadOutput::Channel(channel)) => {
                let entry = discover(&channel, cookie_browser.as_deref(), args.all)?;
                (entry.id, entry.upcoming)
            }
        };

        let mut mpv_options = Vec::new();
        let mut ytdl_options = Vec::new();
        if let Some(browser) = cookie_browser {
            ytdl_options.push(format!("cookies-from-browser={browser}"));
        }
        if !args.debug {
            ytdl_options.push("extractor-args=youtube:player_client=default".to_owned());
        }
        // Repeating --ytdl-raw-options replaces the earlier value, so combine the options.
        if !ytdl_options.is_empty() {
            mpv_options.push(format!("--ytdl-raw-options={}", ytdl_options.join(",")));
        }
        if !args.debug {
            mpv_options.push("--msg-level=all=fatal,statusline=status".to_owned());
        }

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
            mode: if upcoming {
                Mode::Full
            } else if let Some(mode) = args.mode.mode {
                mode
            } else if args.mode.embed {
                Mode::Embed
            } else if args.mode.full {
                Mode::Full
            } else {
                Mode::Mpv
            },
            mpvargs: args.mpvargs,
            mpv_options,
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

            argvec.append(&mut self.mpv_options);

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

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn stream_with(options: &[&str]) -> Stream {
        let args = Cli::try_parse_from(
            ["mpvrun", "https://www.youtube.com/watch?v=abcdefghijk"]
                .into_iter()
                .chain(options.iter().copied()),
        )
        .unwrap();
        Stream::from(args, &Config::default()).unwrap()
    }

    #[test]
    fn cookies_preserve_the_existing_ytdlp_options() {
        let stream = stream_with(&["--use-cookies", "chrome"]);
        assert_eq!(
            stream.mpv_options,
            [
                "--ytdl-raw-options=cookies-from-browser=chrome,extractor-args=youtube:player_client=default",
                "--msg-level=all=fatal,statusline=status",
            ]
        );
    }

    #[test]
    fn explicit_cookies_are_kept_in_debug_mode() {
        let stream = stream_with(&["--debug", "-c", "edge", "--", "--volume=50"]);
        assert_eq!(
            stream.mpv_options,
            ["--ytdl-raw-options=cookies-from-browser=edge"]
        );
        assert_eq!(stream.mpvargs.unwrap(), ["--volume=50"]);
    }

    #[test]
    fn bare_cookie_flag_resolves_to_a_supported_browser() {
        let stream = stream_with(&["--debug", "-c"]);
        let browser = stream.mpv_options[0]
            .strip_prefix("--ytdl-raw-options=cookies-from-browser=")
            .unwrap();
        assert!(crate::browser::SUPPORTED_BROWSERS.contains(&browser));
    }

    #[test]
    fn no_cookie_flag_preserves_previous_options() {
        assert_eq!(
            stream_with(&[]).mpv_options,
            [
                "--ytdl-raw-options=extractor-args=youtube:player_client=default",
                "--msg-level=all=fatal,statusline=status",
            ]
        );
        assert!(stream_with(&["--debug"]).mpv_options.is_empty());
    }
}
