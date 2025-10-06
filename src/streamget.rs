use serde_json;
use std::collections::HashMap;
use std::io::{stdin, stdout, Write};
use std::num::ParseIntError;
use std::process::{Command, Stdio};

pub mod digest {
    use super::*;

    // compile output of command chain into one iterator
    fn zip_streams(input: Vec<u8>) -> Vec<(String, String)> {
        let input_string = String::from_utf8(input).expect("utf-8 parsing error");
        let titles = input_string.lines().skip(0).step_by(2);
        let ids = input_string.lines().skip(1).step_by(2);
        titles
            .zip(ids)
            .map(|(s1, s2)| {
                (
                    serde_json::from_str::<String>(s1).unwrap(),
                    serde_json::from_str::<String>(s2).unwrap(),
                )
            })
            .collect::<Vec<(String, String)>>()
    }

    // get a vector of tuples with all valid live streams for channel
    fn get_streams(channel: String) -> Vec<(String, String)> {
        let link = format!("https://www.youtube.com/@{channel}/streams");

        let ytdlp_output = Command::new("yt-dlp")
            .args([link.as_str(), "--flat-playlist", "-j"])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();

        let jq_output = Command::new("jq")
            .arg("select(.live_status == \"is_live\") | .title, .id")
            .stdin(Stdio::from(ytdlp_output.stdout.unwrap()))
            .output()
            .unwrap()
            .stdout;

        zip_streams(jq_output)
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

    pub enum ReadOutput {
        Id(String),
        Channel(String),
    }

    pub fn read_input(input: String) -> Option<ReadOutput> {
        let channel_hm: HashMap<&str, &str> = HashMap::from(CHANNEL_DICT);

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
        NotYetLive(String), // "This live event will begin in..."
        NotFound,           // "Error 404: Not Found..."
        NotLive(String),    // If channel has no live upcoming; also for mispelling
        Other,              // Catch all for any other error
    }

    pub fn get_id(channel: String) -> Result<String, StreamError> {
        let url = format!("https://www.youtube.com/@{}/live", channel);

        let output = Command::new("yt-dlp")
            .args([url.as_str(), "-O", "id"])
            .output();

        let output_stdout = String::from_utf8(output.as_ref().unwrap().stdout.clone()).unwrap();
        let output_stderr = String::from_utf8(output.as_ref().unwrap().stderr.clone()).unwrap();

        const LIVE_BEGIN: &str = "live event will begin in ";
        if !output_stderr.is_empty() {
            if output_stderr.contains(LIVE_BEGIN) {
                let start = output_stderr.find(LIVE_BEGIN).unwrap() + LIVE_BEGIN.len();
                let time = output_stderr[start..output_stderr.trim().len() - 1].to_string();
                Err(StreamError::NotYetLive(time))
            } else if output_stderr.contains("Error 404: Not Found") {
                Err(StreamError::NotFound)
            } else if output_stderr.contains("channel is not currently live") {
                Err(StreamError::NotLive(channel))
            } else {
                Err(StreamError::Other)
            }
        } else {
            Ok(output_stdout.trim().to_string())
        }
    }
}

pub mod stream {
    use core::panic;
    use digest::*;

    use super::*;
    use crate::{Cli, Mode};

    #[derive(Debug)]
    pub struct Stream {
        id: String,
        mode: Mode,
        mpvargs: Option<Vec<String>>,
        default_mpv_args: Option<Vec<String>>,
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
                        "--msg-level=all=fatal,statusline=status",
                    ]
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

        pub fn play(&self) -> anyhow::Result<()> {
            self.info();

            let mut arg_vec: Vec<String> = Vec::new();
            let program: String;
            let url: String;
            match self.mode {
                Mode::Mpv => {
                    program = "mpv".to_string();
                    url = self.link();
                    arg_vec.append(self.default_mpv_args.clone().unwrap_or(Vec::new()).as_mut());
                    arg_vec.append(self.mpvargs.clone().unwrap_or(Vec::new()).as_mut());
                }
                Mode::Embed => {
                    url = self.embed();
                    program = FIREFOX.to_string();
                }
                Mode::Full => {
                    url = self.link();
                    program = FIREFOX.to_string();
                }
            };

            arg_vec.insert(0, url);
            arg_vec.insert(0, program);
            arg_vec.insert(0, "/C".to_string());
            Command::new("cmd").args(arg_vec).spawn()?;

            Ok(())
        }
    }
}

