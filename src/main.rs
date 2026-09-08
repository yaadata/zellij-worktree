use std::collections::BTreeMap;
use yansi::Paint;
use zellij_tile::prelude::*;
use zellij_worktree::worktree;

#[derive(Debug, Clone, PartialEq)]
enum Mode {
    List,
    Create,
    DeleteConfirm,
}

#[derive(Debug, Clone)]
struct WorktreeInfo {
    path: String,
    branch: Option<String>,
    is_current: bool,
}

struct State {
    mode: Mode,
    input: String,
    worktrees: Vec<WorktreeInfo>,
    selected_index: usize,
    error_message: Option<String>,
    waiting_for_command: bool,
    repo_root: Option<String>,
    creation: Option<worktree::Creation>,
    worktree_root: Option<String>,
    initial_cwd: std::path::PathBuf,
    host_root_ready: bool,
    pending_creation: Option<worktree::Action>,
    initialized: bool,
    first_render: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            mode: Mode::List,
            input: String::new(),
            worktrees: Vec::new(),
            selected_index: 0,
            error_message: None,
            waiting_for_command: false,
            repo_root: None,
            creation: None,
            worktree_root: None,
            initial_cwd: std::path::PathBuf::new(),
            host_root_ready: false,
            pending_creation: None,
            initialized: false,
            first_render: true,
        }
    }
}

register_plugin!(State);

impl State {
    fn parse_worktree_list(&mut self, output: &[u8]) {
        let output = String::from_utf8_lossy(output);
        // Use fully qualified syntax to avoid yansi's deprecated Paint::clear()
        Vec::clear(&mut self.worktrees);

        let mut current_path: Option<String> = None;

        for line in output.lines() {
            if let Some(new_current_path) = line.strip_prefix("worktree ") {
                current_path = Some(new_current_path.to_string());
            } else if let Some(path) = &current_path {
                if let Some(current_branch) = line.strip_prefix("branch ") {
                    let is_current = self.repo_root.as_ref().map(|p| p == path).unwrap_or(false);
                    self.worktrees.push(WorktreeInfo {
                        path: path.to_string(),
                        branch: Some(current_branch.to_string()),
                        is_current,
                    });
                    current_path = None;
                } else if line.starts_with("detached") {
                    let is_current = self.repo_root.as_ref().map(|p| p == path).unwrap_or(false);
                    self.worktrees.push(WorktreeInfo {
                        path: path.to_string(),
                        branch: None,
                        is_current,
                    });
                    current_path = None;
                }
            }
        }
        // Filter out the main worktree (usually first one)
        if !self.worktrees.is_empty() {
            self.worktrees.remove(0);
        }

        self.selected_index = 0;
    }

    fn creation_action(&mut self, result: Result<worktree::Action, String>) {
        match result {
            Ok(worktree::Action::Run(command)) => {
                self.waiting_for_command = true;
                let args: Vec<&str> = command.args.iter().map(String::as_str).collect();
                run_command_with_env_variables_and_cwd(
                    &args,
                    BTreeMap::new(),
                    command.cwd.into(),
                    BTreeMap::from([("command".into(), "create".into())]),
                );
            }
            Ok(worktree::Action::Open(path)) => {
                self.waiting_for_command = false;
                self.creation = None;
                let tab_name = self.get_tab_name(&path);
                new_tab(Some(&tab_name), Some(&path));
                close_self();
            }
            Err(error) => {
                self.waiting_for_command = false;
                self.creation = None;
                self.pending_creation = None;
                self.error_message = Some(error);
            }
        }
    }

    fn get_tab_name(&self, path: &str) -> String {
        std::path::Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("worktree")
            .to_string()
    }

    fn clear_state(&mut self) {
        // Use fully qualified syntax to avoid yansi's deprecated Paint::clear()
        String::clear(&mut self.input);
        self.error_message = None;
        self.waiting_for_command = false;
        self.selected_index = 0;
    }

