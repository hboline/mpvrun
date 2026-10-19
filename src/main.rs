use clap::{Args, Parser, ValueEnum};
use digest::StreamError;
use stream::Stream;

mod browser;
mod config;
mod constants;
mod digest;
mod stream;

// Specify how to open stream
#[allow(dead_code)]
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Debug)]
pub enum Mode {
    Mpv,
    Embed,
    Full,
}

// Mode struct
#[derive(Args)]
#[group(required = false, multiple = false)]
struct ModeStruct {
    /// Choose how to open stream
    #[arg(value_enum, short, long)]
    mode: Option<Mode>,

    /// Open stream in embedded youtube window
    #[arg(long, default_value_t = false)]
    embed: bool,

    /// Open stream in normal youtube window
    #[arg(long, default_value_t = false)]
    full: bool,
}

// Command-line args struct
#[derive(Parser)]
#[command(name = "mpvrun")]
#[command(version = "2.0")]
#[command(about = "open livestreams from the command line", long_about = None)]
pub struct Cli {
    /// Specify target (url/channel name)
    target: String,

    // Specify stream destination
    #[command(flatten)]
    mode: ModeStruct,

    /// Choose a live or upcoming stream (upcoming streams open in the browser)
    #[arg(short, long)]
    all: bool,

    /// Open mpv without default args
    #[arg(short, long, default_value_t = false)]
    debug: bool,

    /// Enable always on top mode
    #[arg(short, long, default_value_t = false)]
    persistent: bool,

    /// Use browser cookies for discovery and playback
    ///
    /// Omit BROWSER to use [yt-dlp].browser from ~/.config/mpvrun/config.toml,
    /// then the system default browser, falling back to firefox.
    /// Setting [yt-dlp].use-cookies = true enables cookies without this flag.
    /// Place a bare flag after the target, e.g. mpvrun TARGET -c.
    #[arg(
        short = 'c',
        long,
        value_name = "BROWSER",
        ignore_case = true,
        num_args = 0..=1,
        value_parser = browser::SUPPORTED_BROWSERS
    )]
    use_cookies: Option<Option<String>>,

    /// Pass additional mpv args
    #[arg(allow_hyphen_values = true, num_args = 1..)]
    mpvargs: Option<Vec<String>>,
}

fn main() -> anyhow::Result<()> {
    let args = Cli::parse();

    let config = config::Config::load()?;
    let stream = Stream::from(args, &config);
    match stream {
        Err(StreamError::NotLive(channel)) => {
            println!("No live streams found for \"{channel}\" in the first {} entries. Use -a to choose an upcoming stream.", digest::STREAM_LIMIT)
        }
        Err(StreamError::NoStreams(channel)) => println!(
            "No live or upcoming streams found for \"{channel}\" in the first {} entries.",
            digest::STREAM_LIMIT
        ),
        Err(StreamError::Cancelled) => println!("Selection cancelled."),
        Err(StreamError::Other(err)) => {
            anyhow::bail!(err)
        }
        Ok(stream) => stream.play()?,
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_supported_browsers_with_either_cookie_flag() {
        for flag in ["--use-cookies", "-c"] {
            for browser in browser::SUPPORTED_BROWSERS {
                for argv in [
                    vec!["mpvrun", "target", flag, browser],
                    vec!["mpvrun", flag, browser, "target"],
                ] {
                    let args = Cli::try_parse_from(argv).unwrap();
                    assert_eq!(args.target, "target");
                    assert_eq!(args.use_cookies, Some(Some(browser.to_owned())));
                }
            }
        }
    }

    #[test]
    fn bare_cookie_flags_request_the_default_browser() {
        for flag in ["--use-cookies", "-c"] {
            for argv in [
                vec!["mpvrun", "target", flag],
                vec!["mpvrun", "target", flag, "--debug"],
                vec!["mpvrun", flag, "--", "target"],
            ] {
                let args = Cli::try_parse_from(argv).unwrap();
                assert_eq!(args.target, "target");
                assert_eq!(args.use_cookies, Some(None));
            }
        }
        assert_eq!(
            Cli::try_parse_from(["mpvrun", "target"])
                .unwrap()
                .use_cookies,
            None
        );
    }

    #[test]
    fn rejects_unsupported_cookie_browsers() {
        for flag in ["--use-cookies", "-c"] {
            let err = Cli::try_parse_from(["mpvrun", "target", flag, "unsupported"])
                .err()
                .unwrap();
            assert_eq!(err.kind(), clap::error::ErrorKind::InvalidValue);
        }
    }

    #[test]
    fn browser_names_are_case_insensitive_on_the_cli() {
        let args = Cli::try_parse_from(["mpvrun", "target", "-c", "FIREFOX"]).unwrap();
        assert_eq!(
            config::Config::default().cookie_browser(args.use_cookies.as_ref()),
            Some("firefox".to_owned())
        );
    }
}
