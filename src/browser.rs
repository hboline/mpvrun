pub const SUPPORTED_BROWSERS: [&str; 9] = [
    "brave", "chrome", "chromium", "edge", "firefox", "opera", "safari", "vivaldi", "whale",
];

pub fn default_browser() -> &'static str {
    browser_or_fallback(system_browser().as_deref())
}

fn browser_or_fallback(identifier: Option<&str>) -> &'static str {
    let identifier = identifier.unwrap_or_default().to_ascii_lowercase();
    SUPPORTED_BROWSERS
        .into_iter()
        .find(|browser| identifier.contains(browser))
        .unwrap_or("firefox")
}

#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
fn command_output(program: &str, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new(program)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

#[cfg(windows)]
fn system_browser() -> Option<String> {
    // Prefer the HTTPS association, which is what the stream links use.
    for scheme in ["https", "http"] {
        let key = format!(
            r"HKCU\Software\Microsoft\Windows\Shell\Associations\UrlAssociations\{scheme}\UserChoice"
        );
        if let Some(output) = command_output("reg", &["query", &key, "/v", "ProgId"]) {
            if let Some((_, value)) = output.split_once("REG_SZ") {
                return Some(value.trim().to_owned());
            }
        }
    }
    None
}

#[cfg(target_os = "linux")]
fn system_browser() -> Option<String> {
    command_output("xdg-settings", &["get", "default-web-browser"])
}

#[cfg(target_os = "macos")]
fn system_browser() -> Option<String> {
    // Query Launch Services through AppKit without opening a browser.
    command_output(
        "osascript",
        &[
            "-l",
            "JavaScript",
            "-e",
            r#"ObjC.import('AppKit');
            $.NSWorkspace.sharedWorkspace.URLForApplicationToOpenURL(
                $.NSURL.URLWithString('https://example.com')
            ).lastPathComponent.js"#,
        ],
    )
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
fn system_browser() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::browser_or_fallback;

    #[test]
    fn maps_platform_identifiers_to_ytdlp_browser_names() {
        for (identifier, expected) in [
            ("BraveHTML", "brave"),
            ("ChromeHTML", "chrome"),
            ("MSEdgeHTM", "edge"),
            ("FirefoxURL-308046B0AF4A39CB", "firefox"),
            ("OperaStable", "opera"),
            ("VivaldiHTM", "vivaldi"),
            ("WhaleHTM", "whale"),
            ("google-chrome.desktop\n", "chrome"),
            ("chromium-browser.desktop", "chromium"),
            ("org.mozilla.firefox.desktop", "firefox"),
            ("com.brave.Browser.desktop", "brave"),
            ("Safari.app\n", "safari"),
            ("Microsoft Edge.app", "edge"),
        ] {
            assert_eq!(browser_or_fallback(Some(identifier)), expected);
        }
    }

    #[test]
    fn falls_back_to_firefox_for_missing_or_unsupported_browsers() {
        for identifier in [None, Some(""), Some("unknown.desktop"), Some("Arc.app")] {
            assert_eq!(browser_or_fallback(identifier), "firefox");
        }
    }
}