    fn refresh_git_info(&mut self) {
        self.initialized = false;
        self.repo_root = None;
        // Use fully qualified syntax to avoid yansi's deprecated Paint::clear()
        Vec::clear(&mut self.worktrees);
        self.error_message = None;
        self.waiting_for_command = true;
        self.mode = Mode::List;
        // Use fully qualified syntax to avoid yansi's deprecated Paint::clear()
        String::clear(&mut self.input);

        let mut context = BTreeMap::new();
        context.insert("command".to_string(), "rev-parse".to_string());
        run_command_with_env_variables_and_cwd(
            &["git", "rev-parse", "--show-toplevel"],
            BTreeMap::new(),
            self.initial_cwd.clone(),
            context,
        );
    }
}

impl ZellijPlugin for State {
    fn load(&mut self, configuration: BTreeMap<String, String>) {
        self.initial_cwd = get_plugin_ids().initial_cwd;
        self.worktree_root = configuration.get("worktree_root").cloned();
        request_permission(&[
            PermissionType::ReadApplicationState,
            PermissionType::ChangeApplicationState,
            PermissionType::RunCommands,
            PermissionType::FullHdAccess,
        ]);

        subscribe(&[
            EventType::Key,
            EventType::RunCommandResult,
            EventType::TabUpdate,
            EventType::Visible,
            EventType::HostFolderChanged,
            EventType::FailedToChangeHostFolder,
            EventType::PermissionRequestResult,
        ]);
    }

