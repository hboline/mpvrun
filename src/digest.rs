use crate::constants::CHANNEL_DICT;
use once_cell::sync::Lazy;
use serde_json;
use serde_json::Value;
use std::collections::HashMap;
use std::io::{stdin, stdout, Write};
use std::num::ParseIntError;
use std::process::Command;

// get a vector of tuples with all valid live streams for channel
fn get_streams(channel: String) -> Vec<(String, String)> {
    let link = format!("https://www.youtube.com/@{channel}/streams");

    let ytdlp_output = String::from_utf8(
        Command::new("yt-dlp")
            .args([link.as_str(), "--flat-playlist", "-j"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();

    serde_json::Deserializer::from_str(ytdlp_output.as_str())
        .into_iter::<Value>()
        .filter_map(Result::ok)
        .filter_map(|obj| {
            (obj["is_live"] == true).then_some({
                let title = obj["title"].to_string().trim_matches('"').to_owned();
                let id = obj["id"].to_string().trim_matches('"').to_owned();
                (title, id)
            })
        })
        .collect()
}

// choose stream (if more than 1) and return id
fn select_stream(stream_list: Vec<(String, String)>) -> String {
    if stream_list.len() == 1 {
        return stream_list.get(0).unwrap().1.clone();
    }

    println!("--> Channel has multiple streams currently live");
    for (i, (stream, _)) in stream_list.iter().enumerate() {
        println!("[{}] {}", i + 1, stream);
    }
    print!("Select stream number: ");

    let range = 1..=stream_list.len();
    let mut buffer = String::new();
    let mut choice: Result<usize, ParseIntError>;

    choice = loop {
        stdout().flush().unwrap();
        stdin().read_line(&mut buffer).unwrap();

        choice = buffer.trim().parse::<usize>();

        match choice {
            Ok(value) if range.contains(&value) => break Ok(value),
            _ => {
                println!("Invalid selection. Please try again: ");
                buffer.clear();
            }
        }
    };

    let index = choice.unwrap().checked_sub(1).unwrap();

    stream_list.get(index).unwrap().1.clone()
}

pub fn get_all_streams(channel: String) -> String {
    let stream_list = get_streams(channel);
    select_stream(stream_list)
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
#[allow(dead_code)]
pub enum StreamError {
    NotYetLive(String), // "This live event will begin in..."
    NotFound,           // "Error 404: Not Found..."
    NotLive(String),    // If channel has no live upcoming; also for mispelling
    Other(String),      // Catch all for any other error
}

pub fn get_id(channel: String) -> Result<String, StreamError> {
    let url = format!("https://www.youtube.com/@{}/live", channel);

    let output = Command::new("yt-dlp")
        .args([url.as_str(), "-O", "id"])
        .output();

    let output_stdout = String::from_utf8(output.as_ref().unwrap().stdout.clone()).unwrap();
    let output_stderr = String::from_utf8(output.as_ref().unwrap().stderr.clone()).unwrap();

    const LIVE_BEGIN: &str = "live event will begin in ";
    match output_stderr.as_str() {
        s if s.contains(LIVE_BEGIN) => {
            let start = s.find(LIVE_BEGIN).unwrap() + LIVE_BEGIN.len();
            let time = s[start..s.trim().len() - 1].to_string();
            Err(StreamError::NotYetLive(time))
        }
        s if s.contains("Error 404: Not Found") => Err(StreamError::NotFound),
        s if s.contains("channel is not currently live") => Err(StreamError::NotLive(channel)),
        _ => Ok(output_stdout.trim().to_string()),
    }
}
