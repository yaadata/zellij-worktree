use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct Command {
    pub args: Vec<String>,
    pub cwd: String,
}

#[derive(Debug)]
pub enum Action {
    Run(Command),
    Open(String),
}

#[derive(Debug, Clone, Copy)]
enum Step {
    Validate,
    List,
    Common,
    Branch,
    Add,
}

pub struct Creation {
    branch: String,
    repo: String,
    common: String,
    path: String,
    exists: bool,
    step: Step,
    root: Option<String>,
}

fn branch_directory(branch: &str) -> String {
    let hash = format!("{:x}", Sha256::digest(branch.as_bytes()));
    format!("{}-{}", branch.replace(['/', '\\'], "-"), &hash[..6])
}

pub fn project_path(root: &str, common: &str, branch: &str) -> String {
    let common_path = Path::new(common);
    let repo_path = if common_path.file_name().is_some_and(|name| name == ".git") {
        common_path.parent().unwrap_or(common_path)
    } else {
        common_path
    };
    let repo_name = repo_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("repository");
    let repo_hash = format!("{:x}", Sha256::digest(common.as_bytes()));
    Path::new(root)
        .join(format!("{repo_name}-{}", &repo_hash[..6]))
        .join(branch_directory(branch))
        .to_string_lossy()
        .into_owned()
}

pub fn resolve_root(config: &str) -> Result<PathBuf, String> {
    let config = config
        .strip_prefix("dir:")
        .ok_or("worktree_root must start with dir:")?;
    let expanded = shellexpand::full_with_context(
        config,
        || std::env::var("HOME").ok().filter(|home| !home.is_empty()),
        |name| std::env::var(name).map(Some),
    )
    .map_err(|error| format!("Could not expand worktree_root: {error}"))?;
    let path = PathBuf::from(expanded.as_ref());
    if !path.is_absolute() {
        return Err("worktree_root must expand to an absolute path".into());
    }
    Ok(path)
}

fn absolute_output(output: &[u8]) -> Result<String, String> {
    let value = std::str::from_utf8(output.strip_suffix(b"\n").unwrap_or(output))
        .map_err(|_| "Host path is not valid UTF-8")?;
    if !Path::new(value).is_absolute() {
        return Err("Host command did not return an absolute path".into());
    }
    Ok(value.into())
}

/// The plugin maps the host filesystem root to /host before starting creation.
fn filesystem_path(path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() {
        return Err("Expected an absolute host path".into());
    }
    Ok(Path::new("/host").join(path.strip_prefix("/").map_err(|e| e.to_string())?))
}

fn canonical_host_directory(path: &Path) -> Result<PathBuf, String> {
    let mapped = filesystem_path(path)?;
    if !mapped.is_dir() {
        return Err(format!("Directory is unavailable: {}", path.display()));
    }
    let canonical = fs::canonicalize(mapped).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(Path::new("/").join(
        canonical
            .strip_prefix("/host")
            .map_err(|_| "Resolved path is outside the host mapping")?,
    ))
}

fn existing_path(output: &[u8], branch: &str) -> Result<Option<String>, String> {
    let target = format!("branch refs/heads/{branch}");
    let mut path = None;
    for field in output.split(|byte| *byte == 0) {
        if field.is_empty() {
            path = None;
        } else if let Some(value) = field.strip_prefix(b"worktree ") {
            path = Some(
                std::str::from_utf8(value)
                    .map_err(|_| "Worktree path is not valid UTF-8")?
                    .to_owned(),
            );
        } else if field == target.as_bytes() {
            return Ok(path);
        }
    }
    Ok(None)
}

impl Creation {
    pub fn start(
        branch: String,
        repo: String,
        root: Option<String>,
    ) -> Result<(Self, Action), String> {
        if branch.is_empty() || branch.starts_with('-') || branch == "HEAD" {
            return Err("Invalid branch name".into());
        }
        let root = root
            .map(|root| resolve_root(&root).map(|path| path.to_string_lossy().into_owned()))
            .transpose()?;
        let creation = Self {
            branch,
            repo,
            common: String::new(),
            path: String::new(),
            exists: false,
            step: Step::Validate,
            root,
        };
        // A full ref rejects revision shortcuts instead of expanding @{-1}.
        let action = creation.command(
            &[
                "git",
                "check-ref-format",
                &format!("refs/heads/{}", creation.branch),
            ],
            None,
        );
        Ok((creation, action))
    }

