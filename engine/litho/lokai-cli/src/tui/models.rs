//! Presentation only: the application owns catalog resolution, credential handling, and selection.
use super::{
    input::InputEffect,
    ui::{panel, ACCENT, BG, GOLD, MUTED},
    App,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use lokai_app::inference_selection::ModelCatalog;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Clear, List, ListItem, ListState, Paragraph, Wrap},
};

pub const CATEGORIES: &[&str] = &["All", "Local", "Anthropic Claude", "OpenAI", "DeepSeek"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AuthMode {
    ApiKey,
    BrowserLogin,
}

#[derive(Debug, Clone)]
pub(crate) struct AuthPrompt {
    pub provider_id: String,
    pub provider_name: String,
    pub target_model: String,
    pub login_url: String,
    pub key_input: String,
    pub mode: AuthMode,
    pub notice: Option<String>,
}

pub(crate) struct Picker {
    pub catalog: ModelCatalog,
    pub query: String,
    pub selected: usize,
    pub category_index: usize,
    pub auth_prompt: Option<AuthPrompt>,
    pub notice: Option<String>,
}

impl Picker {
    pub fn new(catalog: ModelCatalog) -> Self {
        let selected = catalog.models.iter().position(|m| m.current).unwrap_or(0);
        Self {
            catalog,
            query: String::new(),
            selected,
            category_index: 0,
            auth_prompt: None,
            notice: None,
        }
    }

    pub fn matches(&self) -> Vec<&lokai_app::inference_selection::ModelChoice> {
        let query = self.query.to_lowercase();
        let cat = CATEGORIES[self.category_index % CATEGORIES.len()];
        self.catalog
            .models
            .iter()
            .filter(|m| {
                if cat == "Local"
                    && (m.provider_id.is_some()
                        || !m.provider_label.to_lowercase().contains("local"))
                {
                    return false;
                }
                if cat != "All" && cat != "Local" && !m.provider_label.eq_ignore_ascii_case(cat) {
                    return false;
                }
                if !query.is_empty() {
                    let hay = format!("{} {}", m.name, m.provider_label).to_lowercase();
                    if !hay.contains(&query) {
                        return false;
                    }
                }
                true
            })
            .collect()
    }
}

pub fn handle_key(app: &mut App, key: KeyEvent) -> InputEffect {
    let Some(picker) = &mut app.model_picker else {
        return InputEffect::None;
    };

    // If Auth Prompt is active
    if let Some(auth) = &mut picker.auth_prompt {
        match key.code {
            KeyCode::Esc => {
                picker.auth_prompt = None;
                return InputEffect::None;
            }
            KeyCode::Tab | KeyCode::BackTab => {
                auth.mode = match auth.mode {
                    AuthMode::ApiKey => AuthMode::BrowserLogin,
                    AuthMode::BrowserLogin => AuthMode::ApiKey,
                };
                return InputEffect::None;
            }
            KeyCode::Char('1') if auth.key_input.is_empty() => {
                auth.mode = AuthMode::ApiKey;
                return InputEffect::None;
            }
            KeyCode::Char('2') if auth.key_input.is_empty() => {
                auth.mode = AuthMode::BrowserLogin;
                return InputEffect::None;
            }
            KeyCode::Char('o') | KeyCode::Char('O')
                if auth.mode == AuthMode::BrowserLogin && auth.key_input.is_empty() =>
            {
                open_browser_url(&auth.login_url);
                auth.notice =
                    Some("Opened browser to login. Copy and paste your key below.".into());
                return InputEffect::None;
            }
            KeyCode::Backspace => {
                auth.key_input.pop();
            }
            KeyCode::Char('v') | KeyCode::Char('V')
                if key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                if let Some(clip) = super::clipboard::read_clipboard_text() {
                    let clean: String = clip.chars().filter(|c| !c.is_control()).collect();
                    let remaining = 1024_usize.saturating_sub(auth.key_input.len());
                    auth.key_input.extend(clean.chars().take(remaining));
                    auth.notice = None;
                }
                return InputEffect::None;
            }
            KeyCode::Insert if key.modifiers.contains(KeyModifiers::SHIFT) => {
                if let Some(clip) = super::clipboard::read_clipboard_text() {
                    let clean: String = clip.chars().filter(|c| !c.is_control()).collect();
                    let remaining = 1024_usize.saturating_sub(auth.key_input.len());
                    auth.key_input.extend(clean.chars().take(remaining));
                    auth.notice = None;
                }
                return InputEffect::None;
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                auth.key_input.clear();
                return InputEffect::None;
            }
            KeyCode::Delete => {
                auth.key_input.clear();
                return InputEffect::None;
            }
            KeyCode::Char(c)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                if auth.key_input.len() + c.len_utf8() <= 1024 {
                    auth.key_input.push(c);
                }
            }
            KeyCode::Enter => {
                let trimmed = auth.key_input.trim();
                if trimmed.is_empty() {
                    if auth.mode == AuthMode::BrowserLogin {
                        open_browser_url(&auth.login_url);
                        auth.notice = Some(
                            "Opened browser. Copy and paste your token/key below, then press Enter."
                                .into(),
                        );
                    } else {
                        auth.notice =
                            Some("Please enter or paste an API key before submitting.".into());
                    }
                    return InputEffect::None;
                }
                return InputEffect::AuthenticateAndSelectModel {
                    provider_id: auth.provider_id.clone(),
                    key: trimmed.to_string(),
                    model: auth.target_model.clone(),
                };
            }
            _ => {}
        }
        return InputEffect::None;
    }

    // Normal Catalog browsing
    if key.code == KeyCode::Esc {
        app.model_picker = None;
        return InputEffect::None;
    }

    if (key.code == KeyCode::Char('v') || key.code == KeyCode::Char('V'))
        && key.modifiers.contains(KeyModifiers::CONTROL)
    {
        if let Some(clip) = super::clipboard::read_clipboard_text() {
            let clean: String = clip.chars().filter(|c| !c.is_control()).collect();
            let remaining = 4096_usize.saturating_sub(picker.query.len());
            picker.query.extend(clean.chars().take(remaining));
            picker.selected = 0;
        }
        return InputEffect::None;
    }
    if key.code == KeyCode::Insert && key.modifiers.contains(KeyModifiers::SHIFT) {
        if let Some(clip) = super::clipboard::read_clipboard_text() {
            let clean: String = clip.chars().filter(|c| !c.is_control()).collect();
            let remaining = 4096_usize.saturating_sub(picker.query.len());
            picker.query.extend(clean.chars().take(remaining));
            picker.selected = 0;
        }
        return InputEffect::None;
    }

    match key.code {
        KeyCode::Tab => {
            picker.category_index = (picker.category_index + 1) % CATEGORIES.len();
            picker.selected = 0;
        }
        KeyCode::BackTab => {
            picker.category_index = if picker.category_index == 0 {
                CATEGORIES.len() - 1
            } else {
                picker.category_index - 1
            };
            picker.selected = 0;
        }
        KeyCode::Up => picker.selected = picker.selected.saturating_sub(1),
        KeyCode::Down => {
            picker.selected = (picker.selected + 1).min(picker.matches().len().saturating_sub(1))
        }
        KeyCode::Backspace => {
            picker.query.pop();
            picker.selected = 0;
        }
        KeyCode::Char(c)
            if !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            if picker.query.len() + c.len_utf8() <= 4096 {
                picker.query.push(c);
            } else {
                picker.notice = Some("Search limit reached; query unchanged".into());
            }
            picker.selected = 0;
        }
        KeyCode::Enter => {
            if let Some(choice) = picker.matches().get(picker.selected) {
                if choice.requires_auth {
                    let pid = choice.provider_id.clone().unwrap_or_else(|| "cloud".into());
                    let login_url = choice
                        .login_url
                        .clone()
                        .unwrap_or_else(|| "https://console.anthropic.com/settings/keys".into());
                    picker.auth_prompt = Some(AuthPrompt {
                        provider_id: pid,
                        provider_name: choice.provider_label.clone(),
                        target_model: choice.name.clone(),
                        login_url,
                        key_input: String::new(),
                        mode: AuthMode::ApiKey,
                        notice: None,
                    });
                    return InputEffect::None;
                }
                return InputEffect::SelectModel {
                    id: choice.id.clone(),
                    revision: picker.catalog.revision,
                };
            }
        }
        _ => {}
    }
    InputEffect::None
}

