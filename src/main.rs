use clap::{Args, Parser, ValueEnum};
use digest::StreamError;
use stream::Stream;

mod browser;
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

    /// Get all streams (uses yt-dlp)
    #[arg(short, long)]
    all: bool,

    /// Open mpv without default args
    #[arg(short, long, default_value_t = false)]
    debug: bool,

    /// Enable always on top mode
    #[arg(short, long, default_value_t = false)]
    persistent: bool,

    /// Use browser cookies in mpv (omit browser to detect the system default, falling back to firefox)
    ///
    /// Place a bare flag after the target, e.g. mpvrun TARGET -c.
    #[arg(
        short = 'c',
        long,
        value_name = "BROWSER",
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

    let stream = Stream::from(args);
    match stream {
        Err(StreamError::NotYetLive(time)) => println!("Stream will be live in {}.", time),
        Err(StreamError::NotFound) => println!("Stream not found!"),
        Err(StreamError::NotLive(channel)) => {
            println!("Channel \"{}\" has no upcoming livestreams.", channel)
        }
        Err(StreamError::Other(err)) => {
            println!("Unspecified stream input error encountered:\n{}", err)
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
}
