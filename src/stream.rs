use crate::config::Config;
use crate::digest::{discover, read_input, ReadOutput, StreamError};
use crate::{Cli, Mode};
use core::panic;
use open;
use std::io::{self, Read, Write};
use std::process::{Command, Stdio};

#[derive(Debug)]
pub struct Stream {
    id: String,
    mode: Mode,
    mpvargs: Option<Vec<String>>,
    mpv_options: Vec<String>,
    persistent: Option<Vec<String>>,
    suppress_coretext_notes: bool,
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
            suppress_coretext_notes: cfg!(target_os = "macos") && !args.debug,
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
            // Invoke mpv directly instead of going through a platform-specific
            // shell (`cmd /C` only exists on Windows).
            let mut argvec: Vec<String> = vec![url];

            argvec.append(&mut self.mpv_options);

            if let Some(v) = &mut self.mpvargs {
                argvec.append(v);
            }

            if let Some(v) = &mut self.persistent {
                argvec.append(v);
            }

            if self.suppress_coretext_notes {
                // mpv checks stderr's foreground process group before showing status.
                // macOS script keeps a real terminal for mpv, including keyboard input.
                let mut child = Command::new("/usr/bin/script")
                    .args(["-qF", "/dev/null", "mpv"])
                    .args(argvec)
                    .stdout(Stdio::piped())
                    .spawn()?;
                let stdout = child.stdout.take().expect("stdout was piped");
                let result = filter_coretext_notes(stdout, io::stdout().lock());
                if result.is_err() {
                    let _ = child.kill();
                }
                child.wait()?;
                result?;
            } else {
                Command::new("mpv").args(argvec).status()?;
            }
        } else {
            open::that(url)?;
        };

        Ok(())
    }
}

fn is_coretext_note(bytes: &[u8]) -> bool {
    let text = String::from_utf8_lossy(bytes);
    text.contains("CoreText note: Client requested name \".LastResort\"")
        || text
            .contains("CoreText note: Set a breakpoint on CTFontLogSystemFontNameRequest to debug.")
}

fn filter_coretext_notes(mut input: impl Read, mut output: impl Write) -> io::Result<()> {
    const TIMESTAMP: &[u8] = b"0000-00-00 00:00:00.000 ";
    const NOTE: &[u8] = b"CoreText note: ";
    const STATUS_START: &[u8] = b"\x1b[?25l";
    let mut pending = Vec::new();
    let mut diagnostic = false;
    let mut chunk = [0; 8192];
    loop {
        let count = match input.read(&mut chunk) {
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            result => result?,
        };
        if count == 0 {
            break;
        }
        let mut visible = Vec::new();
        for &byte in &chunk[..count] {
            pending.push(byte);
            if diagnostic {
                if byte == b'\n' || pending.len() >= 16384 {
                    if !is_coretext_note(&pending) {
                        visible.extend_from_slice(&pending);
                    }
                    pending.clear();
                    diagnostic = false;
                }
                continue;
            }
            // Only hold possible diagnostic prefixes. Status updates and ANSI
            // controls must pass through without waiting for a newline.
            while !pending.is_empty() {
                if STATUS_START.starts_with(&pending) {
                    if pending.len() == STATUS_START.len() {
                        visible.extend_from_slice(&pending);
                        // Reserve a blank row below the status, then return to
                        // its row so mpv can keep redrawing in place.
                        visible.extend_from_slice(b"\n\x1b[2K\x1b[A");
                        pending.clear();
                    }
                    break;
                }
                let timestamp = pending.len() <= TIMESTAMP.len()
                    && pending.iter().zip(TIMESTAMP).all(|(&b, &pattern)| {
                        if pattern == b'0' {
                            b.is_ascii_digit()
                        } else {
                            b == pattern
                        }
                    });
                let note = NOTE.starts_with(&pending);
                if timestamp || note {
                    diagnostic = (timestamp && pending.len() == TIMESTAMP.len())
                        || (note && pending.len() == NOTE.len());
                    break;
                }
                visible.push(pending.remove(0));
            }
        }
        output.write_all(&visible)?;
        output.flush()?;
    }
    if !is_coretext_note(&pending) {
        output.write_all(&pending)?;
    }
    output.flush()?;
    Ok(())
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
    fn filters_only_the_known_coretext_notes_and_preserves_other_output() {
        let input = concat!(
            "2026-11-15 00:31:39.145 mpv[123:456] CoreText note: Client requested name \".LastResort\", it will get TimesNewRomanPSMT rather than the intended font.\n",
            "2026-11-15 00:31:39.145 mpv[123:456] CoreText note: Set a breakpoint on CTFontLogSystemFontNameRequest to debug.\n",
            "[ffmpeg] Network error\n",
            "CoreText note: A different warning\n",
            "last line without newline",
        );
        let mut output = Vec::new();
        filter_coretext_notes(io::Cursor::new(input), &mut output).unwrap();
        assert_eq!(output, b"[ffmpeg] Network error\nCoreText note: A different warning\nlast line without newline");

        let mut output = Vec::new();
        filter_coretext_notes(io::Cursor::new(b"invalid UTF-8: \xff\n"), &mut output).unwrap();
        assert_eq!(output, b"invalid UTF-8: \xff\n");
        assert!(!stream_with(&["--debug"]).suppress_coretext_notes);
    }

    #[test]
    fn live_status_is_flushed_before_reading_the_next_update() {
        use std::cell::RefCell;
        use std::rc::Rc;

        #[derive(Clone)]
        struct Output(Rc<RefCell<Vec<u8>>>);
        impl Write for Output {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                self.0.borrow_mut().extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let status = b"\x1b[1K\rAV: 00:02:12 Cache: 3.4s/1MB";
        let output = Output(Rc::default());
        let observed = output.clone();
        let mut sent = false;
        struct Reader<F>(F);
        impl<F: FnMut(&mut [u8]) -> io::Result<usize>> Read for Reader<F> {
            fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
                (self.0)(buf)
            }
        }
        let input = Reader(|buf: &mut [u8]| {
            if sent {
                assert_eq!(&*observed.0.borrow(), status);
                Ok(0)
            } else {
                sent = true;
                buf[..status.len()].copy_from_slice(status);
                Ok(status.len())
            }
        });
        filter_coretext_notes(input, output).unwrap();
    }

    #[test]
    fn warnings_split_across_reads_do_not_swallow_adjacent_status_updates() {
        let status = "\x1b[1K\rAV: 00:02:12 Cache: 3.4s/1MB";
        let warning = "2026-11-15 00:31:39.145 mpv[123:456] CoreText note: Client requested name \".LastResort\", fallback warning\r\n";
        let input = format!("{status}{warning}{status}");
        for size in 1..=input.len() {
            struct Chunks<'a>(&'a [u8], usize);
            impl Read for Chunks<'_> {
                fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
                    let count = self.0.len().min(self.1).min(buf.len());
                    buf[..count].copy_from_slice(&self.0[..count]);
                    self.0 = &self.0[count..];
                    Ok(count)
                }
            }
            let mut output = Vec::new();
            filter_coretext_notes(Chunks(input.as_bytes(), size), &mut output).unwrap();
            assert_eq!(output, status.repeat(2).as_bytes(), "chunk size {size}");
        }
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
