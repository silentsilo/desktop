//! macOS Finder Quick Action — installs a Services workflow that forwards files to silentsilo --upload.

use std::path::Path;
use std::process::Command;

pub fn register_context_menu(exe_path: &Path) -> std::io::Result<()> {
    let exe = exe_path
        .canonicalize()
        .unwrap_or_else(|_| exe_path.to_path_buf());
    let exe_str = exe.display().to_string();

    let home = dirs::home_dir().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "home directory not found")
    })?;

    let workflow_root = home.join("Library/Services/Upload to SilentSilo.workflow");
    let contents = workflow_root.join("Contents");
    std::fs::create_dir_all(&contents)?;

    let shell_script = format!(
        r#"for f in "$@"; do
  "{exe_str}" --upload "$f"
done
"#
    );

    let document = generate_document_wflow(&shell_script);
    std::fs::write(contents.join("document.wflow"), document)?;

    // Refresh Services cache so Finder picks up the workflow.
    let _ = Command::new("/System/Library/CoreServices/pbs")
        .args(["-flush"])
        .status();

    Ok(())
}

fn generate_document_wflow(shell_script: &str) -> String {
    let escaped = shell_script
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");

    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>AMApplicationBuild</key><string>523</string>
  <key>AMApplicationVersion</key><string>2.10</string>
  <key>AMDocumentVersion</key><string>2</string>
  <key>actions</key>
  <array>
    <dict>
      <key>action</key><dict>
        <key>AMAccepts</key><dict>
          <key>Container</key><string>List</string>
          <key>Optional</key><true/>
          <key>Types</key><array><string>com.apple.cocoa.path</string></array>
        </dict>
        <key>AMActionVersion</key><string>2.0.3</string>
        <key>AMApplication</key><array><string>Automator</string></array>
        <key>AMParameterProperties</key><dict/>
        <key>AMProvides</key><dict>
          <key>Container</key><string>List</string>
          <key>Types</key><array><string>com.apple.cocoa.path</string></array>
        </dict>
        <key>ActionBundlePath</key><string>/System/Library/Automator/Run Shell Script.action</string>
        <key>ActionName</key><string>Run Shell Script</string>
        <key>ActionParameters</key><dict>
          <key>COMMAND_STRING</key><string>{escaped}</string>
          <key>CheckedForUserDefaultShell</key><true/>
          <key>inputMethod</key><integer>1</integer>
          <key>shell</key><string>/bin/bash</string>
          <key>source</key><string></string>
        </dict>
        <key>BundleIdentifier</key><string>com.apple.RunShellScript</string>
        <key>CFBundleVersion</key><string>2.0.3</string>
        <key>CanShowSelectedItemsWhenRun</key><false/>
        <key>CanShowWhenRun</key><true/>
        <key>Category</key><array><string>AMCategoryUtilities</string></array>
        <key>Class Name</key><string>RunShellScriptAction</string>
        <key>InputUUID</key><string>0</string>
        <key>Keywords</key><array><string>Shell</string><string>Script</string><string>Run</string></array>
        <key>OutputUUID</key><string>1</string>
        <key>UUID</key><string>1</string>
        <key>UnlocalizedApplications</key><array><string>Automator</string></array>
        <key>arguments</key><dict/>
      </dict>
    </dict>
  </array>
  <key>connectors</key><dict/>
  <key>workflowMetaData</key><dict>
    <key>serviceInputTypeIdentifier</key><string>com.apple.Automator.fileSystemObject</string>
    <key>serviceOutputTypeIdentifier</key><string>com.apple.Automator.nothing</string>
    <key>serviceProcessesInput</key><integer>0</integer>
    <key>workflowTypeIdentifier</key><string>com.apple.Automator.services</string>
  </dict>
</dict>
</plist>
"#
    )
}

pub fn unregister_context_menu() -> std::io::Result<()> {
    let home = dirs::home_dir().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "home directory not found")
    })?;
    let workflow = home.join("Library/Services/Upload to SilentSilo.workflow");
    if workflow.exists() {
        std::fs::remove_dir_all(workflow)?;
    }
    Ok(())
}
