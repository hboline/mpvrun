use crate::constants::CHANNEL_DICT;
use once_cell::sync::Lazy;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::io::{stdin, stdout, BufRead, Write};
use std::process::Command;

// Bound discovery so opening a stream never scans the entire channel archive.
pub const STREAM_LIMIT: usize = 50;

#[derive(Clone, Debug)]
pub struct StreamEntry {
    pub id: String,
    pub upcoming: bool,
    title: String,
    members_only: bool,
    release_timestamp: Option<i64>,
}

impl fmt::Display for StreamEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let status = if self.upcoming { "UPCOMING" } else { "LIVE" };
        let members = if self.members_only { " [MEMBERS]" } else { "" };
        // Keep titles on one terminal line, even if the metadata contains controls.
        let title: String = self
            .title
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect();
        write!(f, "[{status}]{members} {title}")
    }
}

fn discovery_command(channel: &str, browser: Option<&str>) -> Command {
    let mut command = Command::new("yt-dlp");
    command.args([
        "--ignore-config",
        "--flat-playlist",
        "--dump-single-json",
        "--playlist-end",
        &STREAM_LIMIT.to_string(),
    ]);
    if let Some(browser) = browser {
        command.args(["--cookies-from-browser", browser]);
    }
    command.arg(format!("https://www.youtube.com/@{channel}/streams"));
    command
}

fn get_streams(channel: &str, browser: Option<&str>) -> Result<Vec<StreamEntry>, StreamError> {
    let output = discovery_command(channel, browser)
        .output()
        .map_err(|err| StreamError::Other(format!("Could not run yt-dlp: {err}")))?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        return Err(StreamError::Other(format!(
            "yt-dlp discovery failed ({}): {}",
            output.status,
            stderr.trim()
        )));
    }
    if !stderr.trim().is_empty() {
        eprint!("{stderr}");
    }
    parse_streams(&output.stdout)
}

fn parse_streams(json: &[u8]) -> Result<Vec<StreamEntry>, StreamError> {
    let listing: Value = serde_json::from_slice(json)
        .map_err(|err| StreamError::Other(format!("Invalid yt-dlp stream listing: {err}")))?;
    let entries = listing["entries"]
        .as_array()
        .ok_or_else(|| StreamError::Other("yt-dlp returned no channel entries array".to_owned()))?;
    let mut seen = HashSet::new();
    let mut streams = Vec::new();
    for entry in entries {
        let upcoming = match entry["live_status"].as_str() {
            Some("is_live") => false,
            Some("is_upcoming") => true,
            _ => continue,
        };
        let Some(id) = entry["id"].as_str().filter(|id| !id.is_empty()) else {
            continue;
        };
        if !seen.insert(id) {
            continue;
        }
        streams.push(StreamEntry {
            id: id.to_owned(),
            title: entry["title"].as_str().unwrap_or(id).to_owned(),
            upcoming,
            members_only: entry["availability"] == "subscriber_only",
            release_timestamp: entry["release_timestamp"].as_i64(),
        });
    }
    // Do not assume that YouTube places live streams before scheduled streams.
    streams.sort_by_key(|stream| {
        (
            stream.upcoming,
            stream.release_timestamp.unwrap_or(i64::MAX),
        )
    });
    Ok(streams)
}

fn select_stream(
    streams: Vec<StreamEntry>,
    input: &mut impl BufRead,
    output: &mut (impl anstream::stream::RawStream + anstream::stream::AsLockedWrite),
) -> Result<StreamEntry, StreamError> {
    if streams.is_empty() {
        return Err(StreamError::Other("No streams to choose from".to_owned()));
    }
    // Adapt colors to the terminal, including Windows consoles and redirected output.
    let mut output = anstream::AutoStream::auto(output);
    writeln!(output, "Select a stream:")?;
    for (i, stream) in streams.iter().enumerate() {
        let mut label = if stream.upcoming {
            format!("\x1b[90m{stream}\x1b[0m")
        } else {
            stream
                .to_string()
                .replacen("[LIVE]", "\x1b[31m[LIVE]\x1b[0m", 1)
        };
        if stream.members_only {
            // Restore gray after the badge on upcoming streams; otherwise reset.
            let restore = if stream.upcoming {
                "\x1b[90m"
            } else {
                "\x1b[0m"
            };
            label = label.replacen("[MEMBERS]", &format!("\x1b[38;5;220m[MEMBERS]{restore}"), 1);
        }
        writeln!(output, "[{}] {label}", i + 1)?;
    }
    if streams.iter().any(|stream| stream.upcoming) {
        writeln!(output)?;
    }

    let mut buffer = String::new();
    loop {
        write!(output, "Stream number: ")?;
        output.flush()?;
        buffer.clear();
        if input.read_line(&mut buffer)? == 0 {
            return Err(StreamError::Cancelled);
        }
        if let Ok(number) = buffer.trim().parse::<usize>() {
            if (1..=streams.len()).contains(&number) {
                return Ok(streams.into_iter().nth(number - 1).unwrap());
            }
        }
        writeln!(
            output,
            "Invalid selection. Enter a number from 1 to {}.",
            streams.len()
        )?;
    }
}