const FIREFOX: &str = "C:\\Program Files\\Mozilla Firefox\\firefox.exe";

#[allow(dead_code)]
const CHANNEL_DICT: [(&str, &str); 72] = [
    // Hololive EN
    ("mori", "MoriCalliope"),
    ("kiara", "TakanashiKiara"),
    ("ina", "NinomaeInanis"),
    ("gura", "GawrGura"),
    ("ame", "WatsonAmelia"),
    ("irys", "IRyS"),
    ("sana", "TsukumoSana"),
    ("fauna", "CeresFauna"),
    ("kronii", "OuroKronii"),
    ("mumei", "NanashiMumei"),
    ("bae", "HakosBaelz"),
    ("shiori", "ShioriNovella"),
    ("nerissa", "NerissaRavencroft"),
    ("biboo", "KosekiBijou"),
    ("fwmc", "FUWAMOCOch"),
    ("gigi", "holoen_gigimurin"),
    ("ceci", "holoen_ceciliaimmergreen"),
    ("raora", "holoen_raorapanthera"),
    ("erb", "holoen_erbloodflame"),
    // Hololive JP
    ("sora", "TokinoSora"),
    ("roboco", "Robocosan"),
    ("azki", "AZKi"),
    ("miko", "SakuraMiko"),
    ("suisei", "HoshimachiSuisei"),
    ("akirose", "AkiRosenthal"),
    ("haachama", "AkaiHaato"),
    ("fbk", "ShirakamiFubuki"),
    ("matsuri", "NatsuiroMatsuri"),
    ("shion", "MurasakiShion"),
    ("ayame", "NakiriAyame"),
    ("choco", "YuzukiChoco"),
    ("subaru", "OozoraSubaru"),
    ("mio", "OokamiMio"),
    ("okayu", "NekomataOkayu"),
    ("korone", "InugamiKorone"),
    ("pekora", "usadapekora"),
    ("flare", "ShiranuiFlare"),
    ("noel", "ShiroganeNoel"),
    ("marine", "HoushouMarine"),
    ("kanata", "AmaneKanata"),
    ("watame", "TsunomakiWatame"),
    ("towa", "TokoyamiTowa"),
    ("luna", "HimemoriLuna"),
    ("lamy", "YukihanaLamy"),
    ("nene", "MomosuzuNene"),
    ("botan", "ShishiroBotan"),
    ("polka", "OmaruPolka"),
    ("laplus", "LaplusDarknesss"),
    ("lui", "TakaneLui"),
    ("koyori", "HakuiKoyori"),
    ("chloe", "SakamataChloe"),
    ("iroha", "kazamairoha"),
    // Hololive ID
    ("risu", "AyundaRisu"),
    ("moona", "MoonaHoshinova"),
    ("iofi", "AiraniIofifteen"),
    ("ollie", "KureijiOllie"),
    ("anya", "AnyaMelfissa"),
    ("reine", "PavoliaReine"),
    ("zeta", "VestiaZeta"),
    ("kaela", "KaelaKovalskia"),
    ("kobo", "KoboKanaeru"),
    // Hololive DEV_IS
    ("ao", "HiodoshiAo"),
    ("kanade", "OtonoseKanade"),
    ("ririka", "IchijouRirika"),
    ("raden", "JuufuuteiRaden"),
    ("hajime", "TodorokiHajime"),
    ("riona", "IsakiRiona"),
    ("niko", "KoganeiNiko"),
    ("su", "MizumiyaSu"),
    ("chihaya", "RindoChihaya"),
    ("vivi", "KikiraraVivi"),
    // Test streams (always live)
    ("test", "TokyoTones"),
];
