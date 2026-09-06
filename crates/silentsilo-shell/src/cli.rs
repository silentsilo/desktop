use std::path::PathBuf;

#[derive(Debug, Clone)]
pub enum ShellAction {
    None,
    Upload { paths: Vec<PathBuf> },
    Download { target_dir: PathBuf },
    Show,
    Autostart,
}

pub fn parse_args(args: impl IntoIterator<Item = String>) -> ShellAction {
    let mut iter = args.into_iter();
    let _ = iter.next(); // skip program name when present

    let mut action = ShellAction::None;
    let mut paths = Vec::new();

    for arg in iter {
        match arg.as_str() {
            "--upload" | "-u" => {
                action = ShellAction::Upload { paths: Vec::new() };
            }
            "--download" | "-d" => {
                action = ShellAction::Download {
                    target_dir: PathBuf::new(),
                };
            }
            "--show" => action = ShellAction::Show,
            // What the OS sign-in entry passes. Same app, but it goes to the
            // notification area instead of putting a window on screen.
            "--autostart" => action = ShellAction::Autostart,
            other if other.starts_with('-') => {}
            path => match action {
                ShellAction::Download { .. } => {
                    action = ShellAction::Download {
                        target_dir: PathBuf::from(path),
                    };
                }
                _ => paths.push(PathBuf::from(path)),
            },
        }
    }

    if let ShellAction::Upload { paths: ref mut p } = action {
        *p = paths;
    }

    action
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_upload_paths() {
        let action = parse_args(["silentsilo", "--upload", "C:\\file.txt"].map(String::from));
        match action {
            ShellAction::Upload { paths } => assert_eq!(paths.len(), 1),
            _ => panic!("expected upload"),
        }
    }

    #[test]
    fn parse_download_target_dir() {
        let action =
            parse_args(["silentsilo", "--download", "C:\\Users\\me\\Desktop"].map(String::from));
        match action {
            ShellAction::Download { target_dir } => {
                assert_eq!(target_dir, PathBuf::from("C:\\Users\\me\\Desktop"))
            }
            _ => panic!("expected download"),
        }
    }

    #[test]
    fn parse_download_with_no_path_yields_empty_target_dir() {
        let action = parse_args(["silentsilo", "--download"].map(String::from));
        match action {
            ShellAction::Download { target_dir } => assert_eq!(target_dir, PathBuf::new()),
            _ => panic!("expected download"),
        }
    }

    #[test]
    fn parse_show() {
        let action = parse_args(["silentsilo", "--show"].map(String::from));
        assert!(matches!(action, ShellAction::Show));
    }

    #[test]
    fn parse_autostart() {
        let action = parse_args(["silentsilo", "--autostart"].map(String::from));
        assert!(matches!(action, ShellAction::Autostart));
    }

    #[test]
    fn parse_no_args_is_none() {
        let action = parse_args(["silentsilo"].map(String::from));
        assert!(matches!(action, ShellAction::None));
    }
}
