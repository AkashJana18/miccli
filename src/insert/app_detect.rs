
/// Get the bundle identifier of the currently focused application.
pub fn get_frontmost_bundle_id() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        get_frontmost_bundle_id_macos()
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

#[cfg(target_os = "macos")]
fn get_frontmost_bundle_id_macos() -> Option<String> {
    use std::process::Command;

    let script = r#"tell application "System Events"
        set frontApp to name of first application process whose frontmost is true
        set frontBundle to bundle identifier of application process frontApp
        return frontBundle
    end tell"#;

    let output = Command::new("osascript")
        .args(["-e", script])
        .output()
        .ok()?;

    if output.status.success() {
        let bundle = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !bundle.is_empty() {
            return Some(bundle);
        }
    }

    // Fallback: use `osascript -e 'id of app "..."` for frontmost app
    let script2 = r#"tell application "System Events"
        set frontApp to name of first application process whose frontmost is true
        return id of application frontApp
    end tell"#;

    let output2 = Command::new("osascript")
        .args(["-e", script2])
        .output()
        .ok()?;

    if output2.status.success() {
        let bundle = String::from_utf8_lossy(&output2.stdout).trim().to_string();
        if !bundle.is_empty() {
            return Some(bundle);
        }
    }

    None
}