    fn update(&mut self, event: Event) -> bool {
        match event {
            Event::Key(key) => {
                if self.waiting_for_command {
                    return false;
                }

                match key.bare_key {
                    BareKey::Esc if self.mode != Mode::List => {
                        self.mode = Mode::List;
                        self.clear_state();
                    }
                    BareKey::Esc => {
                        close_self();
                    }
                    BareKey::Char('c') if key.has_modifiers(&[KeyModifier::Ctrl]) => {
                        close_self();
                    }
                    BareKey::Enter => match self.mode {
                        Mode::List => {
                            if let Some(worktree) = self.worktrees.get(self.selected_index) {
                                let tab_name = self.get_tab_name(&worktree.path);
                                new_tab(Some(&tab_name), Some(&worktree.path));
                                close_self();
                            }
                        }
                        Mode::Create => {
                            if !self.input.is_empty() {
                                self.error_message = None;
                                let result = self
                                    .repo_root
                                    .clone()
                                    .ok_or_else(|| {
                                        "Could not determine repository root".to_string()
                                    })
                                    .and_then(|repo| {
                                        worktree::Creation::start(
                                            self.input.clone(),
                                            repo,
                                            self.worktree_root.clone(),
                                        )
                                    });
                                match result {
                                    Ok((creation, action)) => {
                                        self.creation = Some(creation);
                                        if self.host_root_ready {
                                            self.creation_action(Ok(action));
                                        } else {
                                            self.waiting_for_command = true;
                                            self.pending_creation = Some(action);
                                            change_host_folder(std::path::PathBuf::from("/"));
                                        }
                                    }
                                    Err(error) => self.creation_action(Err(error)),
                                }
                            }
                        }
                        Mode::DeleteConfirm => {
                            if let Some(worktree) = self.worktrees.get(self.selected_index) {
                                self.waiting_for_command = true;
                                self.error_message = None;

                                let mut context = BTreeMap::new();
                                context
                                    .insert("command".to_string(), "worktree-remove".to_string());
                                run_command_with_env_variables_and_cwd(
                                    &["git", "worktree", "remove", &worktree.path],
                                    BTreeMap::new(),
                                    self.initial_cwd.clone(),
                                    context,
                                );
                            }
                        }
                    },
                    BareKey::Backspace => {
                        if self.mode == Mode::Create {
                            self.input.pop();
                        }
                    }
                    BareKey::Char('n') if key.has_no_modifiers() && self.mode == Mode::List => {
                        self.mode = Mode::Create;
                        // Use fully qualified syntax to avoid yansi's deprecated Paint::clear()
                        String::clear(&mut self.input);
                        self.error_message = None;
                    }
                    BareKey::Char('d') if key.has_no_modifiers() && self.mode == Mode::List => {
                        if !self.worktrees.is_empty() && self.selected_index < self.worktrees.len()
                        {
                            self.mode = Mode::DeleteConfirm;
                        }
                    }
                    BareKey::Up | BareKey::Char('k')
                        if key.has_no_modifiers() && self.mode == Mode::List =>
                    {
                        if !self.worktrees.is_empty() {
                            if self.selected_index > 0 {
                                self.selected_index -= 1;
                            } else {
                                self.selected_index = self.worktrees.len() - 1;
                            }
                        }
                    }
                    BareKey::Down | BareKey::Char('j')
                        if key.has_no_modifiers() && self.mode == Mode::List =>
                    {
                        if !self.worktrees.is_empty() {
                            if self.selected_index < self.worktrees.len() - 1 {
                                self.selected_index += 1;
                            } else {
                                self.selected_index = 0;
                            }
                        }
                    }
                    BareKey::Char(c) if c.is_ascii() && key.has_no_modifiers() => {
                        if self.mode == Mode::Create {
                            self.input.push(c);
                        }
                    }
                    _ => {}
                }
                true
            }
            Event::RunCommandResult(exit_code, stdout, stderr, context) => {
                let command_type = context.get("command").map(|s| s.as_str()).unwrap_or("");

                match command_type {
                    "rev-parse" => {
                        if exit_code == Some(0) {
                            let output = String::from_utf8_lossy(&stdout);
                            let path = output.strip_suffix('\n').unwrap_or(&output).to_string();
                            if !path.is_empty() {
                                self.repo_root = Some(path);
                                let mut context = BTreeMap::new();
                                context.insert("command".to_string(), "worktree-list".to_string());
                                run_command_with_env_variables_and_cwd(
                                    &["git", "worktree", "list", "--porcelain"],
                                    BTreeMap::new(),
                                    self.initial_cwd.clone(),
                                    context,
                                );
                            } else {
                                self.waiting_for_command = false;
                                self.error_message =
                                    Some("Could not determine git root".to_string());
                            }
                        } else {
                            self.waiting_for_command = false;
                            self.error_message = Some("Not in a git repository".to_string());
                        }
                    }
                    "worktree-list" => {
                        self.waiting_for_command = false;
                        if exit_code == Some(0) {
                            self.parse_worktree_list(&stdout);
                            self.initialized = true;
                        } else {
                            let error = String::from_utf8_lossy(&stderr).trim().to_string();
                            self.error_message = Some(if error.is_empty() {
                                "Could not list worktrees".to_string()
                            } else {
                                format!("Could not list worktrees: {}", error)
                            });
                        }
                    }
                    "create" => {
                        if let Some(creation) = &mut self.creation {
                            let result = creation.advance(exit_code, &stdout, &stderr);
                            self.creation_action(result);
                        }
                    }
                    "worktree-remove" => {
                        self.waiting_for_command = false;

                        match exit_code {
                            Some(0) => {
                                self.mode = Mode::List;
                                self.clear_state();
                                let mut ctx = BTreeMap::new();
                                ctx.insert("command".to_string(), "worktree-list".to_string());
                                run_command_with_env_variables_and_cwd(
                                    &["git", "worktree", "list", "--porcelain"],
                                    BTreeMap::new(),
                                    self.initial_cwd.clone(),
                                    ctx,
                                );
                                self.waiting_for_command = true;
                            }
                            Some(code) => {
                                let error = String::from_utf8_lossy(&stderr);
                                self.error_message =
                                    Some(format!("Error ({}): {}", code, error.trim()));
                            }
                            None => {
                                self.error_message = Some("Command failed".to_string());
                            }
                        }
                    }
                    _ => {
                        // Unknown command type
                        self.waiting_for_command = false;
                    }
                }
                true
            }
            Event::PermissionRequestResult(PermissionStatus::Denied) => {
                self.creation_action(Err("Required plugin permissions were denied".into()));
                true
            }
            Event::HostFolderChanged(path) => {
                self.host_root_ready = path == std::path::Path::new("/");
                if let Some(action) = self.pending_creation.take() {
                    if self.host_root_ready && std::path::Path::new("/host").is_dir() {
                        self.creation_action(Ok(action));
                    } else {
                        self.creation_action(Err("Host filesystem mapping is unavailable".into()));
                    }
                }
                true
            }
            Event::FailedToChangeHostFolder(error) => {
                self.host_root_ready = false;
                self.creation_action(Err(format!(
                    "Could not access host filesystem: {}",
                    error.unwrap_or_default()
                )));
                true
            }
            Event::TabUpdate(tabs) => {
                // Try to detect current worktree from focused tab
                if let Some(_focused_tab) = tabs.iter().find(|t| t.active) {
                    // We could use the tab's cwd if available
                    // For now, we rely on git rev-parse
                }
                false
            }
            Event::Visible(is_visible) => {
                if is_visible && !self.waiting_for_command {
                    // Refresh git info when plugin becomes visible
                    self.refresh_git_info();
                }
                true
            }
            _ => false,
        }
    }