pub fn render(f: &mut ratatui::Frame, app: &App) {
    let Some(picker) = &app.model_picker else {
        return;
    };

    if let Some(auth) = &picker.auth_prompt {
        render_auth_dialog(f, auth);
    } else {
        render_catalog(f, picker);
    }
}

fn render_auth_dialog(f: &mut ratatui::Frame, auth: &AuthPrompt) {
    let screen = f.area();
    let width = screen.width.min(76);
    let height = screen.height.min(18);
    let area = Rect::new(
        screen.x + (screen.width - width) / 2,
        screen.y + (screen.height - height) / 2,
        width,
        height,
    );
    f.render_widget(Clear, area);
    let title = format!(" Connect Provider: {} ", auth.provider_name);
    let block = panel(title)
        .border_style(Style::default().fg(GOLD))
        .style(Style::default().bg(BG));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(2),
        Constraint::Min(4),
        Constraint::Length(1),
        Constraint::Length(2),
    ])
    .split(inner);

    f.render_widget(
        Paragraph::new(format!("Target model: {}", auth.target_model))
            .style(Style::default().fg(ACCENT)),
        rows[0],
    );

    let m1_style = if auth.mode == AuthMode::ApiKey {
        Style::default().fg(BG).bg(GOLD)
    } else {
        Style::default().fg(MUTED)
    };
    let m2_style = if auth.mode == AuthMode::BrowserLogin {
        Style::default().fg(BG).bg(GOLD)
    } else {
        Style::default().fg(MUTED)
    };
    let tabs = Line::from(vec![
        Span::styled(" [1] Enter API Key ", m1_style),
        Span::raw("   "),
        Span::styled(" [2] Browser / Account Login ", m2_style),
    ]);
    f.render_widget(Paragraph::new(tabs), rows[1]);

    let mut body_lines = Vec::new();
    match auth.mode {
        AuthMode::ApiKey => {
            body_lines.push(Line::from("Enter or paste your API key:"));
            let char_count = auth.key_input.chars().count();
            let masked = if char_count == 0 {
                Span::styled("(paste or type API key here)", Style::default().fg(MUTED))
            } else if char_count <= 8 {
                Span::styled(
                    format!("> {} <", "*".repeat(char_count)),
                    Style::default().fg(GOLD),
                )
            } else {
                let tail: String = auth
                    .key_input
                    .chars()
                    .skip(char_count.saturating_sub(4))
                    .collect();
                Span::styled(
                    format!(
                        "> {}...{} <",
                        "*".repeat(char_count.saturating_sub(4).min(16)),
                        tail
                    ),
                    Style::default().fg(GOLD),
                )
            };
            body_lines.push(Line::from(vec![Span::raw("Key: "), masked]));
            body_lines.push(Line::from(""));
            body_lines.push(Line::from(
                "Press Tab to view browser/corporate SSO login instructions.",
            ));
        }
        AuthMode::BrowserLogin => {
            body_lines.push(Line::from(format!(
                "1. Log in to your corporate or provider account: {}",
                auth.login_url
            )));
            body_lines.push(Line::from(
                "2. Press [O] to open link in your default browser.",
            ));
            body_lines.push(Line::from(
                "3. Copy your API key or token, then paste below:",
            ));
            let masked = if auth.key_input.is_empty() {
                Span::styled("(paste token here)", Style::default().fg(MUTED))
            } else {
                Span::styled(
                    format!("> {} characters entered <", auth.key_input.len()),
                    Style::default().fg(GOLD),
                )
            };
            body_lines.push(Line::from(vec![Span::raw("Token: "), masked]));
        }
    }
    f.render_widget(
        Paragraph::new(body_lines).wrap(Wrap { trim: false }),
        rows[2],
    );

    if let Some(ref notice) = auth.notice {
        f.render_widget(
            Paragraph::new(notice.as_str()).style(Style::default().fg(GOLD)),
            rows[3],
        );
    }

    f.render_widget(
        Paragraph::new("Enter: Connect & apply  (Tab: Switch method, Esc: Return to catalog)")
            .style(Style::default().fg(MUTED)),
        rows[4],
    );
}

