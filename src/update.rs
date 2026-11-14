use std::time::Duration;

fn version(s: &str) -> Option<(u32, u32, u32)> {
    let mut parts = s.trim_start_matches('v').split('.').map(|p| p.parse().ok());
    Some((parts.next()??, parts.next()??, parts.next()??))
}

fn check() -> Result<(), String> {
    let api = env!("CARGO_PKG_REPOSITORY").replace("https://github.com/", "https://api.github.com/repos/") + "/releases/latest";
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(120)))
        .build()
        .into();
    let release: serde_json::Value = agent
        .get(&api)
        .call()
        .map_err(|e| e.to_string())?
        .body_mut()
        .read_json()
        .map_err(|e| e.to_string())?;
    let Some(latest) = version(release["tag_name"].as_str().unwrap_or("")) else { return Ok(()) };
    if Some(latest) <= version(env!("CARGO_PKG_VERSION")) {
        return Ok(());
    }
    let url = release["assets"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|a| a["name"] == "DeckDisplaySetup.exe")
        .and_then(|a| a["browser_download_url"].as_str())
        .filter(|u| u.starts_with(&format!("{}/releases/download/", env!("CARGO_PKG_REPOSITORY"))))
        .ok_or("release has no installer")?;
    let bytes = agent
        .get(url)
        .call()
        .map_err(|e| e.to_string())?
        .body_mut()
        .with_config()
        .limit(64 * 1024 * 1024)
        .read_to_vec()
        .map_err(|e| e.to_string())?;
    let (a, b, c) = latest;
    let path = std::env::temp_dir().join(format!("DeckDisplaySetup-{a}.{b}.{c}.exe"));
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    println!("Updating to {a}.{b}.{c}");
    std::process::Command::new(&path)
        .args(["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART"])
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn start() {
    let installed = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("unins000.exe").exists()))
        .unwrap_or(false);
    if !installed {
        return;
    }
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(60));
        loop {
            if let Err(e) = check() {
                eprintln!("Update check failed: {e}");
            }
            std::thread::sleep(Duration::from_secs(6 * 3600));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::version;

    #[test]
    fn newer_tags_compare_greater() {
        assert!(version("v0.1.10") > version("0.1.2"));
        assert!(version("v1.0.0") > version("0.9.9"));
        assert_eq!(version("v0.1.2"), version("0.1.2"));
        assert_eq!(version("latest"), None);
        assert_eq!(version("v1.2"), None);
    }
}