    fn render(&mut self, _rows: usize, _cols: usize) {
        if self.first_render {
            self.first_render = false;
            self.refresh_git_info();
        }

        if !self.initialized {
            if let Some(error) = &self.error_message {
                println!("{}", error.red());
                println!();
                println!("{}", "Press Esc to close".bright_black());
            } else {
                println!("{}", "Loading...".yellow());
            }
            return;
        }

        match self.mode {
            Mode::List => {
                println!("{}", "Worktrees".cyan().bold());
                println!(
                    "{}",
                    "[j/k] navigate | [Enter] open | [n] new | [d] delete".bright_black()
                );
                println!();

                if self.worktrees.is_empty() {
                    println!("{}", "No worktrees found".bright_black());
                    println!();
                    println!("{}", "Press [n] to create a new worktree".bright_black());
                } else {
                    for (i, wt) in self.worktrees.iter().enumerate() {
                        let marker = if i == self.selected_index { ">" } else { " " };
                        let current = if wt.is_current {
                            format!(" {}", "(current)".yellow())
                        } else {
                            String::new()
                        };
                        let branch = wt.branch.as_deref().unwrap_or("detached");
                        let short_path = wt.path.split('/').next_back().unwrap_or(&wt.path);

                        println!("{} {} {} {}", marker, short_path, branch.cyan(), current);
                    }
                }

                if let Some(error) = &self.error_message {
                    println!();
                    println!("{}", error.red());
                }
            }
            Mode::Create => {
                println!("{}", "Create Worktree".cyan().bold());
                println!("{}", "[Esc] back to list".bright_black());
                println!();
                print!("Branch: {}", self.input);
                println!("{}", "_".blink());

                if let Some(error) = &self.error_message {
                    println!();
                    println!("{}", error.red());
                }

                if self.waiting_for_command {
                    println!();
                    println!("{}", "Creating worktree...".yellow());
                }
            }
            Mode::DeleteConfirm => {
                if let Some(wt) = self.worktrees.get(self.selected_index) {
                    println!("{}", "Confirm Delete".red().bold());
                    println!();
                    println!("Delete worktree: {}", wt.path.cyan());
                    if let Some(branch) = &wt.branch {
                        println!("Branch: {}", branch.yellow());
                    }
                    println!();
                    println!("{}", "[Enter] confirm | [Esc] cancel".bright_black());

                    if let Some(error) = &self.error_message {
                        println!();
                        println!("{}", error.red());
                    }

                    if self.waiting_for_command {
                        println!();
                        println!("{}", "Deleting...".yellow());
                    }
                }
            }
        }
    }
}
