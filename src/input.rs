//! Teclado, ratón y pegado: traducen eventos en cambios de estado.
use crate::actions::*;
use crate::app::*;
use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use std::time::{Duration, Instant};

impl App {
    pub fn on_key(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && k.code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }
        if self.splash {
            if k.code == KeyCode::Char('q') {
                self.quit = true;
            } else if self.got_first {
                self.splash = false;
            }
            return;
        }
        if self.modal.is_some() {
            self.modal_key(k);
        } else if self.detail.is_some() {
            self.detail_key(k);
        } else {
            self.list_key(k);
        }
    }

    pub fn on_paste(&mut self, text: &str) {
        match &mut self.modal {
            Some(Modal::Input { field, .. }) => field.insert(text),
            Some(Modal::Form { fields, focus, .. }) => fields[*focus].field.insert(text),
            Some(_) => {}
            None => {
                if let Some(d) = self.detail.as_mut().filter(|d| d.filtering) {
                    d.filter.insert(text);
                } else if self.filtering {
                    let t = self.tab;
                    self.filters[t.idx()].insert(text);
                    self.sync_cursor(t);
                }
            }
        }
    }

    // ───────────── diálogos ─────────────

    fn modal_key(&mut self, k: KeyEvent) {
        let Some(m) = self.modal.take() else { return };
        match m {
            Modal::Confirm {
                title,
                lines,
                danger,
                spec,
            } => match k.code {
                KeyCode::Char('y') | KeyCode::Char('s') => {
                    self.spawn_job(spec);
                    self.marked.clear();
                }
                KeyCode::Enter if !danger => self.spawn_job(spec),
                KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('q') => {}
                _ => {
                    self.modal = Some(Modal::Confirm {
                        title,
                        lines,
                        danger,
                        spec,
                    })
                }
            },
            Modal::Input {
                title,
                prompt,
                hint,
                mut field,
                kind,
            } => match k.code {
                KeyCode::Esc => {}
                KeyCode::Enter => {
                    let text = field.value.clone();
                    self.submit_input(kind, text);
                }
                _ => {
                    field.key(k);
                    self.modal = Some(Modal::Input {
                        title,
                        prompt,
                        hint,
                        field,
                        kind,
                    });
                }
            },
            Modal::Form {
                title,
                kind,
                mut fields,
                mut focus,
                mut error,
            } => {
                let n = fields.len();
                match k.code {
                    KeyCode::Esc => return,
                    KeyCode::Enter => {
                        self.modal = Some(Modal::Form {
                            title,
                            kind,
                            fields,
                            focus,
                            error,
                        });
                        self.submit_form();
                        return;
                    }
                    KeyCode::Tab | KeyCode::Down => focus = (focus + 1) % n,
                    KeyCode::BackTab | KeyCode::Up => focus = (focus + n - 1) % n,
                    _ => {
                        fields[focus].field.key(k);
                        error = None;
                    }
                }
                self.modal = Some(Modal::Form {
                    title,
                    kind,
                    fields,
                    focus,
                    error,
                });
            }
            Modal::Help => {
                if !matches!(
                    k.code,
                    KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q') | KeyCode::Enter
                ) {
                    self.modal = Some(Modal::Help);
                }
            }
            Modal::JobView { id, mut scroll } => {
                let total = self
                    .jobs
                    .iter()
                    .find(|j| j.id == id)
                    .map(|j| j.lines.len())
                    .unwrap_or(0);
                let h = self.view_h.get().max(1);
                let max = total.saturating_sub(h);
                match k.code {
                    KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('t') => return,
                    KeyCode::Char('x') => self.cancel_job(id),
                    KeyCode::Char('d') => {
                        self.jobs
                            .retain(|j| j.id != id || matches!(j.state, JobState::Running));
                        return;
                    }
                    KeyCode::Down | KeyCode::Char('j') => scroll = (scroll + 1).min(max),
                    KeyCode::Up | KeyCode::Char('k') => scroll = scroll.min(max).saturating_sub(1),
                    KeyCode::PageDown => scroll = (scroll + h).min(max),
                    KeyCode::PageUp => scroll = scroll.min(max).saturating_sub(h),
                    KeyCode::Char('g') | KeyCode::Home => scroll = 0,
                    KeyCode::Char('G') | KeyCode::End => scroll = usize::MAX,
                    _ => {}
                }
                self.modal = Some(Modal::JobView { id, scroll });
            }
        }
    }

    // ───────────── lista ─────────────

    fn list_key(&mut self, k: KeyEvent) {
        let tab = self.tab;
        if self.filtering {
            match k.code {
                KeyCode::Enter => self.filtering = false,
                KeyCode::Esc => {
                    self.filtering = false;
                    self.filters[tab.idx()] = Default::default();
                    self.sync_cursor(tab);
                }
                KeyCode::Down | KeyCode::Up => {
                    self.move_cursor(if k.code == KeyCode::Down { 1 } else { -1 })
                }
                _ => {
                    self.filters[tab.idx()].key(k);
                    self.sync_cursor(tab);
                }
            }
            return;
        }
        match k.code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char('?') => self.modal = Some(Modal::Help),
            KeyCode::F(5) => self.request_refresh(),
            KeyCode::Char(c @ '1'..='5') => self.switch_tab(Tab::ALL[c as usize - '1' as usize]),
            KeyCode::Tab => self.switch_tab(Tab::ALL[(tab.idx() + 1) % 5]),
            KeyCode::BackTab => self.switch_tab(Tab::ALL[(tab.idx() + 4) % 5]),
            KeyCode::Down | KeyCode::Char('j') => self.move_cursor(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_cursor(-1),
            KeyCode::PageDown => self.move_cursor(10),
            KeyCode::PageUp => self.move_cursor(-10),
            KeyCode::Home | KeyCode::Char('g') => self.set_cursor(0),
            KeyCode::End | KeyCode::Char('G') => self.set_cursor(usize::MAX),
            KeyCode::Enter | KeyCode::Right => self.open_detail(),
            KeyCode::Char('/') => self.filtering = true,
            KeyCode::Esc => {
                if !self.marked.is_empty() {
                    self.marked.clear();
                } else if !self.filters[tab.idx()].value.is_empty() {
                    self.filters[tab.idx()] = Default::default();
                    self.sync_cursor(tab);
                }
            }
            KeyCode::Char(' ') => self.toggle_mark(),
            KeyCode::Char('a') => self.mark_all(),
            KeyCode::Char('o') => {
                let n = tab.sorts().len();
                self.sort[tab.idx()] = (self.sort[tab.idx()] + 1) % n;
                self.sync_cursor(tab);
            }
            KeyCode::Char('t') => self.open_jobs(),
            KeyCode::Char('T') => self.dismiss_finished_jobs(),
            _ => self.tab_action(k),
        }
    }

    fn toggle_mark(&mut self) {
        let tab = self.tab;
        if let Some(i) = self.current() {
            let mk = self.mark_key(tab, &self.key_of(tab, i));
            if !self.marked.remove(&mk) {
                self.marked.insert(mk);
            }
            self.move_cursor(1);
        }
    }

    fn mark_all(&mut self) {
        let tab = self.tab;
        let keys: Vec<String> = self
            .visible(tab)
            .into_iter()
            .map(|i| self.mark_key(tab, &self.key_of(tab, i)))
            .collect();
        if keys.iter().all(|k| self.marked.contains(k)) {
            for k in keys {
                self.marked.remove(&k);
            }
        } else {
            self.marked.extend(keys);
        }
    }

    pub fn open_jobs(&mut self) {
        match self.jobs.last().map(|j| j.id) {
            Some(id) => {
                self.modal = Some(Modal::JobView {
                    id,
                    scroll: usize::MAX,
                })
            }
            None => self.toast(ToastKind::Info, "No hay tareas todavía"),
        }
    }

    pub fn open_detail(&mut self) {
        let tab = self.tab;
        let Some(i) = self.current() else { return };
        let key = self.key_of(tab, i);
        let subs = subtabs_for(tab, false);
        let _ = subs;
        self.detail = Some(Detail {
            tab,
            key,
            sub: 0,
            scroll: 0,
            follow: true,
            filter: Default::default(),
            filtering: false,
            file_idx: 0,
            jump: true,
        });
    }

    fn tab_action(&mut self, k: KeyEvent) {
        let c = match k.code {
            KeyCode::Char(c) => c,
            KeyCode::Delete => 'd',
            _ => return,
        };
        match self.tab {
            Tab::Containers => match c {
                's' => self.toggle_start(),
                'r' => self.container_verb("restart"),
                'p' => self.toggle_pause(),
                'K' => self.container_verb("kill"),
                'd' => self.ask_remove(),
                'e' => self.exec_shell(),
                'n' => self.modal = Some(new_container_form("")),
                'b' => self.open_browser(),
                'P' => self.ask_prune(),
                'c' => self.open_detail_on(SubTab::File),
                'l' => self.open_detail_on(SubTab::Logs),
                'i' => self.open_detail_on(SubTab::Inspect),
                'm' => {
                    if let Some(c) = self.current_container() {
                        let n = c.name.clone();
                        self.open_input(InputKind::Rename(n));
                    }
                }
                _ => {}
            },
            Tab::Images => match c {
                'p' => self.open_input(InputKind::Pull),
                'd' => self.ask_remove(),
                'n' | 'r' => {
                    let t = self
                        .current_image()
                        .map(|i| i.tags.first().cloned().unwrap_or_else(|| i.id.clone()))
                        .unwrap_or_default();
                    self.modal = Some(new_container_form(&t));
                }
                'b' => self.modal = Some(build_form()),
                't' => {
                    if let Some(i) = self.current_image() {
                        let id = i.id.clone();
                        self.open_input(InputKind::Tag(id));
                    }
                }
                'P' => self.ask_prune(),
                'i' => self.open_detail_on(SubTab::Inspect),
                _ => {}
            },
            Tab::Volumes => match c {
                'n' => self.open_input(InputKind::NewVolume),
                'd' => self.ask_remove(),
                'P' => self.ask_prune(),
                'i' => self.open_detail_on(SubTab::Inspect),
                _ => {}
            },
            Tab::Networks => match c {
                'n' => self.open_input(InputKind::NewNetwork),
                'd' => self.ask_remove(),
                'P' => self.ask_prune(),
                'i' => self.open_detail_on(SubTab::Inspect),
                _ => {}
            },
            Tab::Compose => match c {
                'u' => self.ask_compose("up"),
                'U' => self.ask_compose("build"),
                'x' => self.ask_compose("stop"),
                'd' => {
                    if self.current_project().is_some_and(|p| p.on_disk_only) {
                        self.forget_project();
                    } else {
                        self.ask_compose("down")
                    }
                }
                'r' => self.ask_compose("restart"),
                'p' => self.ask_compose("pull"),
                'A' | 'n' => self.open_input(InputKind::AddCompose),
                'e' => {
                    if let Some(f) = self
                        .current_project()
                        .and_then(|p| p.files.first().cloned())
                    {
                        self.edit_file(&f, None);
                    }
                }
                'l' => self.open_detail_on(SubTab::Logs),
                'c' => self.open_detail_on(SubTab::File),
                _ => {}
            },
        }
    }

    /// Abre el detalle directamente en una pestaña concreta (si existe para ese elemento).
    pub fn open_detail_on(&mut self, want: SubTab) {
        self.open_detail();
        let Some(d) = &self.detail else { return };
        let subs = self.detail_subtabs(d);
        match subs.iter().position(|s| *s == want) {
            Some(p) => {
                if let Some(d) = &mut self.detail {
                    d.sub = p;
                }
            }
            None => {
                self.detail = None;
                self.toast(
                    ToastKind::Info,
                    match want {
                        SubTab::File => "Este contenedor no viene de un Compose",
                        _ => "No disponible para este elemento",
                    },
                );
            }
        }
    }

    // ───────────── detalle ─────────────

    fn current_sub(&self) -> SubTab {
        let Some(d) = &self.detail else {
            return SubTab::Summary;
        };
        self.detail_subtabs(d)
            .get(d.sub)
            .copied()
            .unwrap_or(SubTab::Summary)
    }

    pub fn set_sub(&mut self, i: usize) {
        let n = self
            .detail
            .as_ref()
            .map(|d| self.detail_subtabs(d).len())
            .unwrap_or(0);
        if let Some(d) = &mut self.detail
            && i < n
        {
            d.sub = i;
            d.jump = true;
            d.scroll = 0;
            d.follow = true;
            d.filtering = false;
        }
    }

    fn scroll_detail(&mut self, delta: i64) {
        let len = self.view_len.get();
        let h = self.view_h.get();
        let max = len.saturating_sub(h);
        let is_logs = self.current_sub() == SubTab::Logs;
        let Some(d) = &mut self.detail else { return };
        d.jump = false;
        let cur = if is_logs && d.follow {
            max
        } else {
            d.scroll.min(max)
        };
        let new = (cur as i64 + delta).clamp(0, max as i64) as usize;
        d.scroll = new;
        if is_logs {
            d.follow = new >= max;
        }
    }

    fn detail_key(&mut self, k: KeyEvent) {
        let sub = self.current_sub();
        let tab = self
            .detail
            .as_ref()
            .map(|d| d.tab)
            .unwrap_or(Tab::Containers);
        // Filtro de logs activo: escribe en el campo.
        if let Some(d) = self.detail.as_mut().filter(|d| d.filtering) {
            match k.code {
                KeyCode::Enter => d.filtering = false,
                KeyCode::Esc => {
                    d.filtering = false;
                    d.filter = Default::default();
                }
                _ => {
                    d.filter.key(k);
                }
            }
            return;
        }
        let n_subs = self
            .detail
            .as_ref()
            .map(|d| self.detail_subtabs(d).len())
            .unwrap_or(1);
        let cur = self.detail.as_ref().map(|d| d.sub).unwrap_or(0);
        let page = self.view_h.get().max(2) as i64 - 1;
        match k.code {
            KeyCode::Esc | KeyCode::Backspace | KeyCode::Char('q') => {
                self.detail = None;
                self.logs = None;
            }
            KeyCode::Char('?') => self.modal = Some(Modal::Help),
            KeyCode::Right | KeyCode::Tab | KeyCode::Char('l') => self.set_sub((cur + 1) % n_subs),
            KeyCode::Left | KeyCode::BackTab | KeyCode::Char('h') => {
                self.set_sub((cur + n_subs - 1) % n_subs)
            }
            KeyCode::Char(c @ '1'..='9') => self.set_sub(c as usize - '1' as usize),
            KeyCode::Down | KeyCode::Char('j') => self.scroll_detail(1),
            KeyCode::Up | KeyCode::Char('k') => self.scroll_detail(-1),
            KeyCode::PageDown | KeyCode::Char(' ') => self.scroll_detail(page),
            KeyCode::PageUp => self.scroll_detail(-page),
            KeyCode::Home | KeyCode::Char('g') => self.scroll_detail(i64::MIN / 2),
            KeyCode::End | KeyCode::Char('G') => self.scroll_detail(i64::MAX / 2),
            KeyCode::Char('t') => self.open_jobs(),
            KeyCode::Char('f') if sub == SubTab::Logs => {
                if let Some(d) = &mut self.detail {
                    d.follow = !d.follow;
                    if d.follow {
                        d.scroll = usize::MAX;
                    }
                }
            }
            KeyCode::Char('c') if sub == SubTab::Logs => {
                if let Some(l) = &mut self.logs {
                    l.lines.clear();
                }
            }
            KeyCode::Char('/') if sub == SubTab::Logs => {
                if let Some(d) = &mut self.detail {
                    d.filtering = true;
                }
            }
            KeyCode::Char('e') if sub == SubTab::File => self.edit_current_file(),
            KeyCode::Char('[') | KeyCode::Char(']') if sub == SubTab::File => {
                let nf = self.detail_files().len().max(1);
                if let Some(d) = &mut self.detail {
                    d.file_idx = if k.code == KeyCode::Char(']') {
                        (d.file_idx + 1) % nf
                    } else {
                        (d.file_idx + nf - 1) % nf
                    };
                    d.scroll = 0;
                    d.jump = true;
                }
            }
            KeyCode::Char(c) => match tab {
                Tab::Containers => match c {
                    's' => self.toggle_start(),
                    'r' => self.container_verb("restart"),
                    'p' => self.toggle_pause(),
                    'K' => self.container_verb("kill"),
                    'd' => self.ask_remove(),
                    'e' => self.exec_shell(),
                    'b' => self.open_browser(),
                    'u' => self.ask_compose("up"),
                    _ => {}
                },
                Tab::Compose => match c {
                    'u' => self.ask_compose("up"),
                    'U' => self.ask_compose("build"),
                    'x' => self.ask_compose("stop"),
                    'd' => self.ask_compose("down"),
                    'r' => self.ask_compose("restart"),
                    'p' => self.ask_compose("pull"),
                    'e' => self.edit_current_file(),
                    _ => {}
                },
                Tab::Images | Tab::Volumes | Tab::Networks => {
                    if c == 'd' {
                        self.ask_remove()
                    }
                }
            },
            _ => {}
        }
    }

    // ───────────── ratón ─────────────

    pub fn on_mouse(&mut self, m: MouseEvent) {
        if self.splash {
            return;
        }
        let (x, y) = (m.column, m.row);
        let inside = |r: ratatui::layout::Rect| {
            x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height
        };
        match m.kind {
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                let d: i64 = if m.kind == MouseEventKind::ScrollDown {
                    3
                } else {
                    -3
                };
                if let Some(Modal::JobView { scroll, .. }) = &mut self.modal {
                    *scroll = if d > 0 {
                        scroll.saturating_add(3)
                    } else {
                        (*scroll).min(10_000).saturating_sub(3)
                    };
                } else if self.modal.is_none() {
                    if self.detail.is_some() {
                        self.scroll_detail(d);
                    } else {
                        self.move_cursor(d);
                    }
                }
            }
            MouseEventKind::Down(MouseButton::Left) if self.modal.is_none() => {
                if let Some((_, t)) = self.hits.tabs.iter().find(|(r, _)| inside(*r)).copied() {
                    self.detail = None;
                    self.logs = None;
                    self.switch_tab(t);
                    return;
                }
                if self.detail.is_some() {
                    if let Some((_, i)) =
                        self.hits.subtabs.iter().find(|(r, _)| inside(*r)).copied()
                    {
                        self.set_sub(i);
                    }
                    return;
                }
                let l = self.hits.list;
                if inside(l) && y >= self.hits.list_first_row {
                    let off = self.tables[self.tab.idx()].offset();
                    let pos = off + (y - self.hits.list_first_row) as usize;
                    if pos < self.visible(self.tab).len() {
                        let dbl = self.last_click.is_some_and(|(t, cx, cy)| {
                            t.elapsed() < Duration::from_millis(450) && cx == x && cy == y
                        });
                        self.set_cursor(pos);
                        if dbl {
                            self.open_detail();
                        }
                        self.last_click = Some((Instant::now(), x, y));
                    }
                }
            }
            _ => {}
        }
    }
}