fn render_catalog(f: &mut ratatui::Frame, picker: &Picker) {
    let screen = f.area();
    let width = screen.width.min(94);
    let height = screen.height.min(24);
    let area = Rect::new(
        screen.x + (screen.width - width) / 2,
        screen.y + (screen.height - height) / 2,
        width,
        height,
    );
    f.render_widget(Clear, area);
    let block = panel(" Model Catalog ")
        .border_style(Style::default().fg(ACCENT))
        .style(Style::default().bg(BG));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(2),
        Constraint::Min(1),
        Constraint::Length(3),
    ])
    .split(inner);

    let mut tab_spans = Vec::new();
    for (idx, cat) in CATEGORIES.iter().enumerate() {
        let is_active = idx == picker.category_index % CATEGORIES.len();
        let style = if is_active {
            Style::default().fg(BG).bg(ACCENT)
        } else {
            Style::default().fg(MUTED)
        };
        tab_spans.push(Span::styled(format!(" {cat} "), style));
        tab_spans.push(Span::raw(" "));
    }
    f.render_widget(Paragraph::new(Line::from(tab_spans)), rows[0]);

    f.render_widget(
        Paragraph::new(format!("Search: {}", picker.query)).style(Style::default().fg(ACCENT)),
        rows[1],
    );

    let matches = picker.matches();
    if matches.is_empty() {
        f.render_widget(
            Paragraph::new("No matching models in this category. Clear search or press Tab.")
                .style(Style::default().fg(MUTED)),
            rows[2],
        );
    } else {
        let items: Vec<_> = matches
            .iter()
            .map(|m| {
                let status_badge = if m.current {
                    "[Current]"
                } else if !m.requires_auth {
                    "[Ready]"
                } else {
                    "[Login / Key Required]"
                };
                let status_style = if m.current {
                    Style::default().fg(Color::Rgb(80, 220, 120))
                } else if !m.requires_auth {
                    Style::default().fg(Color::Rgb(120, 180, 255))
                } else {
                    Style::default().fg(GOLD)
                };
                ListItem::new(vec![
                    Line::from(vec![
                        Span::styled(format!("{:<38}", m.name), Style::default()),
                        Span::raw("  "),
                        Span::styled(
                            format!("{:<18}", m.provider_label),
                            Style::default().fg(MUTED),
                        ),
                        Span::styled(status_badge, status_style),
                    ]),
                    Line::from(vec![
                        Span::raw("  "),
                        Span::styled(&m.availability, Style::default().fg(MUTED)),
                    ]),
                ])
            })
            .collect();

        let mut state = ListState::default().with_selected(Some(picker.selected));
        f.render_stateful_widget(
            List::new(items)
                .highlight_symbol("> ")
                .highlight_style(Style::default().fg(BG).bg(ACCENT)),
            rows[2],
            &mut state,
        );
    }

    let notice = picker.notice.as_deref().unwrap_or(
        "Enter: Select / Configure  (Tab: Filter Provider, Up/Down: Navigate, Esc: Close)",
    );
    f.render_widget(
        Paragraph::new(format!(
            "{notice}\nUnconnected cloud models prompt for API key or browser account login."
        ))
        .wrap(Wrap { trim: false })
        .style(Style::default().fg(MUTED)),
        rows[3],
    );
}