    fn command(&self, args: &[&str], cwd: Option<&str>) -> Action {
        Action::Run(Command {
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
            cwd: cwd.unwrap_or(&self.repo).to_owned(),
        })
    }

    fn locate_branch(&mut self, root: &Path) -> Result<Action, String> {
        let root = root.to_str().ok_or("Worktree root is not valid UTF-8")?;
        self.path = if self.root.is_some() {
            project_path(root, &self.common, &self.branch)
        } else {
            Path::new(root)
                .join(branch_directory(&self.branch))
                .to_string_lossy()
                .into_owned()
        };
        self.step = Step::Branch;
        Ok(self.command(
            &[
                "git",
                "show-ref",
                "--verify",
                "--quiet",
                &format!("refs/heads/{}", self.branch),
            ],
            None,
        ))
    }

    pub fn advance(
        &mut self,
        code: Option<i32>,
        stdout: &[u8],
        stderr: &[u8],
    ) -> Result<Action, String> {
        if code != Some(0) && !(matches!(self.step, Step::Branch) && code == Some(1)) {
            let message = format!(
                "{:?}: {}",
                self.step,
                String::from_utf8_lossy(stderr).trim()
            );
            if matches!(self.step, Step::Add) {
                // Remove only our empty reservation, never any checkout contents.
                if let Ok(path) = filesystem_path(Path::new(&self.path)) {
                    let _ = fs::remove_dir(path);
                }
            }
            return Err(if stderr.is_empty() {
                format!("{:?} command failed ({code:?})", self.step)
            } else {
                message
            });
        }
        Ok(match self.step {
            Step::Validate => {
                self.step = Step::List;
                self.command(&["git", "worktree", "list", "--porcelain", "-z"], None)
            }
            Step::List => {
                if let Some(path) = existing_path(stdout, &self.branch)? {
                    let path = canonical_host_directory(Path::new(&path))?;
                    Action::Open(
                        path.to_str()
                            .ok_or("Worktree path is not valid UTF-8")?
                            .to_owned(),
                    )
                } else if self.root.is_none() {
                    let root = Path::new(&self.repo).join(".worktrees");
                    self.locate_branch(&root)?
                } else {
                    self.step = Step::Common;
                    self.command(
                        &[
                            "git",
                            "rev-parse",
                            "--path-format=absolute",
                            "--git-common-dir",
                        ],
                        None,
                    )
                }
            }
            Step::Common => {
                let common = absolute_output(stdout)?;
                self.common = canonical_host_directory(Path::new(&common))?
                    .to_str()
                    .ok_or("Common Git directory is not valid UTF-8")?
                    .to_owned();
                let root = PathBuf::from(
                    self.root
                        .as_ref()
                        .ok_or("No worktree_root override configured")?,
                );
                self.locate_branch(&root)?
            }
            Step::Branch => {
                self.exists = code == Some(0);
                let path = filesystem_path(Path::new(&self.path))?;
                let parent = path.parent().ok_or("Worktree path has no parent")?;
                fs::create_dir_all(parent)
                    .map_err(|e| format!("Could not create worktree parent: {e}"))?;
                // Atomic reservation fails for an occupied directory or symlink.
                fs::create_dir(&path)
                    .map_err(|e| format!("Could not reserve {}: {e}", self.path))?;
                self.step = Step::Add;
                if self.exists {
                    self.command(
                        &["git", "worktree", "add", "--", &self.path, &self.branch],
                        None,
                    )
                } else {
                    self.command(
                        &[
                            "git",
                            "worktree",
                            "add",
                            "-b",
                            &self.branch,
                            "--",
                            &self.path,
                        ],
                        None,
                    )
                }
            }
            Step::Add => Action::Open(self.path.clone()),
        })
    }
}
