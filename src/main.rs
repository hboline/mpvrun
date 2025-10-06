use clap::{Args, Parser, ValueEnum};
use digest::StreamError;
use stream::Stream;

mod streamget;
pub use crate::streamget::*;

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

    /// Get all streams (uses yt-dlp & jq)
    #[arg(short, long)]
    all: bool,

    /// Open mpv without default args
    #[arg(short, long, default_value_t = false)]
    debug: bool,

    /// Enable always on top mode
    #[arg(short, long, default_value_t = false)]
    persistent: bool,

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
        Err(StreamError::Other) => println!("Unspecified stream input error encountered"),
        Ok(stream) => stream.play()?,
    }

    Ok(())
}