#[cfg(target_os = "windows")]
fn open_browser_url(url: &str) {
    #[link(name = "shell32")]
    extern "system" {
        fn ShellExecuteA(
            hwnd: *mut std::ffi::c_void,
            operation: *const std::ffi::c_char,
            file: *const std::ffi::c_char,
            parameters: *const std::ffi::c_char,
            directory: *const std::ffi::c_char,
            show_cmd: i32,
        ) -> *mut std::ffi::c_void;
    }

    if let Ok(c_url) = std::ffi::CString::new(url) {
        let op = c"open";
        unsafe {
            ShellExecuteA(
                std::ptr::null_mut(),
                op.as_ptr(),
                c_url.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                1,
            );
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn open_browser_url(url: &str) {
    if let Ok(c_url) = std::ffi::CString::new(url) {
        #[cfg(target_os = "macos")]
        let cmd = format!("open '{}'", c_url.to_str().unwrap_or(""));
        #[cfg(not(target_os = "macos"))]
        let cmd = format!("xdg-open '{}'", c_url.to_str().unwrap_or(""));

        if let Ok(c_cmd) = std::ffi::CString::new(cmd) {
            extern "C" {
                fn system(command: *const std::ffi::c_char) -> std::ffi::c_int;
            }
            unsafe {
                let _ = system(c_cmd.as_ptr());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lokai_app::inference_selection::ModelChoice;

    #[test]
    fn picker_renders_current_choice_and_empty_state_at_terminal_sizes() {
        for (width, height) in [(100, 30), (60, 18), (24, 8)] {
            let mut app = App::test_stub();
            app.model_picker = Some(Picker::new(ModelCatalog {
                revision: 0,
                models: vec![ModelChoice {
                    id: "opaque".into(),
                    name: "Example".into(),
                    provider_label: "Default".into(),
                    availability: "Configured".into(),
                    current: true,
                    requires_auth: false,
                    provider_id: None,
                    login_url: None,
                }],
            }));
            let backend = ratatui::backend::TestBackend::new(width, height);
            let mut terminal = ratatui::Terminal::new(backend).unwrap();
            terminal.draw(|f| render(f, &app)).unwrap();
            let text: String = terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|c| c.symbol())
                .collect();
            assert!(text.contains("Model Catalog"));
            if height >= 18 {
                assert!(text.contains("Example"));
            }
            app.model_picker.as_mut().unwrap().query = "missing".into();
            terminal.draw(|f| render(f, &app)).unwrap();
            let text: String = terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|c| c.symbol())
                .collect();
            if height >= 18 {
                assert!(text.contains("No matching models"));
            }
        }
    }

    #[test]
    fn filter_select_and_escape_preserve_composer() {
        let mut app = App::test_stub();
        app.input_buffer = "unfinished prompt".into();
        app.model_picker = Some(Picker::new(ModelCatalog {
            revision: 7,
            models: vec![ModelChoice {
                id: "opaque".into(),
                name: "Example".into(),
                provider_label: "Registered".into(),
                availability: "Configured".into(),
                current: true,
                requires_auth: false,
                provider_id: None,
                login_url: None,
            }],
        }));
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE),
        );
        assert!(matches!(
            handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            InputEffect::None
        ));
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
        );
        assert!(
            matches!(handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)), InputEffect::SelectModel { id, revision: 7 } if id == "opaque")
        );
        handle_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.model_picker.is_none());
        assert_eq!(app.input_buffer, "unfinished prompt");
    }

    #[test]
    fn auth_prompt_flow_submits_credential_and_toggles_mode() {
        let mut app = App::test_stub();
        app.model_picker = Some(Picker::new(ModelCatalog {
            revision: 3,
            models: vec![ModelChoice {
                id: "anthropic:test".into(),
                name: "test-cloud-model".into(),
                provider_label: "Anthropic Claude".into(),
                availability: "Requires API key".into(),
                current: false,
                requires_auth: true,
                provider_id: Some("anthropic".into()),
                login_url: Some("https://console.anthropic.com/settings/keys".into()),
            }],
        }));

        // Press Enter on unauthenticated model -> opens AuthPrompt
        let effect = handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(matches!(effect, InputEffect::None));
        let picker = app.model_picker.as_ref().unwrap();
        assert!(picker.auth_prompt.is_some());
        let auth = picker.auth_prompt.as_ref().unwrap();
        assert_eq!(auth.mode, AuthMode::ApiKey);

        // Tab toggles to BrowserLogin
        handle_key(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(
            app.model_picker
                .as_ref()
                .unwrap()
                .auth_prompt
                .as_ref()
                .unwrap()
                .mode,
            AuthMode::BrowserLogin
        );

        // Tab toggles back to ApiKey
        handle_key(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(
            app.model_picker
                .as_ref()
                .unwrap()
                .auth_prompt
                .as_ref()
                .unwrap()
                .mode,
            AuthMode::ApiKey
        );

        // Type API key characters
        for c in "my-secret-key".chars() {
            handle_key(
                &mut app,
                KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
            );
        }
        assert_eq!(
            app.model_picker
                .as_ref()
                .unwrap()
                .auth_prompt
                .as_ref()
                .unwrap()
                .key_input,
            "my-secret-key"
        );

        // Enter submits AuthenticateAndSelectModel
        let effect = handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        match effect {
            InputEffect::AuthenticateAndSelectModel {
                provider_id,
                key,
                model,
            } => {
                assert_eq!(provider_id, "anthropic");
                assert_eq!(key, "my-secret-key");
                assert_eq!(model, "test-cloud-model");
            }
            _ => panic!("expected AuthenticateAndSelectModel"),
        }

        // Esc cancels AuthPrompt and returns to catalog
        handle_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.model_picker.as_ref().unwrap().auth_prompt.is_none());
        assert!(app.model_picker.is_some());
    }

    #[test]
    fn auth_prompt_accepts_pasted_key() {
        let mut app = App::test_stub();
        app.model_picker = Some(Picker::new(ModelCatalog {
            revision: 3,
            models: vec![ModelChoice {
                id: "anthropic:test".into(),
                name: "test-cloud-model".into(),
                provider_label: "Anthropic Claude".into(),
                availability: "Requires API key".into(),
                current: false,
                requires_auth: true,
                provider_id: Some("anthropic".into()),
                login_url: Some("https://console.anthropic.com/settings/keys".into()),
            }],
        }));

        // Enter opens AuthPrompt
        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.model_picker.as_ref().unwrap().auth_prompt.is_some());

        // Paste via input::handle_paste (bracketed paste from terminal)
        crate::tui::input::handle_paste(&mut app, "sk-ant-api03-secret-key-12345");

        assert_eq!(
            app.model_picker
                .as_ref()
                .unwrap()
                .auth_prompt
                .as_ref()
                .unwrap()
                .key_input,
            "sk-ant-api03-secret-key-12345"
        );

        // Submitting with Enter works with the pasted key
        let effect = handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        match effect {
            InputEffect::AuthenticateAndSelectModel {
                provider_id,
                key,
                model,
            } => {
                assert_eq!(provider_id, "anthropic");
                assert_eq!(key, "sk-ant-api03-secret-key-12345");
                assert_eq!(model, "test-cloud-model");
            }
            _ => panic!("expected AuthenticateAndSelectModel"),
        }
    }
}
