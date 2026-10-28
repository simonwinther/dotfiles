//! Keyboard routing in one place: overlays consume input before the table.
use crate::top::{App, data::Job};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub enum Effect {
    None,
    Quit,
    Connect(Option<crate::top::hosts::Target>),
    RefreshLive,
    Pause(bool),
    OpenLog(Box<Job>),
    ReloadLog,
    Yank,
}
impl App {
    fn picker_key(&mut self, key: KeyEvent) -> Effect {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if let Some(input) = &mut self.picker.input {
            match key.code {
                KeyCode::Esc => {
                    self.picker.input = None;
                    self.picker.error = None;
                }
                KeyCode::Enter => {
                    let host = input.trim().to_string();
                    match self.picker.add(&host) {
                        Ok(()) => {
                            self.picker.input = None;
                            self.picker.error = None;
                        }
                        Err(error) => self.picker.error = Some(format!("{error:#}")),
                    }
                }
                KeyCode::Backspace => {
                    input.pop();
                }
                KeyCode::Char('u') if ctrl => input.clear(),
                KeyCode::Char(c) if !ctrl && input.len() < 255 => input.push(c),
                _ => {}
            }
            return Effect::None;
        }
        let choices = self.picker.choices();
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                self.picker.selected =
                    (self.picker.selected + 1).min(choices.len().saturating_sub(1))
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.picker.selected = self.picker.selected.saturating_sub(1)
            }
            KeyCode::Home | KeyCode::Char('g') => self.picker.selected = 0,
            KeyCode::End | KeyCode::Char('G') => {
                self.picker.selected = choices.len().saturating_sub(1)
            }
            KeyCode::Enter => {
                if let Some(target) = choices.get(self.picker.selected).cloned() {
                    return Effect::Connect(Some(target));
                }
            }
            KeyCode::Char('a') => {
                self.picker.input = Some(String::new());
                self.picker.error = None;
            }
            KeyCode::Char('d') => {
                let removed_active = choices.get(self.picker.selected) == self.args.target.as_ref();
                match self.picker.remove() {
                    Ok(()) => {
                        self.picker.error = None;
                        if removed_active {
                            return Effect::Connect(None);
                        }
                    }
                    Err(error) => self.picker.error = Some(format!("{error:#}")),
                }
            }
            KeyCode::Char('q' | 'Q') | KeyCode::Esc | KeyCode::Char('c') => {
                if self.args.target.is_none() {
                    return Effect::Quit;
                }
                self.picker.open = false;
            }
            _ => {}
        }
        Effect::None
    }
    pub fn begin_search(&mut self) {
        self.query_before_edit = self.query.clone();
        self.editing = true;
    }
    pub fn switch_tab(&mut self, tab: u8) {
        if tab == self.tab {
            return;
        }
        let old = usize::from(self.tab - 1);
        self.tab_queries[old] = self.query.clone();
        self.tab_identities[old] = self.identity();
        self.tab_selections[old] = self.selected;
        self.tab_job_sorts[old] = self.job_sort.clone();
        self.tab = tab;
        let new = usize::from(tab - 1);
        self.query = self.tab_queries[new].clone();
        self.job_sort = self.tab_job_sorts[new].clone();
        self.selected = self.tab_selections[new];
        self.rebuild_views();
        if let Some(id) = &self.tab_identities[new] {
            let position = if tab == 1 {
                self.node_view
                    .iter()
                    .position(|&i| &self.data.nodes[i].name == id)
            } else {
                self.job_view.iter().position(|&i| &self.jobs()[i].id == id)
            };
            if position.is_some() {
                self.selected.select(position);
            }
        }
        self.history_detail_scroll = 0;
        self.move_by(0);
    }
    fn close_layer(&mut self) -> bool {
        if self.detail {
            self.detail = false;
            self.history_detail_scroll = 0;
            true
        } else {
            false
        }
    }
    fn cycle_sort(&mut self) {
        let id = self.identity();
        if self.tab == 1 {
            let modes = ["free", "gpu", "cpu", "state", "node"];
            let i = modes.iter().position(|s| *s == self.sort).unwrap_or(0);
            self.sort = modes[(i + 1) % modes.len()].into();
        } else {
            let modes: &[&str] = if self.tab == 4 {
                &["submitted", "ended", "name", "state"]
            } else {
                &["priority", "submitted", "name", "state"]
            };
            let i = modes.iter().position(|s| *s == self.job_sort).unwrap_or(0);
            self.job_sort = modes[(i + 1) % modes.len()].into();
        }
        self.rebuild_views();
        let position = id.and_then(|id| {
            if self.tab == 1 {
                self.node_view
                    .iter()
                    .position(|&i| self.data.nodes[i].name == id)
            } else {
                self.job_view.iter().position(|&i| self.jobs()[i].id == id)
            }
        });
        self.selected.select(position.or(Some(0)));
        self.move_by(0);
    }
    pub fn handle_key(&mut self, key: KeyEvent, width: u16, height: u16) -> Effect {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && key.code == KeyCode::Char('c') {
            return Effect::Quit;
        }
        if self.picker.open {
            return self.picker_key(key);
        }
        if key.code == KeyCode::Char('Q')
            && !self.editing
            && self.log.as_ref().is_none_or(|log| !log.editing_search)
        {
            return Effect::Quit;
        }
        if self.help {
            self.help = false;
            return Effect::None;
        }
        if key.code == KeyCode::Char('c')
            && !self.editing
            && !self.filter_open
            && self.log.as_ref().is_none_or(|log| !log.editing_search)
        {
            self.picker.reload();
            self.picker.selected = self
                .picker
                .choices()
                .iter()
                .position(|target| Some(target) == self.args.target.as_ref())
                .unwrap_or(0);
            self.picker.open = true;
            return Effect::None;
        }
        if let Some(log) = &mut self.log {
            let page = height.saturating_sub(3) as usize;
            if log.editing_search {
                log.key(key, page);
                return Effect::None;
            }
            match key.code {
                KeyCode::Char('Q') => return Effect::Quit,
                KeyCode::Char('?') => self.help = true,
                KeyCode::Char('q') => self.log = None,
                KeyCode::Esc if log.anchor.is_none() => self.log = None,
                KeyCode::Char('y' | 'Y')
                    if !self.log_loading && !log.copying && !self.copy_loading =>
                {
                    return Effect::Yank;
                }
                KeyCode::Char('r') if !self.log_loading && !log.copying => {
                    log.status = "Reloading log…".into();
                    return Effect::ReloadLog;
                }
                _ => log.key(key, page),
            }
            return Effect::None;
        }
        if self.editing {
            match key.code {
                KeyCode::Enter => self.editing = false,
                KeyCode::Esc => {
                    self.query = self.query_before_edit.clone();
                    self.editing = false;
                    self.reset_selection();
                }
                KeyCode::Backspace => {
                    self.query.pop();
                    self.reset_selection();
                }
                KeyCode::Char('u') if ctrl => {
                    self.query.clear();
                    self.reset_selection();
                }
                KeyCode::Char(c) if !ctrl && self.query.chars().count() < 512 => {
                    self.query.push(c);
                    self.reset_selection();
                }
                _ => {}
            }
            return Effect::None;
        }
        if self.filter_open {
            if key.code == KeyCode::Char('?') {
                self.help = true;
            } else {
                self.filter_key(key.code);
            }
            return Effect::None;
        }
        let page = height.saturating_sub(12).max(1) as isize;
        let delta = match (key.code, ctrl) {
            (KeyCode::Char('d'), true) => Some((page / 2).max(1)),
            (KeyCode::Char('u'), true) => Some(-(page / 2).max(1)),
            (KeyCode::Char('f'), true) | (KeyCode::PageDown, _) => Some(page),
            (KeyCode::Char('b'), true) | (KeyCode::PageUp, _) => Some(-page),
            (KeyCode::Down | KeyCode::Char('j'), _) => Some(1),
            (KeyCode::Up | KeyCode::Char('k'), _) => Some(-1),
            _ => None,
        };
        if let Some(delta) = delta {
            if self.detail {
                let max = crate::top::ui::detail_max_scroll(self, width, height);
                self.history_detail_scroll = self
                    .history_detail_scroll
                    .saturating_add_signed(delta.clamp(i16::MIN as isize, i16::MAX as isize) as i16)
                    .min(max);
            } else {
                self.move_by(delta);
            }
            return Effect::None;
        }
        match key.code {
            KeyCode::Char('Q') => return Effect::Quit,
            KeyCode::Char('q') if !self.close_layer() => return Effect::Quit,
            KeyCode::Esc => {
                self.close_layer();
            }
            KeyCode::Char('?') => self.help = true,
            KeyCode::Char('i') => {
                self.detail = !self.detail;
                self.history_detail_scroll = 0;
            }
            KeyCode::Char('J') => {
                self.history_detail_scroll = self
                    .history_detail_scroll
                    .saturating_add(1)
                    .min(crate::top::ui::detail_max_scroll(self, width, height))
            }
            KeyCode::Char('K') => {
                self.history_detail_scroll = self.history_detail_scroll.saturating_sub(1)
            }
            KeyCode::Char('f') => self.begin_filter(),
            KeyCode::Char('/') => self.begin_search(),
            KeyCode::Char('x') => {
                self.query.clear();
                self.category_masks[usize::from(self.tab - 1)] = 0;
                self.filter_cursors[usize::from(self.tab - 1)] = 0;
                if self.tab == 1 {
                    self.available = false;
                }
                self.reset_selection();
            }
            KeyCode::Char('1'..='4') => {
                if let KeyCode::Char(c) = key.code {
                    self.switch_tab(c as u8 - b'0');
                }
            }
            KeyCode::Tab => self.switch_tab(self.tab % 4 + 1),
            KeyCode::BackTab => self.switch_tab((self.tab + 2) % 4 + 1),
            KeyCode::Home | KeyCode::Char('g') => {
                if self.detail {
                    self.history_detail_scroll = 0;
                } else {
                    self.move_by(-(self.count() as isize));
                }
            }
            KeyCode::End | KeyCode::Char('G') => {
                if self.detail {
                    self.history_detail_scroll =
                        crate::top::ui::detail_max_scroll(self, width, height);
                } else {
                    self.move_by(self.count() as isize);
                }
            }
            KeyCode::Enter | KeyCode::Char('o') if self.tab != 1 => {
                if self.log_loading {
                    self.notice = Some("Previous log is still loading; try again shortly".into());
                } else if let Some(&i) = self.job_view.get(self.selected.selected().unwrap_or(0)) {
                    return Effect::OpenLog(Box::new(self.jobs()[i].clone()));
                }
            }
            KeyCode::Enter => {
                self.detail = !self.detail;
                self.history_detail_scroll = 0;
            }
            KeyCode::Char('r') if self.tab == 4 && !self.history_loading => {
                self.history_loaded = false
            }
            KeyCode::Char('r') if self.tab != 4 && !self.loading => return Effect::RefreshLive,
            KeyCode::Char('a') if self.tab == 1 => {
                self.available = !self.available;
                self.reset_selection();
            }
            KeyCode::Char('p') => {
                self.paused = !self.paused;
                return Effect::Pause(self.paused);
            }
            KeyCode::Char('s') => self.cycle_sort(),
            _ => {}
        }
        Effect::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    fn app() -> App {
        let mut app = App::new(crate::top::Args::parse_from(["mytop", "--user", "me"]));
        app.data.nodes = crate::top::data::parse_nodes(
            "NodeName=a Partitions=gpu Gres=gpu:a100:2 AllocTRES=\nNodeName=b Partitions=gpu Gres=gpu:a100:2 AllocTRES=",
            "gpu",
        );
        app.reset_selection();
        app
    }
    fn press(app: &mut App, code: KeyCode) -> Effect {
        app.handle_key(KeyEvent::new(code, KeyModifiers::NONE), 120, 36)
    }
    #[test]
    fn picker_consumes_navigation_and_allows_uppercase_q_in_host_input() {
        let mut app = app();
        app.picker.open = true;
        app.picker.hosts.hosts = vec!["alpha".into(), "beta".into()];
        press(&mut app, KeyCode::Char('j'));
        assert_eq!(app.picker.selected, 1);
        assert_eq!(app.selected.selected(), Some(0));
        assert!(
            matches!(press(&mut app, KeyCode::Enter), Effect::Connect(Some(crate::top::hosts::Target::Ssh(host))) if host == "beta")
        );
        press(&mut app, KeyCode::Char('a'));
        assert!(matches!(press(&mut app, KeyCode::Char('Q')), Effect::None));
        assert_eq!(app.picker.input.as_deref(), Some("Q"));
        press(&mut app, KeyCode::Esc);
        assert!(app.picker.input.is_none());
        press(&mut app, KeyCode::Esc);
        assert!(!app.picker.open);
    }
    #[test]
    fn layers_close_without_quitting_or_clearing_filters() {
        let mut app = app();
        app.query = "a".into();
        app.reset_selection();
        press(&mut app, KeyCode::Char('i'));
        assert!(matches!(press(&mut app, KeyCode::Char('q')), Effect::None));
        assert!(!app.detail);
        assert_eq!(app.query, "a");
        press(&mut app, KeyCode::Char('f'));
        press(&mut app, KeyCode::Char('q'));
        assert!(!app.filter_open);
        press(&mut app, KeyCode::Esc);
        assert_eq!(app.query, "a");
        assert!(matches!(press(&mut app, KeyCode::Char('q')), Effect::Quit));
    }
    #[test]
    fn filter_tab_moves_without_toggling_and_search_cancel_restores() {
        let mut app = app();
        press(&mut app, KeyCode::Char('f'));
        press(&mut app, KeyCode::Tab);
        assert_eq!(app.category(), "all");
        press(&mut app, KeyCode::Char(' '));
        assert_eq!(app.category(), "idle");
        press(&mut app, KeyCode::Esc);
        app.query = "a".into();
        press(&mut app, KeyCode::Char('/'));
        press(&mut app, KeyCode::Char('b'));
        press(&mut app, KeyCode::Esc);
        assert_eq!(app.query, "a");
    }
    #[test]
    fn tabs_remember_selection_query_and_sort() {
        let mut app = app();
        app.move_by(1);
        let id = app.identity();
        app.query = "a100".into();
        app.switch_tab(4);
        assert_eq!(app.query, "");
        assert_eq!(app.job_sort, "submitted");
        app.query = "failed".into();
        app.job_sort = "state".into();
        app.switch_tab(1);
        assert_eq!(app.query, "a100");
        assert_eq!(app.identity(), id);
        app.switch_tab(4);
        assert_eq!(app.query, "failed");
        assert_eq!(app.job_sort, "state");
        press(&mut app, KeyCode::BackTab);
        assert_eq!(app.tab, 3);
    }
    #[test]
    fn log_search_consumes_y_and_q_and_help_always_works() {
        let mut app = app();
        let mut log = crate::top::history::LogView::loading("test");
        log.lines = vec!["query".into()];
        app.log = Some(log);
        press(&mut app, KeyCode::Char('/'));
        press(&mut app, KeyCode::Char('q'));
        press(&mut app, KeyCode::Char('y'));
        assert_eq!(app.log.as_ref().unwrap().search, "qy");
        press(&mut app, KeyCode::Esc);
        assert!(app.log.is_some());
        press(&mut app, KeyCode::Char('?'));
        assert!(app.help);
        press(&mut app, KeyCode::Char('q'));
        assert!(app.log.is_some());
        assert!(!app.help);
    }
}