pub fn discover(
    channel: &str,
    browser: Option<&str>,
    all: bool,
) -> Result<StreamEntry, StreamError> {
    let mut streams = get_streams(channel, browser)?;
    if all {
        if streams.is_empty() {
            return Err(StreamError::NoStreams(channel.to_owned()));
        }
    } else {
        streams.retain(|stream| !stream.upcoming);
        if streams.is_empty() {
            return Err(StreamError::NotLive(channel.to_owned()));
        }
        if streams.len() == 1 {
            return Ok(streams.remove(0));
        }
    }
    select_stream(streams, &mut stdin().lock(), &mut stdout().lock())
}

#[derive(Debug)]
pub enum ReadOutput {
    Id(String),
    Channel(String),
}

pub fn read_input(input: String) -> Option<ReadOutput> {
    let channel_hm: &Lazy<HashMap<&str, &str>> = &CHANNEL_DICT;

    if input.contains("youtube.com/") {
        let ind = input.find("youtube.com/").unwrap();
        let sub = input[ind + 12..].to_string();

        if &sub[..8] == "watch?v=" {
            Some(ReadOutput::Id(sub[8..19].to_string()))
        } else if sub.starts_with('@') {
            let ind = sub.find('/').unwrap_or(sub.len());
            Some(ReadOutput::Channel(sub[1..ind].to_string()))
        } else if &sub[..5] == "live/" {
            Some(ReadOutput::Id(sub[5..].to_string()))
        } else {
            None
        }
    } else if input.starts_with('@') {
        Some(ReadOutput::Channel(input[1..].to_string()))
    } else {
        let res = channel_hm.get(input.as_str());
        match res {
            Some(&res) => Some(ReadOutput::Channel(res.to_string())),
            None => Some(ReadOutput::Channel(input)),
        }
    }
}

#[derive(Debug)]
pub enum StreamError {
    NotLive(String),
    NoStreams(String),
    Cancelled,
    Other(String),
}

impl From<std::io::Error> for StreamError {
    fn from(err: std::io::Error) -> Self {
        Self::Other(format!(
            "Could not read or write the stream selection: {err}"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    const LISTING: &[u8] = include_bytes!("../tests/fixtures/channel-streams.json");

    #[test]
    fn lists_members_live_after_upcoming_without_extracting_each_video() {
        let streams = parse_streams(LISTING).unwrap();
        let ids: Vec<_> = streams.iter().map(|stream| stream.id.as_str()).collect();
        assert_eq!(ids, ["qEe8JW9u7bU", "iLxNEAxN8_U", "rzxphuacYxo"]);
        assert_eq!(
            streams[0].to_string(),
            "[LIVE] [MEMBERS] Members \"live\" stream 日本語"
        );
        assert!(streams[1].to_string().starts_with("[UPCOMING]"));
    }

    #[test]
    fn empty_listings_are_distinct_from_invalid_output() {
        assert!(parse_streams(br#"{"entries": []}"#).unwrap().is_empty());
        assert!(
            parse_streams(br#"{"entries": [{"id": "old", "live_status": "was_live"}]}"#)
                .unwrap()
                .is_empty()
        );
        for json in [b"not json".as_slice(), b"{}", br#"{"entries": null}"#] {
            assert!(matches!(parse_streams(json), Err(StreamError::Other(_))));
        }
    }

    #[test]
    fn picker_retries_bad_input_and_can_select_upcoming() {
        let mut input = Cursor::new(b"0\n999\ninvalid\nq\nQ\n2\n");
        let mut output = Vec::new();
        let selected =
            select_stream(parse_streams(LISTING).unwrap(), &mut input, &mut output).unwrap();
        assert_eq!(selected.id, "iLxNEAxN8_U");
        assert!(selected.upcoming);
        let output = String::from_utf8(output).unwrap();
        assert_eq!(output.matches("Invalid selection.").count(), 5);
        assert!(output.contains("[LIVE] [MEMBERS]"));
    }

    #[test]
    fn picker_cancels_on_eof_and_never_prompts_for_an_empty_list() {
        for input in ["", "invalid\n"] {
            let result = select_stream(
                parse_streams(LISTING).unwrap(),
                &mut Cursor::new(input),
                &mut Vec::new(),
            );
            assert!(matches!(result, Err(StreamError::Cancelled)));
        }
        let mut output = Vec::new();
        assert!(select_stream(vec![], &mut Cursor::new(""), &mut output).is_err());
        assert!(output.is_empty());
    }

    #[test]
    fn discovery_passes_cookies_and_does_not_filter_before_listing() {
        for browser in [None, Some("firefox"), Some("edge")] {
            let command = discovery_command("holoen_gigimurin", browser);
            let args: Vec<_> = command.get_args().map(|s| s.to_str().unwrap()).collect();
            assert!(args.contains(&"--flat-playlist"));
            assert!(args.contains(&"--dump-single-json"));
            assert!(args.contains(&"--ignore-config"));
            assert!(!args.contains(&"--match-filter"));
            assert!(!args.contains(&"--break-on-reject"));
            assert_eq!(
                args.last(),
                Some(&"https://www.youtube.com/@holoen_gigimurin/streams")
            );
            if let Some(browser) = browser {
                assert!(args
                    .windows(2)
                    .any(|pair| pair == ["--cookies-from-browser", browser]));
            } else {
                assert!(!args.contains(&"--cookies-from-browser"));
            }
        }
    }
}
